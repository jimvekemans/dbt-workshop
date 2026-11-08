#!/usr/bin/env python3
"""
Bootstrap script for the VDAB Enterprise dbt-singlestore Workshop stack.
Configures:
1. SingleStore enterprise topology (edw_dv_rdv, edw_dv_bdv, edw_ldm, edw_fmc,
   im_dimensional, pres_dataproduct, tenant_ai, vdp_mgmt, vdp_ci), RBAC roles/groups,
   personal users (pu_*), service accounts (svc_*_dbt), management tables,
   _olist_master archive, and initial 90-day trickle baseline in analytics_dev (Domain A+B)
   and analytics_prod (Domain A only, Domain B withheld).
2. GitLab CE root PAT, Airflow OAuth2 SSO application, 'workshop' group,
   4 RBAC subgroups, 7 workshop users, and the 'workshop/dbt-vdp' repository
   (pushed from ./dbt-singlestore-workshop) with Modified GitFlow branches (main, dev, changelog),
   tags v1.0.0 / 2026_DROP1.0, and protected branch rules (including force-push on main for Maintainers).
3. Enterprise Docker images ('dbt-singlestore-base:1.11.11' and 'dbt-vdp:{ldv,abt,cbt,prd}')
   and GitLab Runner registration on 'workshop_net'.
4. Initial 'dbt-vdp' deployment sync into the Docker volumes 'workshop_dev' / 'workshop_prod' (mounted at
   /workshop_state/{dev,prod}: Airflow DAGs, dbt project, /mnt/dbt PVC directories, static dbt-docs).
5. SingleStore milestone 'baseline' (manager/milestones.sh): the studio Reset restores the warehouse to it.

Lives in infrastructure/manager/; the dbt project is ../../dbt-singlestore-workshop. Runs in the manager container
(infrastructure/workshop.sh bootstrap): no Python on the host, the stack is reached over workshop_net.
"""

import argparse
import csv
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.parse

try:
    import pymysql
    import requests
except ImportError:
    sys.exit("Missing deps (PyMySQL, requests). Run it in the manager container instead: infrastructure/workshop.sh bootstrap")

MANAGER_DIR = os.path.dirname(os.path.abspath(__file__))            # infrastructure/manager/: this script
WORKSHOP_ROOT = os.path.dirname(MANAGER_DIR)                         # infrastructure/: one folder per service
COMPOSE_DIR = os.path.join(WORKSHOP_ROOT, "compose")                 # docker-compose.yml + .env (OAuth credentials)
COMPOSE_CMD = ["docker", "compose", "--project-directory", COMPOSE_DIR, "-f", os.path.join(COMPOSE_DIR, "docker-compose.yml")]
REPO_ROOT = os.path.dirname(WORKSHOP_ROOT)
DBT_PROJECT_DIR = os.path.join(REPO_ROOT, "dbt-singlestore-workshop")
# Olist CSVs (downloaded once, git-ignored): loaded from the host with LOAD DATA LOCAL INFILE, no mount needed
OLIST_ARCHIVE_DIR = os.path.join(WORKSHOP_ROOT, "singlestore", "olist_archive")
MILESTONES_SCRIPT = os.path.join(MANAGER_DIR, "milestones.sh")
BASELINE_MILESTONE = "baseline"
# The deployed LDV/PRD workspaces are Docker volumes (docker-compose.yml), mounted at /workshop_state/<env>
WORKSHOP_STATE_VOLUMES = {"dev": "workshop_dev", "prod": "workshop_prod"}
WORKSHOP_STATE_MOUNTS = [arg for env, vol in WORKSHOP_STATE_VOLUMES.items() for arg in ("-v", f"{vol}:/workshop_state/{env}")]

# Defaults = run on the host against the published ports. The manager container (infrastructure/workshop.sh) sets
# GITLAB_URL=http://gitlab.local, S2_HOST=singlestore, S2_PORT=3306: it talks to the stack over workshop_net.
GITLAB_URL = os.environ.get("GITLAB_URL", "http://localhost")
S2_HOST = os.environ.get("S2_HOST", "127.0.0.1")
S2_PORT = int(os.environ.get("S2_PORT", os.environ.get("WORKSHOP_S2_PORT", "23306")))
# Containers are named workshop-<service> (docker-compose.yml): the stack never collides with other projects
GITLAB_CONTAINER = os.environ.get("GITLAB_CONTAINER", "workshop-gitlab")
RUNNER_CONTAINER = os.environ.get("RUNNER_CONTAINER", "workshop-gitlab-runner")
AIRFLOW_LDV_PORT = os.environ.get("WORKSHOP_AIRFLOW_LDV_PORT", "8088")
AIRFLOW_PRD_PORT = os.environ.get("WORKSHOP_AIRFLOW_PRD_PORT", "8090")
S2_ROOT_PWD = os.environ.get("S2_ROOT_PWD", "WorkshopRootPwd123!")
SHARED_USER_PWD = "WorkshopPwd2026!"
ROOT_PAT = "glpat-workshop-root-token-2026"

BASE_IMAGE_NAME = "dbt-singlestore-base:1.11.11"
ENTERPRISE_IMAGE_TAGS = [
    "dbt-singlestore-base:latest",
    "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-singlestore-base:1.11.11",
    "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-singlestore-base:1.9.2",
    "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-vdp/dbt-vdp:ldv",
    "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-vdp/dbt-vdp:abt",
    "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-vdp/dbt-vdp:cbt",
    "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-vdp/dbt-vdp:prd",
    "workshop-dbt-ci:latest",
]

USERS = [
    {"username": "admin1",      "name": "Admin One",      "subgroup": "admins",      "access_level": 50, "is_admin": True},
    {"username": "maintainer1", "name": "Maintainer One", "subgroup": "maintainers", "access_level": 40, "is_admin": False},
    {"username": "maintainer2", "name": "Maintainer Two", "subgroup": "maintainers", "access_level": 40, "is_admin": False},
    {"username": "user1",       "name": "User One",       "subgroup": "users",       "access_level": 30, "is_admin": False},
    {"username": "user2",       "name": "User Two",       "subgroup": "users",       "access_level": 30, "is_admin": False},
    {"username": "viewer1",     "name": "Viewer One",     "subgroup": "viewers",     "access_level": 20, "is_admin": False},
    {"username": "viewer2",     "name": "Viewer Two",     "subgroup": "viewers",     "access_level": 20, "is_admin": False},
]


def sanitize_reviews_csv(path):
    temp_path = path + ".tmp"
    needs_sanitization = False
    with open(path, "r", encoding="utf-8") as f:
        reader = csv.reader(f)
        for row in reader:
            if any("\n" in col or "\r" in col for col in row):
                needs_sanitization = True
                break
    if needs_sanitization:
        print("     Sanitizing reviews CSV embedded newlines for SingleStore LOAD DATA...")
        with open(path, "r", encoding="utf-8") as fin, open(temp_path, "w", encoding="utf-8", newline="") as fout:
            reader = csv.reader(fin)
            writer = csv.writer(fout, quoting=csv.QUOTE_MINIMAL)
            for row in reader:
                cleaned = [col.replace("\r\n", " ").replace("\n", " ").replace("\r", " ") for col in row]
                writer.writerow(cleaned)
        os.replace(temp_path, path)


def ensure_olist_archive():
    print("  -> Checking Olist raw archive CSVs in infrastructure/singlestore/olist_archive...")
    archive_dir = OLIST_ARCHIVE_DIR
    os.makedirs(archive_dir, exist_ok=True)
    base_url = "https://raw.githubusercontent.com/olist/work-at-olist-data/master/datasets"
    files = [
        "olist_customers_dataset.csv",
        "olist_orders_dataset.csv",
        "olist_order_payments_dataset.csv",
        "olist_order_reviews_dataset.csv",
        "olist_order_items_dataset.csv",
    ]
    for filename in files:
        target_path = os.path.join(archive_dir, filename)
        if not os.path.exists(target_path):
            print(f"     Downloading {filename}...")
            r = requests.get(f"{base_url}/{filename}", stream=True)
            r.raise_for_status()
            with open(target_path, "wb") as f:
                for chunk in r.iter_content(chunk_size=65536):
                    f.write(chunk)
            if filename == "olist_order_reviews_dataset.csv":
                sanitize_reviews_csv(target_path)

    review_path = os.path.join(archive_dir, "olist_order_reviews_dataset.csv")
    if os.path.exists(review_path):
        sanitize_reviews_csv(review_path)


def parse_sql_script(content):
    delimiter = ";"
    statements = []
    current = []
    for line in content.splitlines():
        stripped = line.strip()
        if stripped.startswith("--") or not stripped:
            continue
        if stripped.upper().startswith("DELIMITER"):
            parts = stripped.split()
            if len(parts) >= 2:
                delimiter = parts[1]
            continue
        current.append(line)
        if stripped.endswith(delimiter):
            stmt = "\n".join(current)
            stmt = stmt[:-len(delimiter)].strip()
            if stmt:
                statements.append(stmt)
            current = []
    if current:
        stmt = "\n".join(current).strip()
        if stmt:
            statements.append(stmt)
    return statements


STUDIO_ENVS = (("analytics_dev", "dev"), ("analytics_prod", "prod"))


def apply_studio_schema(cur):
    """Idempotently (re)create the per-channel fault-injection tables and the trickle procedures
    in both landing databases (see singlestore/studio_schema.sql). Safe on existing stacks."""
    with open(os.path.join(WORKSHOP_ROOT, "singlestore", "studio_schema.sql"), "r") as f:
        template = f.read()
    for db_name, env_name in STUDIO_ENVS:
        script = template.replace("__DB__", db_name).replace("__ENV__", env_name)
        for stmt in parse_sql_script(script):
            cur.execute(stmt)
    # DEV-only objects (QA test-account registry + smoke-test procedure): PRD never has them
    with open(os.path.join(WORKSHOP_ROOT, "singlestore", "studio_schema_dev.sql"), "r") as f:
        for stmt in parse_sql_script(f.read()):
            cur.execute(stmt)


def reset_channel_config(cur):
    for db_name, _ in STUDIO_ENVS:
        cur.execute(f"CALL {db_name}.sp_reset_channel_config();")


def wait_for_singlestore(timeout_seconds=300):
    """A fresh singlestoredb-dev container accepts connections before it has set ROOT_PASSWORD (1045 Access denied)
    and refuses them while it starts (2003): retry until root can log in."""
    start = time.time()
    while True:
        try:
            return pymysql.connect(host=S2_HOST, port=S2_PORT, user="root", password=S2_ROOT_PWD, autocommit=True,
                                   local_infile=True, connect_timeout=5)
        except pymysql.err.OperationalError as e:
            if time.time() - start > timeout_seconds:
                raise TimeoutError(f"SingleStore at {S2_HOST}:{S2_PORT} not ready after {timeout_seconds}s: {e}") from e
            print(f"  -> Waiting for SingleStore at {S2_HOST}:{S2_PORT} ({e.args[0]})...")
            time.sleep(5)


def seed_singlestore(force_reload_archive=False, initial_trickle_days=90):
    print("[1/5] Seeding SingleStore VDAB enterprise topology, RBAC roles, PU/SVC users, and Olist landing zones...")
    ensure_olist_archive()

    with open(os.path.join(WORKSHOP_ROOT, "singlestore", "init_s2.sql"), "r") as f:
        sql_script = f.read()

    conn = wait_for_singlestore()
    with conn.cursor() as cur:
        for stmt in parse_sql_script(sql_script):
            try:
                cur.execute(stmt)
            except pymysql.MySQLError as e:
                # 1022: Role already exists, 1920: Role already assigned, 1917: User already in group
                if e.args[0] in (1022, 1920, 1917):
                    continue
                raise

        cur.execute("SELECT count(*) FROM _olist_master.orders;")
        existing_master_orders = cur.fetchone()[0]

        if force_reload_archive or existing_master_orders == 0:
            print("  -> Loading 5 Olist CSVs into _olist_master via LOAD DATA LOCAL INFILE (streamed from the host)...")
            csv_tables = [
                ("olist_customers_dataset.csv", "_olist_master.customers"),
                ("olist_orders_dataset.csv", "_olist_master.orders"),
                ("olist_order_payments_dataset.csv", "_olist_master.order_payments"),
                ("olist_order_reviews_dataset.csv", "_olist_master.order_reviews"),
                ("olist_order_items_dataset.csv", "_olist_master.order_items"),
            ]
            t0 = time.time()
            for csv_file, table_name in csv_tables:
                cur.execute(f"TRUNCATE TABLE {table_name};")
                cur.execute(f"""
                    LOAD DATA LOCAL INFILE '{os.path.join(OLIST_ARCHIVE_DIR, csv_file)}'
                    INTO TABLE {table_name}
                    FIELDS TERMINATED BY ',' ENCLOSED BY '\"'
                    LINES TERMINATED BY '\\n'
                    IGNORE 1 LINES
                    NULL DEFINED BY '';
                """)
            elapsed = time.time() - t0
            cur.execute(
                "SELECT (SELECT count(*) FROM _olist_master.customers) + "
                "(SELECT count(*) FROM _olist_master.orders) + "
                "(SELECT count(*) FROM _olist_master.order_payments) + "
                "(SELECT count(*) FROM _olist_master.order_reviews) + "
                "(SELECT count(*) FROM _olist_master.order_items);"
            )
            total_rows = cur.fetchone()[0]
            print(f"  -> Loaded {total_rows:,} rows across 5 tables into _olist_master in {elapsed:.2f}s.")
        else:
            print(f"  -> _olist_master already populated ({existing_master_orders:,} orders).")

        # Clean target model databases so re-seeding leaves no orphaned tables
        for target_db in ("edw_dv_bdv", "im_dimensional", "pres_dataproduct", "vdp_ci"):
            cur.execute(
                "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = %s;",
                (target_db,),
            )
            for tbl_name, tbl_type in cur.fetchall():
                if tbl_type == "VIEW":
                    cur.execute(f"DROP VIEW IF EXISTS `{target_db}`.`{tbl_name}`;")
                else:
                    cur.execute(f"DROP TABLE IF EXISTS `{target_db}`.`{tbl_name}`;")

        # Studio schema (channel fault-injection tables + trickle procedures), then healthy channels
        print("  -> Applying Workshop Studio channel schema and resetting channel config...")
        apply_studio_schema(cur)
        reset_channel_config(cur)

        # Reset and trickle initial baseline into analytics_dev (Domain A + Domain B)
        # and analytics_prod (Domain A only, Domain B reviews withheld!)
        print(f"  -> Resetting analytics_dev landing tables and trickling {initial_trickle_days} days (Domain A + Domain B)...")
        cur.execute("USE analytics_dev;")
        cur.execute(f"CALL analytics_dev.sp_reset_olist_landing({int(initial_trickle_days)}, TRUE);")

        print(f"  -> Resetting analytics_prod landing tables and trickling {initial_trickle_days} days (Domain A only; Domain B withheld)...")
        cur.execute("USE analytics_prod;")
        cur.execute(f"CALL analytics_prod.sp_reset_olist_landing({int(initial_trickle_days)}, FALSE);")

        for env_db in ("analytics_dev", "analytics_prod"):
            cur.execute(
                f"SELECT "
                f"(SELECT count(*) FROM {env_db}.raw_olist_orders), "
                f"(SELECT count(*) FROM {env_db}.raw_olist_order_payments), "
                f"(SELECT count(*) FROM {env_db}.raw_olist_order_reviews), "
                f"(SELECT current_watermark FROM {env_db}._trickle_state LIMIT 1);"
            )
            o_cnt, p_cnt, r_cnt, wm = cur.fetchone()
            print(f"     [{env_db}] watermark={wm} | orders={o_cnt:,} | payments(A)={p_cnt:,} | reviews(B)={r_cnt:,}")

    conn.close()
    print("  -> SingleStore VDAB enterprise topology and Olist trickle landing zones configured.")


def wait_for_gitlab(timeout_seconds=360):
    print("  -> Waiting for GitLab services to be ready...")
    start_time = time.time()
    while time.time() - start_time < timeout_seconds:
        try:
            status = subprocess.check_output(
                ["docker", "inspect", "-f", "{{.State.Status}}", GITLAB_CONTAINER],
                text=True
            ).strip()
            if status == "running":
                res = requests.get(f"{GITLAB_URL}/users/sign_in", timeout=3)
                if res.status_code in (200, 302):
                    test_run = subprocess.run(
                        ["docker", "exec", GITLAB_CONTAINER, "gitlab-rails", "runner", "puts 'ready'"],
                        capture_output=True,
                        text=True,
                        timeout=60
                    )
                    if test_run.returncode == 0:
                        print("  -> GitLab Rails is ready.")
                        return
        except Exception:
            pass
        elapsed = int(time.time() - start_time)
        if elapsed > 0 and elapsed % 15 == 0:
            print(f"     Still waiting for GitLab... ({elapsed}s elapsed)")
        time.sleep(5)
    raise TimeoutError("GitLab failed to become ready within the timeout period.")


SIGNING_KEY_RUBY = """
s = ApplicationSetting.current
if s.respond_to?(:ci_job_token_signing_key) && s.ci_job_token_signing_key.blank?
  s.update!(ci_job_token_signing_key: OpenSSL::PKey::RSA.new(2048).to_pem)
  puts 'SIGNING_KEY_SET'
else
  puts 'SIGNING_KEY_OK'
end
"""


def ensure_ci_job_token_signing_key():
    """Recent GitLab images do not initialise the CI job-token signing key when root is created by script; without it
    every runner job fails with scheduler_failure. bootstrap_gitlab_rails sets it on a fresh stack; reset_workshop.py
    calls this so an existing stack picks it up too. Idempotent; a warning (not a failure) if GitLab is not reachable."""
    proc = subprocess.run(["docker", "exec", GITLAB_CONTAINER, "gitlab-rails", "runner", SIGNING_KEY_RUBY],
                          capture_output=True, text=True)
    if proc.returncode != 0:
        print(f"  !! Could not check the GitLab CI job-token signing key: {proc.stderr.strip()[-200:]}")
        return
    print("  -> GitLab CI job-token signing key " + ("set" if "SIGNING_KEY_SET" in proc.stdout else "already present"))


def bootstrap_gitlab_rails():
    print("[2/5] Creating GitLab Root PAT and Airflow OAuth App via gitlab-rails...")
    wait_for_gitlab()
    rails_script = """
    org = Organizations::Organization.first || Organizations::Organization.default_organization
    user = User.find_by_username('root')
    if user.nil?
      user = User.new(
        name: 'Administrator',
        username: 'root',
        email: 'admin@example.com',
        admin: true,
        password: 'WorkshopRootPwd123!',
        password_confirmation: 'WorkshopRootPwd123!'
      )
      user.assign_personal_namespace(org) if user.respond_to?(:assign_personal_namespace)
      user.skip_confirmation!
      user.save!(validate: false)
    end

    # Recent GitLab images do not initialise the CI job-token signing key when root is created here; without it
    # every runner job fails with scheduler_failure ("CI job token signing key is not set").
    settings = ApplicationSetting.current
    if settings.respond_to?(:ci_job_token_signing_key) && settings.ci_job_token_signing_key.blank?
      settings.update!(ci_job_token_signing_key: OpenSSL::PKey::RSA.new(2048).to_pem)
    end

    user.personal_access_tokens.where(name: 'bootstrap_pat').destroy_all
    token = user.personal_access_tokens.create!(
      name: 'bootstrap_pat',
      scopes: [:api, :read_user, :write_repository],
      expires_at: 30.days.from_now
    )
    token.set_token('glpat-workshop-root-token-2026')
    token.save!

    app = Doorkeeper::Application.find_or_initialize_by(name: 'Airflow Workshop SSO')
    app.redirect_uri = "http://localhost:__LDV_PORT__/oauth-authorized/gitlab\\nhttp://localhost:__PRD_PORT__/oauth-authorized/gitlab"
    app.scopes = 'read_user openid profile email'
    app.confidential = true
    app.organization_id = org.id if app.respond_to?(:organization_id=)
    app.save!
    puts({ pat: 'glpat-workshop-root-token-2026', client_id: app.uid, client_secret: app.secret }.to_json)
    """
    rails_script = rails_script.replace("__LDV_PORT__", AIRFLOW_LDV_PORT).replace("__PRD_PORT__", AIRFLOW_PRD_PORT)
    out = subprocess.check_output(
        ["docker", "exec", GITLAB_CONTAINER, "gitlab-rails", "runner", rails_script],
        text=True
    )
    creds = json.loads(out.strip().splitlines()[-1])
    with open(os.path.join(COMPOSE_DIR, ".env"), "w") as f:
        f.write(f"GITLAB_CLIENT_ID={creds['client_id']}\n")
        f.write(f"GITLAB_CLIENT_SECRET={creds['client_secret']}\n")
    print("  -> OAuth credentials written to compose/.env")
    return creds["pat"]


def _git_commit_as(repo_dir, name, email, message):
    subprocess.run(["git", "add", "-A"], cwd=repo_dir, check=True)
    env = os.environ.copy()
    env["GIT_AUTHOR_NAME"] = name
    env["GIT_AUTHOR_EMAIL"] = email
    env["GIT_COMMITTER_NAME"] = name
    env["GIT_COMMITTER_EMAIL"] = email
    subprocess.run(
        ["git", "commit", "-m", message],
        cwd=repo_dir,
        env=env,
        check=True,
        stdout=subprocess.DEVNULL,
    )


def _push_local_project_to_gitlab(source_dir, remote_url, create_release_tags=False):
    """Initialize a clean Git repo from source_dir and push Modified GitFlow branches/tags to GitLab."""
    with tempfile.TemporaryDirectory() as tmpdir:
        repo_dir = os.path.join(tmpdir, "repo")
        shutil.copytree(
            source_dir,
            repo_dir,
            ignore=shutil.ignore_patterns(".git", "target", "dbt_packages", "logs", "artifacts", "__pycache__"),
        )
        subprocess.run(["git", "init", "-b", "main"], cwd=repo_dir, check=True, stdout=subprocess.DEVNULL)
        subprocess.run(["git", "config", "user.email", "admin@vdab.be"], cwd=repo_dir, check=True)
        subprocess.run(["git", "config", "user.name", "VDAB Workshop Bootstrap"], cwd=repo_dir, check=True)

        _git_commit_as(
            repo_dir,
            "VDAB Workshop Bootstrap",
            "admin@vdab.be",
            "feat(core): initial VDAB enterprise project and Olist dimensional baseline",
        )
        subprocess.run(["git", "branch", "-M", "main"], cwd=repo_dir, check=True)

        if create_release_tags:
            # Tag initial baseline on main; 'dev' starts at the same commit. Feature work is done by participants.
            subprocess.run(["git", "tag", "-a", "v1.0.0", "-m", "Initial baseline v1.0.0"], cwd=repo_dir, check=True)
            subprocess.run(["git", "tag", "-a", "2026_DROP1.0", "-m", "Initial production drop 2026_DROP1.0"], cwd=repo_dir, check=True)
            subprocess.run(["git", "branch", "dev", "main"], cwd=repo_dir, check=True)

            # Initialize orphan 'changelog' branch
            subprocess.run(["git", "checkout", "--orphan", "changelog"], cwd=repo_dir, check=True, stdout=subprocess.DEVNULL)
            subprocess.run(["git", "rm", "-rf", "."], cwd=repo_dir, check=True, stdout=subprocess.DEVNULL)
            with open(os.path.join(repo_dir, "CHANGELOG.md"), "w") as f:
                f.write(
                    "# Changelog\n\n"
                    "## v1.0.0\n"
                    "- feat(core): initial VDAB enterprise project and Olist dimensional baseline\n"
                )
            _git_commit_as(
                repo_dir,
                "GitLab CI",
                "gitlab-ci@vdab.be",
                "chore(changelog): initialize CHANGELOG.md for v1.0.0 [skip ci]",
            )
            subprocess.run(["git", "checkout", "dev"], cwd=repo_dir, check=True, stdout=subprocess.DEVNULL)
        else:
            subprocess.run(["git", "checkout", "-b", "dev"], cwd=repo_dir, check=True, stdout=subprocess.DEVNULL)

        subprocess.run(["git", "remote", "add", "origin", remote_url], cwd=repo_dir, check=True)
        subprocess.run(
            ["git", "push", "--force", "--all", "-o", "ci.skip", "origin"],
            cwd=repo_dir,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if create_release_tags:
            subprocess.run(
                ["git", "push", "--force", "--tags", "-o", "ci.skip", "origin"],
                cwd=repo_dir,
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )


def _cleanup_remote_branches_and_tags(pid, headers):
    """Remove any leftover release/* branches or extra tags in GitLab before force-pushing."""
    for b in requests.get(f"{GITLAB_URL}/api/v4/projects/{pid}/repository/branches?per_page=100", headers=headers).json():
        bname = b.get("name", "")
        if bname not in ("main", "dev", "changelog"):
            enc = urllib.parse.quote(bname, safe="")
            requests.delete(f"{GITLAB_URL}/api/v4/projects/{pid}/repository/branches/{enc}", headers=headers)

    for t in requests.get(f"{GITLAB_URL}/api/v4/projects/{pid}/repository/tags?per_page=100", headers=headers).json():
        tname = t.get("name", "")
        if tname not in ("2026_DROP1.0", "v1.0.0"):
            enc = urllib.parse.quote(tname, safe="")
            requests.delete(f"{GITLAB_URL}/api/v4/projects/{pid}/repository/tags/{enc}", headers=headers)


def seed_gitlab_api(pat, register_runner=True):
    print("[3/5] Provisioning GitLab Groups, Users, VDAB Project (dbt-vdp), and Modified GitFlow Rules...")
    headers = {"PRIVATE-TOKEN": pat}

    # 1. Create or update parent 'workshop' group (visibility='public' so dbt deps can clone over HTTP)
    r = requests.post(f"{GITLAB_URL}/api/v4/groups", headers=headers, json={
        "name": "Workshop", "path": "workshop", "visibility": "public"
    })
    parent_id = r.json().get("id") or requests.get(f"{GITLAB_URL}/api/v4/groups/workshop", headers=headers).json()["id"]
    requests.put(f"{GITLAB_URL}/api/v4/groups/{parent_id}", headers=headers, json={"visibility": "public"})

    # Set GITLAB_PUSH_TOKEN at group level so CI release jobs can push tags and trigger downstream tag pipelines
    requests.post(f"{GITLAB_URL}/api/v4/groups/{parent_id}/variables", headers=headers, json={
        "key": "GITLAB_PUSH_TOKEN", "value": pat, "protected": False, "masked": False
    })
    requests.put(f"{GITLAB_URL}/api/v4/groups/{parent_id}/variables/GITLAB_PUSH_TOKEN", headers=headers, json={
        "value": pat, "protected": False, "masked": False
    })

    # 2. Create the 4 role subgroups (used by OIDC for Airflow role mapping)
    subgroup_ids = {}
    for sg in ["admins", "maintainers", "users", "viewers"]:
        res = requests.post(f"{GITLAB_URL}/api/v4/groups", headers=headers, json={
            "name": sg.capitalize(), "path": sg, "parent_id": parent_id, "visibility": "public"
        })
        subgroup_ids[sg] = res.json().get("id") or requests.get(f"{GITLAB_URL}/api/v4/groups/workshop%2F{sg}", headers=headers).json()["id"]

    # 3. Create 'dbt-vdp' inside 'workshop' group
    projects_cfg = [
        {
            "name": "dbt-vdp",
            "source_dir": DBT_PROJECT_DIR,
            "create_release_tags": True,
        },
    ]

    project_ids = {}
    for pcfg in projects_cfg:
        pname = pcfg["name"]
        proj_res = requests.post(f"{GITLAB_URL}/api/v4/projects", headers=headers, json={
            "name": pname,
            "path": pname,
            "namespace_id": parent_id,
            "visibility": "public",
            "initialize_with_readme": False,
            "default_branch": "dev",
            "squash_option": "always",
        })
        pid = proj_res.json().get("id") or requests.get(
            f"{GITLAB_URL}/api/v4/projects/workshop%2F{pname}", headers=headers
        ).json()["id"]
        requests.put(
            f"{GITLAB_URL}/api/v4/projects/{pid}",
            headers=headers,
            json={
                "visibility": "public",
                "default_branch": "dev",
                "squash_option": "always",
            },
        )
        project_ids[pname] = pid

        # Unprotect branches before force-pushing initial repository state
        for bname in ("main", "dev", "changelog", "release/*"):
            enc = urllib.parse.quote(bname, safe="")
            requests.delete(f"{GITLAB_URL}/api/v4/projects/{pid}/protected_branches/{enc}", headers=headers)

        if pcfg["create_release_tags"]:
            _cleanup_remote_branches_and_tags(pid, headers)

        print(f"  -> Pushing {pname} repository and Modified GitFlow branches to GitLab...")
        scheme, host_part = GITLAB_URL.split("://", 1)
        remote_url = f"{scheme}://root:{pat}@{host_part}/workshop/{pname}.git"
        _push_local_project_to_gitlab(
            source_dir=pcfg["source_dir"],
            remote_url=remote_url,
            create_release_tags=pcfg["create_release_tags"],
        )

        # Configure Protected Branches for Modified GitFlow:
        # 'main'      -> Merge: Maintainers (40), Push: Maintainers (40), allow_force_push: True (for merge-base reset + --no-ff merge)
        # 'dev'       -> Merge: Developers+Maintainers (30), Push: Maintainers (40), allow_force_push: False
        # 'release/*' -> Merge: Maintainers (40), Push: Maintainers (40), allow_force_push: True
        requests.post(f"{GITLAB_URL}/api/v4/projects/{pid}/protected_branches", headers=headers, json={
            "name": "main", "push_access_level": 40, "merge_access_level": 40, "allow_force_push": True
        })
        requests.post(f"{GITLAB_URL}/api/v4/projects/{pid}/protected_branches", headers=headers, json={
            "name": "dev", "push_access_level": 40, "merge_access_level": 30, "allow_force_push": False
        })
        requests.post(f"{GITLAB_URL}/api/v4/projects/{pid}/protected_branches", headers=headers, json={
            "name": "release/*", "push_access_level": 40, "merge_access_level": 40, "allow_force_push": True
        })

    # 4. Create the 7 Users and assign to both the subgroup (for OIDC) and projects (for Git RBAC)
    for u in USERS:
        u_res = requests.post(f"{GITLAB_URL}/api/v4/users", headers=headers, json={
            "username": u["username"],
            "name": u["name"],
            "email": f"{u['username']}@local.internal",
            "password": SHARED_USER_PWD,
            "admin": u["is_admin"],
            "skip_confirmation": True
        })
        user_id = u_res.json().get("id") or requests.get(
            f"{GITLAB_URL}/api/v4/users?username={u['username']}", headers=headers
        ).json()[0]["id"]

        requests.post(f"{GITLAB_URL}/api/v4/groups/{subgroup_ids[u['subgroup']]}/members", headers=headers, json={
            "user_id": user_id, "access_level": u["access_level"]
        })
        for pid in project_ids.values():
            requests.post(f"{GITLAB_URL}/api/v4/projects/{pid}/members", headers=headers, json={
                "user_id": user_id, "access_level": u["access_level"]
            })

    if not register_runner:
        return None

    # 5. Create instance runner registration token via API
    runner_res = requests.post(f"{GITLAB_URL}/api/v4/user/runners", headers=headers, json={
        "runner_type": "instance_type",
        "run_untagged": True,
        "tag_list": ["docker", "linux"],
        "description": "local-vdab-dbt-runner"
    })
    return runner_res.json()["token"]


def build_and_register_runner(runner_token):
    print("[4/5] Building VDAB dbt-singlestore-base image and configuring GitLab Runner...")
    base_img_dir = os.path.join(WORKSHOP_ROOT, "dbt-singlestore-base-image")
    subprocess.run(["docker", "build", "-t", BASE_IMAGE_NAME, base_img_dir], check=True)
    for tag in ENTERPRISE_IMAGE_TAGS:
        subprocess.run(["docker", "tag", BASE_IMAGE_NAME, tag], check=True)

    # Reset existing runner config to avoid duplicate inactive runners
    subprocess.run(
        ["docker", "exec", RUNNER_CONTAINER, "sh", "-c", "gitlab-runner unregister --all-runners || true"],
        check=False,
    )
    subprocess.run([
        "docker", "exec", RUNNER_CONTAINER, "gitlab-runner", "register",
        "--non-interactive",
        "--url", "http://gitlab.local",
        "--token", runner_token,
        "--executor", "docker",
        "--docker-image", "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-singlestore-base:1.11.11",
        "--docker-network-mode", "workshop_net",
        "--docker-pull-policy", "if-not-present",
        # the deploy job (.deploy-airflow) writes /workshop_state/<env>: the LDV/PRD workspace volumes
        *[a for env, vol in WORKSHOP_STATE_VOLUMES.items() for a in ("--docker-volumes", f"{vol}:/workshop_state/{env}")],
        "--docker-volumes", "/var/run/docker.sock:/var/run/docker.sock",
    ], check=True)
    print("  -> Runner registered with VDAB base image and attached to workshop_net.")


def initialize_workshop_deployments():
    print("[5/5] Executing initial dbt-vdp build and syncing PRD (2026_DROP1.0) & LDV (v1.0.0) Airflow state...")
    # Sync PRD first (baseline main @ 2026_DROP1.0), then LDV (dev @ v1.0.0)
    for target_env, git_ref, git_tag, s2_user in [
        ("PRD", "main", "2026_DROP1.0", "svc_prd_dbt"),
        ("LDV", "dev", "v1.0.0", "svc_ldv_dbt"),
    ]:
        print(f"  -> Syncing {target_env} environment (branch={git_ref}, tag={git_tag}, user={s2_user})...")
        cmd = [
            "docker", "run", "--rm",
            "--user", "root",
            "--network", "workshop_net",
            "-e", f"DBT_DEPLOY_TARGET={target_env}",
            "-e", "SINGLESTORE_HOST=singlestore",
            "-e", f"SINGLESTORE_USER={s2_user}",
            "-e", "SINGLESTORE_PASS=CiServicePwd2026!",
            "-e", "LOG_RUN_RESULTS=TRUE",
            *WORKSHOP_STATE_MOUNTS,
            BASE_IMAGE_NAME,
            "bash", "-c",
            "set -e; git config --global --add safe.directory '*' && "
            f"git clone -b {git_ref} http://gitlab.local/workshop/dbt-vdp.git /tmp/dbt-vdp && "
            "cd /tmp/dbt-vdp && "
            "dbt deps && "
            "dbt build --full-refresh --target run && "
            f"git checkout {git_tag} && "
            "dbt compile --target run --target-path artifacts && "
            # static docs site for dbt-docs (8081/8082), as the CI deploy job (.deploy-airflow) does. dbt Fusion writes
            # a complete site (index.html + assets/) to --output-dir; it has no --static flag
            "(dbt docs generate --target run --output-dir target/dbt-docs || true) && "
            "for e in dev prod; do "
            "mkdir -p /workshop_state/$e/{dags/utils,dbt,docs,mnt_dbt/target,mnt_dbt/artifacts} && "
            "rsync -rlptD --delete --exclude=__pycache__ --exclude=utils airflow/dags/ /workshop_state/$e/dags/ && "
            "rsync -rlptD --delete --exclude=__pycache__ airflow/utils/ /workshop_state/$e/dags/utils/ && "
            "rsync -rlptD --delete --exclude=__pycache__ ./ /workshop_state/$e/dbt/ && "
            "if [ -f target/dbt-docs/index.html ]; then rsync -rlD --delete target/dbt-docs/ /workshop_state/$e/docs/; fi; done && "
            "chmod -R a+rwX /workshop_state/dev /workshop_state/prod"
        ]
        subprocess.run(cmd, check=True)

    print("  -> Initial PRD (2026_DROP1.0) and LDV (v1.0.0) state synced and materialized in SingleStore.")


def create_baseline_milestone():
    """Snapshot the seeded warehouse as milestone 'baseline' (stops SingleStore for a few seconds). The studio Reset
    restores it: landing tables, model tables, run log, users and grants exactly as after this bootstrap/reset."""
    print(f"  -> Creating SingleStore milestone '{BASELINE_MILESTONE}' (SingleStore restarts once)...")
    subprocess.run(["bash", MILESTONES_SCRIPT, "create", BASELINE_MILESTONE], check=True)


def main():
    parser = argparse.ArgumentParser(description="Bootstrap or reset the VDAB Enterprise dbt Workshop Stack.")
    parser.add_argument(
        "--reset-only",
        action="store_true",
        help="Fast workshop reset: re-seed SingleStore landing zones, reset GitLab branches/tags, and re-sync LDV/PRD without rebuilding Docker images.",
    )
    parser.add_argument(
        "--initial-days",
        type=int,
        default=90,
        help="Initial number of Olist days to trickle into analytics_dev and analytics_prod (default: 90).",
    )
    args = parser.parse_args()

    seed_singlestore(force_reload_archive=False, initial_trickle_days=args.initial_days)
    if args.reset_only:
        seed_gitlab_api(ROOT_PAT, register_runner=False)
        initialize_workshop_deployments()
        create_baseline_milestone()
        print("\nWorkshop state reset complete! SingleStore, GitLab branches/tags, and Airflow deployments are back at the starting state.")
        return

    pat = bootstrap_gitlab_rails()
    runner_token = seed_gitlab_api(pat, register_runner=True)
    build_and_register_runner(runner_token)
    initialize_workshop_deployments()
    create_baseline_milestone()
    print("\nRestarting Airflow containers to pick up OAuth .env credentials and synced DAGs...")
    subprocess.run(
        [*COMPOSE_CMD, "up", "-d", "--force-recreate", "airflow-dev", "airflow-prod", "dbt-docs"],
        check=True,
    )
    print("\nVDAB Enterprise dbt Workshop Stack is ready! Shared Password for all 7 users: WorkshopPwd2026!")


if __name__ == "__main__":
    main()

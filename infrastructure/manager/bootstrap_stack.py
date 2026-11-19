#!/usr/bin/env python3
"""
Bootstrap of the dbt x SingleStore workshop stack (runs in the manager container: `make bootstrap`).

1. SingleStore: per environment (dev, val, prd) the databases, roles, service user and empty landing tables
   (singlestore/init_env.sql.tmpl), then the global part (singlestore/init_global.sql): vdp_ci, CI user, developers, admin.
2. GitLab: root token, the Airflow OAuth application (credentials written to compose/.env), group/project 'workshop/dbt-vdp',
   one maintainer and three developers (pu_dev1..3), protected branches dev and main.
3. Images: dbt-singlestore-base:1.11.11 and dbt-singlestore-dev:1.11.11; the GitLab runner (docker executor, docker socket).
4. The dbt project (../../dbt-singlestore-workshop) is pushed with branches dev and main plus the first tag v1.0.0;
   the tag pipeline builds the dbt-vdp image and deploys it to dev. Bootstrap waits for that pipeline and reports it.
5. Workshop Studio: it starts with the stack, before any of the above exists, so its first baseline check fails. Bootstrap
   ends by resetting the studio (`studio.reset --wait`), which stops that check and runs a new one, and waits for it.
"""

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
    sys.exit("Missing deps (PyMySQL, requests). Run it in the manager container instead: make bootstrap")

MANAGER_DIR = os.path.dirname(os.path.abspath(__file__))            # infrastructure/manager/: this script
WORKSHOP_ROOT = os.path.dirname(MANAGER_DIR)                         # infrastructure/: one folder per service
COMPOSE_DIR = os.path.join(WORKSHOP_ROOT, "compose")                 # docker-compose.yml + .env (OAuth credentials)
COMPOSE_CMD = ["docker", "compose", "--project-directory", COMPOSE_DIR, "-f", os.path.join(COMPOSE_DIR, "docker-compose.yml")]
SQL_DIR = os.path.join(WORKSHOP_ROOT, "singlestore")
REPO_ROOT = os.path.dirname(WORKSHOP_ROOT)
DBT_PROJECT_DIR = os.path.join(REPO_ROOT, "dbt-singlestore-workshop")

ENVS = ("dev", "val", "prd")
ENV_DATABASES = ("landing", "edw_dv_rdv", "edw_dv_bdv", "im_dimensional", "vdp_mgmt")

# Defaults = run on the host against the published ports. The manager container (docker-compose.yml) sets
# GITLAB_URL=http://gitlab.local, S2_HOST=singlestore, S2_PORT=3306: it talks to the stack over workshop_net.
GITLAB_URL = os.environ.get("GITLAB_URL", "http://localhost")
S2_HOST = os.environ.get("S2_HOST", "127.0.0.1")
S2_PORT = int(os.environ.get("S2_PORT", os.environ.get("WORKSHOP_S2_PORT", "23306")))
# Containers are named workshop-<service> (docker-compose.yml): the stack never collides with other projects
GITLAB_CONTAINER = os.environ.get("GITLAB_CONTAINER", "workshop-gitlab")
RUNNER_CONTAINER = os.environ.get("RUNNER_CONTAINER", "workshop-gitlab-runner")
STUDIO_CONTAINER = os.environ.get("STUDIO_CONTAINER", "workshop-studio")
STUDIO_READY_TIMEOUT = 600          # seconds the studio gets to rebuild its baseline (about 70 s when all is empty)
STUDIO_RESET_CMD = f"docker exec {STUDIO_CONTAINER} python -m studio.reset --wait {STUDIO_READY_TIMEOUT}"
AIRFLOW_PORTS = {
    "dev": os.environ.get("WORKSHOP_AIRFLOW_DEV_PORT", "8088"),
    "val": os.environ.get("WORKSHOP_AIRFLOW_VAL_PORT", "8089"),
    "prd": os.environ.get("WORKSHOP_AIRFLOW_PRD_PORT", "8090"),
}
S2_ROOT_PWD = os.environ.get("S2_ROOT_PWD", "WorkshopRootPwd123!")
SHARED_USER_PWD = "WorkshopPwd2026!"
ROOT_PAT = "glpat-workshop-root-token-2026"

BASE_IMAGE_NAME = "dbt-singlestore-base:1.11.11"
DEV_IMAGE_NAME = "dbt-singlestore-dev:1.11.11"
PROJECT_PATH = "workshop/dbt-vdp"
FIRST_TAG = "v1.0.0"
GIT_IDENT = ("Workshop Bootstrap", "bootstrap@workshop.local")

# GitLab users: the subgroup drives the Airflow role (webserver_config.py), the access level the Git rights
USERS = [
    {"username": "maintainer1", "name": "Maintainer One", "subgroup": "maintainers", "access_level": 40},
    {"username": "pu_dev1", "name": "Developer One", "subgroup": "users", "access_level": 30},
    {"username": "pu_dev2", "name": "Developer Two", "subgroup": "users", "access_level": 30},
    {"username": "pu_dev3", "name": "Developer Three", "subgroup": "users", "access_level": 30},
]


# --------------------------------------------------------------------------------------------------------------------
# SingleStore
# --------------------------------------------------------------------------------------------------------------------

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


def wait_for_singlestore(timeout_seconds=300):
    """A fresh singlestoredb-dev container accepts connections before it has set ROOT_PASSWORD (1045 Access denied)
    and refuses them while it starts (2003): retry until root can log in."""
    start = time.time()
    while True:
        try:
            return pymysql.connect(host=S2_HOST, port=S2_PORT, user="root", password=S2_ROOT_PWD, autocommit=True,
                                   connect_timeout=5)
        except pymysql.err.OperationalError as e:
            if time.time() - start > timeout_seconds:
                raise TimeoutError(f"SingleStore at {S2_HOST}:{S2_PORT} not ready after {timeout_seconds}s: {e}") from e
            print(f"  -> Waiting for SingleStore at {S2_HOST}:{S2_PORT} ({e.args[0]})...")
            time.sleep(5)


def run_script(cur, script):
    for stmt in parse_sql_script(script):
        try:
            cur.execute(stmt)
        except pymysql.MySQLError as e:
            # 1022: role/group already exists, 1920: role already assigned, 1917: user already in group
            if e.args[0] in (1022, 1920, 1917):
                continue
            raise


def apply_sql(cur):
    """Idempotent: init_env.sql.tmpl once per environment, then init_global.sql (it needs the roles of all three)."""
    with open(os.path.join(SQL_DIR, "init_env.sql.tmpl")) as f:
        template = f.read()
    for env in ENVS:
        run_script(cur, template.replace("__ENV__", env))
    with open(os.path.join(SQL_DIR, "init_global.sql")) as f:
        run_script(cur, f.read())


def seed_singlestore():
    print("[1/4] Creating the SingleStore databases, roles, users and landing tables (dev, val, prd)...")
    conn = wait_for_singlestore()
    with conn.cursor() as cur:
        apply_sql(cur)
    conn.close()
    print("  -> SingleStore ready.")


# --------------------------------------------------------------------------------------------------------------------
# GitLab
# --------------------------------------------------------------------------------------------------------------------

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
    print("[2/4] Creating GitLab Root PAT and Airflow OAuth App via gitlab-rails...")
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

    # Airflow 3 serves the FAB auth manager under /auth, so the callback is /auth/oauth-authorized/<provider>.
    # GitLab only accepts an exact match ("The redirect URI included is not valid"). Both localhost and 127.0.0.1 are
    # registered because the callback uses whatever host the browser used.
    app = Doorkeeper::Application.find_or_initialize_by(name: 'Airflow Workshop SSO')
    app.redirect_uri = "__REDIRECT_URIS__"
    app.scopes = 'read_user openid profile email'
    app.confidential = true
    app.organization_id = org.id if app.respond_to?(:organization_id=)
    # GitLab stores application secrets hashed, so an existing app's secret cannot be read back: renew it every run
    # and write the plaintext, which is only available right after renewing.
    app.renew_secret
    app.save!
    secret = app.respond_to?(:plaintext_secret) && app.plaintext_secret ? app.plaintext_secret : app.secret
    puts({ pat: 'glpat-workshop-root-token-2026', client_id: app.uid, client_secret: secret }.to_json)
    """
    redirect_uris = "\\n".join(
        f"http://{host}:{port}/auth/oauth-authorized/gitlab"
        for port in AIRFLOW_PORTS.values()
        for host in ("localhost", "127.0.0.1")
    )
    rails_script = rails_script.replace("__REDIRECT_URIS__", redirect_uris)
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


def _git(repo_dir, *args, **kwargs):
    env = {**os.environ, "GIT_AUTHOR_NAME": GIT_IDENT[0], "GIT_AUTHOR_EMAIL": GIT_IDENT[1],
           "GIT_COMMITTER_NAME": GIT_IDENT[0], "GIT_COMMITTER_EMAIL": GIT_IDENT[1]}
    return subprocess.run(["git", *args], cwd=repo_dir, env=env, check=True, stdout=subprocess.DEVNULL, **kwargs)


def push_project(remote_url):
    """Fresh single-commit repo from the dbt project: branches main and dev at the same commit, tag FIRST_TAG on dev.
    The branches go with ci.skip; the tag push starts the pipeline that builds dbt-vdp:<tag> and deploys it to dev."""
    with tempfile.TemporaryDirectory() as tmpdir:
        repo_dir = os.path.join(tmpdir, "repo")
        shutil.copytree(
            DBT_PROJECT_DIR,
            repo_dir,
            ignore=shutil.ignore_patterns(".git", "target", "dbt_packages", "logs", "artifacts", "__pycache__"),
        )
        _git(repo_dir, "init", "-b", "main")
        _git(repo_dir, "add", "-A")
        _git(repo_dir, "commit", "-m", "feat(core): initial dbt project baseline")
        _git(repo_dir, "branch", "dev", "main")
        _git(repo_dir, "tag", "-a", FIRST_TAG, "dev", "-m", f"Initial baseline {FIRST_TAG}")
        _git(repo_dir, "remote", "add", "origin", remote_url)
        _git(repo_dir, "push", "--force", "--all", "-o", "ci.skip", "origin", stderr=subprocess.DEVNULL)
        _git(repo_dir, "push", "--force", "origin", FIRST_TAG, stderr=subprocess.DEVNULL)


def _delete_leftover_refs(pid, headers):
    """Remove every branch except main/dev and every tag in GitLab before the baseline is force-pushed."""
    for b in requests.get(f"{GITLAB_URL}/api/v4/projects/{pid}/repository/branches?per_page=100", headers=headers).json():
        if b["name"] not in ("main", "dev"):
            enc = urllib.parse.quote(b["name"], safe="")
            requests.delete(f"{GITLAB_URL}/api/v4/projects/{pid}/repository/branches/{enc}", headers=headers)
    for t in requests.get(f"{GITLAB_URL}/api/v4/projects/{pid}/repository/tags?per_page=100", headers=headers).json():
        enc = urllib.parse.quote(t["name"], safe="")
        requests.delete(f"{GITLAB_URL}/api/v4/projects/{pid}/repository/tags/{enc}", headers=headers)


def seed_gitlab(pat, register_runner=True):
    """Group, project, users, CI variable, baseline push. Returns (runner_token or None, project id)."""
    print("[3/4] Provisioning GitLab group, users, project and protected branches...")
    headers = {"PRIVATE-TOKEN": pat}
    api = f"{GITLAB_URL}/api/v4"

    # 'workshop' group (public so dbt deps can clone over HTTP) and the role subgroups the Airflow SSO maps.
    # root creates them and is therefore their owner: through `admins` root signs in to every Airflow as Admin.
    r = requests.post(f"{api}/groups", headers=headers, json={"name": "Workshop", "path": "workshop", "visibility": "public"})
    parent_id = r.json().get("id") or requests.get(f"{api}/groups/workshop", headers=headers).json()["id"]
    requests.put(f"{api}/groups/{parent_id}", headers=headers, json={"visibility": "public"})
    subgroup_ids = {}
    for sg in ("admins", "maintainers", "users"):
        res = requests.post(f"{api}/groups", headers=headers, json={
            "name": sg.capitalize(), "path": sg, "parent_id": parent_id, "visibility": "public"
        })
        subgroup_ids[sg] = res.json().get("id") or requests.get(f"{api}/groups/workshop%2F{sg}", headers=headers).json()["id"]

    # The CI release jobs push tags with this token (a tag pushed with the job token would not start a pipeline)
    requests.post(f"{api}/groups/{parent_id}/variables", headers=headers, json={
        "key": "GITLAB_PUSH_TOKEN", "value": pat, "protected": False, "masked": False
    })
    requests.put(f"{api}/groups/{parent_id}/variables/GITLAB_PUSH_TOKEN", headers=headers, json={
        "value": pat, "protected": False, "masked": False
    })

    # Project dbt-vdp, default branch dev, squash on merge
    settings = {"visibility": "public", "default_branch": "dev", "squash_option": "always"}
    proj_res = requests.post(f"{api}/projects", headers=headers, json={
        "name": "dbt-vdp", "path": "dbt-vdp", "namespace_id": parent_id, "initialize_with_readme": False, **settings
    })
    pid = proj_res.json().get("id") or requests.get(f"{api}/projects/{urllib.parse.quote(PROJECT_PATH, safe='')}", headers=headers).json()["id"]
    requests.put(f"{api}/projects/{pid}", headers=headers, json=settings)

    # Users: one maintainer and three developers (the GitLab twins of the SingleStore pu_dev1..3)
    for u in USERS:
        u_res = requests.post(f"{api}/users", headers=headers, json={
            "username": u["username"], "name": u["name"], "email": f"{u['username']}@local.internal",
            "password": SHARED_USER_PWD, "skip_confirmation": True
        })
        user_id = u_res.json().get("id") or requests.get(f"{api}/users?username={u['username']}", headers=headers).json()[0]["id"]
        for url in (f"{api}/groups/{subgroup_ids[u['subgroup']]}/members", f"{api}/projects/{pid}/members"):
            requests.post(url, headers=headers, json={"user_id": user_id, "access_level": u["access_level"]})

    runner_token = None
    if register_runner:
        runner_res = requests.post(f"{api}/user/runners", headers=headers, json={
            "runner_type": "instance_type", "run_untagged": True, "tag_list": ["docker", "linux"],
            "description": "workshop-dbt-runner"
        })
        runner_token = runner_res.json()["token"]
    return runner_token, pid


def push_baseline(pat, pid):
    """(Re)set the repository to the baseline and start the first tag pipeline."""
    headers = {"PRIVATE-TOKEN": pat}
    api = f"{GITLAB_URL}/api/v4"
    for bname in ("main", "dev"):   # protected branches reject the force push
        requests.delete(f"{api}/projects/{pid}/protected_branches/{bname}", headers=headers)
    _delete_leftover_refs(pid, headers)
    print(f"  -> Pushing the dbt project (branches dev and main, tag {FIRST_TAG}) to GitLab...")
    scheme, host_part = GITLAB_URL.split("://", 1)
    push_project(f"{scheme}://root:{pat}@{host_part}/{PROJECT_PATH}.git")
    # dev: developers merge (30), maintainers push (40). main: maintainers only, force push allowed for the
    # merge-base reset of the release promotion
    requests.post(f"{api}/projects/{pid}/protected_branches", headers=headers, json={
        "name": "dev", "push_access_level": 40, "merge_access_level": 30, "allow_force_push": False})
    requests.post(f"{api}/projects/{pid}/protected_branches", headers=headers, json={
        "name": "main", "push_access_level": 40, "merge_access_level": 40, "allow_force_push": True})


def latest_pipeline_id(pat, pid, ref=FIRST_TAG):
    """Id of the newest pipeline of `ref`, 0 when there is none."""
    pipelines = requests.get(f"{GITLAB_URL}/api/v4/projects/{pid}/pipelines?ref={ref}&order_by=id&sort=desc&per_page=1",
                             headers={"PRIVATE-TOKEN": pat}).json()
    return pipelines[0]["id"] if pipelines else 0


def wait_for_pipeline(pat, pid, ref=FIRST_TAG, timeout_seconds=1800, newer_than=0):
    """Wait for the pipeline of the first tag and print its status and jobs. Returns the final status.

    `newer_than`: id of the newest pipeline before the push. A re-run finds the previous run's pipeline first
    (GitLab needs a moment to create the new one), which would be reported as the result.
    """
    print(f"  -> Waiting for the pipeline of tag {ref} (build dbt-vdp image, deploy to dev)...")
    headers = {"PRIVATE-TOKEN": pat}
    api = f"{GITLAB_URL}/api/v4/projects/{pid}"
    start = time.time()
    pipeline = None
    status = "not started"
    while time.time() - start < timeout_seconds:
        pipelines = requests.get(f"{api}/pipelines?ref={ref}&order_by=id&sort=desc&per_page=1", headers=headers).json()
        if pipelines and pipelines[0]["id"] > newer_than:
            pipeline = pipelines[0]
            status = pipeline["status"]
            if status in ("success", "failed", "canceled", "skipped"):
                break
        time.sleep(10)
    if pipeline:
        jobs = requests.get(f"{api}/pipelines/{pipeline['id']}/jobs?per_page=100", headers=headers).json()
        for job in jobs:
            print(f"     {job['stage']:>18} / {job['name']}: {job['status']}")
    print(f"  -> Pipeline status for {ref}: {status}")
    return status


# --------------------------------------------------------------------------------------------------------------------
# Images, runner, Airflow
# --------------------------------------------------------------------------------------------------------------------

def build_images():
    print("[4/4] Checking the dbt images and registering the GitLab runner...")
    # The base image is built on the host by `make bootstrap` / `make images`: its Dockerfile needs BuildKit
    # (RUN --mount), which the docker CLI in the manager container does not have.
    if subprocess.run(["docker", "image", "inspect", BASE_IMAGE_NAME], capture_output=True).returncode != 0:
        raise SystemExit(f"{BASE_IMAGE_NAME} not found: run `make images` (or `make bootstrap`) on the host first.")
    # The dev image is only needed for the devcontainer; `make images` builds it from its own folder.
    if subprocess.run(["docker", "image", "inspect", DEV_IMAGE_NAME], capture_output=True).returncode != 0:
        print(f"  -> {DEV_IMAGE_NAME} not found: run `make images` before opening the devcontainer.")


def register_runner(runner_token):
    # Reset existing runner config to avoid duplicate inactive runners
    subprocess.run(
        ["docker", "exec", RUNNER_CONTAINER, "sh", "-c", "gitlab-runner unregister --all-runners || true"],
        check=False,
    )
    # No state volumes here: the deploy job reaches workshop_<env> through the docker socket (docker run -v ...)
    subprocess.run([
        "docker", "exec", RUNNER_CONTAINER, "gitlab-runner", "register",
        "--non-interactive",
        "--url", "http://gitlab.local",
        "--token", runner_token,
        "--executor", "docker",
        "--docker-image", BASE_IMAGE_NAME,
        "--docker-network-mode", "workshop_net",
        "--docker-pull-policy", "if-not-present",
        "--docker-volumes", "/var/run/docker.sock:/var/run/docker.sock",
    ], check=True)
    print("  -> Runner registered (docker executor, docker socket, workshop_net).")


def restart_airflow_and_docs():
    """Recreate the Airflows so they pick up the OAuth credentials in compose/.env, then wait until each answers
    (the deploy job runs `airflow variables set` in them)."""
    print("  -> Restarting Airflow and dbt docs to pick up the OAuth credentials...")
    containers = [f"airflow-{env}" for env in ENVS]
    subprocess.run([*COMPOSE_CMD, "up", "-d", "--force-recreate", *containers, "dbt-docs"], check=True)
    for env in ENVS:
        start = time.time()
        while subprocess.run(["docker", "exec", f"workshop-airflow-{env}", "airflow", "variables", "list"],
                             capture_output=True).returncode != 0:
            if time.time() - start > 300:
                print(f"  !! workshop-airflow-{env} not answering after 300s; the deploy job may fail")
                break
            time.sleep(5)


def reset_studio(pipeline_status):
    """Last step. Workshop Studio starts with `up`, before the landing tables and `dbt-vdp:v1.0.0` exist, so its first
    baseline check ends `failed` and stays that way. Reset it now (this stops that check, forgets its state and starts
    a new one) and wait until the baseline is ready. Never fatal: everything else is done, so on a problem only a
    warning with the manual command is printed. Returns True when the studio is ready."""
    print("  -> Resetting Workshop Studio and waiting for its baseline (data, version and tables in dev, val, prd)...")
    if pipeline_status != "success":
        print(f"  !! The first pipeline is {pipeline_status}, so dbt-vdp:{FIRST_TAG} was not built and the studio cannot "
              f"get ready. Fix the pipeline, then run: {STUDIO_RESET_CMD}")
        return False
    try:
        # the output (progress lines of the wait) goes straight to the console
        code = subprocess.run(["docker", "exec", STUDIO_CONTAINER, "python", "-m", "studio.reset",
                               "--wait", str(STUDIO_READY_TIMEOUT)], timeout=STUDIO_READY_TIMEOUT + 120).returncode
    except (OSError, subprocess.TimeoutExpired) as e:
        code = f"{type(e).__name__}: {e}"
    if code == 0:
        print("  -> Workshop Studio is ready.")
        return True
    print(f"  !! Workshop Studio is not ready ({'exit code ' + str(code) if isinstance(code, int) else code}). "
          f"Check `docker logs {STUDIO_CONTAINER}`, then run: {STUDIO_RESET_CMD}")
    return False


def main():
    seed_singlestore()
    pat = bootstrap_gitlab_rails()
    runner_token, pid = seed_gitlab(pat, register_runner=True)
    build_images()
    register_runner(runner_token)
    restart_airflow_and_docs()
    previous = latest_pipeline_id(pat, pid)
    push_baseline(pat, pid)
    status = wait_for_pipeline(pat, pid, newer_than=previous)
    studio = reset_studio(status)
    print(f"\nWorkshop stack is ready (first pipeline: {status}, Workshop Studio: {'ready' if studio else 'NOT ready, see above'}). Password for the GitLab users and pu_*: {SHARED_USER_PWD}")


if __name__ == "__main__":
    main()

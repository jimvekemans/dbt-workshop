"""Workshop overview: live state, health and users of every service of the stack.

GET /            the page (index.html)
GET /api/status  JSON status of all services (cached for a few seconds)
GET /healthz     "ok"

Standard library only, plus pymysql for the SingleStore probe. Facts come from the Docker Engine API (socket),
from probes inside workshop_net and from the user lists of SingleStore and GitLab.

The status includes each user's password, which the page keeps hidden until asked: this is the presenter's reference
for a demonstration stack and is published on localhost only. Every password is read from where the stack defines it
(container environments, the SingleStore SQL files, bootstrap_stack.py), so none is duplicated here. Tokens are never
part of the output.
"""
import calendar
import http.client
import json
import os
import re
import socket
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import quote, urlsplit

import pymysql

HERE = os.path.dirname(os.path.abspath(__file__))
TIMEOUT = 2
CACHE_SECONDS = 5
DOCKER_SOCKET = "/var/run/docker.sock"
S2_HOST = os.environ.get("S2_HOST", "singlestore")
S2_PORT = int(os.environ.get("S2_PORT", "3306"))
S2_ROOT_PWD = os.environ.get("S2_ROOT_PWD", "")
GITLAB_URL = os.environ.get("GITLAB_URL", "http://gitlab.local")
GITLAB_PAT = os.environ.get("GITLAB_PAT", "")
# Mounted read-only by compose: where the SingleStore users and the GitLab users get their passwords
SQL_DIR = os.environ.get("WORKSHOP_SQL_DIR", "/workshop/singlestore")
BOOTSTRAP_FILE = os.environ.get("WORKSHOP_BOOTSTRAP_FILE", "/workshop/bootstrap_stack.py")
ENVIRONMENTS = ("dev", "val", "prd")
UNREACHABLE = "unknown (service unreachable)"


def load_config():
    with open(os.path.join(HERE, "services.json"), encoding="utf-8") as f:
        return json.load(f)


def expand(template):
    """Fill ${VAR:default} from the environment (same variables and defaults as docker-compose.yml)."""
    return re.sub(r"\$\{(\w+):([^}]*)\}", lambda m: os.environ.get(m.group(1)) or m.group(2), template)


def build_services(config):
    """Return [(group_id, title, [service, ...])]: the shared group and one group per environment."""
    groups = [("shared", config["shared"]["title"], config["shared"]["services"])]
    per_env = config["per_environment"]
    for env in ENVIRONMENTS:
        ports = per_env["ports"][env]
        values = {"env": env, "airflow_port": expand(ports["airflow"]), "docs_port": ports["docs"]}
        services = []
        for svc in per_env["services"]:
            text = json.dumps(svc)
            for key, value in values.items():
                text = text.replace("{" + key + "}", value)
            services.append(json.loads(text))
        groups.append((env, env, services))
    return groups


# ---- Docker Engine API over the unix socket ------------------------------------------------------------------------

class UnixHTTPConnection(http.client.HTTPConnection):
    def __init__(self, path, timeout):
        super().__init__("localhost", timeout=timeout)
        self.socket_path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.socket_path)


def container_inspect(name):
    """(HTTP status, parsed body) of `docker inspect`, or (None, {}) without Docker access."""
    try:
        conn = UnixHTTPConnection(DOCKER_SOCKET, TIMEOUT)
        conn.request("GET", "/containers/" + quote(name, safe="") + "/json")
        resp = conn.getresponse()
        body = resp.read()
        conn.close()
    except OSError:
        return None, {}
    return resp.status, json.loads(body) if resp.status == 200 else {}


def container_env(name):
    """Environment of a container as a dict; empty when it does not exist or Docker cannot be asked."""
    env = (container_inspect(name)[1].get("Config") or {}).get("Env") or []
    return dict(item.split("=", 1) for item in env if "=" in item)


def container_state(name):
    """{'state': running|exited|missing|..., 'health': str|None, 'since': epoch|None} or None without Docker access."""
    status, body = container_inspect(name)
    if status == 404:
        return {"state": "missing", "health": None, "since": None}
    if status != 200:
        return None
    state = body.get("State", {})
    since = None
    if state.get("Running"):
        started = re.match(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)", state.get("StartedAt", ""))
        if started:
            since = calendar.timegm(time.strptime(started.group(1), "%Y-%m-%dT%H:%M:%S"))
    return {"state": state.get("Status", "unknown"), "health": (state.get("Health") or {}).get("Status"),
            "since": since}


# ---- probes: each returns (status, detail, extra) with status up | degraded | down ---------------------------------

def http_get(url, headers=None):
    parts = urlsplit(url)
    conn = http.client.HTTPConnection(parts.hostname, parts.port or 80, timeout=TIMEOUT)
    try:
        conn.request("GET", (parts.path or "/") + ("?" + parts.query if parts.query else ""), headers=headers or {})
        resp = conn.getresponse()
        return resp.status, resp.read(1 << 20)
    finally:
        conn.close()


def probe_http(p):
    code, _ = http_get(p["url"])
    ok = code < p.get("ok_below", 300)
    return ("up" if ok else "down"), f"HTTP {code}"


def probe_airflow(p):
    code, body = http_get(p["url"])
    if code != 200:
        return "down", f"health endpoint answered HTTP {code}"
    data = json.loads(body)
    parts = []
    for key, label in (("metadatabase", "database"), ("scheduler", "scheduler"), ("dag_processor", "dag-processor"),
                       ("triggerer", "triggerer")):
        if key in data:
            parts.append(f"{label} {data[key].get('status', 'unknown')}")
    bad = any(not x.endswith("healthy") or x.endswith("unhealthy") for x in parts)
    return ("degraded" if bad else "up"), ", ".join(parts) or "health reported"


def probe_docs(p):
    code, _ = http_get(p["url"])
    if code in (403, 404):
        return "up", "nginx answers, no docs deployed yet"
    return ("up" if code < 400 else "down"), f"HTTP {code}"


def probe_tcp(p):
    with socket.create_connection((p["host"], p["port"]), timeout=TIMEOUT):
        pass
    return "up", f"TCP {p['port']} accepts connections"


def s2_connect():
    return pymysql.connect(host=S2_HOST, port=S2_PORT, user="root", password=S2_ROOT_PWD, connect_timeout=TIMEOUT,
                           read_timeout=TIMEOUT, write_timeout=TIMEOUT)


def read_singlestore_users(cur):
    """[(user, [group, ...])] from information_schema.users and SHOW GROUPS FOR USER."""
    cur.execute("select user, host from information_schema.users where is_deleted = 0 order by user")
    users = []
    for name, host in cur.fetchall():
        esc = lambda v: v.replace("\\", "\\\\").replace("'", "''")
        cur.execute(f"SHOW GROUPS FOR USER '{esc(name)}'@'{esc(host)}'")
        users.append((name, [row[0] for row in cur.fetchall()]))
    return users


def probe_singlestore(p):
    conn = s2_connect()
    try:
        cur = conn.cursor()
        cur.execute("select 1")
        cur.fetchall()
        try:
            users = read_singlestore_users(cur)
        except pymysql.MySQLError:
            users = None
    finally:
        conn.close()
    return "up", "login and select 1 succeeded", users


def gitlab_users():
    """[{'name', 'admin', 'groups': ['workshop/users', ...]}] through the REST API; None when unreachable."""
    headers = {"PRIVATE-TOKEN": GITLAB_PAT}
    base = GITLAB_URL.rstrip("/") + "/api/v4"

    def get(path):
        code, body = http_get(base + path, headers)
        if code != 200:
            raise OSError(f"GitLab API HTTP {code}")
        return json.loads(body)

    try:
        users = {u["id"]: {"name": u["username"], "admin": u.get("is_admin", False), "groups": []}
                 for u in get("/users?per_page=100&active=true&exclude_internal=true")}
        for sub in get("/groups/workshop/subgroups?per_page=100"):
            for member in get(f"/groups/{sub['id']}/members?per_page=100"):
                if member["id"] in users:
                    users[member["id"]]["groups"].append(sub["full_path"])
        return sorted(users.values(), key=lambda u: u["name"])
    except (OSError, ValueError, KeyError):
        return None


PROBES = {"http": probe_http, "airflow": probe_airflow, "docs": probe_docs, "tcp": probe_tcp,
          "singlestore": probe_singlestore}


def run_probe(svc):
    """Docker state plus the service probe -> status dict (without users)."""
    docker = container_state(svc["container"])
    probe = svc["probe"]
    status, detail, extra = "down", "", None
    missing = docker is not None and docker["state"] == "missing"
    if docker is not None and docker["state"] not in ("running", "missing"):
        detail = f"container {docker['state']}"
    elif probe["type"] == "docker":
        if missing:
            detail = "container not found (stack not started?)"
        elif docker is None:
            detail = "Docker socket not available"
        else:
            status, detail = "up", "container running"
            if docker["health"] in ("starting", "unhealthy"):
                status, detail = "degraded", f"container {docker['health']}"
    else:
        try:
            result = PROBES[probe["type"]](probe)
            status, detail = result[0], result[1]
            extra = result[2] if len(result) > 2 else None
        except (OSError, ValueError, pymysql.MySQLError) as exc:
            detail = "container not found (stack not started?)" if missing else f"unreachable: {type(exc).__name__}"
        if missing and status == "up":
            status, detail = "degraded", detail + "; but container " + svc["container"] + " not found"
        if status == "up" and docker and docker["health"] == "starting":
            status, detail = "degraded", detail + "; container health check still starting"
        elif status == "up" and docker and docker["health"] == "unhealthy":
            status, detail = "degraded", detail + "; container health check unhealthy"
    return {"status": status, "detail": detail, "docker": docker, "extra": extra}


# ---- users ---------------------------------------------------------------------------------------------------------

def split_group(group):
    """'grp_dev_reader' -> ('dev', 'reader'); groups without an environment prefix -> (None, 'ci_dbt')."""
    name = group[4:] if group.startswith("grp_") else group
    head, _, rest = name.partition("_")
    return (head, rest) if head in ENVIRONMENTS and rest else (None, name)


def singlestore_users(s2_users, env):
    """Users shown under `env`, or under the shared entry when env is None."""
    out = []
    for name, groups in s2_users:
        roles = [split_group(g) for g in groups]
        if env is None:
            others = [r for e, r in roles if e is None]
            if others or not any(e for e, _ in roles):
                out.append({"name": name, "role": ", ".join(others) or ("superuser" if name == "root" else "no group")})
        else:
            mine = [r for e, r in roles if e == env]
            if mine:
                out.append({"name": name, "role": ", ".join(mine)})
    return out


def file_text(path):
    try:
        with open(path, encoding="utf-8") as f:
            return f.read()
    except OSError:
        return ""


def singlestore_passwords():
    """{user: password} from the CREATE USER statements of the SQL files (the template once per environment)."""
    out = {}
    for name in sorted(os.listdir(SQL_DIR)) if os.path.isdir(SQL_DIR) else []:
        text = file_text(os.path.join(SQL_DIR, name))
        variants = [text.replace("__ENV__", env) for env in ENVIRONMENTS] if "__ENV__" in text else [text]
        for variant in variants:
            out.update(re.findall(r"CREATE USER (?:IF NOT EXISTS )?'([^']+)'@'[^']*' IDENTIFIED BY '([^']*)'",
                                  variant, re.IGNORECASE))
    return out


def collect_passwords(config):
    """Passwords by where they apply. A value that cannot be found is None and the page says so."""
    containers = {svc["id"]: svc["container"] for svc in config["shared"]["services"]}
    singlestore = singlestore_passwords()
    singlestore["root"] = container_env(containers["singlestore"]).get("ROOT_PASSWORD") or S2_ROOT_PWD or None
    shared_user = re.search(r"""^SHARED_USER_PWD\s*=\s*["']([^"']+)["']""", file_text(BOOTSTRAP_FILE), re.MULTILINE)
    return {
        "singlestore": singlestore,
        "gitlab_root": container_env(containers["gitlab"]).get("GITLAB_ROOT_PASSWORD"),
        "gitlab_user": shared_user.group(1) if shared_user else None,
    }


def resolve_users(svc, env, shared):
    """([{'name', 'role', 'password'}] or None, note or None)."""
    spec = svc["users"]
    source = spec["source"]
    passwords = shared["passwords"]
    if source == "none":
        return None, spec.get("note", "No accounts.")
    if source == "static":
        # `password_env` names the variable of the service's own container that holds the password
        env_vars = container_env(svc["container"])
        return [{"name": u["name"], "role": u["role"], "password": env_vars.get(u.get("password_env", ""))}
                for u in spec["users"]], None
    if source == "singlestore":
        users = shared["s2_users"]
        if users is None:
            return None, UNREACHABLE
        return [{**u, "password": passwords["singlestore"].get(u["name"])}
                for u in singlestore_users(users, spec.get("env"))], None
    users = shared["gl_users"]
    if users is None:
        # Airflow accounts come from GitLab: say which service is the one that cannot be asked
        return None, UNREACHABLE if source == "gitlab" else "unknown (GitLab unreachable: sign-in goes through GitLab)"

    def gitlab_password(name):
        return passwords["gitlab_root"] if name == "root" else passwords["gitlab_user"]

    if source == "gitlab":
        return [{"name": u["name"], "password": gitlab_password(u["name"]),
                 "role": ", ".join(g.split("/")[-1] for g in u["groups"])
                 or ("administrator" if u["admin"] else "no workshop group")} for u in users], None
    mapping = shared["airflow_roles"][env]
    out = [{"name": u["name"], "password": gitlab_password(u["name"]),
            "role": " / ".join(dict.fromkeys(mapping[g] for g in u["groups"] if g in mapping))}
           for u in users if any(g in mapping for g in u["groups"])]
    return out, "Sign-in through GitLab with the GitLab password; the role follows the workshop subgroup."


# ---- assembling the status -----------------------------------------------------------------------------------------

def collect(config):
    groups = build_services(config)
    probed = [svc for _, _, services in groups for svc in services if "probe" in svc]
    with ThreadPoolExecutor(max_workers=len(probed) + 2) as pool:
        gl_future = pool.submit(gitlab_users)
        futures = {id(svc): pool.submit(run_probe, svc) for svc in probed}
        results = {key: f.result() for key, f in futures.items()}
        gl_users = gl_future.result()
    s2_result = next(results[id(s)] for s in groups[0][2] if s["id"] == "singlestore")
    shared = {"gl_users": gl_users, "s2_users": s2_result["extra"], "airflow_roles": config["airflow_roles"],
              "passwords": collect_passwords(config)}
    if s2_result["status"] == "down":
        shared["s2_users"] = None

    out_groups, up, total = [], 0, 0
    for group_id, title, services in groups:
        cards = []
        for svc in services:
            res = results[id(svc)] if "probe" in svc else next(
                results[id(s)] for s in groups[0][2] if s["id"] == svc["same_as"])
            users, note = resolve_users(svc, group_id, shared)
            docker = res["docker"]
            cards.append({
                "id": f"{group_id}-{svc['id']}", "name": svc["name"], "purpose": svc["purpose"],
                "container": svc["container"], "status": res["status"], "detail": res["detail"],
                "docker": docker, "users": users, "users_note": note,
                "link": expand(svc["link"]) if svc.get("link") else None,
                "link_label": svc.get("link_label", "Open"), "credentials": svc.get("credentials", ""),
            })
            total += 1
            up += res["status"] == "up"
        out_groups.append({"id": group_id, "title": title, "services": cards})
    return {"generated_at": int(time.time()), "up": up, "total": total, "groups": out_groups}


class Cache:
    def __init__(self, config):
        self.config, self.lock, self.value, self.stamp = config, threading.Lock(), None, 0.0

    def get(self):
        if self.value is not None and time.time() - self.stamp < CACHE_SECONDS:
            return self.value
        if not self.lock.acquire(blocking=self.value is None):
            return self.value  # another request is refreshing: serve the previous result
        try:
            if self.value is None or time.time() - self.stamp >= CACHE_SECONDS:
                self.value, self.stamp = collect(self.config), time.time()
            return self.value
        finally:
            self.lock.release()


class Handler(BaseHTTPRequestHandler):
    cache = None

    def send(self, code, ctype, body):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/":
            with open(os.path.join(HERE, "index.html"), "rb") as f:
                self.send(200, "text/html; charset=utf-8", f.read())
        elif path == "/api/status":
            self.send(200, "application/json", json.dumps(self.cache.get()).encode())
        elif path == "/healthz":
            self.send(200, "text/plain", b"ok")
        else:
            self.send(404, "text/plain", b"not found")

    def log_message(self, fmt, *args):
        pass


if __name__ == "__main__":
    Handler.cache = Cache(load_config())
    ThreadingHTTPServer(("0.0.0.0", 8000), Handler).serve_forever()

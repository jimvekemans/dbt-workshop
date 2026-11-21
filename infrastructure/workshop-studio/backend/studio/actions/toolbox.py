"""`toolbox.run`: what a developer does in the developer toolbox, run in a throwaway container.

The container is the developer image (`dbt-singlestore-dev:1.11.11`) with the project copied out of the package the
development environment runs, opened as one developer (`as: developer:pu_dev1`) or as the development robot (`as:
service`, the account the nightly load uses, `svc_dev_dbt`; its password always travels in the environment). Every command
of `commands` runs in the project folder, one after the other, as its own shell script. Each says what must happen
(`expect`):

* `success`  the command ends with exit code 0
* `refused`  the toolbox itself refuses (its own `vdp: ...` message, exit code other than 0)
* `failed`   the command ends with an exit code other than 0

`auth` says how the developer's password reaches the toolbox: `password` in the environment (as `dbt.run` does), `file`
in the settings file `.devcontainer/.env` of the project copy (the way a laptop without a vault works), or `vault`: the
settings file holds only the two vault ids of the account and the toolbox fetches the password when a command starts.

What the audience reads is written in the scenario: `say` is one plain line (or a list of them) per command, with the
facts of that command in `{braces}` (see `FACTS`); the first line of the action is its summary, the rest are the
`reasons` of a `result` card. `must_contain` lists text that has to be in the command's output (never shown); a
command with `changes: true` also reports what it changed in the project's `.yml` files, in plain lines (new feeds and
columns). Nothing is written outside the throwaway container and the developer's own copies in the database; those
copies are dropped by the cleaner `toolbox:<env>:<user>`. The robot has no copies of its own: it builds and writes in the
shared development tables, so a scenario that runs it registers the undo of what it builds first (a `sql.exec` with `undo:`,
as `ci:copies` does for the rehearsals) and the action reports nothing to clean up by itself. Passwords and vault ids
never appear in any output.
"""

from __future__ import annotations

import io
import re
import tarfile
import time
from dataclasses import dataclass, field
from typing import Any, Literal

import yaml
from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from .. import names
from . import clients
from .base import Ctx, Params, Result, StepFailed, action, cleaner
from .dbt import DEV_IMAGE

ACCOUNTS = ("pu_dev1", "pu_dev2", "pu_dev3")
ROBOT = "service"           # `as: service`: the development robot
ROBOT_USER = "svc_dev_dbt"
HOME = "/usr/app/dbt"
PROJECT = f"{HOME}/workspace"
# the facts a `say` line may use; each is text with its noun ("29 tables"), so a sentence never says "1 tables"
FACTS = ("tables", "checks", "errors", "time", "listed", "last", "user", "account", "new_feeds", "columns_added",
         "columns_removed", "columns_before", "columns_after", "changed_files")
PLACEHOLDER = re.compile(r"\{(\w+)\}")
VAULT_IDS = re.compile(r"workshop-(?:role|secret)-id-[\w-]+")
PROCESSED = re.compile(r"Processed:.*?\|?\s*(\d+) models?")
PROCESSED_TESTS = re.compile(r"Processed:.*?(\d+) tests?")
ERRORS = re.compile(r"Summary:.*?(\d+) error")
FINISHED_TIME = re.compile(r"Finished '[\w-]+'.*\[([\d.]+)s\]")
LISTED = re.compile(r'^\{"database"', re.MULTILINE)
SECTION = re.compile(r"^@@(begin|end|changes|endchanges|file|before|after|endfile)(?: (.*))?$")


def plural(n: int, one: str, many: str) -> str:
    return f"{names.format_number(n)} {one if n == 1 else many}"


def upper_first(text: str) -> str:
    return text[:1].upper() + text[1:]


def commands_words(n: int) -> str:
    return plural(n, "commando", "commando's")


class ToolboxCommand(BaseModel):
    model_config = ConfigDict(extra="forbid")

    run: str                                   # shell text, run in the project folder
    expect: Literal["success", "refused", "failed"] = "success"
    say: list[str] = Field(default_factory=list)   # plain lines with {facts}; a single string is one line
    must_contain: list[str] = Field(default_factory=list)   # text the output must hold ({account} = the login name)
    changes: bool = False                      # also report what the command changed in the project's .yml files

    @field_validator("say", mode="before")
    @classmethod
    def one_line_is_a_list(cls, value: Any) -> Any:
        return [value] if isinstance(value, str) else value

    @field_validator("say", "must_contain")
    @classmethod
    def known_placeholders(cls, lines: list[str]) -> list[str]:
        for line in lines:
            for found in PLACEHOLDER.findall(line):
                if found not in FACTS:
                    raise ValueError(f"unknown fact {{{found}}} (known: {', '.join(FACTS)})")
        return lines

    @field_validator("run")
    @classmethod
    def not_empty(cls, text: str) -> str:
        if not text.strip():
            raise ValueError("a command needs text to run")
        return text


class ToolboxRunParams(Params):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    as_: str = Field(alias="as")
    auth: Literal["password", "file", "vault"] = "password"
    commands: list[ToolboxCommand] = Field(min_length=1, max_length=8)
    timeout: int = Field(600, ge=10, le=1800)
    # project files (paths inside the dbt project) taken from the repository's project instead of the package: for a
    # change that is in the repository but not in a package yet. Identical files make it a no-op once packaged.
    use_repository: list[str] = Field(default_factory=list, max_length=8)

    @field_validator("use_repository")
    @classmethod
    def plain_project_paths(cls, paths: list[str]) -> list[str]:
        for path in paths:
            parts = path.split("/")
            if not path or path.startswith("/") or any(part in ("", "..") or part.startswith(".") for part in parts):
                raise ValueError(f"`use_repository` takes plain paths inside the project, not '{path}'")
        return paths

    @field_validator("as_")
    @classmethod
    def a_developer_or_the_robot(cls, who: str) -> str:
        if who != ROBOT and not re.fullmatch(r"developer:(" + "|".join(ACCOUNTS) + ")", who):
            raise ValueError("`as` is developer:pu_dev1, developer:pu_dev2, developer:pu_dev3 or service")
        return who

    @model_validator(mode="after")
    def only_development(self) -> "ToolboxRunParams":
        if self.env not in (None, "dev"):
            raise ValueError("the toolbox works on development only")
        return self

    @model_validator(mode="after")
    def the_robot_has_no_settings_file(self) -> "ToolboxRunParams":
        if self.as_ == ROBOT and self.auth != "password":
            raise ValueError("the robot connects with the password in the environment: `auth` stays `password`")
        return self

    @property
    def user(self) -> str:
        return ROBOT_USER if self.as_ == ROBOT else self.as_.split(":", 1)[1]


# --- the container ----------------------------------------------------------------------------------------------


def settings_file(user: str, auth: str, password: str) -> str:
    """The developer's `.devcontainer/.env`, as `.devcontainer/post_create.sh` writes it."""
    lines = ["SINGLESTORE_HOST=singlestore", f"SINGLESTORE_USER={user}"]
    if auth == "file":
        lines.append(f"SINGLESTORE_PASS={password}")
    lines += ["SINGLESTORE_DB=dev_edw_dv_rdv", "DBT_DEPLOY_TARGET=dev"]
    if auth == "vault":
        lines += [f"KEYVAULT_ROLE_ID=workshop-role-id-{user}", f"KEYVAULT_SECRET_ID=workshop-secret-id-{user}"]
    return "\n".join(lines) + "\n"


def main_script(commands: list[ToolboxCommand]) -> str:
    """One script that runs the command files in turn and marks where each one starts and ends (`@@begin 0`,
    `@@end 0 <exit code> <seconds>`), and for a command with `changes` lists the `.yml` files it changed."""
    parts = [f"cd {PROJECT} || exit 99"]
    for i, command in enumerate(commands):
        if command.changes:
            parts.append("rm -rf /tmp/snap; cp -a models /tmp/snap")
        parts += [
            f"echo '@@begin {i}'",
            "t0=$(date +%s.%N)",
            f"bash {HOME}/commands/{i}.sh 2>&1",
            "code=$?",
            "t1=$(date +%s.%N)",
            f"echo \"@@end {i} $code $(awk \"BEGIN{{printf \\\"%.2f\\\", $t1 - $t0}}\")\"",
        ]
        if command.changes:
            parts += [
                f"echo '@@changes {i}'",
                "for f in $( (cd /tmp/snap && find . -name '*.yml'; cd " + PROJECT + "/models && find . -name '*.yml') | sort -u); do",
                "  if ! cmp -s \"/tmp/snap/$f\" \"models/$f\"; then",
                "    echo \"@@file $f\"; echo '@@before'; cat \"/tmp/snap/$f\" 2>/dev/null; echo '@@after'; cat \"models/$f\" 2>/dev/null; echo '@@endfile'",
                "  fi",
                "done",
                f"echo '@@endchanges {i}'",
            ]
    return "\n".join(parts) + "\n"


def add_files(archive: bytes, files: dict[str, bytes]) -> bytes:
    """The tar `archive` (the project, `workspace/...`) with more files, paths relative to `/usr/app/dbt`, owned by the
    container's dbt user (1000:0)."""
    out = io.BytesIO()
    have: set[str] = set()
    with tarfile.open(fileobj=io.BytesIO(archive)) as src, tarfile.open(fileobj=out, mode="w") as dst:
        for member in src:
            dst.addfile(member, src.extractfile(member) if member.isfile() else None)
            have.add(member.name.rstrip("/"))
        now = int(time.time())

        def owned(info: tarfile.TarInfo) -> tarfile.TarInfo:
            info.uid, info.gid, info.uname, info.gname, info.mtime = 1000, 0, "", "", now
            return info

        for path, data in sorted(files.items()):
            folder = ""
            for part in path.split("/")[:-1]:
                folder = f"{folder}/{part}" if folder else part
                if folder not in have:
                    info = tarfile.TarInfo(folder)
                    info.type, info.mode = tarfile.DIRTYPE, 0o755
                    dst.addfile(owned(info))
                    have.add(folder)
            info = tarfile.TarInfo(path)
            info.size, info.mode = len(data), 0o644
            dst.addfile(owned(info), io.BytesIO(data))
    return out.getvalue()


def hide_ids(text: str) -> str:
    return VAULT_IDS.sub("***", text)


def run_in_toolbox(ctx: Ctx, p: ToolboxRunParams) -> clients.Proc:
    user = p.user
    password = clients.password_for(ctx.settings, user)
    tag = clients.deployed_tag(ctx, ctx.env)
    files = {f"commands/{i}.sh": f"cd {PROJECT}\n{c.run}\n".encode() for i, c in enumerate(p.commands)}
    files["commands/main.sh"] = main_script(p.commands).encode()
    root = ctx.settings.dbt_project_dir
    for path in p.use_repository:
        source = root / path
        if not source.is_file():
            raise StepFailed("Een bestand uit het project van de repository kon niet gelezen worden.", f"missing: {path}")
        files[f"workspace/{path}"] = source.read_bytes()
    if p.auth != "password":
        files["workspace/.devcontainer/.env"] = settings_file(user, p.auth, password).encode()
    project = add_files(clients.project_tar(ctx, f"dbt-vdp:{tag}"), files)
    name = clients.throwaway_name()
    env = {"DBT_DEPLOY_TARGET": ctx.env, "SINGLESTORE_HOST": ctx.settings.s2_host, "SINGLESTORE_USER": user, "HOME": "/tmp"}
    flags = ["--network", ctx.settings.docker_network, "-w", PROJECT]
    for key, value in env.items():
        flags += ["-e", f"{key}={value}"]
    secret_env = {"SINGLESTORE_PASS": password} if p.auth == "password" else {}
    for key in secret_env:
        flags += ["-e", key]
    try:
        clients.docker(ctx, "create", *clients.throwaway_flags(name), *flags, DEV_IMAGE, "bash", f"{HOME}/commands/main.sh",
                       env=secret_env or None)
        clients.docker_bytes(ctx, "cp", "-", f"{name}:{HOME}", data=project)
        return clients.docker(ctx, "start", "-a", name, timeout=p.timeout, check=False)
    finally:
        clients.remove_container(name)


# --- reading the output -----------------------------------------------------------------------------------------


@dataclass
class Ran:
    """What one command did, read from the marked output."""

    index: int
    code: int = 0
    seconds: float = 0.0
    lines: list[str] = field(default_factory=list)
    files: list[tuple[str, str, str]] = field(default_factory=list)   # (path, before, after) of changed .yml files
    ended: bool = False

    @property
    def output(self) -> str:
        return "\n".join(self.lines)


def parse(output: str) -> dict[int, Ran]:
    ran: dict[int, Ran] = {}
    current: Ran | None = None
    in_changes = False
    file_state: str | None = None
    path, before, after = "", [], []
    for line in output.splitlines():
        found = SECTION.match(line)
        if found:
            kind, rest = found.group(1), (found.group(2) or "").strip()
            if kind == "begin":
                current, in_changes = ran.setdefault(int(rest), Ran(int(rest))), False
            elif kind == "end" and current is not None:
                parts = rest.split()   # "<index> <exit code> <seconds>"
                current.code, current.seconds, current.ended = int(parts[1]), float(parts[2]), True
            elif kind == "changes":
                in_changes = True
            elif kind == "endchanges":
                in_changes = False
            elif kind == "file" and in_changes:
                path, before, after, file_state = rest, [], [], "before"
            elif kind == "before":
                file_state = "before"
            elif kind == "after":
                file_state = "after"
            elif kind == "endfile" and current is not None:
                current.files.append((path, "\n".join(before), "\n".join(after)))
                file_state = None
            continue
        if in_changes and file_state:
            (before if file_state == "before" else after).append(line)
        elif current is not None and not in_changes and not current.ended:
            current.lines.append(line)
    return ran


def source_columns(text: str) -> dict[str, list[str]]:
    """`{table: [column, ...]}` of every source table a YAML file describes; a file that does not read gives nothing."""
    try:
        doc = yaml.safe_load(text) or {}
    except yaml.YAMLError:
        return {}
    found: dict[str, list[str]] = {}
    for source in (doc.get("sources") or []) if isinstance(doc, dict) else []:
        for table in (source.get("tables") or []) if isinstance(source, dict) else []:
            if isinstance(table, dict) and table.get("name"):
                found[str(table["name"])] = [str(c["name"]) for c in table.get("columns") or [] if isinstance(c, dict) and c.get("name")]
    return found


@dataclass
class Changes:
    new_feeds: dict[str, list[str]] = field(default_factory=dict)
    added: dict[str, list[str]] = field(default_factory=dict)
    removed: dict[str, list[str]] = field(default_factory=dict)
    before: int = 0   # columns of the feeds that were described before
    after: int = 0    # the same feeds, after
    files: list[str] = field(default_factory=list)


def describe_changes(files: list[tuple[str, str, str]]) -> Changes:
    out = Changes()
    for path, before_text, after_text in files:
        old, new = source_columns(before_text), source_columns(after_text)
        out.files.append(path)
        for table, columns in new.items():
            if table not in old:
                out.new_feeds[table] = columns
                continue
            out.before += len(old[table])
            out.after += len(columns)
            if gone := [c for c in old[table] if c not in columns]:
                out.removed[table] = gone
            if more := [c for c in columns if c not in old[table]]:
                out.added[table] = more
    return out


def feed_words(table: str) -> str:
    return names.model_title(table) or re.sub(r"^raw_", "", table).replace("_", " ").capitalize() + " feed"


def change_lines(changes: Changes) -> list[str]:
    """What changed, one plain line each: the new feed and its columns, the columns added to or removed from a feed."""
    lines = []
    for table, columns in changes.new_feeds.items():
        lines.append(f"Nieuwe feed beschreven: {feed_words(table).lower()}")
        lines += [f"Kolom: {names.column_words(c)}" for c in columns]
    for table, columns in changes.added.items():
        lines.append(f"Toegevoegd aan {feed_words(table).lower()}: {', '.join(names.column_words(c) for c in columns)}")
    for table, columns in changes.removed.items():
        lines.append(f"Verwijderd uit {feed_words(table).lower()}: {', '.join(names.column_words(c) for c in columns)}")
    return lines


def time_words(seconds: float) -> str:
    return "minder dan een seconde" if seconds < 1 else plural(round(seconds), "seconde", "seconden")


def facts_of(ran: Ran, user: str, changes: Changes | None) -> dict[str, str]:
    output = ran.output
    built = PROCESSED.search(output)
    tests = PROCESSED_TESTS.search(output)
    errors = ERRORS.search(output)
    took = FINISHED_TIME.findall(output)
    last = next((ln.strip() for ln in reversed(ran.lines) if ln.strip()), "")
    facts = {
        "tables": plural(int(built[1]), "tabel", "tabellen") if built else "",
        "checks": plural(int(tests[1]), "check", "checks") if tests else "",
        "errors": plural(int(errors[1]), "fout", "fouten") if errors else "",
        "time": time_words(float(took[-1]) if took else ran.seconds),
        "listed": plural(len(LISTED.findall(output)), "tabel", "tabellen"),
        "last": hide_ids(last)[:80],
        "user": names.account_words(user) or user,
        "account": user,
        "new_feeds": plural(len(changes.new_feeds), "nieuwe feed", "nieuwe feeds") if changes else "",
        "columns_added": "",
        "columns_removed": "",
        "columns_before": plural(changes.before, "kolom", "kolommen") if changes else "",
        "columns_after": plural(changes.after, "kolom", "kolommen") if changes else "",
        "changed_files": plural(len(changes.files), "bestand", "bestanden") if changes else "",
    }
    if changes:
        added = sum(len(c) for c in changes.new_feeds.values()) + sum(len(c) for c in changes.added.values())
        facts["columns_added"] = plural(added, "kolom", "kolommen")
        facts["columns_removed"] = plural(sum(len(c) for c in changes.removed.values()), "kolom", "kolommen")
    return facts


def say(line: str, facts: dict[str, str], user: str) -> str:
    """The line with its facts filled in; a fact the output did not hold is a failed step, never a broken sentence."""
    missing = [k for k in PLACEHOLDER.findall(line) if not facts.get(k) and k not in ("columns_removed", "last")]
    if missing:
        raise StepFailed("Het resultaat van de toolbox kon niet gelezen worden.", f"no value for {{{missing[0]}}} in the output")
    return PLACEHOLDER.sub(lambda m: facts[m.group(1)], line)


def tail(text: str, lines: int = 25) -> str:
    return "\n".join(text.splitlines()[-lines:])


def how(auth: str) -> str:
    return {"password": "the password in the environment", "file": "a settings file that holds the password",
            "vault": "a settings file that holds only the vault ids"}[auth]


@action("toolbox.run", ToolboxRunParams, lambda p: f"{upper_first(names.account_words(p.user))} werkt in de toolbox")
def toolbox_run(ctx: Ctx, p: ToolboxRunParams) -> Result:
    # a developer's own copies are dropped by the cleaner; the robot has none (the scenario registers its own undo)
    touched = set() if p.as_ == ROBOT else {f"toolbox:{ctx.env}:{p.user}"}
    try:
        return _toolbox_run(ctx, p, touched)
    except StepFailed as e:
        e.touched |= touched
        raise


def _toolbox_run(ctx: Ctx, p: ToolboxRunParams, touched: set[str]) -> Result:
    if ctx.env != "dev":
        raise StepFailed("De toolbox werkt alleen in development.")
    ctx.progress(f"{upper_first(names.account_words(p.user))} runt {commands_words(len(p.commands))} in de toolbox")
    proc = run_in_toolbox(ctx, p)
    ctx.check_cancel()
    text = hide_ids(clients.mask(ctx.settings, proc.text))
    ran = parse(text)
    details = [f"image {DEV_IMAGE}; {p.user} with {how(p.auth)}"]
    if p.use_repository:
        details.append("files taken from the repository's project: " + ", ".join(p.use_repository))
    lines: list[str] = []
    data_commands: list[dict[str, Any]] = []
    problem: str | None = None
    for i, command in enumerate(p.commands):
        done = ran.get(i)
        shown = command.run.strip().splitlines()
        details.append(f"$ {shown[0]}" + (f"  (+{len(shown) - 1} more lines)" if len(shown) > 1 else ""))
        if done is None or not done.ended:
            details.append("(the command did not run to its end)")
            problem = problem or "Een commando in de toolbox is niet afgeraakt."
            break
        details.append(f"exit code {done.code}, {done.seconds:.1f} s\n{tail(done.output)}")
        refused = any(ln.startswith("vdp:") for ln in done.lines)
        ok = {"success": done.code == 0, "refused": done.code != 0 and refused, "failed": done.code != 0}[command.expect]
        if not ok:
            problem = {
                "success": "Een commando in de toolbox is met problemen geëindigd.",
                "refused": "De toolbox had een commando moeten weigeren, maar deed dat niet.",
                "failed": "Een commando in de toolbox had moeten mislukken, maar is zonder problemen afgelopen.",
            }[command.expect]
            break
        wanted = [t.replace("{account}", p.user) for t in command.must_contain]
        if absent := [t for t in wanted if t not in done.output]:
            problem = "De toolbox gaf niet het antwoord dat deze stap nodig heeft."
            details.append(f"missing in the output: {absent[0]}")
            break
        changes = describe_changes(done.files) if command.changes else None
        facts = facts_of(done, p.user, changes)
        lines += [say(line, facts, p.user) for line in command.say]
        if changes:
            lines += change_lines(changes)
        data_commands.append({"expect": command.expect, "exit": done.code, "seconds": round(done.seconds, 1)})
    if problem:
        raise StepFailed(problem, "\n".join(details), touched)
    if not lines:
        lines = [f"{upper_first(names.account_words(p.user))} heeft {commands_words(len(p.commands))} gerund in de toolbox."]
    stopped = any(c.expect != "success" for c in p.commands)
    data = {"reasons": lines[1:], "refused": stopped, "commands": data_commands}
    return Result(lines[0], details="\n".join(details), touched=touched, data=data)


# --- clean-up ---------------------------------------------------------------------------------------------------


def developer_copies(ctx: Ctx, env: str, user: str) -> list[tuple[str, str, str]]:
    """Every table and view of the developer's own copies in the environment's databases: `(database, name, type)`."""
    conn = clients.s2_connect(ctx.settings, "root", timeout=30)
    try:
        with conn.cursor() as cur:
            cur.execute("select table_schema, table_name, table_type from information_schema.tables "
                        "where table_schema like %s and table_name like %s", (f"{env}%", f"{user[:2]}%"))
            return [(s, t, k) for s, t, k in cur.fetchall() if s.startswith(f"{env}_") and t.startswith(f"{user}_")]
    finally:
        conn.close()


@cleaner("toolbox")
def clean_toolbox(ctx: Ctx, resource: str) -> Result:
    """Drops every table and view of the developer's own copies, whichever database it is in."""
    _, env, user = resource.split(":")
    if not re.fullmatch(r"pu_dev[123]", user) or env != "dev":
        raise StepFailed("Deze opkuis kent enkel de kopieën van de developers in development.", resource)
    found = developer_copies(ctx, env, user)
    conn = clients.s2_connect(ctx.settings, "root", timeout=30)
    try:
        with conn.cursor() as cur:
            for schema, table, kind in found:
                cur.execute(f"drop {'view' if kind == 'VIEW' else 'table'} if exists `{schema}`.`{table}`")
    finally:
        conn.close()
    who = names.account_words(user)
    return Result(f"De testtabellen van {who} zijn verwijderd.", details=f"dropped {len(found)} tables and views of {user}")

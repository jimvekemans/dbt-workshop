"""`sql.exec`: one statement on SingleStore as root or as a named user, with an undo or an expected refusal.

Undo bookkeeping: the statements to undo are kept in the small JSON file `<data_dir>/sql_undo.json`
(`{undo_id: {"statement": ...}}`) and the touched resource is `sql:<undo_id>`. The file survives a restart of the
studio, so a leftover is still undone later. Undo statements always run as root: they restore the baseline, whoever
made the change.
"""

from __future__ import annotations

import json
import re
import threading
import uuid
from pathlib import Path
from typing import Literal

import pymysql
from pydantic import ConfigDict, Field, model_validator

from .. import names
from . import clients
from .base import Ctx, Params, Result, StepFailed, action, cleaner

DENIED_CODES = {1044, 1045, 1142, 1143, 1227, 1370}
IS_READ = re.compile(r"(?is)^\s*(select|show|with|describe)\b")
TABLE_DENIED = re.compile(r"(?i)\b(\w+)\s+command denied to user '[^']+'.*?for table '([^']+)'")
DB_DENIED = re.compile(r"(?i)access denied for user '[^']+'.*?to database '([^']+)'")
QUALIFIED = re.compile(r"(?i)\b(?:from|into|update|table|join)\s+`?(\w+)`?\.`?\w+")
VERBS = {"select": "read", "show": "read"}  # everything else (insert, update, delete, drop, alter, create) is a change
ALREADY_UNDONE = {1051, 1091, 1146}   # unknown table, can't drop (column does not exist), table doesn't exist
_lock = threading.Lock()


class SqlExecParams(Params):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    as_: str = Field("root", alias="as")
    statement: str
    expect: Literal["ok", "refused"] = "ok"
    undo: str | None = None
    # the undo statement cannot erase who made the change: also rebuild the environment's tables when cleaning up
    rebuild: bool = False

    @model_validator(mode="after")
    def undo_is_required_for_changes(self) -> "SqlExecParams":
        if self.expect == "ok" and not IS_READ.match(self.statement) and not self.undo:
            raise ValueError(
                "a statement that changes something needs `undo:` (only reads and refused statements do not)"
            )
        return self


def is_refusal(error: BaseException) -> bool:
    """True for SingleStore's access-denied family of errors (not for syntax errors or missing tables)."""
    return isinstance(error, pymysql.MySQLError) and bool(error.args) and error.args[0] in DENIED_CODES


def refusal_parts(message: str, statement: str = "") -> tuple[str, str | None]:
    """The verb (`read` or `change`) and the database of a refusal. The server names the database only sometimes
    (`... for table 'db.t'`, `... to database 'db'`); otherwise it is taken from the statement (`FROM db.t`)."""
    on_table, on_database = TABLE_DENIED.search(message), DB_DENIED.search(message)
    verb = on_table[1] if on_table else statement.split()[0] if statement.split() else ""
    in_statement = QUALIFIED.search(statement)
    database = (
        on_database[1]
        if on_database
        else on_table[2].split(".")[0] if on_table and "." in on_table[2] else in_statement[1] if in_statement else None
    )
    return VERBS.get(verb.lower(), "change"), database


def describe_refusal(user: str, message: str, statement: str = "") -> str:
    """'The database refused: the development robot may not change tables in production.' (every database of an
    environment is just "tables in <environment>"; the exact database is in the technical details)"""
    verb, database = refusal_parts(message, statement)
    who = names.account_words(user) or user
    if database is None:
        return f"The database refused: {who} may not run this statement."
    return f"The database refused: {who} may not {verb} {names.tables_in(database)}."


def technical_refusal(user: str, message: str, statement: str = "") -> str:
    """The same sentence with the real account and database names, for the technical details."""
    verb, database = refusal_parts(message, statement)
    if database is None:
        return f"The database refused: {user} may not run this statement."
    return f"The database refused: {user} may not {verb} tables in {database}."


def sql_label(p: SqlExecParams) -> str:
    if p.expect == "refused":
        return f"Checking whether {names.account_words(p.as_) or p.as_} is allowed to do this"
    return "Reading from the database" if IS_READ.match(p.statement) else "Changing the database"


# --- undo registry --------------------------------------------------------------------------------


def _registry(ctx: Ctx) -> Path:
    return Path(ctx.settings.data_dir) / "sql_undo.json"


def _load(ctx: Ctx) -> dict[str, dict[str, str]]:
    path = _registry(ctx)
    return json.loads(path.read_text()) if path.exists() else {}


def _store(ctx: Ctx, entries: dict[str, dict[str, str]]) -> None:
    path = _registry(ctx)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(entries, indent=1))
    tmp.replace(path)


def remember_undo(ctx: Ctx, statement: str) -> str:
    undo_id = uuid.uuid4().hex[:10]
    with _lock:
        entries = _load(ctx)
        entries[undo_id] = {"statement": statement}
        _store(ctx, entries)
    return undo_id


# --- the action -----------------------------------------------------------------------------------


def execute(ctx: Ctx, user: str, statement: str) -> list[tuple]:
    conn = clients.s2_connect(ctx.settings, user, timeout=15)
    try:
        with conn.cursor() as cur:
            cur.execute(statement)
            return list(cur.fetchmany(200)) if cur.description else []
    finally:
        conn.close()


def answer_words(rows: list) -> str:
    """"The database answered with 3 rows." (singular for one); a single number is just "The database answered."
    because the number itself is on the card."""
    if len(rows) == 1 and len(rows[0]) == 1:
        return "The database answered."
    return f"The database answered with {len(rows)} row{'' if len(rows) == 1 else 's'}."


@action("sql.exec", SqlExecParams, sql_label)
def sql_exec(ctx: Ctx, p: SqlExecParams) -> Result:
    try:
        rows = execute(ctx, p.as_, p.statement)
    except pymysql.MySQLError as exc:
        text = clients.mask(ctx.settings, str(exc))
        if is_refusal(exc):
            if p.expect == "refused":
                return Result(
                    describe_refusal(p.as_, text, p.statement),
                    details=f"{technical_refusal(p.as_, text, p.statement)}\n{p.statement}\n-> {text}",
                    data={"refused": True},
                )
            raise StepFailed(
                describe_refusal(p.as_, text, p.statement), f"{technical_refusal(p.as_, text, p.statement)}\n{text}"
            ) from exc
        raise StepFailed("The database could not run the statement.", text) from exc
    if p.expect == "refused":
        who = names.account_words(p.as_) or p.as_
        raise StepFailed(f"The database allowed {who} to do this, but it should have been refused.",
                         f"{p.as_}\n{p.statement}")
    touched = {f"sql:{remember_undo(ctx, p.undo)}"} if p.undo else set()
    if p.rebuild:
        touched.add(f"models:{ctx.env}")
    summary = answer_words(rows) if IS_READ.match(p.statement) else "The database made the change."
    return Result(summary, details=p.statement, touched=touched, data={"rows": [list(r) for r in rows[:20]]})


@cleaner("sql")
def undo_sql(ctx: Ctx, resource: str) -> Result:
    """Runs the stored undo statement as root and forgets it; an unknown id means it was already undone."""
    _, undo_id = resource.split(":", 1)
    with _lock:
        entries = _load(ctx)
        entry = entries.get(undo_id)
        if entry is None:
            return Result("Nothing left to undo in the database.")
        try:
            execute(ctx, "root", entry["statement"])
        except pymysql.MySQLError as exc:
            if exc.args and exc.args[0] in ALREADY_UNDONE:   # a rebuild or an earlier clean-up got there first
                del entries[undo_id]
                _store(ctx, entries)
                return Result("The change in the database was already gone.", details=entry["statement"])
            raise StepFailed(
                "A change in the database could not be undone.",
                f"{entry['statement']}\n-> {clients.mask(ctx.settings, str(exc))}",
            ) from exc
        del entries[undo_id]
        _store(ctx, entries)
    return Result("Undid a change in the database.", details=entry["statement"])

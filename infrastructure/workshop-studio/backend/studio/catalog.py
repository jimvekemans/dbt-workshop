"""Loads and validates every scenario file against the real code that will run it.

Validation collects ALL problems and raises one `CatalogError`, so a content author fixes a file in one pass.
`python -m studio.catalog --check` prints the problems and exits non-zero (tests and content CI use it).
"""
from __future__ import annotations

import importlib
import pkgutil
import re
import sys
from dataclasses import dataclass, field
from datetime import date
from pathlib import Path
from typing import Any, Callable, Iterator

import yaml
from pydantic import ValidationError

from . import names
from .actions import base
from .config import Settings
from .models import (ENVIRONMENTS, OUTCOME_MAX, GlossaryFile, Group, GroupsFile, OutcomeCheck, Scenario, Step)
from .templating import TemplateContext, TemplateError, render, render_input, render_item

FEEDS = ("customers", "orders", "payments", "reviews", "carrier", "web")
CONDITIONS = ("paused", "late_days", "duplicate_pct", "dirty_pct", "bad_rows", "format", "format_from", "bots")
DEMO_BRANCH_KEYS = {"gitlab.branch": "name", "gitlab.commit": "branch", "gitlab.merge_request": "source",
                    "gitlab.merge": "source"}
TERM = re.compile(r"\[\[([^\]]+)\]\]")
GITLAB_PROJECT = "workshop/dbt-vdp"
INFRA_SQL_PREFIX = "../infrastructure/singlestore/"
DEV_IMAGE_PREFIX = "dev-image/"   # the developer image's recipe: `file: dev-image/Dockerfile`


class CatalogError(Exception):
    def __init__(self, problems: list[str]):
        super().__init__(f"{len(problems)} problem(s) in the scenario files:\n" + "\n".join(f"  - {p}" for p in problems))
        self.problems = problems


def load_registry() -> None:
    """Import every module that registers actions, cleaners and evidence providers."""
    from . import actions
    for module in pkgutil.iter_modules(actions.__path__):
        importlib.import_module(f"{actions.__name__}.{module.name}")
    try:
        importlib.import_module("studio.evidence")
    except ModuleNotFoundError as e:
        if e.name != "studio.evidence":
            raise


@dataclass
class Catalog:
    groups: list[Group]
    glossary: dict[str, str]
    scenarios: dict[str, Scenario]
    blocks: dict[str, list[dict[str, Any]]] = field(default_factory=dict)   # resolved code, per scenario id

    def groups_payload(self, recorded: Callable[[Scenario], bool]) -> list[dict[str, Any]]:
        """The home page: groups in file order with their scenarios in the order of the group's `scenarios` list (a
        group nobody uses is left out)."""
        out = []
        for group in self.groups:
            members = [self.scenarios[i] for i in group.scenarios if i in self.scenarios]
            if members:
                out.append({"id": group.id, "title": group.title, "blurb": group.blurb, "scenarios": [
                    {"id": s.id, "title": s.title, "promise": s.promise, "minutes": s.minutes,
                     "environment": s.environment, "related": [r.model_dump() for r in s.related],
                     "recorded": recorded(s)} for s in members]})
        return out

    def scenario_payload(self, scenario_id: str, ctx: TemplateContext,
                         usual_seconds: Callable[[str], float | None] = lambda step_id: None,
                         prepare_usual_seconds: float | None = None) -> dict[str, Any]:
        """The scenario for the UI, built field by field: only what the screen shows. Never the actions (`do`,
        `setup`), queries or checks. Code blocks are resolved, input defaults and ranges rendered."""
        sc = self.scenarios[scenario_id]
        values: dict[str, Any] = {}
        steps = []
        for step in sc.demo.steps:
            inputs = []
            for inp in step.inputs:
                rendered = render_input(inp, ctx.with_inputs(values))
                values[inp.id] = _safe_default(rendered)
                inputs.append(rendered.model_dump())
            step_ctx = ctx.with_inputs(values)
            steps.append({"id": step.id, "title": step.title, "say": _soft_render(step.say, step_ctx),
                          "button": step.button, "inputs": inputs,
                          "show": [{"type": item["type"], "title": _soft_render(item.get("title"), step_ctx),
                                    "caption": _soft_render(item.get("caption"), step_ctx),
                                    "before_of": item.get("before_of")} for item in step.show],
                          "outcome": step.outcome if isinstance(step.outcome, str) else None,
                          "usual_seconds": usual_seconds(step.id)})
        return {"id": sc.id, "group": sc.group, "title": sc.title, "promise": sc.promise, "minutes": sc.minutes,
                "environment": sc.environment, "related": [r.model_dump() for r in sc.related],
                "problem": sc.problem.model_dump(by_alias=True), "idea": sc.idea.model_dump(by_alias=True),
                "code": self.blocks[scenario_id],
                "demo": {"intro": sc.demo.intro, "steps": steps},
                "prepare_usual_seconds": prepare_usual_seconds}


def _safe_default(inp: Any) -> Any:
    try:
        return inp.coerce(inp.default)
    except ValueError:
        return inp.default


def _soft_render(text: str | None, ctx: TemplateContext) -> str | None:
    """Render a text for the UI; a text that cannot be rendered (the catalog check reports those) is shown as is."""
    if text is None:
        return None
    try:
        return str(render(text, ctx))
    except TemplateError:
        return text


# --- loading -----------------------------------------------------------------------------------------------------

def _read_yaml(path: Path, problems: list[str]) -> Any:
    try:
        return yaml.safe_load(path.read_text(encoding="utf-8"))
    except (OSError, yaml.YAMLError) as e:
        problems.append(f"{path.name}: cannot be read ({e})")
        return None


def _describe(error: ValidationError, where: str) -> list[str]:
    return [f"{where}: {'.'.join(str(p) for p in e['loc']) or '(file)'}: {e['msg']}" for e in error.errors()]


def _load_groups(path: Path, problems: list[str]) -> list[Group]:
    data = _read_yaml(path, problems)
    if isinstance(data, list):
        data = {"groups": data}
    elif isinstance(data, dict) and "groups" not in data:
        data = {"groups": [{"id": k, "title": v} for k, v in data.items()]}
    try:
        return GroupsFile.model_validate(data).groups
    except ValidationError as e:
        problems.extend(_describe(e, path.name))
        return []


def _load_glossary(path: Path, problems: list[str]) -> dict[str, str]:
    data = _read_yaml(path, problems)
    if isinstance(data, dict) and "terms" not in data:
        data = {"terms": data}
    try:
        return GlossaryFile.model_validate(data).terms
    except ValidationError as e:
        problems.extend(_describe(e, path.name))
        return {}


def load_catalog(settings: Settings, today: Callable[[], date] = date.today) -> Catalog:
    """Load `_groups.yaml`, `_glossary.yaml` and every scenario; raise CatalogError listing every problem."""
    problems: list[str] = []
    groups = _load_groups(settings.scenarios_dir / "_groups.yaml", problems)
    glossary = _load_glossary(settings.scenarios_dir / "_glossary.yaml", problems)
    files = sorted(p for p in settings.scenarios_dir.glob("*.yaml") if not p.name.startswith("_"))
    known_ids = {p.stem for p in files}
    scenarios: dict[str, Scenario] = {}
    blocks: dict[str, list[dict[str, Any]]] = {}
    for path in files:
        scenario = _load_scenario(path, problems)
        if scenario is None:
            continue
        found = _validate(scenario, path.name, settings, groups, glossary, known_ids, today)
        problems.extend(found.problems)
        scenarios[scenario.id] = scenario
        blocks[scenario.id] = found.blocks
    if not files:
        problems.append(f"{settings.scenarios_dir}: no scenario files found")
    problems.extend(_check_order(groups, scenarios))
    if problems:
        raise CatalogError(problems)
    return Catalog(groups, glossary, scenarios, blocks)


def _check_order(groups: list[Group], scenarios: dict[str, Scenario]) -> list[str]:
    """Every scenario is listed exactly once, in the list of its own group, and every listed id has a file."""
    problems: list[str] = []
    seen: dict[str, str] = {}
    for group in groups:
        for sid in group.scenarios:
            if sid in seen:
                problems.append(f"_groups.yaml: scenario '{sid}' is listed twice (in '{seen[sid]}' and '{group.id}')")
                continue
            seen[sid] = group.id
            if sid not in scenarios:
                problems.append(f"_groups.yaml: group '{group.id}' lists '{sid}', which has no scenario file")
            elif scenarios[sid].group != group.id:
                problems.append(f"_groups.yaml: scenario '{sid}' is listed under '{group.id}' but its file says "
                                f"group '{scenarios[sid].group}'")
    if groups:
        for sid in sorted(set(scenarios) - set(seen)):
            if scenarios[sid].group not in {g.id for g in groups}:
                continue   # its own file already says the group is unknown
            problems.append(f"_groups.yaml: scenario '{sid}' is missing from the list of group '{scenarios[sid].group}'")
    return problems


def _load_scenario(path: Path, problems: list[str]) -> Scenario | None:
    data = _read_yaml(path, problems)
    if data is None:
        return None
    try:
        scenario = Scenario.model_validate(data)
    except ValidationError as e:
        problems.extend(_describe(e, path.name))
        return None
    if scenario.id != path.stem:
        problems.append(f"{path.name}: id '{scenario.id}' must equal the file name")
    return scenario


# --- validation of one scenario ----------------------------------------------------------------------------------

@dataclass
class _Found:
    problems: list[str] = field(default_factory=list)
    blocks: list[dict[str, Any]] = field(default_factory=list)


def _strings(value: Any) -> Iterator[str]:
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for v in value.values():
            yield from _strings(v)
    elif isinstance(value, list):
        for v in value:
            yield from _strings(v)


def _validate(sc: Scenario, name: str, settings: Settings, groups: list[Group], glossary: dict[str, str],
              known_ids: set[str], today: Callable[[], date]) -> _Found:
    found = _Found()
    add = lambda message: found.problems.append(f"{name}: {message}")  # noqa: E731
    if groups and sc.group not in {g.id for g in groups}:
        add(f"group '{sc.group}' is not in _groups.yaml")
    for rel in sc.related:
        if rel.id == sc.id or rel.id not in known_ids:
            add(f"related id '{rel.id}' is not another scenario")
    for term in sorted({m for s in _strings(sc.model_dump(by_alias=True)) for m in TERM.findall(s)} - set(glossary)):
        add(f"glossary term [[{term}]] is not in _glossary.yaml")
    for block in sc.code:
        found.blocks.append(_resolve_block(block, settings, add))
    _validate_demo(sc, add, today)
    return found


def _validate_demo(sc: Scenario, add: Callable[[str], None], today: Callable[[], date]) -> None:
    base_ctx = TemplateContext(sc.environment, {}, today)
    values: dict[str, Any] = {}
    _check_items(sc.demo.setup, "setup", "actions", base_ctx, add)
    for step in sc.demo.steps:
        step_ctx = TemplateContext(sc.environment, values, today)
        _collect_defaults(step, step_ctx, values, add)
        where = f"step '{step.id}'"
        _check_items(step.do, f"{where} do", "actions", step_ctx, add)
        _check_items(step.show, f"{where} show", "evidence", step_ctx, add)
        _check_before_of(sc, step, where, add)
        _check_inputs(step, where, step_ctx, values, add)
        _check_outcome(step, where, step_ctx, add)


def _check_before_of(sc: Scenario, step: Step, where: str, add: Callable[[str], None]) -> None:
    earlier = {s.id: s for s in sc.demo.steps[:[s.id for s in sc.demo.steps].index(step.id)]}
    for n, item in enumerate(step.show, 1):
        target = item.get("before_of")
        if target is None:
            continue
        label = f"{where} show item {n}: before_of '{target}'"
        if target not in earlier:
            add(f"{label} must name an earlier step of this demo")
        elif n > len(earlier[target].show) or earlier[target].show[n - 1].get("type") != item.get("type"):
            add(f"{label}: that step has no {item.get('type')} item at position {n}")


def _check_inputs(step: Step, where: str, ctx: TemplateContext, values: dict[str, Any],
                  add: Callable[[str], None]) -> None:
    for inp in step.inputs:
        if not inp.mark:
            continue
        try:
            rendered = render_input(inp, ctx)
            low, high = date.fromisoformat(str(rendered.min)), date.fromisoformat(str(rendered.max))
            if not low <= date.fromisoformat(str(rendered.mark.day)) <= high:
                add(f"{where} input '{inp.id}': the mark day is outside the range of the slider")
        except (TemplateError, ValueError) as e:
            add(f"{where} input '{inp.id}': the mark day is not a usable day ({e})")


def _check_outcome(step: Step, where: str, ctx: TemplateContext, add: Callable[[str], None]) -> None:
    """Render the closing sentence(s) (an object outcome with a sample number) and check the number query."""
    with_value = ctx.with_value(123456)
    if isinstance(step.outcome, str):
        sentences, label, with_value = {"outcome": step.outcome}, where, ctx    # a plain sentence has no number
    else:
        sentences, label = {"good": step.outcome.good, "not_quite": step.outcome.not_quite}, f"{where} outcome"
        _check_query(step.outcome, label, ctx, add)
    for name, text in sentences.items():
        try:
            rendered = str(render(text, with_value))
        except TemplateError as e:
            add(f"{label} {name}: {e}")
            continue
        if len(rendered) > OUTCOME_MAX:
            add(f"{label} {name}: {len(rendered)} characters after filling in the templates, the limit is {OUTCOME_MAX}")


def _check_query(outcome: OutcomeCheck, label: str, ctx: TemplateContext, add: Callable[[str], None]) -> None:
    try:
        query = str(render(outcome.check, ctx)).strip().rstrip(";").strip()
    except TemplateError as e:
        add(f"{label} check: {e}")
        return
    if not re.match(r"(?is)^(select|with)\b", query) or ";" in query:
        add(f"{label} check: it must be a single read-only SELECT")


def _collect_defaults(step: Step, ctx: TemplateContext, values: dict[str, Any], add: Callable[[str], None]) -> None:
    """Make every input's default available to templates (as the engine does) and check the default itself."""
    for inp in step.inputs:
        try:
            rendered = render_input(inp, ctx)
            values[inp.id] = rendered.coerce(rendered.default)
        except (TemplateError, ValueError) as e:
            add(f"step '{step.id}' input '{inp.id}': {e}")
            values[inp.id] = inp.default


def _check_items(items: list[dict[str, Any]], where: str, kind: str, ctx: TemplateContext,
                 add: Callable[[str], None]) -> None:
    registry = base.ACTIONS if kind == "actions" else base.EVIDENCE
    key = "action" if kind == "actions" else "type"
    for n, item in enumerate(items, 1):
        label = f"{where} item {n}"
        name = item.get(key)
        if name not in registry:
            add(f"{label}: unknown {key} '{name}'")
            continue
        try:
            rendered, env = render_item(item, ctx)
        except TemplateError as e:
            add(f"{label} ({name}): {e}")
            continue
        if env not in ENVIRONMENTS:
            add(f"{label} ({name}): env '{env}' is not one of {', '.join(ENVIRONMENTS)}")
        if kind == "evidence" and not rendered.get("title"):
            add(f"{label} ({name}): evidence needs a title")
        skip = (key, "before_of") if kind == "evidence" else (key,)
        try:
            registry[name].params.model_validate({k: v for k, v in rendered.items() if k not in skip})
        except ValidationError as e:
            add(f"{label} ({name}): " + "; ".join(f"{'.'.join(map(str, x['loc']))}: {x['msg']}" for x in e.errors()))
        if kind == "actions":
            for problem in _action_rules(name, rendered):
                add(f"{label} ({name}): {problem}")


def _action_rules(name: str, item: dict[str, Any]) -> list[str]:
    """Rules that go beyond field types: demo-only branches, feed vocabulary, SQL needs an undo."""
    problems = []
    branch = str(item.get(DEMO_BRANCH_KEYS.get(name, ""), "demo/"))
    if name in DEMO_BRANCH_KEYS and not branch.startswith("demo/") \
            and not (name == "gitlab.branch" and re.fullmatch(r"release/\d{4}-Q[1-4]", branch)):
        problems.append(f"'{DEMO_BRANCH_KEYS[name]}' must start with demo/")
    if name.startswith("feed."):
        feeds = [item["feed"]] if "feed" in item else item.get("feeds") or []
        problems += [f"unknown feed '{f}'" for f in feeds if f not in FEEDS]
        problems += [f"unknown condition '{k}'" for k in item.get("set") or {} if k not in CONDITIONS]
        if item.get("set", {}).get("format") not in (None, "v1", "v2"):
            problems.append("format must be v1 or v2")
    if name == "sql.exec" and not item.get("undo") and item.get("expect") != "refused" \
            and not str(item.get("statement", "")).lstrip().lower().startswith("select"):
        problems.append("a statement that changes something needs an undo")
    return problems


# --- code blocks -------------------------------------------------------------------------------------------------

def _language(path: str, text: str) -> str:
    suffix = Path(path).suffix
    if suffix == ".sql":
        return "jinja-sql" if "{{" in text or "{%" in text else "sql"
    return {".yml": "yaml", ".yaml": "yaml", ".py": "python", ".sh": "bash", ".md": "markdown"}.get(suffix, "text")


def _block_path(settings: Settings, file: str) -> Path | None:
    """Where a code block's file lives. Three roots only: the dbt project, the folder with the database setup
    files (`../infrastructure/singlestore/<name>`) and the developer image's recipe (`dev-image/<name>`), both mounted
    read-only; anything else, including any other `..`, is
    refused."""
    for prefix, folder in ((INFRA_SQL_PREFIX, settings.sql_dir), (DEV_IMAGE_PREFIX, settings.dev_image_dir)):
        if file.startswith(prefix):   # a plain file name inside that one folder, nothing else
            name = file[len(prefix):]
            plain_name = name and "/" not in name and "\\" not in name and not name.startswith(".")
            return folder / name if plain_name else None
    root = settings.dbt_project_dir.resolve()
    path = (root / file).resolve()
    return path if root in path.parents else None


_DOC_QUOTES = ('"""', "'''")
_TRAILING_COMMENT = re.compile(r"\s(#|--|//)\s")
_COMMENT_STARTS = ("#", "--", "//", "{#", "/*", "*")


def readable_lines(text: list[str]) -> list[str]:
    """The lines of a block as they are shown: secrets masked, rst/markdown double backticks removed from comment and
    docstring lines, and the whitespace that every line shares at the front taken off (the file's line numbers stay)."""
    shown, in_doc = [], False
    for line in text:
        quotes = sum(line.count(q) for q in _DOC_QUOTES)
        comment = in_doc or quotes > 0 or line.lstrip().startswith(_COMMENT_STARTS)
        if quotes % 2:
            in_doc = not in_doc
        if comment:
            line = line.replace("``", "")
        elif (trailing := _TRAILING_COMMENT.search(line)):   # `x = 1  # a ``note```: only the comment part
            line = line[:trailing.end()] + line[trailing.end():].replace("``", "")
        shown.append(names.mask_secrets(line))
    common = min((len(t) - len(t.lstrip()) for t in shown if t.strip()), default=0)
    return [t[common:] if t.strip() else "" for t in shown]


def _only_match(lines: list[str], text: str, start: int = 0) -> list[int]:
    return [i for i in range(start, len(lines)) if text in lines[i]]


def _resolve_block(block: Any, settings: Settings, add: Callable[[str], None]) -> dict[str, Any]:
    """Read the block from the real dbt project: `from` must match exactly one line, `to` is the first line at or
    after it, the block may not exceed `max_lines`, every note must hit exactly one line of the block."""
    where = f"code '{block.title}' ({block.file})"
    resolved: dict[str, Any] = {"title": block.title, "file": block.file, "language": "text", "lines": [],
                                "gitlab_url": f"{settings.gitlab_url}/{GITLAB_PROJECT}/-/blob/dev/{block.file}"}
    if block.file_label:
        resolved["file_label"] = block.file_label
    path = _block_path(settings, block.file)
    if path is None:
        add(f"{where}: the file must be inside the dbt project or in {INFRA_SQL_PREFIX} or {DEV_IMAGE_PREFIX}")
        return resolved
    if block.file.startswith((INFRA_SQL_PREFIX, DEV_IMAGE_PREFIX)):
        resolved["gitlab_url"] = ""   # these files are not in the dbt project, so there is nothing to link to
    if not path.is_file():
        add(f"{where}: file not found")
        return resolved
    lines = path.read_text(encoding="utf-8").splitlines()
    starts = _only_match(lines, block.from_)
    if len(starts) != 1:
        add(f"{where}: 'from' matches {len(starts)} lines, it must match exactly one")
        return resolved
    ends = _only_match(lines, block.to, starts[0])
    if not ends:
        add(f"{where}: 'to' is not found at or after the 'from' line")
        return resolved
    first, last = starts[0], ends[0]
    if last - first + 1 > block.max_lines:
        add(f"{where}: the block has {last - first + 1} lines, the limit is {block.max_lines}")
    text = lines[first:last + 1]
    notes: dict[int, str] = {}
    for note in block.notes:
        hits = _only_match(text, note.line)
        if len(hits) != 1:
            add(f"{where}: note line '{note.line}' matches {len(hits)} lines of the block, it must match exactly one")
        elif hits[0] in notes:
            add(f"{where}: two notes point at the same line '{note.line}'")
        else:
            notes[hits[0]] = note.says
    resolved["language"] = _language(block.file, "\n".join(text))
    resolved["lines"] = [{"n": first + i + 1, "text": t, **({"note": notes[i]} if i in notes else {})}
                         for i, t in enumerate(readable_lines(text))]
    if resolved["gitlab_url"]:
        resolved["gitlab_url"] += f"#L{first + 1}-L{last + 1}"
    return resolved


def main(argv: list[str]) -> int:
    if "--check" not in argv:
        print("usage: python -m studio.catalog --check")
        return 2
    load_registry()
    try:
        catalog = load_catalog(Settings.from_env())
    except CatalogError as e:
        print(e)
        return 1
    print(f"OK: {len(catalog.scenarios)} scenarios in {len(catalog.groups)} groups")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

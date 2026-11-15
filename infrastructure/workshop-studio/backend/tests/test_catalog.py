from __future__ import annotations

import copy
from datetime import datetime
from dataclasses import replace
from pathlib import Path

import pytest
import yaml

from studio.catalog import CatalogError, load_catalog, main
from studio.templating import TemplateContext, TemplateError, render
from tests.conftest import TODAY, scenario, write_catalog


pytestmark = pytest.mark.usefixtures("fake_registry")


def problems_of(settings, tmp_path: Path, *scenarios: dict) -> list[str]:
    folder = write_catalog(tmp_path / "bad", list(scenarios))
    with pytest.raises(CatalogError) as e:
        load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)
    return e.value.problems


def mutate(path: list, value, sid: str = "demo-one") -> dict:
    """The valid scenario with the value at `path` replaced (a `None` value deletes the key)."""
    sc = copy.deepcopy(scenario(sid))
    node = sc
    for key in path[:-1]:
        node = node[key]
    if value is None:
        del node[path[-1]]
    else:
        node[path[-1]] = value
    return sc


def test_valid_catalog_loads_and_resolves_code(settings):
    catalog = load_catalog(settings, lambda: TODAY)
    assert set(catalog.scenarios) == {"demo-one", "demo-two"}
    block = catalog.blocks["demo-one"][0]
    assert block["language"] == "jinja-sql" and block["lines"][0]["n"] == 2
    noted = [line for line in block["lines"] if "note" in line]
    assert len(noted) == 1 and "where event_ts <" in noted[0]["text"]
    assert block["gitlab_url"].endswith("/models/events.sql#L2-L10")


def test_payloads(settings):
    catalog = load_catalog(settings, lambda: TODAY)
    groups = catalog.groups_payload(lambda sc: sc.id == "demo-one")
    assert [s["recorded"] for s in groups[0]["scenarios"]] == [True, False]
    payload = catalog.scenario_payload("demo-one", TemplateContext("dev", {}, lambda: TODAY))
    day = payload["demo"]["steps"][1]["inputs"][1]
    assert (day["default"], day["min"], day["max"]) == ("2026-03-09", "2026-03-03", "2026-03-10")


@pytest.mark.parametrize("sc, expected", [
    (mutate(["promise"], "x" * 111), "promise"),
    (mutate(["problem", "story"], "too short"), "40 to 90 words"),
    (mutate(["idea", "points"], ["a", "b"]), "points"),
    (mutate(["group"], "nope"), "group 'nope'"),
    (mutate(["title"], "Uses [[unknown term]]"), "glossary term [[unknown term]]"),
    (mutate(["related"], [{"id": "ghost", "why": "x"}]), "related id 'ghost'"),
    (mutate(["id"], "other-name"), "must equal the file name"),
    (mutate(["demo", "steps", 0, "do", 0, "action"], "no.such"), "unknown action 'no.such'"),
    (mutate(["demo", "steps", 0, "show", 0, "type"], "no_such"), "unknown type 'no_such'"),
    (mutate(["demo", "steps", 0, "do", 0, "key"], "{{ nonsense }}"), "unknown expression"),
    (mutate(["demo", "steps", 1, "do", 0, "value"], "{{ inputs.ghost }}"), "unknown input 'ghost'"),
    (mutate(["demo", "steps", 0, "show", 0, "title"], ""), "needs a title"),
    (mutate(["demo", "steps", 0, "do"], [{"action": "gitlab.branch", "name": "feature/x"}]), "must start with demo/"),
    (mutate(["demo", "steps", 0, "do"], [{"action": "sql.exec", "statement": "drop table t"}]), "needs an undo"),
    (mutate(["demo", "steps", 0, "do"], [{"action": "feed.condition", "feed": "tea", "set": {"mood": 1}}]),
     "unknown feed 'tea'"),
    (mutate(["demo", "steps", 0, "do"], [{"action": "fake.set", "env": "qa", "key": "k"}]), "env 'qa'"),
    (mutate(["code", 0, "notes", 0, "line"], "not in block"), "matches 0 lines"),
    (mutate(["code", 0, "from"], "select"), "'from' matches"),
    (mutate(["code", 0, "to"], "no such text"), "'to' is not found"),
    (mutate(["code", 0, "max_lines"], 3), "the limit is 3"),
    (mutate(["code", 0, "file"], "models/missing.sql"), "file not found"),
    (mutate(["demo", "steps", 1, "inputs", 1, "default"], "2020-01-01"), "not allowed"),
    (mutate(["extra_key"], 1), "extra_key"),
])
def test_each_class_of_error_is_reported(settings, tmp_path, sc, expected):
    folder = write_catalog(tmp_path / "bad", [])      # the file is always demo-one.yaml, whatever its id says
    (folder / "demo-one.yaml").write_text(yaml.safe_dump(sc, sort_keys=False))
    with pytest.raises(CatalogError) as e:
        load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)
    assert any(expected in p for p in e.value.problems), e.value.problems


def test_all_problems_are_collected_at_once(settings, tmp_path):
    first = mutate(["group"], "nope")
    second = mutate(["code", 0, "file"], "models/missing.sql", "demo-two")
    problems = problems_of(settings, tmp_path, first, second)
    assert len(problems) == 2 and "demo-one.yaml" in problems[0] and "demo-two.yaml" in problems[1]


def test_empty_scenarios_folder_is_a_problem(settings, tmp_path):
    assert any("no scenario files" in p for p in problems_of(settings, tmp_path))


def test_cli_check(settings, monkeypatch, capsys):
    monkeypatch.setenv("SCENARIOS_DIR", str(settings.scenarios_dir))
    monkeypatch.setenv("DBT_PROJECT_DIR", str(settings.dbt_project_dir))
    monkeypatch.setattr("studio.catalog.load_registry", lambda: None)
    assert main(["--check"]) == 0 and "OK: 2 scenarios" in capsys.readouterr().out
    monkeypatch.setenv("DBT_PROJECT_DIR", "/nonexistent")
    assert main(["--check"]) == 1 and "file not found" in capsys.readouterr().out


# --- templating --------------------------------------------------------------------------------------------------

def test_templating_forms():
    ctx = TemplateContext("val", {"n": 5, "d": "2026-01-01"}, lambda: TODAY, lambda: datetime(2026, 1, 2, 3, 4, 5))
    assert render("{{ env }}/{{ db('landing') }}", ctx) == "val/val_landing"
    assert render("{{ day(-4) }}", ctx) == "2026-03-06"
    assert render("{{ inputs.n }}", ctx) == 5                    # exactly one expression keeps its type
    assert render("n={{ inputs.n }}", ctx) == "n=5"
    assert render({"a": ["{{ now() }}", 3]}, ctx) == {"a": ["2026-01-02T03:04:05", 3]}


@pytest.mark.parametrize("text", ["{{ __import__('os') }}", "{{ day(x) }}", "{{ db('nope') }}", "{{ inputs.missing }}",
                                  "{{ 1 + 1 }}", "{{ env.upper() }}"])
def test_templating_rejects_everything_else(text):
    with pytest.raises(TemplateError):
        render(text, TemplateContext("dev", {}, lambda: TODAY))


def test_code_blocks_may_come_from_the_database_setup_folder_only(settings, tmp_path):
    sql_dir = tmp_path / "sql"
    sql_dir.mkdir()
    (sql_dir / "init.sql").write_text("-- one\nGRANT SELECT ON a TO b;\n-- two\n")
    block = scenario()["code"][0] | {"file": "../infrastructure/singlestore/init.sql", "from": "-- one",
                                      "to": "-- two", "notes": [{"line": "GRANT", "says": "A grant."}]}
    ok = replace(settings, scenarios_dir=write_catalog(tmp_path / "ok", [scenario(code=[block])]), sql_dir=sql_dir)
    catalog = load_catalog(ok, lambda: TODAY)
    assert catalog.blocks["demo-one"][0]["gitlab_url"] == ""
    assert [line["text"] for line in catalog.blocks["demo-one"][0]["lines"]][1] == "GRANT SELECT ON a TO b;"
    for file in ("../secrets.txt", "../infrastructure/singlestore/../x.sql", "../../etc/passwd"):
        bad = replace(settings, sql_dir=sql_dir, scenarios_dir=write_catalog(
            tmp_path / "bad2", [scenario(code=[block | {"file": file}])]))
        with pytest.raises(CatalogError):
            load_catalog(bad, lambda: TODAY)


def test_secrets_in_code_blocks_are_masked_but_notes_still_match(settings, tmp_path):
    sql_dir = tmp_path / "sql"
    sql_dir.mkdir()
    (sql_dir / "users.sql").write_text("-- one\nCREATE USER 'a'@'%' IDENTIFIED BY 'Secret2026!';\n-- two\n")
    block = scenario()["code"][0] | {"file": "../infrastructure/singlestore/users.sql", "from": "-- one", "to": "-- two",
                                      "notes": [{"line": "IDENTIFIED BY 'Secret2026!'", "says": "A user."}]}
    ok = replace(settings, scenarios_dir=write_catalog(tmp_path / "ok", [scenario(code=[block])]), sql_dir=sql_dir)
    lines = load_catalog(ok, lambda: TODAY).blocks["demo-one"][0]["lines"]
    assert lines[1]["text"] == "CREATE USER 'a'@'%' IDENTIFIED BY '••••••';" and lines[1]["note"] == "A user."

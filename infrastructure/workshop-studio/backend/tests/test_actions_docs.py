import io

import pyarrow as pa
import pyarrow.parquet as pq
import pydantic
import pytest

from studio.actions import clients, docs
from studio.actions.base import StepFailed
from test_actions_support import make_ctx

OLD = "Management data product: order volume per order month."
NEW = "Management data product: how many orders we get each month."
YML = "models:\n  - name: report\n    description: \"{}\"\n  - name: other\n    description: x\n"


def parquet(rows):
    out = io.BytesIO()
    pq.write_table(pa.table({"name": [n for n, _ in rows], "description": [d for _, d in rows], "extra": [1] * len(rows)}),
                   out, compression="zstd")
    return out.getvalue()


def test_the_description_is_read_from_the_published_table_of_tables():
    data = parquet([("other", "x"), ("report", OLD)])
    assert docs.description_in(data, "report") == OLD
    assert docs.description_in(data, "missing") is None


def test_the_description_in_the_package_is_read_from_its_yml():
    assert docs.description_in_yml(YML.format(OLD), "report") == OLD
    assert docs.description_in_yml(YML.format(OLD), "nope") is None
    assert docs.description_in_yml("", "report") is None


def test_params_only_accept_a_yml_below_models():
    docs.ReadDescriptionParams()
    for bad in ("../etc/passwd", "models/../x.yml", "dbt_project.yml", "models/a.sql"):
        with pytest.raises(pydantic.ValidationError):
            docs.ReadDescriptionParams(file=bad)


class FakeSites:
    """Each environment: the version it runs and the sentence its site shows; the package has the sentence of its version."""

    def __init__(self, monkeypatch, ctx, sites, packages):
        self.sites, self.packages = sites, packages
        monkeypatch.setattr(clients, "deployed_tag", lambda c, env: sites[env][0])
        monkeypatch.setattr(docs, "fetch", self.fetch)
        monkeypatch.setattr(docs, "package_description", lambda c, tag, p: packages[tag])
        self.env = None

    def fetch(self, settings, env, path):
        version, text = self.sites[env]
        body = parquet([("report", text)]) if path.endswith(".parquet") else b'{"generated_at":"2026-11-15T17:14:54Z"}'
        return type("R", (), {"content": body, "text": body.decode(errors="replace")})()


def test_each_site_is_shown_with_its_version_and_whether_it_matches_the_package(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    FakeSites(monkeypatch, ctx, {"dev": ("v1.0.1", NEW), "val": ("v1.0.0", OLD)}, {"v1.0.1": NEW, "v1.0.0": OLD})
    out = docs.read_description(ctx, docs.ReadDescriptionParams(model="report", expect="different"))
    assert out.data["reasons"] == ["Development: version v1.0.1, documentation page written 15 November, 18:14", NEW,
                                   "Validation: version v1.0.0, documentation page written 15 November, 18:14", OLD]
    assert out.data["same"] is False and all(s["matches_package"] for s in out.data["sites"])
    assert out.data["sites"][0]["written_at"] == "2026-11-15T17:14:54Z"


def test_a_site_that_does_not_match_its_package_says_so(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    FakeSites(monkeypatch, ctx, {"dev": ("v1.0.1", OLD), "val": ("v1.0.0", OLD)}, {"v1.0.1": NEW, "v1.0.0": OLD})
    out = docs.read_description(ctx, docs.ReadDescriptionParams(model="report"))
    assert out.data["reasons"][2] == "This is not the text of that version."
    assert out.data["same"] is True


@pytest.mark.parametrize("expect,dev_text,message", [("same", NEW, "do not say the same"), ("different", OLD, "say the same")])
def test_the_expectation_fails_in_plain_words(tmp_path, monkeypatch, expect, dev_text, message):
    ctx = make_ctx(tmp_path)
    FakeSites(monkeypatch, ctx, {"dev": ("v1.0.1", dev_text), "val": ("v1.0.0", OLD)}, {"v1.0.1": dev_text, "v1.0.0": OLD})
    with pytest.raises(StepFailed) as err:
        docs.read_description(ctx, docs.ReadDescriptionParams(model="report", expect=expect))
    assert message in err.value.summary


def test_a_site_that_is_down_or_empty_is_a_plain_failure(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    monkeypatch.setattr(docs.requests, "get", lambda *a, **k: type("R", (), {"status_code": 404})())
    with pytest.raises(StepFailed) as err:
        docs.fetch(ctx.settings, "val", docs.MODELS_FILE)
    assert err.value.summary == "The documentation page of validation has no data yet."
    assert docs.site_url(ctx.settings, "prd", "index.html") == "http://workshop-dbt-docs:8086/index.html"

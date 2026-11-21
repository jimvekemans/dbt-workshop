import io
import json
import tarfile

import pyarrow as pa
import pyarrow.parquet as pq
import pydantic
import pytest

from studio.actions import clients, docs, storage
from studio.actions.base import StepFailed
from test_actions_support import make_ctx

OLD = "Management data product: order volume per order month."
NEW = "Management data product: orders per month, counted in the month the order was placed."


def parquet(rows):
    out = io.BytesIO()
    pq.write_table(pa.table({"name": [n for n, _ in rows], "description": [d for _, d in rows]}), out)
    return out.getvalue()


def tarball(description, aliases=None):
    """A stored target.tar.gz: the documentation's table of tables and the catalogue file."""
    manifest = {"nodes": {f"model.p.{m}": {"resource_type": "model", "name": m, "alias": a}
                          for m, a in (aliases or {"fct_olist_orders": "fct_olist_orders"}).items()}}
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w:gz") as tar:
        for name, data in ((storage.DESCRIPTIONS_FILE, parquet([("mart_olist_management_kpi", description)])),
                           (storage.CATALOGUE_FILE, json.dumps(manifest).encode())):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return out.getvalue()


class FakeRegistry:
    """The GitLab calls the storage actions make: tags, packages, package files and downloads."""

    def __init__(self):
        self.tags = [
            {"name": "v1.0.0", "created_at": "2026-11-21T17:36:02Z",
             "commit": {"short_id": "eee7cc98", "parent_ids": [], "title": "feat(core): initial baseline", "committed_date": "2026-11-21T17:36:02Z"}},
            {"name": "v1.0.1", "created_at": "2026-11-21T17:50:00Z",
             "commit": {"short_id": "3f9a2b1c", "parent_ids": ["p1", "sq"], "title": "Merge branch 'demo/x' into 'dev'", "committed_date": "2026-11-21T17:49:00Z"}},
        ]
        self.packages = [
            {"id": 1, "name": "dbt-artifacts", "version": "v1.0.0"},
            {"id": 2, "name": "dbt-artifacts", "version": "v1.0.1"},
            {"id": 3, "name": "dbt-artifacts-env", "version": "dev"},
        ]
        self.files = {
            1: [{"id": 10, "file_name": "target.tar.gz", "size": 1_850_000, "file_sha256": "aaaa1111" * 8, "created_at": "2026-11-21T17:36:42Z"}],
            2: [{"id": 20, "file_name": "target.tar.gz", "size": 1_900_000, "file_sha256": "bbbb2222" * 8, "created_at": "2026-11-21T17:50:30Z"}],
            3: [{"id": 30, "file_name": "target.tar.gz", "size": 1_850_000, "file_sha256": "aaaa1111" * 8, "created_at": "2026-11-21T17:36:50Z"},
                {"id": 31, "file_name": "deployed.json", "size": 215, "file_sha256": "n1", "created_at": "2026-11-21T17:36:51Z"},
                {"id": 32, "file_name": "target.tar.gz", "size": 1_900_000, "file_sha256": "bbbb2222" * 8, "created_at": "2026-11-21T17:51:00Z"},
                {"id": 33, "file_name": "deployed.json", "size": 215, "file_sha256": "n2", "created_at": "2026-11-21T17:51:01Z"}],
        }
        self.downloads = {("dbt-artifacts", "v1.0.0", "target.tar.gz"): tarball(OLD, {"fct_olist_orders": "fct_olist_orders"}),
                          ("dbt-artifacts", "v1.0.1", "target.tar.gz"): tarball(NEW),
                          ("dbt-artifacts-env", "dev", "deployed.json"): json.dumps(
                              {"version": "v1.0.1", "environment": "dev", "sha256": "bbbb2222" * 8, "deployed_at": "2026-11-21T17:51:01Z"}).encode()}
        self.deleted: list[str] = []

    def p(self, path):
        return path

    def maybe(self, path):   # the squashed change behind the merge that carries v1.0.1
        return {"title": "Say which month an order is counted in"} if path == "/repository/commits/sq" else None

    def get(self, path, **params):
        if path == "/repository/tags":
            return self.tags
        if path == "/packages":
            return [p for p in self.packages if params.get("package_name", "") in p["name"]]   # GitLab's fuzzy name filter
        return [dict(f, package_id=int(path.split("/")[2])) for f in self.files[int(path.split("/")[2])]]

    def req(self, method, path, ok=(200,), **kw):
        if method == "DELETE":
            self.deleted.append(path)
            return type("R", (), {"status_code": 204})()
        key = tuple(path.split("/")[3:])
        data = self.downloads.get(key)
        return type("R", (), {"status_code": 200 if data else 404, "content": data or b""})()


@pytest.fixture
def registry(monkeypatch):
    fake = FakeRegistry()
    monkeypatch.setattr(clients, "GitLabApi", lambda settings: fake)
    return fake


def test_numbers_and_codes_are_the_same_changes_seen_two_ways(tmp_path, registry):
    ctx = make_ctx(tmp_path)
    numbers = storage.versions(ctx, storage.VersionsParams(show="numbers"))
    assert numbers.data["reasons"] == ["v1.0.1: Say which month an order is counted in", "v1.0.0: De eerste versie"]
    codes = storage.versions(ctx, storage.VersionsParams(show="codes"))
    assert codes.data["reasons"] == ["Code 3f9a2b1c: Say which month an order is counted in", "Code eee7cc98: De eerste versie"]
    assert "v1.0" not in " ".join(codes.data["reasons"])


def test_stored_versions_say_when_and_how_big(tmp_path, registry):
    out = storage.versions(make_ctx(tmp_path), storage.VersionsParams(show="stored"))
    assert out.summary == "De storage bevat 2 versies; de nieuwste is v1.0.1."
    assert out.data["reasons"][0].startswith("v1.0.1: opgeslagen op 21 november, 18:50, 1,9 MB")
    assert out.data["reasons"][1].startswith("v1.0.0: opgeslagen op 21 november, 18:36, 1,9 MB") or "1,8 MB" in out.data["reasons"][1]


def test_an_empty_storage_is_a_plain_failure(tmp_path, registry):
    registry.packages = []
    with pytest.raises(StepFailed) as err:
        storage.versions(make_ctx(tmp_path), storage.VersionsParams(show="stored"))
    assert err.value.summary == "De storage bevat nog geen versie."


def test_the_note_names_the_one_stored_version_its_fingerprint_equals(tmp_path, registry):
    out = storage.environment_copy(make_ctx(tmp_path), storage.EnvironmentCopyParams(show="note", expect_match=True))
    assert out.data["matches"] == ["v1.0.1"] and out.data["note"]["version"] == "v1.0.1"
    assert out.summary == "De kopie van development is gelijk aan het opgeslagen bestand van v1.0.1, en aan geen enkele andere opgeslagen versie."
    assert out.data["reasons"][1] == "Fingerprint van de kopie: bbbb2222"
    assert out.data["reasons"][2].endswith("bbbb2222 (dezelfde)") and not out.data["reasons"][3].endswith("(dezelfde)")


def test_a_copy_that_is_not_what_its_note_says_fails(tmp_path, registry):
    registry.downloads[("dbt-artifacts-env", "dev", "deployed.json")] = json.dumps({"version": "v1.0.0"}).encode()
    with pytest.raises(StepFailed) as err:
        storage.environment_copy(make_ctx(tmp_path), storage.EnvironmentCopyParams(show="note", expect_match=True))
    assert "komt niet exact overeen met de versie uit de notitie" in err.value.summary


def test_the_copy_alone_shows_time_and_size_but_no_version(tmp_path, registry):
    out = storage.environment_copy(make_ctx(tmp_path), storage.EnvironmentCopyParams(show="copy"))
    assert out.data["reasons"][0] == "De kopie van development is opgeslagen op 21 november, 18:51 en is 1,9 MB groot."
    assert "geen versie" in out.data["reasons"][1]


def test_an_environment_without_a_copy_says_so(tmp_path, registry):
    registry.packages = [p for p in registry.packages if p["name"] != "dbt-artifacts-env"]
    with pytest.raises(StepFailed) as err:
        storage.environment_copy(make_ctx(tmp_path), storage.EnvironmentCopyParams())
    assert err.value.summary == "Van development staat nog geen kopie in storage."


def test_the_stored_description_of_an_older_version_can_still_be_read(tmp_path, registry):
    ctx = make_ctx(tmp_path)
    old = storage.description(ctx, storage.DescriptionParams(version="previous"))
    new = storage.description(ctx, storage.DescriptionParams(version="latest"))
    assert (old.data["version"], old.data["description"]) == ("v1.0.0", OLD)
    assert (new.data["version"], new.data["description"]) == ("v1.0.1", NEW)
    assert old.data["reasons"][0] == "Opgeslagen bij versie v1.0.0 op 21 november, 18:36:"


def test_a_version_that_is_not_stored_fails_in_plain_words(tmp_path, registry):
    with pytest.raises(StepFailed) as err:
        storage.description(make_ctx(tmp_path), storage.DescriptionParams(version="v9.9.9"))
    assert err.value.summary == "Versie v9.9.9 staat niet in storage."
    registry.packages = registry.packages[:1] + registry.packages[2:]
    with pytest.raises(StepFailed) as err:
        storage.description(make_ctx(tmp_path), storage.DescriptionParams(version="previous"))
    assert "geen vorige" in err.value.summary


def test_params_only_accept_known_versions_and_table_names():
    storage.DescriptionParams(version="v1.2.3")
    storage.CatalogueNamesParams(models=["fct_olist_orders"], compare_with="developer:pu_dev1")
    for bad in ({"version": "newest"}, {"models": ["x; drop"]}, {"compare_with": "service"}):
        with pytest.raises(pydantic.ValidationError):
            storage.CatalogueNamesParams.model_validate(bad)
    with pytest.raises(pydantic.ValidationError):
        storage.DescriptionParams(version="main")


def test_catalogue_names_next_to_a_developers_own_compile(tmp_path, registry, monkeypatch):
    ctx = make_ctx(tmp_path)
    registry.downloads[("dbt-artifacts", "v1.0.1", "target.tar.gz")] = tarball(NEW, {"fct_olist_orders": "fct_olist_orders", "dim_olist_customers": "dim_olist_customers"})
    monkeypatch.setattr(storage, "run_dbt", lambda c, env, who, cmd, timeout=0: clients.Proc(0, "\n".join(
        json.dumps({"name": m, "alias": f"pu_dev1_{m}"}) for m in ("fct_olist_orders", "dim_olist_customers")), ""))
    out = storage.catalogue_names(ctx, storage.CatalogueNamesParams(models=["fct_olist_orders", "dim_olist_customers"], compare_with="developer:pu_dev1"))
    assert out.data["reasons"] == [
        {"label": "Bestellingentabel", "left": "fct_olist_orders", "right": "pu_dev1_fct_olist_orders", "same": False},
        {"label": "Klantentabel", "left": "dim_olist_customers", "right": "pu_dev1_dim_olist_customers", "same": False}]
    assert out.data["label_title"] == "Tabel" and out.data["right_title"] == "In de eigen versie van een developer"
    alone = storage.catalogue_names(ctx, storage.CatalogueNamesParams(models=["fct_olist_orders"]))
    assert alone.data["reasons"] == [{"label": "Bestellingentabel", "left": "fct_olist_orders"}]


def test_a_catalogue_file_without_a_table_asked_for_fails(tmp_path, registry):
    with pytest.raises(StepFailed) as err:
        storage.catalogue_names(make_ctx(tmp_path), storage.CatalogueNamesParams(models=["fct_olist_orders", "dim_olist_customers"]))
    assert "niet elke gevraagde tabel" in err.value.summary


def test_a_developers_compile_that_names_too_few_tables_fails(tmp_path, monkeypatch):
    monkeypatch.setattr(storage, "run_dbt", lambda *a, **k: clients.Proc(0, json.dumps({"name": "a", "alias": "pu_a"}), ""))
    with pytest.raises(StepFailed):
        storage.compiled_aliases(make_ctx(tmp_path), "developer:pu_dev1", ["a", "b"])


def test_two_sealed_packages_are_opened_side_by_side(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    monkeypatch.setattr(storage, "built_tags", lambda c: ["v1.0.0", "v1.0.1"])
    monkeypatch.setattr(storage.images, "list_tools", lambda c, image: {"report_builder": "2.0.11", "connector": "abc123def456"})
    monkeypatch.setattr(storage.docs, "package_description", lambda c, tag, p: OLD if tag == "v1.0.0" else NEW)
    monkeypatch.setattr(storage, "printed_version", lambda c, tag: tag)
    out = storage.open_package(ctx, storage.OpenPackageParams())
    rows = {r["label"]: r for r in out.data["reasons"]}
    assert rows["Nummer dat erop gedrukt staat"]["left"] == "v1.0.0" and rows["Nummer dat erop gedrukt staat"]["right"] == "v1.0.1"
    assert rows["Rapportbouwer"] == {"label": "Rapportbouwer", "left": "versie 2.0.11", "right": "versie 2.0.11", "same": True}
    assert rows["Databaseconnector"]["same"] is True and rows["Databaseconnector"]["left"] == "dezelfde build"
    assert rows["Beschrijving van het rapport in de code"]["same"] is False
    assert out.data["left_title"] == "Package v1.0.0" and out.data["right_title"] == "Package v1.0.1"


def test_a_package_that_was_never_built_cannot_be_opened(tmp_path, monkeypatch):
    monkeypatch.setattr(storage, "built_tags", lambda c: ["v1.0.0"])
    with pytest.raises(StepFailed):
        storage.open_package(make_ctx(tmp_path), storage.OpenPackageParams(versions=["v1.0.1"]))


def test_stored_packages_of_versions_whose_tag_is_gone_are_dropped(registry):
    assert storage.drop_orphans(registry, {"v1.0.0"}) == ["v1.0.1"]
    assert registry.deleted == ["/packages/2"]
    registry.deleted.clear()
    assert storage.drop_orphans(registry, {"v1.0.0", "v1.0.1"}) == [] and registry.deleted == []


def test_the_copy_of_the_environment_goes_back_to_the_baseline_pair(tmp_path, registry):
    ctx = make_ctx(tmp_path)
    assert storage.restore_env_copy(ctx, "dev") == "2 newer files removed"
    assert registry.deleted == ["/packages/3/package_files/32", "/packages/3/package_files/33"]
    registry.deleted.clear()
    registry.files[3] = registry.files[3][:2]
    assert storage.restore_env_copy(ctx, "dev") == "already as at the baseline" and registry.deleted == []


def test_a_copy_without_a_baseline_pair_is_removed_whole(tmp_path, registry):
    registry.packages.append({"id": 4, "name": "dbt-artifacts-env", "version": "val"})
    registry.files[4] = [{"id": 40, "file_name": "target.tar.gz", "size": 1, "file_sha256": "bbbb2222" * 8, "created_at": "2026-11-21T17:55:00Z"}]
    assert storage.restore_env_copy(make_ctx(tmp_path), "val") == "copy removed"
    assert registry.deleted == ["/packages/4"]
    assert storage.restore_env_copy(make_ctx(tmp_path), "prd") == "no copy in storage"


def test_the_front_page_is_read_from_the_docs_blocks_of_the_site(tmp_path, monkeypatch):
    page = ("# Data platform\n\n- **Version:** `v1.0.1`\n- **Environment:** dev\n- **Generated:** 2026-11-21 17:46 UTC\n"
            "- **Tables:** 30, **checks:** 36, **feeds:** 14\n\n## Teams\n\n- `a` (a@x.org)\n- `b` (b@x.org)\n")
    standard = "### Welcome!"

    def blocks(rows):
        out = io.BytesIO()
        pq.write_table(pa.table({"name": ["__overview__"] * len(rows), "package_name": [r[0] for r in rows], "content": [r[1] for r in rows]}), out)
        return out.getvalue()

    both = blocks([("dbt", standard), ("dbt_swf", page)])
    assert docs.front_page_in(both) == page and docs.front_page_in(blocks([("dbt", standard)])) is None
    parsed = docs.parse_front_page(page)
    assert parsed == {"written": True, "version": "v1.0.1", "written_at": "2026-11-21T17:46:00Z", "with_seconds": False, "tables": 30, "checks": 36, "feeds": 14, "teams": 2}
    assert docs.front_page_lines("Development", parsed) == [
        "Development: de startpagina vermeldt versie v1.0.1, geschreven op 21 november, 18:46.",
        "Ze telt 30 tabellen, 36 checks en 14 feeds en noemt 2 teams."]
    precise = docs.parse_front_page(page.replace("17:46 UTC", "17:46:35 UTC"))   # newer versions of the script write the second too
    assert precise["written_at"] == "2026-11-21T17:46:35Z"
    assert docs.front_page_lines("Development", precise)[0] == "Development: de startpagina vermeldt versie v1.0.1, geschreven op 21 november, 18:46:35."
    ctx = make_ctx(tmp_path)
    monkeypatch.setattr(docs, "fetch", lambda s, env, path: type("R", (), {"content": both})())
    monkeypatch.setattr(clients, "deployed_tag", lambda c, env: "v1.0.1")
    out = docs.read_front_page(ctx, docs.FrontPageParams(expect="running"))
    assert out.data["pages"][0]["version"] == "v1.0.1" and out.data["reasons"][0].startswith("Development: de startpagina vermeldt versie v1.0.1")
    monkeypatch.setattr(clients, "deployed_tag", lambda c, env: "v1.0.2")
    with pytest.raises(StepFailed) as err:
        docs.read_front_page(ctx, docs.FrontPageParams(expect="running"))
    assert err.value.summary == "De startpagina van development vermeldt niet de versie die daar runt."


def test_the_standard_front_page_names_nothing(tmp_path, monkeypatch):
    out = io.BytesIO()
    pq.write_table(pa.table({"name": ["__overview__"], "package_name": ["dbt"], "content": ["### Welcome!"]}), out)
    monkeypatch.setattr(docs, "fetch", lambda s, env, path: type("R", (), {"content": out.getvalue()})())
    monkeypatch.setattr(clients, "deployed_tag", lambda c, env: "v1.0.0")
    got = docs.read_front_page(make_ctx(tmp_path), docs.FrontPageParams())
    assert got.data["reasons"] == ["Development: de startpagina is de standaardpagina. Ze vermeldt geen versie, geen tijdstip en geen grootte."]
    with pytest.raises(StepFailed):
        docs.read_front_page(make_ctx(tmp_path), docs.FrontPageParams(expect="running"))

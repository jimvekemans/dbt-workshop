"""Backend support of the scenarios same-toolbox, only-what-changed, docs-match-version and production-waits."""
import pytest

from studio import names
from studio.actions import clients, gitlab, images
from studio.actions.dbt import DEV_IMAGE
from test_actions_images import PRODUCTION, TOOLBOX
from test_actions_support import make_ctx


def log(models, closing="Finished 'build' successfully for target 'ci' [1.9s]"):
    lines = ["Finished 'compile' successfully for target 'run' [2.1s]", "Processed: 1 hook | 28 models | 21 tests"]
    lines += [f"Succeeded model demo_x.demo_x_{m} (table) [{i} of {len(models)} in 0.4s]" for i, m in enumerate(models, 1)]
    return "\n".join(lines + ["", closing, f"Processed: 1 hook | {len(models)} models", "Summary: x total"]) + "\n"


# --- what a rehearsal covered ----------------------------------------------------------------------------------

@pytest.mark.parametrize("closing,seconds", [
    ("Finished 'build' successfully for target 'ci' [518ms]", 0.52), ("Finished 'build' successfully for target 'ci' [1.9s]", 1.9),
    ("Finished 'build' with errors for target 'ci' [2m]", 120.0)])
def test_the_build_time_is_the_one_in_the_closing_line_of_the_build(closing, seconds):
    assert gitlab.seconds_in(log(["dim_olist_customers"], closing)) == seconds   # not the compile's 2.1s


def test_no_closing_line_means_no_time():
    assert gitlab.seconds_in("Succeeded model a (table)\n") is None
    assert gitlab.seconds_in("Finished 'compile' successfully [2.1s]\n") is None


def test_names_are_plain_and_each_table_once():
    assert gitlab.names_in(log(["mart_olist_management_kpi"])) == ["Management report table"]
    assert gitlab.names_in(log(["dim_olist_customers", "fct_olist_orders", "dim_olist_customers"])) == [
        "Customers table", "Orders table"]
    assert gitlab.names_in("nothing here") == []


def test_more_than_six_names_end_in_one_entry_with_the_rest():
    found = gitlab.names_in(log([f"int_table_{i}" for i in range(28)]))
    assert len(found) == 7 and found[-1] == "and 22 more" and found[0] == "Table 0 table"
    assert len(gitlab.names_in(log([f"int_table_{i}" for i in range(6)]))) == 6   # exactly six: no "and 0 more"


@pytest.mark.parametrize("seconds,words", [(0.4, "1 second"), (1.4, "1 second"), (1.9, "2 seconds"), (59.6, "60 seconds"),
                                           (125, "2 minutes and 5 seconds"), (180, "3 minutes")])
def test_durations_are_whole_plain_words(seconds, words):
    assert gitlab.duration_words(seconds) == words


def test_the_summary_sentence_has_the_tables_the_data_checks_and_the_time():
    found = {"tables": 1, "checks": 0, "tables_total": 28, "seconds": 1.9}
    assert gitlab.rehearsed_words(found) == "1 of 28 tables and 0 data checks in 2 seconds"
    assert gitlab.rehearsed_words({**found, "seconds": None}) == "1 of 28 tables and 0 data checks"


# --- local time -------------------------------------------------------------------------------------------------

def test_times_are_local_and_never_say_utc():
    assert names.local_time_words("2026-11-15T17:06:00.367Z") == "15 November, 18:06"
    assert names.local_time_words("2026-07-01T10:00:00Z") == "1 July, 12:00"   # summer time
    assert names.local_time_words("nonsense") is None and names.local_time_words(None) is None
    assert "UTC" not in str(gitlab.approval_lines({"user": {"name": "A"}, "started_at": "2026-11-15T17:06:00Z"}))


# --- the toolbox against a second toolbox -----------------------------------------------------------------------

def fake_two(monkeypatch, second=TOOLBOX):
    ran = []

    def run_container(ctx, image, command, **kw):
        ran.append(image)
        return clients.Proc(0, TOOLBOX if len(ran) == 1 or image != DEV_IMAGE else second, "")

    monkeypatch.setattr(clients, "run_container", run_container)
    return ran


def test_two_fresh_toolboxes_are_opened_and_every_line_is_the_same(tmp_path, monkeypatch):
    ran = fake_two(monkeypatch)
    out = images.versions(make_ctx(tmp_path), images.ImageVersionsParams(compare_with="toolbox", expect_same=["report_builder"]))
    assert ran == [DEV_IMAGE, DEV_IMAGE]
    assert (out.data["left_title"], out.data["right_title"]) == ("First toolbox", "Second toolbox")
    assert len(out.data["reasons"]) == 6 and all(r["same"] for r in out.data["reasons"])
    assert out.data["reasons"][1] == {"label": "Database connector", "left": "the same build", "right": "the same build", "same": True}
    assert set(out.data["reasons"][0]) == {"label", "left", "right", "same"}
    assert out.summary == "Opened two fresh toolboxes and compared their tools." and "production_version" not in out.data


def test_a_different_connector_build_is_said_in_words_and_the_hash_stays_in_the_details(tmp_path, monkeypatch):
    fake_two(monkeypatch, TOOLBOX.replace("74194c79b0fc", "aaaaaaaaaaaa"))
    out = images.versions(make_ctx(tmp_path), images.ImageVersionsParams(compare_with="toolbox"))
    assert out.data["reasons"][1] == {"label": "Database connector", "left": "this build", "right": "a different build", "same": False}
    assert "74194c7" in out.details and "aaaaaaa" in out.details
    assert "74194c7" not in str(out.data["reasons"])


def test_a_tool_missing_on_one_side_has_no_right_text(tmp_path, monkeypatch):
    monkeypatch.setattr(clients, "run_container", lambda ctx, image, command, **kw: clients.Proc(
        0, TOOLBOX if image == DEV_IMAGE else PRODUCTION, ""))
    monkeypatch.setattr(clients, "deployed_tag", lambda ctx, env: "v1.0.0")
    out = images.versions(make_ctx(tmp_path), images.ImageVersionsParams(compare_with="production"))
    missing = [r for r in out.data["reasons"] if r["right"] is None]
    assert [r["label"] for r in missing] == ["Layout robot", "Rule checker", "Helper tool", "Pre-save checks"]
    assert all(r["same"] is False for r in missing)
    assert sum(r["same"] for r in out.data["reasons"]) == 2


def test_the_schema_prefix_of_a_private_copy_is_dropped_before_naming():
    trace = ("Succeeded model demo_step_after_every_table.demo_step_after_every_table_dbt_table_grants (table) [1 of 2 in 0.3s]\n"
             "Succeeded model demo_step_after_every_table.demo_step_after_every_table_dim_olist_customers (table) [2 of 2 in 0.3s]\n")
    assert gitlab.names_in(trace) == ["Access rights", "Customers table"]

"""`precommit.run`: what a developer's computer does when they try to save a change, run on a scratch copy.

The project of the baseline image is copied into a fresh container of the developer image, the patch is applied, a git
repository is created and `pre-commit run --all-files` runs the hooks of `.pre-commit-config.yaml`: sqlfmt (layout of
SQL), `dbt parse` (the project still reads) and dbt-bouncer (the rules of `dbt-bouncer.yml`). Nothing is kept.
"""

from __future__ import annotations

import io
import re
import tarfile
import tempfile
from pathlib import Path
from typing import Literal

from ..patches import Patch, apply_to_tree, load_patch
from . import clients
from .base import Ctx, Params, Result, StepFailed, action
from .dbt import DEV_IMAGE

HOOK_RESULT = re.compile(r"^(.+?)\.{3,}\s*(Passed|Failed|Skipped)", re.MULTILINE)
HOOK_WORDS = {
    "sqlfmt": "De code kon niet opgemaakt worden zoals de regels van het team vragen.",
    "dbt parse": "Het project kan niet meer correct gelezen worden.",
}
# The rules of dbt-bouncer.yml in plain words. dbt-bouncer prints `<check>:<position>:<model>`, the position being the
# index of the check among those of the same kind in dbt-bouncer.yml (to_char is the third check of the file, so
# `regexp_pattern:2`; str_to_date the fourth, `:3`). Keep this list in step with that file.
RULE_WORDS = (  # (regular expression on the output, the rule as one plain line)
    (r"check_model_has_semi_colon", "De code eindigt met een puntkomma."),
    (r"check_model_hard_coded_references", "De code verwijst rechtstreeks naar een tabel, in plaats van via het project."),
    (r"regexp_pattern:2:", "De code gebruikt een datumfunctie die het team verboden heeft (to_char)."),
    (r"regexp_pattern:3:", "De code gebruikt een datumfunctie die het team verboden heeft (str_to_date)."),
)
SQL_SUMS = "find models -name '*.sql' -exec md5sum {} + | sort"
SCRIPT = (
    "cd /usr/app/dbt/workspace && git init -q && git add -A && " + SQL_SUMS + " > /tmp/before.md5; "
    "pre-commit run --all-files 2>&1; echo \"=== tidied $(" + SQL_SUMS + " | diff /tmp/before.md5 - | grep -c '^>')\""
)
TIDIED = re.compile(r"^=== tidied (\d+)$", re.MULTILINE)


class PreCommitParams(Params):
    patch: str
    expect: Literal["ok", "refused"] = "ok"


def patched_project(archive: bytes, patch: Patch) -> bytes:
    """The project tar of `archive` with the patch applied, files owned by the container's dbt user (1000:0)."""

    def owned_by_dbt(member: tarfile.TarInfo) -> tarfile.TarInfo:
        member.uid, member.gid, member.uname, member.gname = 1000, 0, "", ""
        return member

    with tempfile.TemporaryDirectory() as tmp:
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            tar.extractall(tmp, filter="data")
        apply_to_tree(patch, Path(tmp) / "workspace")
        out = io.BytesIO()
        with tarfile.open(fileobj=out, mode="w") as tar:
            tar.add(Path(tmp) / "workspace", arcname="workspace", filter=owned_by_dbt)
        return out.getvalue()


def refusal_reasons(output: str) -> list[str]:
    """The failed hooks (and, for dbt-bouncer, the broken rule) in plain words."""
    failed = [name.strip() for name, state in HOOK_RESULT.findall(output) if state == "Failed"]
    reasons = []
    for hook in failed:
        if hook == "dbt-bouncer":
            reasons += [words for text, words in RULE_WORDS if re.search(text, output)] or [
                "Een regel van het team over de code is overtreden."
            ]
        else:
            reasons.append(HOOK_WORDS.get(hook, f"De check '{hook}' is mislukt."))
    return reasons


@action("precommit.run", PreCommitParams, lambda p: "De wijziging nakijken op de regels van het team")
def precommit_run(ctx: Ctx, p: PreCommitParams) -> Result:
    patch = load_patch(ctx.settings.patches_dir, p.patch)
    project = patched_project(clients.project_tar(ctx, f"dbt-vdp:{ctx.settings.baseline_tag}"), patch)
    proc = clients.run_container(
        ctx,
        DEV_IMAGE,
        ["bash", "-c", SCRIPT],
        project=project,
        timeout=600,
        env={
            "DBT_DEPLOY_TARGET": ctx.env,
            "SINGLESTORE_HOST": ctx.settings.s2_host,
            "SINGLESTORE_USER": "svc_ci_dbt",
            "HOME": "/tmp",
        },
        secret_env={"SINGLESTORE_PASS": clients.password_for(ctx.settings, "svc_ci_dbt")},
    )
    details = "\n".join(proc.text.splitlines()[-60:])
    if not HOOK_RESULT.search(proc.text):
        raise StepFailed("De robot kon de wijziging niet nakijken.", details)
    reasons = refusal_reasons(proc.text)
    count = f"{len(reasons)} van de regels van het team"
    if reasons and p.expect == "ok":
        raise StepFailed(f"De robot weigerde de wijziging: ze overtreedt {count}.", "\n".join(reasons) + "\n" + details)
    if not reasons and p.expect == "refused":
        raise StepFailed("De robot had de wijziging moeten weigeren, maar keurde ze goed.", details)
    tidied = int(m.group(1)) if (m := TIDIED.search(proc.text)) else 0
    tidy = f"De robot kuiste de opmaak van {tidied} {'bestanden' if tidied != 1 else 'bestand'} op en " if tidied else ""
    if reasons:
        summary = f"{tidy or 'De robot '}weigerde de wijziging: ze overtreedt {count}."
    else:
        summary = f"{tidy or 'De robot '}keurde de wijziging goed: ze volgt de regels van het team."
    return Result(summary, details=details, data={"refused": bool(reasons), "reasons": reasons, "tidied": tidied})

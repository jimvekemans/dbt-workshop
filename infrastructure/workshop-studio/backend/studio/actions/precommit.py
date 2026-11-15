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
    "sqlfmt": "The code could not be laid out the way the team's rules ask.",
    "dbt parse": "The project no longer reads correctly.",
}
# The rules of dbt-bouncer.yml in plain words. dbt-bouncer prints `<check>:<position>:<model>`, the position being the
# index of the check among those of the same kind in dbt-bouncer.yml (to_char is the third check of the file, so
# `regexp_pattern:2`; str_to_date the fourth, `:3`). Keep this list in step with that file.
RULE_WORDS = (  # (regular expression on the output, the rule as one plain line)
    (r"check_model_has_semi_colon", "The code ends with a semicolon."),
    (r"check_model_hard_coded_references", "It names a table directly instead of going through the project."),
    (r"regexp_pattern:2:", "It uses a date function the team has banned (to_char)."),
    (r"regexp_pattern:3:", "It uses a date function the team has banned (str_to_date)."),
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
                "A rule of the team about the code is broken."
            ]
        else:
            reasons.append(HOOK_WORDS.get(hook, f"The check '{hook}' failed."))
    return reasons


@action("precommit.run", PreCommitParams, lambda p: "Checking the change against the team's rules")
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
        raise StepFailed("The robot could not check the change.", details)
    reasons = refusal_reasons(proc.text)
    count = f"{len(reasons)} of the team's rules"
    if reasons and p.expect == "ok":
        raise StepFailed(f"The robot refused the change: it breaks {count}.", "\n".join(reasons) + "\n" + details)
    if not reasons and p.expect == "refused":
        raise StepFailed("The robot was expected to refuse the change but accepted it.", details)
    tidied = int(m.group(1)) if (m := TIDIED.search(proc.text)) else 0
    tidy = f"The robot tidied the layout of {tidied} file{'s' if tidied != 1 else ''} and " if tidied else ""
    if reasons:
        summary = f"{tidy or 'The robot '}refused the change: it breaks {count}."
    else:
        summary = f"{tidy or 'The robot '}accepted the change: it follows the team's rules."
    return Result(summary, details=details, data={"refused": bool(reasons), "reasons": reasons, "tidied": tidied})

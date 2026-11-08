"""Patch sets (workshop-studio/patches/<name>/): repo-relative files plus an optional `_patch.yaml`
(title, commit message, branch, find/replace `ops`). The studio commits them to *feature branches* via the
GitLab API - `main` is never touched directly."""
from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

import yaml

META = "_patch.yaml"


class PatchError(RuntimeError):
    pass


@dataclass
class Patch:
    name: str
    title: str
    message: str
    branch: str
    files: dict[str, str] = field(default_factory=dict)         # repo path -> full new content
    ops: list[dict[str, str]] = field(default_factory=list)     # {path, find, replace}

    def touched_paths(self) -> list[str]:
        return sorted(set(self.files) | {o["path"] for o in self.ops})


def load_patch(patches_dir: Path, name: str) -> Patch:
    root = patches_dir / name
    if not root.is_dir():
        raise PatchError(f"unknown patch set {name!r} (looked in {patches_dir})")
    meta = yaml.safe_load((root / META).read_text()) if (root / META).exists() else {}
    files = {
        p.relative_to(root).as_posix(): p.read_text()
        for p in sorted(root.rglob("*")) if p.is_file() and p.name != META
    }
    return Patch(name=name, title=meta.get("title", name), message=meta.get("message", f"chore: apply {name}"),
                 branch=meta.get("branch", f"feature/{name}"), files=files, ops=meta.get("ops", []))


def resolve_contents(patch: Patch, read: Callable[[str], str | None]) -> dict[str, str]:
    """Final content for every touched path. `read(path)` returns the current content (or None if absent)."""
    out = dict(patch.files)
    for op in patch.ops:
        cur = out.get(op["path"])
        if cur is None:
            cur = read(op["path"])
        if cur is None:
            raise PatchError(f"{patch.name}: {op['path']} does not exist, cannot apply find/replace")
        hits = cur.count(op["find"])
        if hits != 1:   # an anchor that matches twice would insert twice
            what = "not found" if hits == 0 else f"found {hits} times (must be unique)"
            raise PatchError(f"{patch.name}: pattern {what} in {op['path']}: {op['find'][:60]!r}")
        out[op["path"]] = cur.replace(op["find"], op["replace"])
    return out


def apply_to_tree(patch: Patch, root: Path) -> list[str]:
    """Write the patch into a checked-out working tree (used by git scripts and tests)."""
    def read(path: str) -> str | None:
        p = root / path
        return p.read_text() if p.exists() else None

    contents = resolve_contents(patch, read)
    for path, text in contents.items():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    return sorted(contents)

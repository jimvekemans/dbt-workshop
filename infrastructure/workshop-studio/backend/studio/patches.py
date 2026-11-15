"""Patch sets (`patches/<name>/`): repo-relative files holding their full new content, plus an optional `_patch.yaml`
(title, commit message, find/replace `ops`). A demo commits a patch to a `demo/*` branch through the GitLab API,
so `dev` and `main` are never touched directly."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

import yaml

from .actions.base import StepFailed

META = "_patch.yaml"


class PatchError(StepFailed):
    """The prepared code change cannot be applied; `details` says which file and pattern."""

    def __init__(self, details: str):
        super().__init__("The prepared code change does not fit the current code.", details)


@dataclass
class Patch:
    name: str
    title: str
    message: str
    files: dict[str, str] = field(default_factory=dict)  # repo path -> full new content
    ops: list[dict[str, str]] = field(default_factory=list)  # {path, find, replace}

    def touched_paths(self) -> list[str]:
        return sorted(set(self.files) | {op["path"] for op in self.ops})


def load_patch(patches_dir: Path, name: str) -> Patch:
    root = Path(patches_dir) / name
    if not root.is_dir():
        raise PatchError(f"unknown patch set {name!r} (looked in {patches_dir})")
    meta = yaml.safe_load((root / META).read_text()) if (root / META).exists() else {}
    files = {
        p.relative_to(root).as_posix(): p.read_text() for p in sorted(root.rglob("*")) if p.is_file() and p.name != META
    }
    return Patch(
        name=name,
        title=meta.get("title", name),
        message=meta.get("message", f"Apply {name}"),
        files=files,
        ops=meta.get("ops", []),
    )


def resolve_contents(patch: Patch, read: Callable[[str], str | None]) -> dict[str, str]:
    """Final content of every touched path; `read(path)` gives the current content or None when the file is absent."""
    out = dict(patch.files)
    for op in patch.ops:
        current = out.get(op["path"])
        if current is None:
            current = read(op["path"])
        if current is None:
            raise PatchError(f"{patch.name}: {op['path']} does not exist, cannot apply find/replace")
        hits = current.count(op["find"])
        if hits != 1:  # an anchor that matches twice would insert twice
            what = "not found" if hits == 0 else f"found {hits} times (must be unique)"
            raise PatchError(f"{patch.name}: pattern {what} in {op['path']}: {op['find'][:60]!r}")
        out[op["path"]] = current.replace(op["find"], op["replace"])
    return out


def apply_to_tree(patch: Patch, root: Path) -> list[str]:
    """Write the patch into a working tree; returns the changed paths."""

    def read(path: str) -> str | None:
        target = root / path
        return target.read_text() if target.exists() else None

    contents = resolve_contents(patch, read)
    for path, text in contents.items():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    return sorted(contents)

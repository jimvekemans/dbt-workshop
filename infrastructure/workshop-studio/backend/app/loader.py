from __future__ import annotations

import re
from pathlib import Path

import yaml

from .schema import Scenario, World


class ContentError(ValueError):
    pass


def load_world(scenarios_dir: Path) -> World:
    path = scenarios_dir / "world.yaml"
    try:
        return World.model_validate(yaml.safe_load(path.read_text()))
    except Exception as e:  # noqa: BLE001 - surface file name
        raise ContentError(f"{path}: {e}") from e


def _natural(path: Path) -> list:
    """E1 < E2 < ... < E9 < E10 (a plain sort puts E10_ before E1_)."""
    return [int(part) if part.isdigit() else part for part in re.split(r"(\d+)", path.name)]


def load_scenarios(scenarios_dir: Path, world: World) -> dict[str, Scenario]:
    node_ids = {n.id for n in world.nodes}
    out: dict[str, Scenario] = {}
    for path in sorted(scenarios_dir.glob("*.yaml"), key=_natural):
        if path.name == "world.yaml":
            continue
        try:
            sc = Scenario.model_validate(yaml.safe_load(path.read_text()))
        except Exception as e:  # noqa: BLE001
            raise ContentError(f"{path.name}: {e}") from e
        missing = sc.referenced_nodes() - node_ids
        if missing:
            raise ContentError(f"{path.name}: unknown node(s) {sorted(missing)}")
        if sc.id in out:
            raise ContentError(f"{path.name}: duplicate scenario id {sc.id}")
        out[sc.id] = sc
    for sc in out.values():
        if sc.depends_on and sc.depends_on not in out:
            raise ContentError(f"{sc.id}: depends_on unknown scenario {sc.depends_on}")
    return out

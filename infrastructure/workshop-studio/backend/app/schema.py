"""Pydantic schema for world.yaml and scenarios/*.yaml. The YAML is the single source of truth:
`sim:` blocks drive the offline animation, `live:` blocks drive the real backend."""
from __future__ import annotations

from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

Health = Literal["ok", "warn", "fail", "idle"]
Group = Literal["sources", "platform", "consumers"]
Layer = Literal["all", "sources", "engineering", "consumers"]

HEALTH_RANK: dict[str, int] = {"idle": 0, "ok": 1, "warn": 2, "fail": 3}


class Strict(BaseModel):
    model_config = ConfigDict(extra="forbid")


class Effect(Strict):
    """What a scenario/step/day does to ONE node. Dotted keys (`metrics.delay_batches: 2`) are accepted."""
    health: Health | None = None
    note: str | None = None
    metrics: dict[str, Any] = Field(default_factory=dict)
    knobs: dict[str, Any] = Field(default_factory=dict)

    @model_validator(mode="before")
    @classmethod
    def _dotted(cls, data: Any) -> Any:
        if not isinstance(data, dict):
            return data
        out: dict[str, Any] = {"metrics": dict(data.get("metrics") or {}), "knobs": dict(data.get("knobs") or {})}
        for k, v in data.items():
            if k.startswith("metrics."):
                out["metrics"][k.split(".", 1)[1]] = v
            elif k.startswith("knobs."):
                out["knobs"][k.split(".", 1)[1]] = v
            elif k not in ("metrics", "knobs"):
                out[k] = v
        return out


Effects = dict[str, Effect]


class LiveAction(BaseModel):
    """`{action: set_channel, env: prod, ...}` - free-form params validated by the action itself."""
    model_config = ConfigDict(extra="allow")
    action: str

    def params(self) -> dict[str, Any]:
        return {k: v for k, v in self.model_dump().items() if k != "action"}


def _as_list(v: Any) -> Any:
    if v is None:
        return []
    return v if isinstance(v, list) else [v]


class Knob(Strict):
    key: str
    label: str
    type: Literal["bool", "int", "select"] = "int"
    default: Any = 0
    min: int | None = None
    max: int | None = None
    options: list[str] | None = None
    # live binding: knob change -> action (value injected as `value`)
    live: LiveAction | None = None


class Node(Strict):
    id: str
    label: str
    kind: str
    group: Group
    x: float = 0
    y: float = 0
    w: float | None = None
    h: float | None = None
    lane: str | None = None
    desc: str = ""
    owner: str | None = None
    knobs: list[Knob] = Field(default_factory=list)
    metrics: dict[str, Any] = Field(default_factory=dict)
    # live probes: substrings of dbt test names (vdp_mgmt.dbt_run_results) that make this node unhealthy
    tests: list[str] = Field(default_factory=list)
    # live probes: exact dbt model names whose own result counts (error -> fail, skipped -> warn), for nodes whose
    # failure is the model itself (out of memory, unknown column) rather than a test
    models: list[str] = Field(default_factory=list)


class Edge(Strict):
    source: str
    target: str
    kind: Literal["data", "code", "ci"] = "data"


class GroupDef(Strict):
    id: Group
    label: str
    subtitle: str = ""
    x: float
    y: float
    w: float
    h: float


class SprintDay(Strict):
    day: int
    title: str
    milestone: str | None = None
    # Live mode: what this day only animates (its `live:` is just the trickle). Shown on the timeline and in the log,
    # so the presenter does not narrate a merge or deploy that did not happen.
    live_note: str | None = None
    sim: Effects = Field(default_factory=dict)
    live: list[LiveAction] = Field(default_factory=list)

    _l = field_validator("live", mode="before")(_as_list)


class Sprint(Strict):
    days_step: int = 1
    days: list[SprintDay]


class World(Strict):
    groups: list[GroupDef]
    nodes: list[Node]
    edges: list[Edge]
    sprint: Sprint

    @model_validator(mode="after")
    def _refs(self) -> "World":
        ids = {n.id for n in self.nodes}
        if len(ids) != len(self.nodes):
            raise ValueError("duplicate node ids in world.yaml")
        for e in self.edges:
            for end in (e.source, e.target):
                if end not in ids:
                    raise ValueError(f"edge references unknown node {end!r}")
        return self


class Expect(Strict):
    node: str
    health: Health


class Step(Strict):
    title: str
    explain: str = ""
    code: str = ""
    zoom: str | None = None            # group id or node id to zoom to while presenting this step
    highlight: list[str] = Field(default_factory=list)
    sim: Effects = Field(default_factory=dict)
    live: list[LiveAction] = Field(default_factory=list)
    expect: Expect | None = None

    _l = field_validator("live", mode="before")(_as_list)


class Impact(Strict):
    node: str
    health: Health
    note: str | None = None


class Inject(Strict):
    narrative: str
    sim: Effects = Field(default_factory=dict)
    live: list[LiveAction] = Field(default_factory=list)

    _l = field_validator("live", mode="before")(_as_list)


class Scenario(Strict):
    id: str
    layer: Layer
    title: str
    summary: str = ""
    star: bool = False
    status: Literal["implemented", "narrative"] = "implemented"
    focus: list[str] = Field(default_factory=list)
    detect: str = ""
    depends_on: str | None = None
    # Live-mode preconditions (check_* actions, read-only), run before the injection; each may carry a `hint`
    requires: list[LiveAction] = Field(default_factory=list)
    inject: Inject
    impact: list[Impact] = Field(default_factory=list)
    steps: list[Step] = Field(default_factory=list)

    _r = field_validator("requires", mode="before")(_as_list)

    @field_validator("requires")
    @classmethod
    def _checks_only(cls, v: list[LiveAction]) -> list[LiveAction]:
        bad = [a.action for a in v if not a.action.startswith("check_")]
        if bad:
            raise ValueError(f"requires: only read-only check_* actions are allowed, got {bad}")
        return v

    def requirement_hints(self) -> list[str]:
        return [a.params().get("hint") or a.action for a in self.requires]

    def referenced_nodes(self) -> set[str]:
        refs: set[str] = set(self.focus) | {i.node for i in self.impact} | set(self.inject.sim)
        for s in self.steps:
            refs |= set(s.sim) | set(s.highlight)
            if s.expect:
                refs.add(s.expect.node)
        return refs

    def live_actions(self) -> list[LiveAction]:
        acts = list(self.inject.live)
        for s in self.steps:
            acts += s.live
        return acts

"""Pydantic models: the scenario file contract, the group and glossary files, and the API request bodies.

Every model forbids unknown keys, so a typo in a content file is a load-time error rather than a silent no-op.
"""
from __future__ import annotations

import re
from datetime import date
from typing import Annotated, Any, Literal, Union

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

ENVIRONMENTS = ("dev", "val", "prd")
KEBAB = r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$"
IDENT = r"^[a-z][a-z0-9_]*$"
VisualState = Literal["ok", "bad", "warn", "idle"]
Status = Literal["today", "next"]                             # "in place today" or "next step"
FEATURE_MAX = 50
WHY_MAX = 140          # `why`: what goes wrong as long as a feature is not there (one plain sentence)
TECH_ITEMS_MAX = 8     # `tech` of a scenario: how many technical keywords
TECH_ITEM_MAX = 40     # ... and how long one is
STEP_TECH_MAX = 60     # `tech` of a step: one short technical line
NodeState = Literal["ok", "bad", "warn", "idle", "fix"]    # `fix`: the box that is the safeguard (flow only)
OUTCOME_MAX = 300
VALUE_PLACEHOLDER = re.compile(r"\{\{\s*value\s*\}\}")
SAMPLE_VALUE = "000000"                                       # what `{{ value }}` is counted as when lengths are checked


class Strict(BaseModel):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)


# --- visuals (parts 1 and 2) -------------------------------------------------------------------------------------

class FlowNode(Strict):
    id: str = Field(pattern=IDENT)
    label: str = Field(min_length=1, max_length=30)
    state: NodeState = "idle"
    note: str | None = Field(default=None, max_length=80)


class Flow(Strict):
    type: Literal["flow"]
    nodes: list[FlowNode] = Field(min_length=3, max_length=6)
    links: list[tuple[str, str]]
    caption: str | None = Field(default=None, max_length=140)

    @model_validator(mode="after")
    def links_point_at_nodes(self) -> "Flow":
        ids = {n.id for n in self.nodes}
        unknown = {end for link in self.links for end in link} - ids
        if unknown:
            raise ValueError(f"links mention unknown nodes: {', '.join(sorted(unknown))}")
        if sum(n.state == "fix" for n in self.nodes) > 1:
            raise ValueError("at most one node may be the 'fix' box")
        return self


class TimelineDay(Strict):
    label: str = Field(min_length=1, max_length=20)
    state: VisualState = "idle"
    note: str | None = Field(default=None, max_length=80)


class Marker(Strict):
    after: int = Field(ge=0)
    label: str = Field(min_length=1, max_length=40)


class Timeline(Strict):
    type: Literal["timeline"]
    days: list[TimelineDay] = Field(min_length=5, max_length=8)
    marker: Marker | None = None
    caption: str | None = Field(default=None, max_length=140)

    @model_validator(mode="after")
    def marker_inside_days(self) -> "Timeline":
        if self.marker and self.marker.after >= len(self.days) - 1:
            raise ValueError("marker.after must be an index with a day after it")
        return self


class CompareSide(Strict):
    title: str = Field(min_length=1, max_length=40)
    items: list[str] = Field(min_length=1, max_length=5)
    state: VisualState = "idle"

    @field_validator("items")
    @classmethod
    def items_are_short(cls, items: list[str]) -> list[str]:
        if any(len(i) > 100 for i in items):
            raise ValueError("each item must be at most 100 characters")
        return items


class Compare(Strict):
    type: Literal["compare"]
    left: CompareSide
    right: CompareSide
    caption: str | None = Field(default=None, max_length=140)


Visual = Annotated[Union[Flow, Timeline, Compare], Field(discriminator="type")]


# --- scenario parts ----------------------------------------------------------------------------------------------

class Related(Strict):
    id: str
    why: str = Field(min_length=1, max_length=160)


class Problem(Strict):
    headline: str = Field(min_length=1, max_length=90)
    story: str
    cost: str = Field(min_length=1, max_length=200)
    visual: Visual

    @field_validator("story")
    @classmethod
    def story_length(cls, story: str) -> str:
        words = len(story.split())
        if not 40 <= words <= 90:
            raise ValueError(f"the story must have 40 to 90 words, it has {words}")
        return story.strip()


class Idea(Strict):
    headline: str = Field(min_length=1, max_length=90)
    points: list[Annotated[str, Field(min_length=1, max_length=160)]] = Field(min_length=3, max_length=3)
    visual: Visual


class Note(Strict):
    line: str = Field(min_length=1)       # text of a line inside the block
    says: str = Field(min_length=1, max_length=200)


class CodeBlock(Strict):
    title: str = Field(min_length=1, max_length=80)
    file: str                             # path inside the dbt project
    from_: str = Field(alias="from", min_length=1)
    to: str = Field(min_length=1)
    max_lines: int = Field(ge=1, le=80)
    file_label: str | None = Field(default=None, min_length=1, max_length=60)   # plain words shown instead of the path
    status: Status | None = None          # a badge on the block: in place today, or the next step
    notes: list[Note] = Field(min_length=1, max_length=4)


class Option(Strict):
    value: str
    label: str = Field(min_length=1, max_length=30)


class HelpOnOff(Strict):
    on: str = Field(min_length=1, max_length=160)
    off: str = Field(min_length=1, max_length=160)


class Mark(Strict):
    day: str = Field(min_length=1)                         # a day (templated), shown as a tick on the slider
    label: str = Field(min_length=1, max_length=40)


class Input(Strict):
    id: str = Field(pattern=IDENT)
    type: Literal["day", "number", "choice", "toggle"]
    label: str = Field(min_length=1, max_length=60)
    help: Annotated[str, Field(max_length=160)] | HelpOnOff | None = None
    mark: Mark | None = None
    default: Any
    min: Any = None
    max: Any = None
    step: float | None = None
    unit: str | None = Field(default=None, max_length=20)
    options: list[Option] | None = Field(default=None, min_length=2, max_length=4)

    @model_validator(mode="after")
    def fields_match_type(self) -> "Input":
        needs_range = self.type in ("day", "number")
        if needs_range and (self.min is None or self.max is None):
            raise ValueError(f"input '{self.id}' of type {self.type} needs min and max")
        if self.type == "choice" and not self.options:
            raise ValueError(f"input '{self.id}' of type choice needs options")
        if self.type != "choice" and self.options:
            raise ValueError(f"input '{self.id}' only a choice has options")
        if isinstance(self.help, HelpOnOff) and self.type != "toggle":
            raise ValueError(f"input '{self.id}': help with on and off belongs to a toggle")
        if self.mark and self.type != "day":
            raise ValueError(f"input '{self.id}': only a day can have a mark")
        return self

    def coerce(self, value: Any) -> Any:
        """The value as the right type, or ValueError with a plain sentence. Call on an input whose templates are
        already rendered (day ranges are dates then)."""
        try:
            if self.type == "toggle":
                return _as_bool(value)
            if self.type == "choice":
                return _as_option(self, value)
            if self.type == "number":
                number = float(value)
                low, high = float(self.min), float(self.max)
                if not low <= number <= high:
                    raise ValueError
                return int(number) if number == int(number) else number
            chosen = date.fromisoformat(str(value))
            if not date.fromisoformat(str(self.min)) <= chosen <= date.fromisoformat(str(self.max)):
                raise ValueError
            return chosen.isoformat()
        except (TypeError, ValueError):
            raise ValueError(f"'{self.label}' has a value that is not allowed here.") from None


def _as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    raise ValueError


def _as_option(inp: Input, value: Any) -> str:
    if str(value) in {o.value for o in inp.options or []}:
        return str(value)
    raise ValueError


class OutcomeCheck(Strict):
    """A closing sentence that depends on what the stack shows: `check` is one SELECT returning one number."""

    check: str = Field(min_length=1)
    good_when: Literal["zero", "positive"]
    good: str = Field(min_length=1)
    not_quite: str = Field(min_length=1)

    @model_validator(mode="after")
    def sentences_fit(self) -> "OutcomeCheck":
        for name in ("good", "not_quite"):
            text = VALUE_PLACEHOLDER.sub(SAMPLE_VALUE, getattr(self, name))
            if len(text) > OUTCOME_MAX:
                raise ValueError(f"'{name}' must be at most {OUTCOME_MAX} characters")
        return self

    def is_good(self, value: float) -> bool:
        return value == 0 if self.good_when == "zero" else value > 0


def _feature_and_status_together(owner: Any, what: str) -> None:
    if (owner.feature is None) != (owner.status is None):
        raise ValueError(f"{what}: 'feature' and 'status' come together: give both or neither")
    if owner.why is not None and owner.feature is None:
        raise ValueError(f"{what}: 'why' only goes with 'feature' and 'status': add both, or remove 'why'")


def _check_tech_list(value: list[str] | None) -> list[str] | None:
    """A scenario's `tech`: 1 to 8 keywords of 1 to 40 characters, said in plain sentences."""
    if value is None:
        return value
    if not 1 <= len(value) <= TECH_ITEMS_MAX:
        raise ValueError(f"'tech' lists 1 to {TECH_ITEMS_MAX} keywords, this one has {len(value)}. Leave it out for none.")
    for item in value:
        if not isinstance(item, str) or not item.strip():
            raise ValueError("every keyword in 'tech' needs some text.")
        if len(item) > TECH_ITEM_MAX:
            raise ValueError(f"the keyword '{item}' in 'tech' is {len(item)} characters long; "
                             f"a keyword takes at most {TECH_ITEM_MAX}.")
    return value


def _check_tech_line(value: str | None) -> str | None:
    """A step's `tech`: one line of 1 to 60 characters, said in plain sentences."""
    if value is None:
        return value
    if not value.strip():
        raise ValueError("'tech' needs some text; leave it out for none.")
    if len(value) > STEP_TECH_MAX:
        raise ValueError(f"'tech' is {len(value)} characters long; a step's technical line takes at most {STEP_TECH_MAX}.")
    return value


class Step(Strict):
    id: str = Field(pattern=IDENT)
    title: str = Field(min_length=1, max_length=60)
    feature: str | None = Field(default=None, min_length=1, max_length=FEATURE_MAX)   # the plain name of what this stage is about
    status: Status | None = None
    why: str | None = Field(default=None, min_length=1, max_length=WHY_MAX)   # what goes wrong without it (closing page)
    tech: str | None = None        # for the presenter only: the real names behind this step (1-60 characters)
    say: str | None = Field(default=None, max_length=300)
    button: str = Field(min_length=1, max_length=40)
    inputs: list[Input] = []
    do: list[dict[str, Any]] = Field(min_length=1)
    show: list[dict[str, Any]] = []
    outcome: Annotated[str, Field(min_length=1, max_length=OUTCOME_MAX)] | OutcomeCheck

    @field_validator("tech")
    @classmethod
    def tech_is_one_short_line(cls, value: str | None) -> str | None:
        return _check_tech_line(value)

    @model_validator(mode="after")
    def feature_comes_with_status(self) -> "Step":
        _feature_and_status_together(self, f"step '{self.id}'")
        return self


class Demo(Strict):
    intro: str = Field(min_length=1, max_length=300)
    setup: list[dict[str, Any]] = []
    steps: list[Step] = Field(min_length=2, max_length=8)   # 2-4 for a scenario, 3-8 for a flow (see Scenario)

    @model_validator(mode="after")
    def unique_ids(self) -> "Demo":
        step_ids = [s.id for s in self.steps]
        input_ids = [i.id for s in self.steps for i in s.inputs]
        for kind, ids in (("step", step_ids), ("input", input_ids)):
            if len(set(ids)) != len(ids):
                raise ValueError(f"{kind} ids must be unique within a scenario")
        return self


# what a scenario and a flow may hold: (demo steps, code blocks, minutes)
LIMITS = {"scenario": ((2, 4), (1, 3), 10), "flow": ((3, 8), (1, 8), 30)}


class Scenario(Strict):
    id: str = Field(pattern=KEBAB)
    kind: Literal["scenario", "flow"] = "scenario"
    group: str
    title: str = Field(min_length=1, max_length=60)
    promise: str = Field(min_length=1, max_length=110)
    feature: str | None = Field(default=None, min_length=1, max_length=FEATURE_MAX)   # what an ordinary scenario is about
    status: Status | None = None
    why: str | None = Field(default=None, min_length=1, max_length=WHY_MAX)   # what goes wrong without it (closing page)
    tech: list[str] | None = None  # for the presenter only: 1 to 8 keywords naming what the scenario really uses
    minutes: int = Field(ge=1, le=30)
    environment: Literal["dev", "val", "prd"] = "dev"
    related: list[Related] = []
    problem: Problem
    idea: Idea
    code: list[CodeBlock] = Field(min_length=1, max_length=8)
    demo: Demo

    @field_validator("tech")
    @classmethod
    def tech_is_a_short_list(cls, value: list[str] | None) -> list[str] | None:
        return _check_tech_list(value)

    @model_validator(mode="after")
    def fits_its_kind(self) -> "Scenario":
        """A scenario is short (2-4 steps, 1-3 code blocks, 10 minutes); a flow is long (3-8, 1-8, 30)."""
        (low, high), (code_low, code_high), minutes = LIMITS[self.kind]
        name = "A flow" if self.kind == "flow" else "A scenario"
        hint = " Make it a flow (kind: flow) for more." if self.kind == "scenario" else ""
        _feature_and_status_together(self, "the scenario")
        steps = len(self.demo.steps)
        if not low <= steps <= high:
            raise ValueError(f"{name} has {low} to {high} demo steps, this one has {steps}.{hint}")
        if not code_low <= len(self.code) <= code_high:
            raise ValueError(f"{name} has {code_low} to {code_high} code blocks, this one has {len(self.code)}.{hint}")
        if self.minutes > minutes:
            raise ValueError(f"{name} takes at most {minutes} minutes, this one says {self.minutes}.{hint}")
        if self.kind == "flow":
            for step in self.demo.steps[:-1]:
                if step.feature is None:
                    raise ValueError(f"In a flow every stage needs a 'feature' and a 'status', except the last one; "
                                     f"step '{step.id}' has none.")
        return self


class RoadmapExtra(Strict):
    """A row of the closing overview that has no demo of its own."""
    area: str = Field(min_length=1, max_length=40)
    feature: str = Field(min_length=1, max_length=60)
    status: Status
    note: str | None = Field(default=None, min_length=1, max_length=160)
    why: str | None = Field(default=None, min_length=1, max_length=WHY_MAX)   # what goes wrong without it


class RoadmapFile(Strict):
    """`_roadmap.yaml`: the text around the closing overview and the rows that have no scenario."""
    title: str = Field(min_length=1, max_length=60)
    intro: str = Field(min_length=1, max_length=300)
    closing: str | None = Field(default=None, min_length=1, max_length=300)
    extras: list[RoadmapExtra] = []


# --- group and glossary files ------------------------------------------------------------------------------------

class Group(Strict):
    id: str = Field(pattern=IDENT)
    title: str = Field(min_length=1, max_length=60)
    blurb: str = Field(default="", max_length=200)
    scenarios: list[str] = Field(default_factory=list)   # the scenario ids of this group, in the order they are shown


class GroupsFile(Strict):
    groups: list[Group] = Field(min_length=1)


class GlossaryFile(Strict):
    terms: dict[str, str]

    @field_validator("terms")
    @classmethod
    def explanations_are_short(cls, terms: dict[str, str]) -> dict[str, str]:
        too_long = [t for t, text in terms.items() if not text.strip() or len(text) > 400]
        if too_long:
            raise ValueError(f"explanations must be 1 to 400 characters: {', '.join(too_long)}")
        return terms


# --- API request bodies ------------------------------------------------------------------------------------------

class RunBody(Strict):
    inputs: dict[str, Any] = {}


class ModeBody(Strict):
    mode: Literal["live", "replay"]


class FeederBody(Strict):
    running: bool

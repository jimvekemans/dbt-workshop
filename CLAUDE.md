# dbt × SingleStore workshop

A local Docker stack for a hands-on workshop on running dbt as a team of teams. It includes SingleStore (LDV + PRD),
GitLab with CI, two Airflow instances, dbt docs and the Workshop Studio. The participant-facing overview is in
`README.md`.

## Layout

- `dbt-singlestore-workshop/`: the dbt project, its Airflow DAGs/utils/tests and `.gitlab-ci.yml`. It's pushed to the
  workshop GitLab as `dbt-vdp`.
- `infrastructure/`: everything that runs the stack, with `workshop.sh` as the single entry point.
- `.agents/skills/`: the workshop skills (see below), symlinked into `.claude/skills/` so Claude Code loads them.
  `.claude/skills/dbt-base-image` lives there directly.

## Ground rules

- Everything goes through `infrastructure/workshop.sh`. Don't install Python on the host and don't run the manager
  scripts directly.
- The user's personal SingleStore container `singlestore` (port 3306) is not part of the workshop. Never stop, change or
  target it. Workshop containers are named `workshop-*`.
- `infrastructure/dbt-singlestore-base-image/src/` contains git submodules: a large Rust repo and a Go repo. Leave
  them out of searches unless the task is about building dbt or the driver.

## Skills: read the matching one before starting work in its area

| Skill | Path | Covers |
|---|---|---|
| Workshop architecture | [.agents/skills/vdab-dbt-workshop-architecture/SKILL.md](.agents/skills/vdab-dbt-workshop-architecture/SKILL.md) | Repo layout, `workshop.sh` commands and the manager container, containers/ports/credentials, Docker volumes, SingleStore layout, RBAC and SQL quirks, Airflow 3.3.2 patterns, drift audit, common operations, tests. **Start here for anything stack-wide.** |
| Release flow | [.agents/skills/dbt-release-flow/SKILL.md](.agents/skills/dbt-release-flow/SKILL.md) | Modified GitFlow (`dev` → `release/YYYY-Qx` → `main`), the Domain A/B collision and revert exercise, GitLab CI verify/tag/deploy jobs, the Airflow `dbt_release` DAG with state tracking and drift gate, reset/trickle commands, Workshop Studio scenarios and patches. |
| Reload pipeline | [.agents/skills/dbt-reload-pipeline/SKILL.md](.agents/skills/dbt-reload-pipeline/SKILL.md) | Blue/green chunked full-refresh via the Airflow `dbt_reload` DAG: `meta.reload_strategy`, `reload_helpers.sql`, the empty-table + SQL-capture hook, batch/retry handling, delegation from `dbt_release`, `dummy_reload_*` models. |
| Base image | [.claude/skills/dbt-base-image/SKILL.md](.claude/skills/dbt-base-image/SKILL.md) | The `dbt-singlestore-base` image: Fusion dbt binary + SingleStore ADBC driver, used from local prebuilt files or built from the submodules. Covers driver discovery, build pitfalls (release build only, protobuf deps, memory, `-tags driverlib`), tags and consumers. |

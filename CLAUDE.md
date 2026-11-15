# dbt × SingleStore workshop

A local Docker stack for a hands-on workshop on running dbt as a team of teams: one SingleStore with three environments
(dev, val, prd), GitLab with CI, one Airflow and one dbt docs site per environment, and Mailpit for failure mails. The
stack ships without data. The participant-facing overview is in `README.md`.

## Layout

- `dbt-singlestore-workshop/`: the dbt project, its `Dockerfile`, Airflow DAGs/utils/tests and `.gitlab-ci.yml`. It's
  pushed to the workshop GitLab as `dbt-vdp`.
- `infrastructure/`: everything that runs the stack (compose, manager, Airflow, GitLab, SingleStore SQL, nginx, images).
  The root `Makefile` is the entry point.
- `.agents/skills/`: the workshop skills (see below), symlinked into `.claude/skills/` so Claude Code loads them.
  `.claude/skills/dbt-base-image` lives there directly.

## Ground rules

- Everything goes through the root `Makefile` (`make help`). Don't install Python on the host (`make test` needs
  `python3` with `pytest` already there) and don't run the manager scripts directly.
- The user's personal SingleStore container `singlestore` (port 3306) is not part of the workshop. Never stop, change or
  target it. Workshop containers are named `workshop-*`.
- `infrastructure/dbt-singlestore-base-image/src/` contains git submodules: a large Rust repo and a Go repo. Leave
  them out of searches unless the task is about building dbt or the driver.
- `infrastructure/workshop-studio/` is the presenter's app (scenarios, live demos, data feeder). Its scenario files
  are validated by `docker exec workshop-studio python -m studio.catalog --check`; read its README before changing it.

## Skills: read the matching one before starting work in its area

| Skill | Path | Covers |
|---|---|---|
| Workshop architecture | [.agents/skills/vdab-dbt-workshop-architecture/SKILL.md](.agents/skills/vdab-dbt-workshop-architecture/SKILL.md) | Layout, `make` targets and the manager container, containers/ports/credentials, volumes, environments dev/val/prd, SingleStore databases and RBAC, images, Airflow patterns, failure e-mail, drift audit, tests. **Start here for anything stack-wide.** |
| Release flow | [.agents/skills/dbt-release-flow/SKILL.md](.agents/skills/dbt-release-flow/SKILL.md) | Modified GitFlow (`dev` → `release/YYYY-Qx` → `main`), GitLab CI jobs (verify, tag, build image, deploy), image tag rollback, the Airflow `dbt_release` DAG, reset. |
| Reload pipeline | [.agents/skills/dbt-reload-pipeline/SKILL.md](.agents/skills/dbt-reload-pipeline/SKILL.md) | Blue/green chunked full-refresh via the Airflow `dbt_reload` DAG: `meta.reload_strategy`, `reload_helpers.sql`, batch/retry handling, delegation from `dbt_release`, `dummy_reload_*` models. |
| Base image | [.claude/skills/dbt-base-image/SKILL.md](.claude/skills/dbt-base-image/SKILL.md) | The `dbt-singlestore-base` image: Fusion dbt binary + SingleStore ADBC driver, local prebuilt files or built from the submodules; the dev image on top of it. |
| Studio scenarios | [.agents/skills/workshop-studio-scenarios/SKILL.md](.agents/skills/workshop-studio-scenarios/SKILL.md) | Adding or changing a Workshop Studio scenario: the four-part file, content rules for a non-technical audience, actions and evidence, patches, new backend actions, and the validate / run live / record / capture / review loop (with a runner script). |

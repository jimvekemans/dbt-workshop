# dbt × SingleStore workshop

A local Docker stack for a hands-on workshop on running dbt as a team of teams: one SingleStore with three environments
(dev, val, prd), GitLab with CI, one Airflow and one dbt docs site per environment, Mailpit for failure mails, and a dev-mode Vault that can hold the
developers' database password. The stack ships without data. The participant-facing overview is in `README.md`.

## Layout

- `dbt-singlestore-workshop/`: the dbt project, its `Dockerfile`, Airflow DAGs/utils/tests, `scripts/` (the generated
  documentation front page) and `.gitlab-ci.yml`. It's pushed to the workshop GitLab as `dbt-vdp`.
- `infrastructure/`: everything that runs the stack (compose, manager, Airflow, GitLab, SingleStore SQL, nginx, the
  Vault stand-in in `vault/`, the overview page in `overview/`, the Workshop Studio, images). The root `Makefile` is the
  entry point.
- `infrastructure/dbt-singlestore-dev-image/`: the developer image on top of the base image. Its `src/` holds the
  `sqlfmt` and `dbt-bouncer` forks and three tracked Rust workspaces: `vdp-rust-utils` (the `vdp` toolkit, built into
  the dev image) and `release-ui` and `vaultspeed-framework` (standalone: built into no image, not used by compose,
  the Makefile, nginx, the overview or CI). `local/` there is gitignored (untouched client copies).
- `.agents/skills/`: the workshop skills (see below), symlinked into `.claude/skills/` so Claude Code loads them.
  `.claude/skills/dbt-base-image` lives there directly; the three Rust skills live in their workspaces.
- `.claude/launch.json`: the dev server `studio-frontend-mock` (Workshop Studio frontend on port 5199; open `/?mock`
  for a fake API) and `studio-frontend-mock-docker`, the same server in a Node container for a host without Node.
- `requirements.md`: the owner's own minimum requirements for the stack. The stack exceeds them: three environments
  (dev, val, prd), not two.

## Ground rules

- Everything goes through the root `Makefile` (`make help`). Don't install Python on the host (`make test` needs
  `python3` with `pytest` already there) and don't run the manager scripts directly.
- The user's personal SingleStore container `singlestore` (port 3306) is not part of the workshop. Never stop, change or
  target it. Workshop containers are named `workshop-*`.
- `infrastructure/dbt-singlestore-base-image/src/` contains git submodules: a large Rust repo and a Go repo. Leave
  them out of searches unless the task is about building dbt or the driver.
- The Rust workspaces under `infrastructure/dbt-singlestore-dev-image/src/` have `target/` build folders: leave them
  out of searches too (like the submodules). The workspaces and `local/` are described in the skills below.
- Check the Rust workspaces with the Makefile too, so the host needs no Rust toolchain: `make rust-check` (fmt, clippy,
  tests, docs; per-feature clippy for `vdp-rust-utils`), `make rust-test`, `make rust-cargo S="<cargo arguments>"`;
  `W=vdp-rust-utils|release-ui|vaultspeed-framework` limits them to one workspace (default: all three). They run in a
  `rust:<version>-trixie` container as the host user, with the cargo registry and the build output in Docker volumes
  (`make rust-clean` removes them). A host `cargo` (rustup picks up the pinned toolchain) works as well, but its
  `target/` folders are large; neither is part of `make test` or `make bootstrap`.
- `infrastructure/workshop-studio/` is the presenter's app (26 scenarios in five groups, among them five long "flows",
  a quiz with a question for the room per scenario and flow stage, a "Bij ons gebeurd" line of what really happened at the
  client (facts: `client-history-report.md`), a closing page, live demos, data feeder). Its scenario files are validated by `docker exec workshop-studio python -m studio.catalog --check`; read its README before changing it.

## Skills: read the matching one before starting work in its area

| Skill | Path | Covers |
|---|---|---|
| Workshop architecture | [.agents/skills/vdab-dbt-workshop-architecture/SKILL.md](.agents/skills/vdab-dbt-workshop-architecture/SKILL.md) | Layout, `make` targets and the manager container, containers/ports/credentials, volumes, environments dev/val/prd, SingleStore databases and RBAC, images, deploys, Airflow patterns (asset scheduling and events, the notice and on-demand DAGs), failure e-mail, drift audit, the Vault stand-in, tests. **Start here for anything stack-wide.** |
| Release flow | [.agents/skills/dbt-release-flow/SKILL.md](.agents/skills/dbt-release-flow/SKILL.md) | Modified GitFlow (`dev` → `release/YYYY-Qx` → `main`), GitLab CI jobs (the two-round rehearsal, verify, tag, build image, `publish_artifacts`, deploy), image tag rollback, the Airflow `dbt_release` DAG, reset. |
| Reload pipeline | [.agents/skills/dbt-reload-pipeline/SKILL.md](.agents/skills/dbt-reload-pipeline/SKILL.md) | Blue/green chunked full-refresh via the Airflow `dbt_reload` DAG: `meta.reload_strategy`, `reload_helpers.sql`, batch/retry handling, delegation from `dbt_release`, `dummy_reload_*` models. |
| Base image | [.claude/skills/dbt-base-image/SKILL.md](.claude/skills/dbt-base-image/SKILL.md) | The `dbt-singlestore-base` image: Fusion dbt binary + SingleStore ADBC driver, local prebuilt files or built from the submodules; the dev image on top of it. |
| Studio scenarios | [.agents/skills/workshop-studio-scenarios/SKILL.md](.agents/skills/workshop-studio-scenarios/SKILL.md) | Adding or changing a Workshop Studio scenario or flow: the four-part file, status labels and the closing page, content rules for a non-technical audience, actions and evidence, patches, new backend actions, and the validate / run live / record / capture / review loop (with a runner script). |
| Rust toolkit (`vdp-rust-utils`) | [infrastructure/dbt-singlestore-dev-image/src/vdp-rust-utils/.agents/skills/vdp-rust-utils/SKILL.md](infrastructure/dbt-singlestore-dev-image/src/vdp-rust-utils/.agents/skills/vdp-rust-utils/SKILL.md) | The `vdp` toolkit (dbt wrapper, sources, release flows, sql, qa): crate map, rules, how a new command, crate or binary fits the dev image and its consumers. |
| Release hub (`vdp-release-ui`) | [infrastructure/dbt-singlestore-dev-image/src/release-ui/.agents/skills/vdp-release-ui/SKILL.md](infrastructure/dbt-singlestore-dev-image/src/release-ui/.agents/skills/vdp-release-ui/SKILL.md) | `release-ui`: the release hub backend (HTTP API, jobs, extension SPI, announcements) and how it would be wired into the stack. |
| VaultSpeed deployer (`vdp-vaultspeed-framework`) | [infrastructure/dbt-singlestore-dev-image/src/vaultspeed-framework/.agents/skills/vdp-vaultspeed-framework/SKILL.md](infrastructure/dbt-singlestore-dev-image/src/vaultspeed-framework/.agents/skills/vdp-vaultspeed-framework/SKILL.md) | `vaultspeed-framework`: the VaultSpeed deployer (planner, ledger, history, releases, hub extension), build and fit. |

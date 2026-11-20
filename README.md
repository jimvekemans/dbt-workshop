# dbt × SingleStore workshop

A hands-on workshop on running dbt as a team of teams: how a change travels from a feature branch through CI, a tagged
image and three environments, and how Airflow, tests and a failure e-mail guard it. It all runs locally in Docker:

- one SingleStore with the environments **dev**, **val** and **prd** as separate prefixed databases (`dev_*`, `val_*`, `prd_*`)
- GitLab with a runner and CI for the dbt project (`workshop/dbt-vdp`)
- one Airflow 3 and one dbt docs site per environment
- Mailpit, which catches the failure mails of Airflow
- the dbt project, run by the dbt Fusion binary with the SingleStore ADBC driver

The stack ships **without data**: the landing tables are empty until the Workshop Studio starts. The Studio
(`infrastructure/workshop-studio/`, http://localhost:8085) fills them with a small made-up web shop, keeps new data
arriving, and presents twenty short scenarios for a non-technical audience: what goes wrong, the idea, the code and
a live demo on this stack. It cleans up after each demo by itself. See its
[README](infrastructure/workshop-studio/README.md).

## Prerequisites

- Linux on x86-64 with Docker Engine 27.2 or newer (the Airflow and Studio containers pin the Docker API to 1.47), its compose
  plugin (v2) and `make`. That is what the stack is built and tested on; nothing is installed on the host besides
  Docker.
- about 16 GB of free memory (SingleStore is capped at 6 GB, GitLab at 4 GB) and 40 GB of disk for images and volumes
- these host ports free: 80 and 2222 (GitLab), 8025 (Mailpit), 8081, 8082, 8086 (dbt docs), 8083 and 23306
  (SingleStore), 8085 (Studio), 8088 to 8090 (Airflow), 8099 (overview). Most can be moved with the
  `WORKSHOP_*_PORT` variables in `infrastructure/compose/docker-compose.yml`; GitLab's port 80 cannot.
- one line in `/etc/hosts`, because the browser is sent to GitLab under that name when you sign in to Airflow:
  `127.0.0.1 gitlab.local`
- for the devcontainer: VS Code
- for `make test`: `python3` with `pytest`

## Quick start

```bash
git clone --recurse-submodules git@github.com:jimvekemans/dbt-workshop.git
cd dbt-workshop
echo "127.0.0.1 gitlab.local" | sudo tee -a /etc/hosts   # once per machine
make images                   # base, dev, Airflow and manager images
make bootstrap                # start the stack and provision everything (SingleStore, GitLab, runner, first pipeline)
make help                     # all targets: up, down, stop, ps, logs, images, bootstrap, reset, shell, test, rust-*
```

Then open the Workshop Studio at http://localhost:8085. It fills the practice data and prepares the three environments
by itself (about a minute the first time); the home page shows when everything is ready. The recorded demos in
`infrastructure/workshop-studio/recordings/` come with the repository, so replay mode works straight away.

**The first `make images` on a new machine compiles the dbt engine and the database driver from the two git submodules**
(`infrastructure/dbt-singlestore-base-image/src/`), because the prebuilt binaries are too large for the repository.
That took 8 minutes on a 20-core machine; allow up to an hour on a small laptop (later builds reuse the result).
If you cloned without `--recurse-submodules`, run `git submodule update --init` first. To skip the
compile, copy `dbt` and `libadbc_driver_singlestore.so` from a machine that has them into
`infrastructure/dbt-singlestore-base-image/`; they are used as they are when present.

The Rust code next to the developer image (three Cargo workspaces under `infrastructure/dbt-singlestore-dev-image/src/`) is
checked in Docker as well, so you need no Rust toolchain: `make rust-check` runs formatting, clippy, tests and docs for
all three (`W=vdp-rust-utils`, `W=release-ui` or `W=vaultspeed-framework` for one), `make rust-test` only the tests and
`make rust-cargo S="<cargo arguments>"` any other cargo command. The first run downloads the Rust image and compiles every
dependency (2.5 minutes for all three on a 20-core machine, DuckDB makes `vaultspeed-framework` the slowest; later runs
take seconds); the image (3 GB), the cargo registry and the build output (7 GB together) stay in Docker volumes that
`make rust-clean` removes. These targets are not part of `make test` or `make bootstrap`.

`make reset` goes back to the baseline: databases, GitLab repository, Airflow overrides and the Studio's saved state.
`make stop` leaves the stack stopped; the data stays in Docker volumes. Credentials are fixtures for a local practice stack; they are defined in
[init_global.sql](infrastructure/singlestore/init_global.sql),
[init_env.sql.tmpl](infrastructure/singlestore/init_env.sql.tmpl),
[bootstrap_stack.py](infrastructure/manager/bootstrap_stack.py) and
[docker-compose.yml](infrastructure/compose/docker-compose.yml), and the
[overview page](http://localhost:8099) lists them per service.

## Where things are

| Service | URL / port | Purpose |
| :-- | :-- | :-- |
| Workshop Studio | http://localhost:8085 | the scenarios and live demos; also feeds the practice data |
| Workshop overview | http://localhost:8099 | every service with its live state, its users (passwords on demand) and a link to its web app |
| GitLab | http://gitlab.local (or http://localhost) | repo `workshop/dbt-vdp`, CI pipelines, merge requests |
| Airflow dev / val / prd | http://localhost:8088 / :8089 / :8090 | `dbt_daily_load`, `dbt_release`, `dbt_reload` (and `dbt_housekeeping` in dev) |
| dbt docs dev / val / prd | http://localhost:8081 / :8082 / :8086 | lineage per environment |
| Mailpit | http://localhost:8025 | failure e-mails of Airflow |
| SingleStore | `localhost:23306`, Studio UI :8083 | inside the stack: `singlestore:3306` |

Host ports stay clear of a personal SingleStore on 3306. Every container is named `workshop-<service>`.

## How a change travels

| Step | What happens |
| :-- | :-- |
| feature branch, MR to `dev` | `verify_feature_to_dev` builds only the modified models in `vdp_ci` |
| merge to `dev` | tag `vX.Y.Z`, image `dbt-vdp:<tag>`, deployed to dev |
| `release/YYYY-Qx`, merge to `main` | `verify_release`, tag `YYYY-Qx.N`, deployed to val |
| manual job | the same tag is deployed to prd |

Airflow runs exactly the image tag in its Variable `dbt_image_tag`; rolling back means setting it to an older tag.
Developers work in the devcontainer (`dbt-singlestore-workshop/.devcontainer/`) with a `pu_dev1`..`pu_dev3` account: their
builds land in the `dev_*` databases with their user name as prefix.

## Go deeper

The skills are the technical reference: most in `.agents/skills/`, the base-image skill in `.claude/skills/`, and
three next to the Rust sources of the dev image:

- [Workshop architecture](.agents/skills/vdab-dbt-workshop-architecture/SKILL.md): the single source for the stack
- [Release flow](.agents/skills/dbt-release-flow/SKILL.md): branches, tags, CI jobs, deploys, `dbt_release`
- [Reload pipeline](.agents/skills/dbt-reload-pipeline/SKILL.md): blue/green reloads with `dbt_reload`
- [Base image](.claude/skills/dbt-base-image/SKILL.md): the dbt Fusion + ADBC driver image
- [Studio scenarios](.agents/skills/workshop-studio-scenarios/SKILL.md): adding or changing a Workshop Studio scenario
- [vdp toolkit](infrastructure/dbt-singlestore-dev-image/src/vdp-rust-utils/.agents/skills/vdp-rust-utils/SKILL.md): the Rust toolkit that fronts `dbt` in the dev image
- [Release hub](infrastructure/dbt-singlestore-dev-image/src/release-ui/.agents/skills/vdp-release-ui/SKILL.md): the release UI's backend (standalone, not part of the stack)
- [VaultSpeed deployer](infrastructure/dbt-singlestore-dev-image/src/vaultspeed-framework/.agents/skills/vdp-vaultspeed-framework/SKILL.md): the VaultSpeed framework's deployer (standalone, not part of the stack)

The developer image has its own [README](infrastructure/dbt-singlestore-dev-image/README.md).

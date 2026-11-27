# dbt-singlestore-dev-image

Developer image `dbt-singlestore-dev:1.11.11` for the devcontainer. It is the base image
(`dbt-singlestore-base:1.11.11`, Fusion dbt + SingleStore ADBC driver) plus:

- `vdp`, the Rust toolkit. `dbt` on the PATH is `vdp`; it runs the real engine from `/usr/local/libexec/dbt/dbt`
  (`VDP_DBT_BIN`). The driver is still found in `/usr/local/lib/dbt/`.
- `sqlfmt` (fork with SingleStore `:>` casts and the jinja formatting extra), `dbt-bouncer` (fork) and `pre-commit`,
  in the venv `/opt/devtools`, linked into `/usr/local/bin`.

## Tool sources

Everything is built from this folder, the same way the base image handles dbt and the driver.

    src/sqlfmt/           sqlfmt fork with SingleStore `:>` casts          in the repository
    src/dbt-bouncer/      dbt-bouncer fork                                 in the repository
    src/vdp-rust-utils/   vdp toolkit (Rust workspace, crate vdp-cli)      in the repository, cleaned (see below)
    src/vaultspeed-framework/  VaultSpeed framework (workspace, crate vdp-vaultspeed-cli) in the repository, not built into the image
    src/release-ui/       release UI backend (workspace, crate vdp-release-ui-cli)  in the repository, not built into the image
    vdp                   optional prebuilt release binary of vdp-cli      gitignored
    local/                untouched copies kept on the machine they came from, gitignored and not in the image

The vdp source in the repository is a **cleaned copy**: e-mail addresses and host names of the organisation it was
written for are replaced by placeholders (`person1@example.com`, `git.example.com`, ...), and its fixtures, docs,
sample configuration and the golden, differential and live test suites that need them are left out (the unit and
process tests that need none stay, about 250 in `src/vdp-rust-utils`). The build needs none of those; the workshop configures vdp
through `dbt-singlestore-workshop/vdp.toml`. Its `Cargo.toml` still states the original licence.

For `vdp`:

- `./vdp` (prebuilt release binary) is copied in as is when present;
- otherwise `cargo build --release --locked -p vdp-cli` runs on `src/vdp-rust-utils` in a `rust:1.96-trixie` stage
  (`--build-arg CARGO_BUILD_JOBS=2` on a busy host; a few minutes the first time);
- with neither of them the image is built **without vdp**: `dbt` is then the plain engine, and sqlfmt, dbt-bouncer
  and pre-commit work the same.

`src/vaultspeed-framework/` and `src/release-ui/` are workspaces of their own next to `src/vdp-rust-utils/`. They are not
built into the image (the image builds `-p vdp-cli` from `src/vdp-rust-utils/` only, which depends on neither); build them by
hand in their folders:

- `src/release-ui/`: the release UI's backend (the release hub and the announcements it sends). It imports crates from
  `src/vdp-rust-utils/` and builds `vdp-release-ui`, which `vdp release-ui ...` runs:
  `cargo build --release --locked -p vdp-release-ui-cli`.
- `src/vaultspeed-framework/`: imports crates from `src/vdp-rust-utils/` and `src/release-ui/` and builds `vdp-vaultspeed`,
  which `vdp vaultspeed ...` runs: `cargo build --release --locked -p vdp-vaultspeed-cli`. The `vdp-ssh` crate (remote
  access over OpenSSH) belongs to that workspace, not to `src/vdp-rust-utils/`.

Put the binaries next to `vdp` (`/usr/local/bin`) or on `PATH`. The dependencies run `vdp-rust-utils` <- `release-ui` <-
`vaultspeed-framework`.

The code of all three workspaces is checked in Docker from the repository root, so the host needs no Rust toolchain:
`make rust-check` (formatting, clippy, tests and docs; `W=vdp-rust-utils`, `W=release-ui` or `W=vaultspeed-framework` for one),
`make rust-test` and `make rust-cargo S="<cargo arguments>"`. They use a `rust:<version>-trixie` image (`../rust-check/`) whose version is the
`RUST_VERSION` of this Dockerfile, mount this `src/` folder and keep the registry and build output in Docker volumes (`make rust-clean`
removes them). A host `cargo` works as well; each workspace's README lists the commands.

`sqlfmt` and `dbt-bouncer` are pip-installed from their folders into the venv `/opt/devtools`.

    docker build -t dbt-singlestore-dev:1.11.11 .

`--build-arg BASE_IMAGE=...` picks another base image. `make bootstrap` and `make images` at the repository root build this image.

## Skills and docs

Each Rust workspace has a skill and a README:

- `src/vdp-rust-utils/`: [skill](src/vdp-rust-utils/.agents/skills/vdp-rust-utils/SKILL.md),
  [README](src/vdp-rust-utils/README.md)
- `src/release-ui/`: [skill](src/release-ui/.agents/skills/vdp-release-ui/SKILL.md), [README](src/release-ui/README.md)
- `src/vaultspeed-framework/`: [skill](src/vaultspeed-framework/.agents/skills/vdp-vaultspeed-framework/SKILL.md),
  [README](src/vaultspeed-framework/README.md)

The stack around them is described in §12 of the
[architecture skill](../../.agents/skills/vdab-dbt-workshop-architecture/SKILL.md).

## Workshop Studio

The Workshop Studio mounts this folder read-only (`/workshop/dev-image`). Its scenario `same-toolbox`
(`infrastructure/workshop-studio/scenarios/same-toolbox.yaml`) quotes the `Dockerfile` between
`RUN --mount=type=bind,source=src/sqlfmt` and `ln -s`, and its demo runs `vdp --version` and the other tools in fresh
containers of this image. After editing the Dockerfile run `docker exec workshop-studio python -m studio.catalog --check`.

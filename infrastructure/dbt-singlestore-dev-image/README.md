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
    vdp                   optional prebuilt release binary of vdp-cli      gitignored
    local/                untouched copies kept on the machine they came from, gitignored and not in the image

The vdp source in the repository is a **cleaned copy**: e-mail addresses and host names of the organisation it was
written for are replaced by placeholders (`person1@example.com`, `git.example.com`, ...), and its fixtures, docs,
sample configuration and integration tests are left out. The build needs none of those; the workshop configures vdp
through `dbt-singlestore-workshop/vdp.toml`. Its `Cargo.toml` still states the original licence.

For `vdp`:

- `./vdp` (prebuilt release binary) is copied in as is when present;
- otherwise `cargo build --release --locked -p vdp-cli` runs on `src/vdp-rust-utils` in a `rust:1.96-trixie` stage
  (`--build-arg CARGO_BUILD_JOBS=2` on a busy host; a few minutes the first time);
- with neither of them the image is built **without vdp**: `dbt` is then the plain engine, and sqlfmt, dbt-bouncer
  and pre-commit work the same.

`sqlfmt` and `dbt-bouncer` are pip-installed from their folders into the venv `/opt/devtools`.

    docker build -t dbt-singlestore-dev:1.11.11 .

`--build-arg BASE_IMAGE=...` picks another base image. `make images` at the repository root builds this image.

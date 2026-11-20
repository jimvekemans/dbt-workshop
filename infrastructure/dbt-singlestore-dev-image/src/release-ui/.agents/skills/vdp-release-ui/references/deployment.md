# Wiring the release hub into the workshop stack: a plan, not implemented

Status today (checked by grep over `infrastructure/` and `dbt-singlestore-workshop/`, `src/` folders excluded): the release hub is **not** in the
stack. No compose service, Makefile target, nginx route, `overview/services.json` entry, CI job or manager step mentions it, no image contains
`vdp-release-ui`, and `dbt-singlestore-workshop/vdp.toml` has no `[release_hub]` or `[notify]` section. Nothing below has been built or run. The
stack's own pattern names are quoted from the files they live in, so that whoever does this follows them.

First decide whether the hub belongs in the workshop at all. It drives the client's DROP flow (branches `release/<DROP>`, tickets `INTEL-n`, Jira, `changelog/<DROP>.md`);
the workshop's release flow is tag-based and runs in Airflow and GitLab CI (`.agents/skills/dbt-release-flow/SKILL.md`), has no Jira, and its commits carry no ticket ids. A hub in the
stack would show sign-in, the clone, repo health, change views and the announcement mail, but its analyses would list nothing useful.

## What has to exist, and the choice that shapes it

The binary is built from `src/release-ui`, which needs `src/vdp-rust-utils` next to it (path dependencies), so any image build needs **both folders** in its context.

| | A. In the dev image | B. A dedicated image (recommended for a server) |
|---|---|---|
| Binary | a second build stage in `infrastructure/dbt-singlestore-dev-image/Dockerfile`, installed next to `vdp` | a new `infrastructure/release-hub/Dockerfile` |
| Gains | `vdp release-ui ...` works in the devcontainer; `--validate` has the dbt engine | small runtime image (`git`, CA certificates), no coupling to the dev image's build time |
| Costs | every `make images` and devcontainer build pays for a third cargo build; the dev image runs as user `dbt` (volume permissions); `git` in it is unchecked | a second Dockerfile; `--validate` would need the dbt engine too (base the image on `dbt-singlestore-base` then) |
| Compose | `image: dbt-singlestore-dev:1.11.11` and a `command:` | `build:` + `image: workshop-release-hub:latest` + `pull_policy: build`, like `overview` |

## Steps

1. **Dockerfile (A).** In `infrastructure/dbt-singlestore-dev-image/Dockerfile`, add a stage after `vdp-build` that copies its shape: `FROM rust:${RUST_VERSION}-trixie AS release-ui-build`, `ARG CARGO_BUILD_JOBS=`,
   the same `RUN --mount=type=bind,source=src,target=/src-all` with the two cargo cache mounts and its own target cache (`--mount=type=cache,target=/cargo-target-release-ui`), skip with a message when
   `/src-all/release-ui/Cargo.toml` is absent (as `vdp-build` does for `vdp-rust-utils`), then
   `cd /src-all/release-ui; CARGO_TARGET_DIR=/cargo-target-release-ui cargo build --release --locked -p vdp-release-ui-cli; cp /cargo-target-release-ui/release/vdp-release-ui /out/`.
   In the final stage add `COPY --from=release-ui-build /out/ /tmp/release-ui-out/` next to the `COPY --from=vdp-build /out/ /tmp/vdp-out/` and, in that `RUN`, an
   `install -m 755 /tmp/release-ui-out/vdp-release-ui /usr/local/bin/vdp-release-ui` guarded by `[ -f ... ]`, so `vdp` finds it next to itself. A prebuilt `./vdp-release-ui` would be matched by the existing
   `COPY Dockerfile vdp* /prebuilt/` glob; add `infrastructure/dbt-singlestore-dev-image/vdp-release-ui` to the root `.gitignore`, which ignores `infrastructure/dbt-singlestore-dev-image/vdp` today.
   `.dockerignore` already drops `**/target`, `src/*/.claude` and `src/*/.agents`.
2. **Dockerfile (B).** `infrastructure/release-hub/Dockerfile`, built with `build.context: ../dbt-singlestore-dev-image/src` and `dockerfile: ../../release-hub/Dockerfile` (a context root that holds both workspaces; compose accepting a Dockerfile outside the context is not tested here; give it a
   `Dockerfile.dockerignore` next to it that drops `**/target`, `**/.git`, `**/node_modules`, `sqlfmt`, `dbt-bouncer`). Stage 1 `rust:1.96-trixie`: `COPY vdp-rust-utils` and `release-ui`, the same cargo cache mounts, `cargo build --release --locked -p vdp-release-ui-cli` in
   `release-ui`. Stage 2 a pinned slim Debian trixie (pin an exact tag like the other images) with `git` and `ca-certificates`, the binary in `/usr/local/bin`, `EXPOSE 8765`, `ENTRYPOINT ["vdp-release-ui"]`, `CMD ["release", "serve"]`.
   Same glibc as the dev image's build stage, so the binary runs there.
3. **Configuration file.** The hub container has no dbt project, so its settings go in `/etc/vdp/vdp.toml` (a layer `vdp` reads): `infrastructure/release-hub/vdp.toml`, mounted read-only. Content (inside `workshop_net`, `gitlab.local` and `mailpit` resolve):

   ```toml
   [release_hub]
   repo_url       = "http://gitlab.local/workshop/dbt-vdp.git"
   gitlab_url     = "http://gitlab.local"
   gitlab_project = "workshop/dbt-vdp"
   push_url       = "http://gitlab.local/workshop/dbt-vdp.git"
   [notify]
   smtp_host = "mailpit"
   smtp_port = 1025
   security  = "none"
   from      = "release-ui@example.com"
   ```

   `repo_dir` (`/opt/project`) and `state_dir` (`/var/lib/vdp-hub`) keep their defaults and become volumes. Do not add these sections to `dbt-singlestore-workshop/vdp.toml` unless the hub should read the project's file; `vdp` does not warn about
   `[release_hub]`/`[notify]` there. Run the service with `--no-vault` (the stack has no Vault); with no Jira secrets the hub starts with a warning.
4. **Compose service.** In `infrastructure/compose/docker-compose.yml`, following `overview` and `workshop-studio`: service `release-hub`, `container_name: workshop-release-hub`, `restart: on-failure`, the config mount, the two volumes, `networks: [workshop_net]`, and
   ```yaml
       ports:
         - "127.0.0.1:${WORKSHOP_RELEASE_UI_PORT:-8765}:8765"
   ```
   Bind to `127.0.0.1` as the overview and the studio do: the hub is plain HTTP, the GitLab token crosses it at sign-in, and `auth = "none"` would let anyone release. Port 8765 is unused in the stack (none of 80, 2222, 8025, 8081-8083, 8085, 8086, 8088-8090, 8099, 23306 collide).
   No `depends_on` for GitLab: it has no healthcheck in the compose file, and the hub needs GitLab only at sign-in and clone. Top-level `volumes:` gets `release_hub_clone` (`name: workshop_release_hub_clone`) and `release_hub_state` (`name: workshop_release_hub_state`), the fixed-name pattern of
   `studio_state`. Choice A adds `command: ["vdp-release-ui", "release", "serve", "--no-vault"]` and, because the dev image runs as `dbt`, a `user: root` or an init step like `workshop-state-init`.
   Add `WORKSHOP_RELEASE_UI_PORT (8765)` to the port list in the header comment of the compose file.
5. **Frontend (optional).** The hub serves a built frontend itself with `--ui-dir`: mount the build read-only and pass `--ui-dir`. If nginx should serve it instead, `infrastructure/nginx/nginx.conf` (now three `server` blocks for the dbt docs sites, in the `dbt-docs` service) needs a new
   `server` on a new published port with `location /api/ { proxy_pass http://release-hub:8765; }`: same origin, so no CORS. Nothing of this exists; the frontend repository is separate.
6. **Overview.** Add an entry to the `shared.services` list of `infrastructure/overview/services.json`, in the shape of `mailpit` and `studio`: `id`, `name`, `container: "workshop-release-hub"`, `purpose`, `probe: {"type": "http", "url": "http://workshop-release-hub:8765/api/v1/status"}`
   (`GET /status` is public), `link: "http://localhost:${WORKSHOP_RELEASE_UI_PORT:8765}/api/v1/status"` (or the UI), `users: {"source": "none", "note": "Signs in with a GitLab personal access token."}`, `credentials`. The `${VAR:default}` in the link is expanded from the overview
   container's environment, so add `WORKSHOP_RELEASE_UI_PORT: ${WORKSHOP_RELEASE_UI_PORT:-8765}` to the `overview` service's `environment:` (as for the other `WORKSHOP_*_PORT`) or the default is shown.
7. **Makefile and the manager.** `make up` and `make bootstrap` run `docker compose up -d --build`, which builds choice B's image by itself; `make images` builds only the base, dev, Airflow and manager images, so choice A is built there and by the compose-less devcontainer build. Optionally echo the hub URL after
   `up` like the Studio and overview lines. `make test` checks the compose file with `docker compose config -q`; it runs no Rust (build and gates stay manual: see the skill's "Verify"). `make reset` recreates databases, the GitLab repository and Airflow overrides but knows nothing of the hub's volumes: after a reset the
   stored analyses and the clone are stale (`stale` flags, old heads). Either add the two volumes to what the manager's reset removes, or document `POST /repo/reclone` and deleting `analyses.json` as the follow-up.
8. **Workshop Studio.** The studio mounts `infrastructure/dbt-singlestore-dev-image` read-only and the scenario `same-toolbox` quotes the Dockerfile from `RUN --mount=type=bind,source=src/sqlfmt` to the first `ln -s` (at most 12 lines). Keep new stages and install lines outside that span (before it or after it), then run
   `docker exec workshop-studio python -m studio.catalog --check`. The `image.versions` action runs `vdp --version` in throwaway dev-image containers; adding `vdp-release-ui --version` is optional.
9. **Documentation to update** when it is done (all say "standalone / not in the stack" today): root `README.md` (host ports under requirements, the services table, the Release hub link line), root `CLAUDE.md` (the Layout bullet on the three workspaces, the skills-table row), the architecture skill
   `.agents/skills/vdab-dbt-workshop-architecture/SKILL.md` (§12 "Rust toolkit workspaces": the `release-ui` row and the bullet "Neither workspace is referenced by compose ...", plus the container and port table in §3 and the volumes in §4), `infrastructure/dbt-singlestore-dev-image/README.md` (if choice A), the compose header
   comment, and in this workspace `README.md`, `CLAUDE.md`, `SKILL.md` ("How it fits the stack") and this file (drop "a plan, not implemented").
10. **Verify.** `make test` (compose config), `make up S=release-hub`, `make logs S=release-hub` shows `release hub on http://0.0.0.0:8765/api/v1 (repository /opt/project)`, `curl -s localhost:8765/api/v1/status` is 200, sign in with a maintainer's token (`POST /api/v1/session`), clone through
    `POST /api/v1/repo/clone`, check Mailpit at `http://localhost:8025` after `POST /api/v1/announcements`, and the overview page shows the service.

## Risks to settle first

- Memory and time: a release build of the three crates plus `vdp-cli`'s dependencies is memory-hungry (the dev image documents `--build-arg CARGO_BUILD_JOBS=2` for busy hosts); a compose `build.args` entry is the equivalent for choice B.
- Sign-in over plain HTTP puts the personal access token in the request body and, on push, in an `Authorization` header to `http://gitlab.local`: acceptable on `workshop_net` and loopback, not elsewhere.
- Clone and fetch use the hub's own git credentials; the workshop project is created public, so anonymous HTTPS works. A private project would need credentials in the container.
- GitLab protects `dev` and `main` only (maintainers push, `main` allows force push): `can_release` is true for a maintainer and false for a developer, which is the behaviour to show.

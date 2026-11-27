# Entry point of the workshop stack. Needs only Docker with the compose plugin
# (and python3 with pytest for `make test`). Extra arguments go through S, e.g. `make logs S=airflow-dev`.
#
# The manager gets the repo mounted at its HOST path, because bootstrap runs `docker compose up` and creates
# `docker run -v` containers whose bind sources the Docker daemon resolves on the host.
REPO     := $(patsubst %/,%,$(dir $(abspath $(lastword $(MAKEFILE_LIST)))))
INFRA    := $(REPO)/infrastructure
COMPOSE_DIR := $(INFRA)/compose
COMPOSE  := docker compose --project-directory $(COMPOSE_DIR) -f $(COMPOSE_DIR)/docker-compose.yml
# no TTY to attach in CI / scripts / VS Code tasks
TTY      := $(shell [ -t 0 ] && [ -t 1 ] || echo -T)
MANAGER  := $(COMPOSE) --profile manager run --rm --build $(TTY) -v $(REPO):$(REPO) -w $(INFRA)/manager manager
S        ?=

.DEFAULT_GOAL := help
.PHONY: help up down stop ps logs images bootstrap reset shell test rust-image rust-check rust-test rust-cargo rust-clean

help: ## list the targets
	@grep -E '^[a-z_-]+:.*## ' $(MAKEFILE_LIST) | sed -E 's/^([a-z_-]+):.*## (.*)/  make \1\t\2/'
	@echo "Extra arguments: make <target> S=<args>, e.g. make logs S=airflow-dev"
	@echo "Rust workspaces: make rust-<target> W=<vdp-rust-utils|release-ui|vaultspeed-framework|all>, default all"

up: ## build and start the stack (S=services to limit)
	$(COMPOSE) up -d --build $(S)
	@echo "Workshop Studio:   http://localhost:$${WORKSHOP_STUDIO_PORT:-8085}"
	@echo "Workshop overview: http://localhost:$${WORKSHOP_OVERVIEW_PORT:-8099}"

down: ## stop and remove the containers (volumes are kept)
	$(COMPOSE) down $(S)

stop: ## stop the containers
	$(COMPOSE) stop $(S)

ps: ## list the containers
	$(COMPOSE) ps $(S)

logs: ## follow the logs (S=service)
	$(COMPOSE) logs -f --tail 100 $(S)

images: ## build the base, dev, Airflow and manager images
	docker build -t dbt-singlestore-base:1.11.11 $(INFRA)/dbt-singlestore-base-image
	docker build -t dbt-singlestore-dev:1.11.11 $(INFRA)/dbt-singlestore-dev-image
	$(COMPOSE) --profile manager build airflow-dev manager

bootstrap: ## first time: build the base and dev images, start the stack, then provision everything in the manager container
	docker build -t dbt-singlestore-base:1.11.11 $(INFRA)/dbt-singlestore-base-image
	docker build -t dbt-singlestore-dev:1.11.11 $(INFRA)/dbt-singlestore-dev-image
	$(COMPOSE) up -d --build
	$(MANAGER) bootstrap $(S)

reset: ## back to the baseline: databases, GitLab repo, Airflow overrides
	$(MANAGER) reset $(S)
	@docker exec workshop-studio python -m studio.reset || echo "Workshop Studio did not reset its state (not running, or a demo is busy): see the line above."

shell: ## bash in the manager container
	$(MANAGER) shell $(S)

test: ## Airflow util tests (host pytest) and a compose config check
	cd $(REPO)/dbt-singlestore-workshop/airflow && python3 -m pytest -q tests
	$(COMPOSE) config -q
	$(COMPOSE) --profile manager config -q

# --- Rust workspaces ---------------------------------------------------------------------------------------------------
# The three Cargo workspaces under infrastructure/dbt-singlestore-dev-image/src/ (vdp-rust-utils, release-ui,
# vaultspeed-framework) are checked in a toolchain container (infrastructure/rust-check), so the host needs no Rust.
# The container works in /src/<workspace> with the whole src/ folder mounted, because the workspaces depend on each other
# by path. It runs as the host user (no root-owned files); build output and the cargo registry live in named volumes,
# never in the repository. W picks the workspace (default: all), CARGO_BUILD_JOBS limits the compile parallelism.
RUST_SRC     := $(INFRA)/dbt-singlestore-dev-image/src
RUST_ALL     := vdp-rust-utils release-ui vaultspeed-framework
# the toolchain version is kept in one place: the ARG of the dev image Dockerfile
RUST_VERSION := $(shell sed -n 's/^ARG RUST_VERSION=//p' $(INFRA)/dbt-singlestore-dev-image/Dockerfile)
RUST_IMAGE   := workshop-rust-check:$(RUST_VERSION)
W            ?= all
RUST_WS      := $(if $(filter all,$(W)),$(RUST_ALL),$(W))
CARGO_BUILD_JOBS ?=
RUST_VOLUMES := workshop-rust-registry workshop-rust-git $(addprefix workshop-rust-target-,$(RUST_ALL))
# $(call rust_each,<command and arguments to run in the container>): once per workspace in W, with the duration of each.
# A terminal gets -it (colours, and Ctrl-C reaches cargo); --init forwards signals when there is none.
define rust_each
tty=; if [ -t 0 ] && [ -t 1 ]; then tty=-it; fi; \
for w in $(RUST_WS); do \
  echo "=== $$w"; start=$$(date +%s); \
  docker run --rm --init $$tty --user $$(id -u):$$(id -g) $(if $(CARGO_BUILD_JOBS),-e CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS)) \
    -v $(RUST_SRC):/src -v workshop-rust-registry:/usr/local/cargo/registry -v workshop-rust-git:/usr/local/cargo/git \
    -v workshop-rust-target-$$w:/cargo-target -w /src/$$w $(RUST_IMAGE) $(1) || exit 1; \
  echo "=== $$w: done in $$(( $$(date +%s) - start ))s"; \
done
endef

rust-image:
	@$(if $(RUST_VERSION),,$(error no ARG RUST_VERSION found in the dev image Dockerfile))
	@$(if $(filter-out $(RUST_ALL),$(RUST_WS)),$(error W=$(W): use all, or one or more of: $(RUST_ALL)))
	@docker build -q -t $(RUST_IMAGE) --build-arg RUST_VERSION=$(RUST_VERSION) $(INFRA)/rust-check >/dev/null

rust-check: rust-image ## Rust workspaces in Docker, whole gate: fmt, clippy (per feature), tests, docs (W=<workspace|all>)
	@$(call rust_each,rust-check check)

rust-test: rust-image ## Rust workspaces in Docker, tests only (W=<workspace|all>, S=cargo test args, e.g. S="-p vdp-dbt")
	@$(call rust_each,rust-check test $(S))

rust-cargo: rust-image ## any cargo command in the Rust toolchain container (S=args, e.g. W=release-ui S=fmt)
	@$(if $(S),,$(error S is empty: make rust-cargo W=<workspace> S="<cargo arguments>"))
	@$(call rust_each,cargo $(S))

rust-clean: ## remove the Rust toolchain image and its cache volumes (registry, build output: about 7 GB)
	-docker volume rm $(RUST_VOLUMES)
	-docker rmi $(RUST_IMAGE)

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
.PHONY: help up down stop ps logs images bootstrap reset shell test

help: ## list the targets
	@grep -E '^[a-z_-]+:.*## ' $(MAKEFILE_LIST) | sed -E 's/^([a-z_-]+):.*## (.*)/  make \1\t\2/'
	@echo "Extra arguments: make <target> S=<args>, e.g. make logs S=airflow-dev"

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

bootstrap: ## first time: start the stack, then provision everything in the manager container
	docker build -t dbt-singlestore-base:1.11.11 $(INFRA)/dbt-singlestore-base-image
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

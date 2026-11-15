# Workshop manager: the Python side of the workshop (bootstrap, reset) in a
# container, so the host needs nothing but Docker - no venv (the exFAT workshop drive cannot hold one), no Python.
# It orchestrates the other containers through the Docker socket and reaches them over workshop_net
# (singlestore:3306, gitlab.local). Run it through the root Makefile, which mounts the repo at its HOST path: the compose
# commands bootstrap issues create bind mounts, and the daemon resolves those against the host filesystem.
FROM python:3.13-slim

ARG DOCKER_CLI_VERSION=27.5.1
ARG COMPOSE_VERSION=v2.32.4
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && curl -fsSL "https://download.docker.com/linux/static/stable/x86_64/docker-${DOCKER_CLI_VERSION}.tgz" \
       | tar -xz --strip-components=1 -C /usr/local/bin docker/docker \
    && mkdir -p /usr/local/lib/docker/cli-plugins \
    && curl -fsSL -o /usr/local/lib/docker/cli-plugins/docker-compose \
       "https://github.com/docker/compose/releases/download/${COMPOSE_VERSION}/docker-compose-linux-x86_64" \
    && chmod +x /usr/local/lib/docker/cli-plugins/docker-compose \
    && docker --version && docker compose version

COPY requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt && rm /tmp/requirements.txt

# git pushes the dbt project to GitLab from a temp copy; the repo mount is owned by the host user
RUN git config --global --add safe.directory '*'

COPY entrypoint.sh /usr/local/bin/workshop-manager
RUN chmod +x /usr/local/bin/workshop-manager
ENV PYTHONUNBUFFERED=1
ENTRYPOINT ["workshop-manager"]
CMD ["help"]

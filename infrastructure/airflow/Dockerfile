FROM apache/airflow:slim-3.3.2-python3.14

USER root
RUN apt-get update && apt-get install -y --no-install-recommends git curl docker.io rsync sudo gcc pkg-config libmariadb-dev && \
    echo "airflow ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers && \
    rm -rf /var/lib/apt/lists/*
# Debian's docker.io CLI is 20.10: too old for `docker run --mount type=volume,...,volume-subpath=dbt` (CLI 26+, API 1.45),
# which the dbt runner containers use to mount the workshop_dev / workshop_prod volumes (utils/dbt_pod_utils.py).
# The static CLI in /usr/local/bin comes first on PATH.
ARG DOCKER_CLI_VERSION=27.5.1
RUN curl -fsSL "https://download.docker.com/linux/static/stable/x86_64/docker-${DOCKER_CLI_VERSION}.tgz" \
    | tar -xz --strip-components=1 -C /usr/local/bin docker/docker && docker --version

USER airflow
# dbt runs in ephemeral docker containers, so no dbt venv here
RUN pip install --no-cache-dir apache-airflow-providers-fab \
    apache-airflow-providers-standard authlib requests mysqlclient aiomysql pymysql

COPY --chown=airflow:0 entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]

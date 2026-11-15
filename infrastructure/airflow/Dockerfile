# mysqlclient (the MySQL driver of the Airflow metadata database) has no Linux wheels. The Airflow 3.4 images are built
# on Docker Hardened Images (Debian 13), whose patched system packages only install from dhi.io (needs a Docker login),
# so a compiler cannot be added there: the wheel is built here, on the same Debian release and Python minor version.
FROM python:3.14-slim-trixie AS mysqlclient-build
RUN apt-get update && apt-get install -y --no-install-recommends gcc pkg-config libmariadb-dev && \
    pip wheel --no-cache-dir --no-deps -w /wheels mysqlclient

# Ships git, curl, rsync, sudo, libmariadb and a docker CLI (29.x: `docker run --mount type=volume,...,volume-subpath=`,
# which utils/dbt_pod_utils.py uses for the workshop_dev / workshop_val / workshop_prd volumes, needs CLI 26+).
FROM apache/airflow:slim-3.4.0-python3.14

USER root
# entrypoint.sh opens the mounted docker socket for the airflow user
RUN echo "airflow ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

USER airflow
# dbt runs in ephemeral docker containers, so no dbt venv here
COPY --from=mysqlclient-build /wheels /tmp/wheels
RUN pip install --no-cache-dir /tmp/wheels/*.whl apache-airflow-providers-fab \
    apache-airflow-providers-standard authlib requests aiomysql pymysql

COPY --chown=airflow:0 entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]

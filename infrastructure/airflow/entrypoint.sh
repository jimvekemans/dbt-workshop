#!/usr/bin/env bash
set -e

if [ -S /var/run/docker.sock ]; then
    sudo chmod 666 /var/run/docker.sock || true
fi

airflow db migrate
airflow dags reserialize || true

# Launch the four required Airflow 3.0 components in the same process group
airflow api-server --port 8080 &
airflow scheduler &
airflow dag-processor &
airflow triggerer &

wait -n



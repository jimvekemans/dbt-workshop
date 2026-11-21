# Project image dbt-vdp:<git tag>, built by CI after every tag. Airflow and the deploy jobs run exactly this image.
ARG BASE_IMAGE=dbt-singlestore-base:1.11.11
FROM ${BASE_IMAGE}

ARG GIT_TAG=untagged
ARG GIT_COMMIT=unknown
LABEL org.opencontainers.image.version="${GIT_TAG}" org.opencontainers.image.revision="${GIT_COMMIT}"

COPY --chown=1000:0 . /usr/app/dbt/workspace
WORKDIR /usr/app/dbt/workspace
USER dbt:root
# a failed download of the packages (network of the moment) is tried twice more
RUN dbt deps || (sleep 5 && dbt deps) || (sleep 15 && dbt deps)

CMD ["dbt", "--version"]

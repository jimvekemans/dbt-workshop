#! /bin/bash

echo "Performing post-create steps..."

cd "$1" || exit 1
ENV_FILE="${1}/.devcontainer/.env"

echo "Which developer account do you use (pu_dev1, pu_dev2 or pu_dev3)? [pu_dev1]"
read -r SINGLESTORE_USER
SINGLESTORE_USER="$(echo "${SINGLESTORE_USER:-pu_dev1}" | tr '[:upper:]' '[:lower:]')"

echo "Password for $SINGLESTORE_USER (used for the SingleStore connection):"
read -rs SINGLESTORE_PASS

cat > "$ENV_FILE" <<ENV
SINGLESTORE_HOST=singlestore
SINGLESTORE_USER=$SINGLESTORE_USER
SINGLESTORE_PASS=$SINGLESTORE_PASS
SINGLESTORE_DB=dev_edw_dv_rdv
DBT_DEPLOY_TARGET=dev
ENV
echo "Wrote $ENV_FILE"

# Install dbt dependencies
if [ ! -d "dbt_packages" ]; then
    dbt deps || true
fi

# Add safe directory to git config
git config --global --add safe.directory "$1" || true

# Git identity from the developer account
git config --local user.name "$SINGLESTORE_USER" || true
git config --local user.email "$SINGLESTORE_USER@workshop.local" || true

# Set up pre-commit hooks once the project defines them
if [ -f .pre-commit-config.yaml ]; then
    pre-commit install || true
fi

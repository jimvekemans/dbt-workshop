#!/bin/bash
# Test a feature branch in the dev environment: build only what changed against the merge-base with origin/dev and
# defer everything else to the state of that merge-base (artifacts/).
export DBT_DEPLOY_TARGET="${DBT_DEPLOY_TARGET:-dev}"

read -p "Branch to test: " branch_to_test

if [ -z "$branch_to_test" ]; then
    echo "Error: Branch to test is required."
    exit 1
fi

echo "Select build mode:"
echo "1) Build with user prefix (default target, schema = your user)"
echo "2) Build with dev prefix"
echo "3) Build with branch-based schema name"
read -p "Enter choice [1]: " build_mode
build_mode=${build_mode:-1}

echo "Running setup for branch: $branch_to_test (environment: $DBT_DEPLOY_TARGET)"

git fetch origin dev && \
git switch "$branch_to_test" && \
git pull origin "$branch_to_test" --rebase && \
MERGE_BASE=$(git merge-base "$branch_to_test" origin/dev) && \
git checkout "$MERGE_BASE" && \
dbt compile --target run --target-path artifacts && \
git switch "$branch_to_test"

if [ $? -ne 0 ]; then
    echo "Git operations failed."
    exit 1
fi

if [ "$build_mode" = "3" ]; then
    schema_name="${branch_to_test//[\/-]/_}"
    dbt build -s state:modified --defer --state artifacts --target run --vars "SCHEMA: ${schema_name}"
elif [ "$build_mode" = "2" ]; then
    dbt build -s state:modified --defer --state artifacts --target run --vars "SCHEMA: dev"
else
    dbt build -s state:modified --defer --state artifacts
fi

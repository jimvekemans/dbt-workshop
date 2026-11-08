#! /bin/bash

echo "Performing post-create steps..."

cd $HOME

# Start ssh-agent if not already running
if [ -z "$SSH_AUTH_SOCK" ]; then
    eval "$(ssh-agent -s)"
fi

# Add all non .pub keys to ssh-agent
if [ -d ".ssh" ]; then
    echo "Adding SSH keys to ssh-agent..."
    find ".ssh" -type f -not -name "*.pub" -not -name "known_hosts" -not -name "config" -not -name "authorized_keys" | while read -r keyfile; do
        # Check if file is a valid private key
        ssh-keygen -l -f "$keyfile" >/dev/null 2>&1
        if [ $? -eq 0 ]; then
            ssh-add "$keyfile" 2>/dev/null
            if [ $? -eq 0 ]; then
                echo "Added key: $keyfile"
            fi
        fi
    done
fi

cd $1

ENV_FILE="${1}/.devcontainer/.env"

echo "Creating .env file..."

echo "Please enter your PU-username:"
read SINGLESTORE_USER

echo "Please enter password for $SINGLESTORE_USER (used for singlestore connection):"
read -s SINGLESTORE_PASS

echo "SINGLESTORE_HOST=singlestore-master00-rel1ldv.ops.vdab.be" > $ENV_FILE
echo "SINGLESTORE_USER=$(echo $SINGLESTORE_USER | tr '[:upper:]' '[:lower:]')" >> $ENV_FILE
echo "SINGLESTORE_PASS=$SINGLESTORE_PASS" >> $ENV_FILE
echo "SINGLESTORE_DB=edw_dv_rdv" >> $ENV_FILE

# Install dbt dependencies
if [ ! -d "dbt_packages" ]; then
    dbt deps || true
fi

# Add safe directory to git config
git config --global --add safe.directory $1 || true

# Check if user.email is set in local git config
if ! git config --local --get user.email >/dev/null; then
    read -p "Enter your GitLab email address (@vdab.be): " email
    git config --local user.email "$email" || true
fi

# Check if user.name is set in local git config
if ! git config --local --get user.name >/dev/null; then
    git config --local user.name "$SINGLESTORE_USER" || true
fi

# Set up pre-commit hooks
pre-commit install || true

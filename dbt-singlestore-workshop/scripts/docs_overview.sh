#!/usr/bin/env bash
# Writes models/__overview__.md: the front page of the documentation site, a `{% docs __overview__ %}` block that
# `dbt docs generate` shows as the landing page. The deploy job calls it just before `dbt docs generate`, so the page says
# which version runs in which environment, when the site was generated, how big the project is and who the teams are.
# Needs only bash, awk and dbt (the project image has no Python). Best effort: it never fails the deploy, and without the
# file the site keeps its default landing page. The generated file is not committed (.gitignore).
#
# Input (all optional): CI_COMMIT_TAG (version), DEPLOY_ENV or DBT_DEPLOY_TARGET (environment), DBT_TARGET (dbt target
# used to list the project, default `run`).
set -uo pipefail
cd "$(dirname "$0")/.." || exit 0

out="models/__overview__.md"
version="${CI_COMMIT_TAG:-unknown}"
environment="${DEPLOY_ENV:-${DBT_DEPLOY_TARGET:-dev}}"
generated="$(date -u '+%Y-%m-%d %H:%M:%S UTC')"

# Number of nodes of one resource type (a table is a model, a check is a test, a feed is a source). `--select` is explicit
# because the default selector (daily_load) would leave out the on-demand models and most checks.
count() {
  local n
  n="$(dbt ls --target "${DBT_TARGET:-run}" --select "resource_type:$1" --resource-type "$1" --output name --quiet 2>/dev/null \
    | grep -c -E '^[A-Za-z0-9_.-]+$')"
  if [ "${n:-0}" -gt 0 ]; then echo "$n"; else echo "n/a"; fi
}

tables="$(count model)"
checks="$(count test)"
feeds="$(count source)"

# Teams: the groups of models/groups.yml with the owner's address.
teams="$(awk '
  /^  - name:/ { name = $3 }
  /^ +email:/  { if (name != "") { print "- `" name "` (" $2 ")"; name = "" } }
' models/groups.yml 2>/dev/null)"
[ -n "$teams" ] || teams="- (no teams defined)"

tmp="$(mktemp)" || exit 0
chmod 644 "$tmp"
cat > "$tmp" <<EOF
{% docs __overview__ %}
# Data platform

- **Version:** \`${version}\`
- **Environment:** ${environment}
- **Generated:** ${generated}
- **Tables:** ${tables}, **checks:** ${checks}, **feeds:** ${feeds}

## Teams

${teams}
{% enddocs %}
EOF

if mv "$tmp" "$out"; then
  echo "docs_overview: wrote $out (version ${version}, environment ${environment}, ${tables} tables, ${checks} checks, ${feeds} feeds)"
else
  rm -f "$tmp"
  echo "docs_overview: could not write $out, the site keeps its default front page"
fi
exit 0

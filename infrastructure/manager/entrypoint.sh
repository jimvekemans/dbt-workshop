#!/usr/bin/env bash
# Entry point of the workshop manager container (see Dockerfile). The working directory is infrastructure/manager/
# at its host path (set by the root Makefile), which holds bootstrap_stack.py and reset_workshop.py.
set -euo pipefail

usage() {
    cat <<'USAGE'
workshop manager - runs inside the workshop-manager container (start it with make bootstrap / make reset)

  bootstrap   first-time setup: SingleStore, GitLab, runner, images, baseline push, first pipeline
  reset       drop and re-create the environment databases, reset the GitLab repo, clear the Airflow overrides
  shell       bash in the manager, with the repo mounted
  <command>   any other command, e.g. python -c ...
USAGE
}

cmd="${1:-help}"
[ $# -gt 0 ] && shift
case "$cmd" in
    bootstrap)      exec python bootstrap_stack.py "$@" ;;
    reset)          exec python reset_workshop.py "$@" ;;
    shell)          exec bash "$@" ;;
    help|-h|--help) usage ;;
    *)              exec "$cmd" "$@" ;;
esac

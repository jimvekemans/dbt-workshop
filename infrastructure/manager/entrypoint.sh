#!/usr/bin/env bash
# Entry point of the workshop manager container (see Dockerfile). The working directory is infrastructure/manager/
# at its host path (set by ../workshop.sh), which holds bootstrap_stack.py, reset_workshop.py and milestones.sh.
set -euo pipefail

usage() {
    cat <<'EOF'
workshop manager - runs inside the workshop-manager container (start it with infrastructure/workshop.sh)

  bootstrap [--reset-only] [--initial-days N]   first-time setup: SingleStore, GitLab, runner, images, deploy, milestone
  reset [reset_workshop.py args]                 full reset to the baseline, or trickle / channel faults / milestones:
                                                   reset --trickle-prod 14 | --channel kafka --delay-batches 2 --env prod
                                                   reset --show-channels | --reset-channels
  milestone list | create|restore|delete NAME    SingleStore restore points (stops SingleStore for a few seconds)
  shell                                          bash in the manager, with the repo mounted
  <command ...>                                  any other command, e.g. python -c ...
EOF
}

cmd="${1:-help}"
[ $# -gt 0 ] && shift
case "$cmd" in
    bootstrap)            exec python bootstrap_stack.py "$@" ;;
    reset)                exec python reset_workshop.py "$@" ;;
    milestone|milestones) exec bash milestones.sh "$@" ;;
    shell)                exec bash "$@" ;;
    help|-h|--help)       usage ;;
    *)                    exec "$cmd" "$@" ;;
esac

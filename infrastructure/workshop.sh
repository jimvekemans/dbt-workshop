#!/usr/bin/env bash
# One entry point for the workshop stack. Needs only Docker (with the compose plugin) on the host.
#
#   infrastructure/workshop.sh up [svc...]        build + start the stack (docker compose up -d --build)
#   infrastructure/workshop.sh bootstrap          first time: up, then provision everything (manager container)
#   infrastructure/workshop.sh reset [args]       full reset to the baseline / trickle / channel faults (manager)
#   infrastructure/workshop.sh milestone list | create|restore|delete NAME
#   infrastructure/workshop.sh shell              bash in the manager container
#   infrastructure/workshop.sh session            VS Code: up now, stop when the last window closes (folderOpen task)
#   infrastructure/workshop.sh session --devcontainer <folder>   same, tied to that folder's dev container
#   infrastructure/workshop.sh stop | down | ps | logs [svc]   plain docker compose
#
# Layout: compose/ (docker-compose.yml + .env), manager/ (the Python side: bootstrap_stack.py, reset_workshop.py,
# milestones.sh, run in the manager container), one folder per image/service config (airflow, gitlab, nginx,
# singlestore, workshop-studio, dbt-singlestore-base-image).
#
# The manager gets the repo mounted at its HOST path, because bootstrap runs `docker compose up` and creates
# `docker run -v` containers whose bind sources the Docker daemon resolves on the host.
set -euo pipefail

SCRIPT="$(readlink -f "${BASH_SOURCE[0]}")"
INFRA="$(dirname "$SCRIPT")"
REPO="$(dirname "$INFRA")"
COMPOSE_DIR="$INFRA/compose"
COMPOSE=(docker compose --project-directory "$COMPOSE_DIR" -f "$COMPOSE_DIR/docker-compose.yml")
SESSION_DIR="${XDG_RUNTIME_DIR:-/tmp}/dbt-workshop-sessions"

manager() {
    local tty=()
    [ -t 0 ] && [ -t 1 ] || tty=(-T)    # CI / scripts / VS Code tasks: no TTY to attach
    "${COMPOSE[@]}" --profile manager run --rm --build "${tty[@]}" -v "$REPO:$REPO" -w "$INFRA/manager" manager "$@"
}

# ---------------------------------------------------------------------------------------------------------------
# session: ties the stack to VS Code (was cluster_session.sh). Two ways in:
#   session                           started by the "Workshop cluster" folderOpen task in
#                                     dbt-singlestore-workshop/.vscode/tasks.json: brings the stack up, then blocks.
#                                     When the window closes, VS Code tears its terminal down (SIGHUP/SIGTERM) and the
#                                     trap stops the stack.
#   session --devcontainer <folder>   run on the host by the devcontainer's initializeCommand: brings the stack up,
#                                     then hands off to a detached watcher that stops it once the dev container for
#                                     that folder has stopped (shutdownAction: stopContainer).
# Either way the stack is only stopped when no other session is still open.
devcontainer_ids() {
    docker ps -q --filter "label=devcontainer.local_folder=$1"
}

live_sessions() {
    local f pid count=0
    for f in "$SESSION_DIR"/[0-9]*; do
        [ -e "$f" ] || continue
        pid="$(basename "$f")"
        if kill -0 "$pid" 2>/dev/null; then count=$((count + 1)); else rm -f "$f"; fi
    done
    echo "$count"
}

stop_unless_other_sessions() {
    if [ "$(live_sessions)" -eq 0 ]; then
        # detached, so the stop survives VS Code killing this terminal's process group
        setsid nohup "${COMPOSE[@]}" stop >"$SESSION_DIR/last-stop.log" 2>&1 < /dev/null &
        echo "Stopping workshop cluster (log: $SESSION_DIR/last-stop.log)"
    else
        echo "Other VS Code sessions still open, leaving cluster running"
    fi
}

session() {
    mkdir -p "$SESSION_DIR"
    if [ "${1:-}" = "--devcontainer" ]; then
        local folder="$2" watcher_file
        watcher_file="$SESSION_DIR/devcontainer-$(printf '%s' "$folder" | md5sum | cut -c1-12)"
        echo "Starting workshop cluster..."
        "${COMPOSE[@]}" up -d
        # a rebuild/reopen re-runs initializeCommand: keep a single watcher per folder
        if [ -f "$watcher_file" ] && kill -0 "$(cat "$watcher_file")" 2>/dev/null; then
            return 0
        fi
        setsid nohup "$SCRIPT" session --watch-devcontainer "$folder" "$watcher_file" \
            >"$watcher_file.log" 2>&1 < /dev/null &
        echo $! > "$watcher_file"
        echo "Cluster will be stopped when the dev container stops (log: $watcher_file.log)"
        return 0
    fi

    local session_file="$SESSION_DIR/$$"
    echo $$ > "$session_file"
    trap 'trap - HUP INT TERM EXIT; rm -f "'"$session_file"'"; stop_unless_other_sessions; exit 0' HUP INT TERM EXIT

    if [ "${1:-}" = "--watch-devcontainer" ]; then
        local folder="$2"
        # initializeCommand runs before the container exists: give it time to start (first builds can be slow)
        for _ in $(seq 120); do
            [ -n "$(devcontainer_ids "$folder")" ] && break
            sleep 5
        done
        while [ -n "$(devcontainer_ids "$folder")" ]; do sleep 10; done
        echo "Dev container for $folder stopped"
        rm -f "$3"
        exit 0
    fi

    echo "Starting workshop cluster..."
    "${COMPOSE[@]}" up -d
    echo
    echo "Cluster is up. It will be stopped when this VS Code window closes."
    echo "(Closing this terminal or killing the task also stops it.)"
    # block until VS Code tears the terminal down; sleep in the background so traps fire immediately
    while true; do
        sleep 3600 &
        wait $!
    done
}

cmd="${1:-help}"
[ $# -gt 0 ] && shift
case "$cmd" in
    up)        "${COMPOSE[@]}" up -d --build "$@" ;;
    bootstrap) "${COMPOSE[@]}" up -d --build && manager bootstrap "$@" ;;
    reset|milestone|milestones|shell) manager "$cmd" "$@" ;;
    manager)   manager "$@" ;;
    session)   session "$@" ;;
    stop|down|ps|logs|config|restart|pull) "${COMPOSE[@]}" "$cmd" "$@" ;;
    help|-h|--help)
        sed -n '2,19p' "$SCRIPT" | sed 's/^# \{0,1\}//' ;;
    *) echo "unknown command '$cmd' (try: $0 help)" >&2; exit 2 ;;
esac

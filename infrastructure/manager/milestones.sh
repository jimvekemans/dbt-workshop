#!/usr/bin/env bash
# Workshop "milestones" for SingleStore: named restore points of the whole warehouse.
#
#   milestones.sh create  <name>   snapshot the SingleStore data volume (e.g. "baseline" after bootstrap)
#   milestones.sh restore <name>   put the warehouse back exactly as it was at <name>
#   milestones.sh delete  <name>
#   milestones.sh list
#
# Why not CREATE MILESTONE / BACKUP DATABASE: the singlestoredb-dev image is the Developer Edition. Milestones need
# unlimited-storage databases ("ERROR 2633: Feature 'self-hosted unlimited storage' is not supported in SingleStore
# Developer Image Edition") and BACKUP needs Enterprise ("ERROR 1727: 'distributed backup'"). So a milestone here is a
# copy of the data volume (/data in the container), taken while SingleStore is stopped: every database, table, view,
# procedure, user, group and grant comes back - landing tables, model tables and the vdp_mgmt run log alike.
#
# A create or restore stops SingleStore for a few seconds (restore ~15 s including the restart): connected clients
# (an Airflow task, a dbt run) fail. Airflow metadata (MySQL), GitLab and the LDV/PRD workspaces are not part of it.
#
# Used by bootstrap_stack.py / reset_workshop.py (manager container) and by the studio (mounted at /app/bin/milestones.sh).
set -euo pipefail

S2_CONTAINER="${S2_CONTAINER:-workshop-singlestore}"
DATA_VOLUME="${S2_DATA_VOLUME:-workshop_s2_data}"
SNAP_VOLUME="${S2_MILESTONE_VOLUME:-workshop_s2_milestones}"
HELPER_IMAGE="${MILESTONE_HELPER_IMAGE:-alpine:3.22}"
HEALTH_TIMEOUT="${S2_HEALTH_TIMEOUT:-300}"

die() { echo "milestones: $*" >&2; exit 1; }

valid_name() {
    [[ "${1:-}" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || die "invalid milestone name '${1:-}' (letters, digits, . _ -)"
}

helper() {  # helper <mount args...> -- <sh script>
    local mounts=()
    while [ "$1" != "--" ]; do mounts+=("$1"); shift; done
    shift
    docker run --rm "${mounts[@]}" "$HELPER_IMAGE" sh -c "$1"
}

is_running() { [ "$(docker inspect -f '{{.State.Running}}' "$S2_CONTAINER" 2>/dev/null)" = "true" ]; }

wait_healthy() {
    local waited=0 status
    while :; do
        status="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$S2_CONTAINER")"
        [ "$status" = "healthy" ] && return 0
        [ "$waited" -ge "$HEALTH_TIMEOUT" ] && die "$S2_CONTAINER not healthy after ${HEALTH_TIMEOUT}s (status: $status)"
        sleep 2; waited=$((waited + 2))
    done
}

stop_s2() { docker stop -t 60 "$S2_CONTAINER" >/dev/null; }
start_s2() { docker start "$S2_CONTAINER" >/dev/null; wait_healthy; }

exists() {
    helper -v "$SNAP_VOLUME:/m:ro" -- "test -d '/m/$1'" 2>/dev/null
}

cmd_list() {
    docker volume inspect "$SNAP_VOLUME" >/dev/null 2>&1 || { echo "(no milestones: volume $SNAP_VOLUME does not exist)"; return 0; }
    helper -v "$SNAP_VOLUME:/m:ro" -- '
        found=0
        for d in /m/*/; do
            [ -d "$d" ] || continue
            n=$(basename "$d"); found=1
            printf "%-24s %-26s %s\n" "$n" "$(cat "/m/.meta/$n" 2>/dev/null || echo "?")" "$(du -sh "$d" | cut -f1)"
        done
        [ "$found" = 1 ] || echo "(no milestones yet)"'
}

cmd_create() {
    local name="$1" was_running=0
    valid_name "$name"
    docker volume inspect "$DATA_VOLUME" >/dev/null 2>&1 || die "data volume $DATA_VOLUME does not exist (is the stack up?)"
    is_running && was_running=1
    [ "$was_running" = 1 ] && stop_s2
    # copy into <name>.tmp first: an interrupted copy never replaces a good milestone
    if ! helper -v "$DATA_VOLUME:/from:ro" -v "$SNAP_VOLUME:/m" -- "
        set -e
        rm -rf '/m/$name.tmp' && mkdir -p '/m/$name.tmp' /m/.meta
        cd /from && tar cf - --exclude='*.sock' . | tar xpf - -C '/m/$name.tmp'
        rm -rf '/m/$name' && mv '/m/$name.tmp' '/m/$name'
        date -u '+%Y-%m-%d %H:%M:%S UTC' > '/m/.meta/$name'"; then
        [ "$was_running" = 1 ] && start_s2
        die "snapshot failed; SingleStore restarted unchanged"
    fi
    [ "$was_running" = 1 ] && start_s2
    echo "milestone '$name' created"
}

cmd_restore() {
    local name="$1"
    valid_name "$name"
    exists "$name" || die "no milestone '$name' (milestones.sh list)"
    is_running && stop_s2
    helper -v "$DATA_VOLUME:/data" -v "$SNAP_VOLUME:/m:ro" -- "
        set -e
        find /data -mindepth 1 -delete
        cd '/m/$name' && tar cf - . | tar xpf - -C /data"
    start_s2
    echo "milestone '$name' restored"
}

cmd_delete() {
    local name="$1"
    valid_name "$name"
    exists "$name" || die "no milestone '$name'"
    helper -v "$SNAP_VOLUME:/m" -- "rm -rf '/m/$name' '/m/.meta/$name'"
    echo "milestone '$name' deleted"
}

case "${1:-}" in
    list) cmd_list ;;
    create|restore|delete)
        [ -n "${2:-}" ] || die "usage: $0 $1 <name>"
        "cmd_$1" "$2" ;;
    *) echo "usage: $0 create|restore|delete <name> | list" >&2; exit 2 ;;
esac

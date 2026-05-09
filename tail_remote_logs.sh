#!/usr/bin/env bash
# Tail logs from a remote Matchbox Audio Pi.
#
# Usage:
#   ./tail_remote_logs.sh [hostname]
#
# Default hostname: matchbox-audio.local

set -euo pipefail

REMOTE_HOST="${1:-${MATCHBOX_AUDIO_HOST:-matchbox-audio.local}}"
REMOTE_USER="${MATCHBOX_AUDIO_USER:-matchbox}"
REMOTE_TARGET="${REMOTE_USER}@${REMOTE_HOST}"
SERVICES="${MATCHBOX_AUDIO_LOG_SERVICES:-mba-device.service NetworkManager.service mba-player.service}"
SINCE="${MATCHBOX_AUDIO_LOGS_SINCE:-now}"

COLOR_RESET='\033[0m'
COLOR_CYAN='\033[36m'
COLOR_YELLOW='\033[33m'

SERVICE_ARGS=()
for service in $SERVICES; do
    SERVICE_ARGS+=("-u" "$service")
done

shell_quote() {
    printf "'"
    printf "%s" "$1" | sed "s/'/'\\\\''/g"
    printf "'"
}

REMOTE_ARGS=(sudo -n journalctl "${SERVICE_ARGS[@]}" -f --no-pager --output=short-precise --since "$SINCE")
REMOTE_CMD=""
for arg in "${REMOTE_ARGS[@]}"; do
    if [ -n "$REMOTE_CMD" ]; then
        REMOTE_CMD+=" "
    fi
    REMOTE_CMD+="$(shell_quote "$arg")"
done

printf "${COLOR_CYAN}Tailing logs from %s${COLOR_RESET}\n" "$REMOTE_TARGET"
printf "${COLOR_YELLOW}Services: %s${COLOR_RESET}\n" "$SERVICES"
printf "${COLOR_YELLOW}Since: %s${COLOR_RESET}\n" "$SINCE"
printf "\n"

ssh -o ConnectTimeout=10 -o BatchMode=yes "$REMOTE_TARGET" "$REMOTE_CMD"

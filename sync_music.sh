#!/usr/bin/env bash
# Sync a local music library to the Matchbox Audio data partition.
#
# Defaults:
#   source: ~/Music
#   target: matchbox@matchbox-audio.local:/data/music
#
# Examples:
#   ./sync_music.sh --dry-run
#   ./sync_music.sh --host 10.42.0.1
#   ./sync_music.sh --delete

set -euo pipefail

REMOTE_HOST="${MATCHBOX_AUDIO_HOST:-matchbox-audio.local}"
REMOTE_USER="${MATCHBOX_AUDIO_USER:-matchbox}"
SOURCE_DIR="${MATCHBOX_AUDIO_MUSIC_SOURCE:-$HOME/Music}"
DEST_DIR="${MATCHBOX_AUDIO_MUSIC_DEST:-/data/music}"
DRY_RUN=0
DELETE=0

usage() {
    cat <<'EOF'
Usage: ./sync_music.sh [options]

Options:
  --host HOST       Remote host or IP. Default: MATCHBOX_AUDIO_HOST or matchbox-audio.local
  --user USER       Remote SSH user. Default: MATCHBOX_AUDIO_USER or matchbox
  --source DIR      Local music directory. Default: MATCHBOX_AUDIO_MUSIC_SOURCE or ~/Music
  --dest DIR        Remote music directory. Default: MATCHBOX_AUDIO_MUSIC_DEST or /data/music
  --dry-run         Show changes without copying files.
  --delete          Delete remote files missing from the local source.
  -h, --help        Show this help.
EOF
}

shell_quote() {
    printf "'"
    printf "%s" "$1" | sed "s/'/'\\\\''/g"
    printf "'"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --host)
            REMOTE_HOST="${2:?missing value for --host}"
            shift 2
            ;;
        --user)
            REMOTE_USER="${2:?missing value for --user}"
            shift 2
            ;;
        --source)
            SOURCE_DIR="${2:?missing value for --source}"
            shift 2
            ;;
        --dest)
            DEST_DIR="${2:?missing value for --dest}"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --delete)
            DELETE=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [ ! -d "$SOURCE_DIR" ]; then
    echo "error: source directory does not exist: $SOURCE_DIR" >&2
    exit 1
fi

REMOTE_TARGET="${REMOTE_USER}@${REMOTE_HOST}"
REMOTE_DEST="${REMOTE_TARGET}:${DEST_DIR%/}/"
REMOTE_PREPARE="sudo -n mkdir -p $(shell_quote "$DEST_DIR")"
REMOTE_PREPARE+=" && sudo -n chown $(shell_quote "$REMOTE_USER:$REMOTE_USER")"
REMOTE_PREPARE+=" $(shell_quote "$DEST_DIR")"

RSYNC_ARGS=(
    -az
    --partial
    --human-readable
    --info=progress2
)

if [ "$DRY_RUN" -eq 1 ]; then
    RSYNC_ARGS+=(--dry-run)
fi

if [ "$DELETE" -eq 1 ]; then
    RSYNC_ARGS+=(--delete)
fi

printf "Syncing music from %s to %s\n" "$SOURCE_DIR" "$REMOTE_DEST"
if [ "$DRY_RUN" -eq 1 ]; then
    printf "Dry run enabled; no files will be copied.\n"
fi
if [ "$DELETE" -eq 1 ]; then
    printf "Delete enabled; remote files missing locally will be removed.\n"
fi

ssh -o ConnectTimeout=10 -o BatchMode=yes "$REMOTE_TARGET" "$REMOTE_PREPARE"
rsync "${RSYNC_ARGS[@]}" "$SOURCE_DIR"/ "$REMOTE_DEST"

#!/usr/bin/env bash
# Convenience wrapper for full Yocto A/B updates to a Matchbox Audio target.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
YOCTO_DIR="$SCRIPT_DIR/yocto"
SKIP_CONFIRM=true
PASSTHROUGH=()

show_help() {
    cat <<'EOF'
Matchbox Audio Remote Update

Usage:
  ./update.sh [options]

Common options:
  --host <host>       Hostname or IP address (default: matchbox-audio.local)
  --target <host>     Alias for --host, matching the dirtsim wrapper
  --skip-build        Reuse latest built image
  --dry-run           Show planned work without changing the device
  --smoke             Run the remote smoke test after reboot
  --confirm           Require the interactive A/B flash confirmation
  -h, --help          Show this help

By default this wrapper skips the final confirmation prompt, matching the
dirtsim update wrapper. Use --confirm for the interactive prompt.
EOF
}

while (($#)); do
    case "$1" in
        --confirm)
            SKIP_CONFIRM=false
            shift
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            PASSTHROUGH+=("$1")
            shift
            ;;
    esac
done

if [ ! -d "$YOCTO_DIR" ]; then
    echo "Error: yocto directory not found at $YOCTO_DIR" >&2
    exit 1
fi

CMD=(npm run update --)
if [ "$SKIP_CONFIRM" = true ]; then
    CMD+=(--yes)
fi
CMD+=("${PASSTHROUGH[@]}")

cd "$YOCTO_DIR"
exec "${CMD[@]}"

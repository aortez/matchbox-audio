#!/bin/sh
# Capture a screenshot from an attached Android device.

set -eu

ACTIVITY="dev.matchbox.audio/.MainActivity"
OUTPUT="docs/screenshots/android-now-playing.png"
WAIT_SECONDS=2
SERIAL=""

usage() {
    cat <<'EOF'
usage: tools/android-capture.sh [options]

Options:
  --activity <component>  Activity component to launch before capture.
                          Default: dev.matchbox.audio/.MainActivity
  --output <path>         PNG output path.
                          Default: docs/screenshots/android-now-playing.png
  --serial <serial>       adb device serial.
  --wait <seconds>        Seconds to wait after launching the activity.
                          Default: 2
  -h, --help              Show this help.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --activity)
            ACTIVITY="${2:?missing --activity value}"
            shift 2
            ;;
        --output)
            OUTPUT="${2:?missing --output value}"
            shift 2
            ;;
        --serial)
            SERIAL="${2:?missing --serial value}"
            shift 2
            ;;
        --wait)
            WAIT_SECONDS="${2:?missing --wait value}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

case "$WAIT_SECONDS" in
    ''|*[!0-9]*)
        echo "error: --wait must be a non-negative integer" >&2
        exit 2
        ;;
esac

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
case "$OUTPUT" in
    /*) OUTPUT_PATH="$OUTPUT" ;;
    *) OUTPUT_PATH="$REPO_ROOT/$OUTPUT" ;;
esac

if [ -n "$SERIAL" ]; then
    ADB="adb -s $SERIAL"
else
    ADB="adb"
fi

OUT_DIR=$(dirname -- "$OUTPUT_PATH")
mkdir -p "$OUT_DIR"

$ADB shell input keyevent KEYCODE_WAKEUP >/dev/null 2>&1 || true
$ADB shell wm dismiss-keyguard >/dev/null 2>&1 || true
$ADB shell cmd statusbar collapse >/dev/null 2>&1 || true
$ADB shell am start -n "$ACTIVITY" >/dev/null
sleep "$WAIT_SECONDS"
$ADB exec-out screencap -p >"$OUTPUT_PATH"

printf 'wrote %s\n' "$OUTPUT_PATH"

#!/usr/bin/env bash
# Capture the current Matchbox Audio LCD framebuffer and save it locally as
# screenshot.png. Defaults to the matchbox-audio.local mDNS name; pass an
# alternate hostname (or user@host) as the first argument.
set -euo pipefail

HOST="${1:-matchbox-audio.local}"
OUTPUT="screenshot.png"

# Allow overriding the SSH user via "user@host"; default to matchbox.
if [[ "${HOST}" != *@* ]]; then
  TARGET="matchbox@${HOST}"
else
  TARGET="${HOST}"
fi

ssh -o ConnectTimeout=5 -o BatchMode=yes "${TARGET}" 'mba-cli screenshot' > "${OUTPUT}"
echo "wrote ${OUTPUT} ($(stat -c %s "${OUTPUT}") bytes) from ${TARGET}"

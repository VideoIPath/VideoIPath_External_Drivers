#!/bin/sh
# Local opt-in qualification only; the test creates isolated synthetic children.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
case $(uname -s) in
    Linux) ;;
    *) echo 'The local soak requires Linux /proc and Unix sockets.' >&2; exit 1 ;;
esac
SOAK_SECONDS=${SOAK_SECONDS:-60}
case "$SOAK_SECONDS" in
    ''|*[!0-9]*) echo 'SOAK_SECONDS must be an integer from 2 through 86400.' >&2; exit 1 ;;
esac
if [ "$SOAK_SECONDS" -lt 2 ] || [ "$SOAK_SECONDS" -gt 86400 ]; then
    echo 'SOAK_SECONDS must be from 2 through 86400.' >&2
    exit 1
fi
export SOAK_SECONDS
# No hidden build-profile override: this qualifies the Cargo debug executable.
exec cargo test --locked --test soak elapsed_soak -- --ignored --exact --nocapture

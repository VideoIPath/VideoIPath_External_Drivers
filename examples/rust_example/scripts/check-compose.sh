#!/bin/sh
# Validate without rendering resolved environment values into terminals or CI artifacts.
set -eu
# Resolve relative files from the example, not from the caller's working directory.
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
if [ "$#" -gt 1 ]; then
    echo 'usage: sh scripts/check-compose.sh [absolute-env-file]' >&2
    exit 2
fi
env_file=${1:-.env}
# Shell variables retain Compose precedence; deployment automation must control them.
# Even a malformed .env parser error may contain source text, so suppress raw diagnostics.
if ! docker compose --env-file "$env_file" -f compose.yaml config --quiet >/dev/null 2>&1; then
    echo 'Compose validation failed; check required settings and .env syntax privately (raw diagnostics suppressed).' >&2
    exit 1
fi
if ! docker compose --env-file "$env_file" -f compose.yaml -f compose.build.yaml config --quiet >/dev/null 2>&1; then
    echo 'Compose build-override validation failed (raw diagnostics suppressed).' >&2
    exit 1
fi
echo 'Compose deployment and local-build configurations are valid (values not printed).'

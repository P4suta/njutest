#!/usr/bin/env bash
# One-time contributor seed. mise is the only prerequisite; the checked-in
# manifests install the exact Rust and development toolchain.
set -euo pipefail

if ! command -v mise >/dev/null 2>&1; then
    echo "error: mise not found. Install it from https://mise.jdx.dev/getting-started.html" >&2
    exit 1
fi

mise install
echo "tools installed — running the locked setup…"
exec mise run bootstrap

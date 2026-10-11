#!/usr/bin/env bash
# Re-renders the README demos from docs/demo/*.tape against the demo fixtures.
# Run from anywhere: `mise run demo`. Needs vhs, ttyd (both in mise.toml) and ffmpeg.
set -euo pipefail
cd "$(dirname "$0")/../.."

command -v ffmpeg >/dev/null || { echo "render.sh: vhs needs ffmpeg on PATH" >&2; exit 1; }
cargo build --features fixtures

for tape in docs/demo/topology.tape docs/demo/devices.tape; do
  vhs "$tape"
done
# topology.tape only wants its screenshot.
rm -f docs/demo/topology.gif

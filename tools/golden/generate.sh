#!/usr/bin/env bash
# Regenerates tests/golden from the Go implementation: a fixture-backed
# homelab-api network server plus hlctl's network commands. Output is
# normalized to unifictl's intentional differences (no `uri`, no `unifi.` ID
# prefix). Run via `mise run golden`; commit the result.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# Sibling repos live next to the main checkout (this may be a linked worktree).
SIBLINGS="$(dirname "$(dirname "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)")")"
API="${HOMELAB_API_DIR:-$SIBLINGS/homelab-api}"
CLI="${HOMELAB_CLI_DIR:-$SIBLINGS/homelab-cli}"
OUT="$ROOT/tests/golden"
PORT="${GOLDEN_PORT:-18089}"
WORK="$(mktemp -d)"
SERVER_PID=""

cleanup() {
  if [[ -n "$SERVER_PID" ]]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

# 1. Generated Go code (gitignored in both repos). homelab-api's spec
#    submodule may be uninitialized, so reuse hlctl's bundled spec. Bundling
#    needs npx + network (redocly), so only bundle/generate when missing.
SPEC="$CLI/spec/dist/openapi.bundled.yaml"
[[ -f "$SPEC" ]] || make -C "$CLI" bundle
make -C "$API" generate SKIP_BUNDLE=1 SPEC_FILE="$SPEC" >/dev/null
[[ -f "$CLI/internal/api/network/api.gen.go" ]] || make -C "$CLI" generate >/dev/null
make -C "$CLI" build >/dev/null
HLCTL="$CLI/bin/hlctl"

# 2. Fixture server, injected into the homelab-api module via -overlay.
printf '{"Replace":{"%s/cmd/unifictl-golden/main.go":"%s/tools/golden/server/main.go"}}' \
  "$API" "$ROOT" >"$WORK/overlay.json"
(cd "$API" && go build -overlay "$WORK/overlay.json" -o "$WORK/server" ./cmd/unifictl-golden)
FIXTURE_DIR="$ROOT/tests/fixtures" PORT="$PORT" "$WORK/server" >"$WORK/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 100); do
  curl -sf "http://127.0.0.1:$PORT/network/devices" >/dev/null && break
  sleep 0.1
done
curl -sf "http://127.0.0.1:$PORT/network/devices" >/dev/null || { cat "$WORK/server.log"; exit 1; }

# 3. Isolated hlctl environment.
export HOME="$WORK/home" XDG_CONFIG_HOME="$WORK/home/.config"
export HOMELAB_API_URL="http://127.0.0.1:$PORT" HOMELAB_TOKEN=golden
mkdir -p "$XDG_CONFIG_HOME"

hl() { "$HLCTL" network "$@"; }
strip_json() {
  jq -c 'walk(if type == "object" then del(.uri) else . end)
         | walk(if type == "string" and startswith("unifi.") then .[6:] else . end)'
}
strip_text() { sed 's/unifi\.//g'; }

rm -rf "$OUT"
mkdir -p "$OUT"
: >"$OUT/cases.txt"

# case_ NAME HLCTL-ARGS... (IDs in hlctl form, with the unifi. prefix)
case_() {
  local name="$1"
  shift
  local args
  args="$(printf '%s ' "$@" | sed 's/unifi\.//g; s/ $//')"
  printf '%s\t%s\n' "$name" "$args" >>"$OUT/cases.txt"
  if ! hl -o json "$@" >"$WORK/out.json" 2>"$WORK/err.txt"; then
    cp "$WORK/err.txt" "$OUT/$name.err"
    return
  fi
  strip_json <"$WORK/out.json" >"$OUT/$name.json"
  hl "$@" | strip_text >"$OUT/$name.txt"
}

ids() { hl -o json "$@" | jq -r '.items[].id'; }

case_ devices-list devices list
for id in $(ids devices list); do
  case_ "devices-get-${id#unifi.}" devices get "$id"
  case_ "devices-get-${id#unifi.}-all-ports" devices get "$id" --all-ports
done
case_ devices-get-missing devices get unifi.missing

case_ clients-list clients list
case_ clients-list-online clients list --status online
case_ clients-list-offline clients list --status offline
client_ids="$( (ids clients list
  hl -o json topology --include-clients | jq -r '.nodes[] | select(.kind == "client") | .id') | sort -u)"
for id in $client_ids; do
  case_ "clients-get-${id#unifi.}" clients get "$id"
done

case_ topology topology
case_ topology-clients topology --include-clients
case_ topology-wireless topology --include-wireless

case_ ports-list ports list
case_ ports-list-wide ports list --wide
case_ ports-list-all ports list --all-ports
case_ ports-list-all-wide ports list --all-ports --wide
case_ ports-state-down ports list --state down
case_ ports-mode-trunk ports list --mode trunk --all-ports
case_ ports-mode-access ports list --mode access --all-ports
for v in 1 10 20 100; do
  case_ "ports-vlan-$v" ports list --vlan "$v" --all-ports
done
for id in $(hl -o json devices list | jq -r '.items[] | select(.type == "switch" or .type == "gateway") | .id'); do
  case_ "ports-device-${id#unifi.}" ports list --device "$id" --all-ports
done

for kind in vlans ssids wans; do
  case_ "$kind-list" "$kind" list
  for id in $(ids "$kind" list); do
    case_ "$kind-get-${id#unifi.}" "$kind" get "$id"
  done
  case_ "$kind-get-missing" "$kind" get unifi.missing
done

echo "wrote $(wc -l <"$OUT/cases.txt") cases to $OUT"

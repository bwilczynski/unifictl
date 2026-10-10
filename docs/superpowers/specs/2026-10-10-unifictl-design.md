# unifictl — standalone UniFi CLI (design)

Date: 2026-10-10
Status: draft, awaiting review

## Goal

A standalone Rust CLI, `unifictl`, that talks directly to a UniFi controller and
provides the same network functionality as `hlctl network …` — without running
`homelab-api`. Behaviour is ported from three sources and must be **functionally
equivalent** to them:

- [bwilczynski/homelab-api](https://github.com/bwilczynski/homelab-api):
  `internal/network/*_service.go` + `internal/adapters/unifi.go` — data fetching
  and mapping logic
- [bwilczynski/homelab-cli](https://github.com/bwilczynski/homelab-cli) (`hlctl`):
  `internal/cli/network/` + `internal/output/` — flags, client-side filtering,
  table rendering
- [bwilczynski/homelab-api-spec](https://github.com/bwilczynski/homelab-api-spec):
  `openapi/components/schemas/network/` — the output model

Paths below such as `homelab-api/internal/network/…` are relative to the root of
the named repository.

Audience: the author's homelab first, but usable by any UniFi owner — so it has its
own config location and no homelab-specific naming.

### Functional equivalence (not byte parity)

Equivalent means: same fields (names, types, nullability, presence), same values,
same filters, same row order, same table columns/headers/cells/sections.
**Not** required: JSON key order, number formatting (`8` vs `8.0`), compact vs pretty
JSON, exact table padding.

### Intentional differences from hlctl / homelab-api

1. **No controller prefix in IDs.** One config = one controller. `unifi.switch-core`
   becomes `switch-core`; client IDs become `{kebab(name)}-{mac[0:2]}`. Applies to
   every ID field and nested ref (`uplink`, `connectedTo`, `device`,
   `broadcastingAps`, `nativeVlan`, `taggedVlans`, `wans`, topology node/edge
   endpoints) and to inputs (`get <id>`, `ports list --device`). A prefixed ID is
   simply not found — no compatibility shim.
2. **No `uri` field.** Dropped from all schemas carrying it: `NetworkDevice`,
   `NetworkDeviceRef`, `NetworkClient`, `NetworkClientRef`, `Vlan`, `NetworkVlanRef`,
   `Ssid`, `Wan`, `WanRef`.
3. **Unreachable controller is an error.** homelab-api's health monitor silently
   skips unreachable backends (empty lists); unifictl fails with exit code 1.
4. **No `network` command level, no `--watch`.**
5. **Errors are plain stderr messages**, not RFC 9457 problem documents. Not-found
   messages keep the API wording (`Network device not found: <id>`, `Network client
   not found: <id>`, `VLAN not found: <id>`, `SSID not found: <id>`,
   `WAN not found: <id>`).

### Non-goals

Multiple controllers or sites per invocation, write operations, `config`/`login`
commands, watch mode, shell completion generation (clap can add it later), release
packaging.

## Project

Repository: `unifictl`, its own git repository, independent of the homelab repos
above. Binary: `unifictl`.

Stack: Rust 1.99.0 via mise (2024 edition), `clap` (derive), blocking `reqwest` with rustls
and cookie store, `serde`/`serde_json`, `toml`, `tabwriter`, `thiserror` (library
errors) + `anyhow` (main). Exact versions are pinned during planning, with APIs
checked via context7 (docs.rs for `tabwriter`, which context7 does not index).

### Toolchain (mise)

No global Rust install. The toolchain is pinned per project in `mise.toml`, following
hlctl's convention; `mise install` sets everything up, and mise's activation puts
`cargo`/`rustc` on PATH only inside the project.

```toml
# Toolchain for local development. Rust crates are pinned in Cargo.toml/Cargo.lock.
[tools]
rust = { version = "1.99.0", profile = "minimal", components = ["clippy", "rustfmt"] }
go = "1.26.2"   # golden harness only: builds homelab-api + hlctl (both go 1.26.2)

[tasks]
build  = "cargo build --release"
test   = "cargo test --features fixtures"
lint   = "cargo clippy --all-targets --features fixtures -- -D warnings && cargo fmt --check"
golden = "tools/golden/generate.sh"
```

`mise.lock` is committed, as in hlctl. Day-to-day: `mise run test`, `mise run lint`.

### Module layout

```
unifictl/
  Cargo.toml
  mise.toml          pinned rust + go toolchains, dev tasks
  src/
    main.rs            parse args → load config → build client → dispatch → render → exit code
    cli.rs             clap derive command tree, global -o / --config
    config.rs          TOML file + env overlay → validated Config
    error.rs           Error enum (Config, NotFound, Unifi, …)
    unifi/
      mod.rs           trait UnifiApi
      client.rs        HTTP implementation (API key or session)
      types.rs         raw controller structs, ported 1:1 from adapters/unifi.go
    model/             output types = spec network schemas minus `uri`
    service/           devices, clients, topology, vlans, ssids, wans, ports (+ shared helpers)
    render/
      tab.rs           tabwriter wrapper (minwidth 0, padding 2, ' ', left-aligned)
      format.rs        uptime / bytes / bytes-per-sec / link-speed / time helpers
      <resource>.rs    one renderer per hlctl template
  tests/
    fixtures/          sanitized UniFi JSON copied from homelab-api/internal/network/testdata
    golden/            expected outputs + cases.txt manifest (generated)
    golden.rs          equivalence test runner
  tools/golden/
    generate.sh        regenerates tests/golden from homelab-api + hlctl
    server/main.go     fixture-backed server, compiled into homelab-api via go -overlay
```

Dependency direction: `cli/main → service → unifi::UnifiApi (trait)`;
`service → model`; `render → model`. Services never see HTTP or credentials.

## Configuration

Path: `--config PATH` > `UNIFICTL_CONFIG` > `$XDG_CONFIG_HOME/unifictl/config.toml` >
`~/.config/unifictl/config.toml`.

```toml
host = "192.168.1.1"          # required; host[:port]; always https
api_key = "…"                 # UniFi OS mode (preferred)
# username = "admin"          # legacy controller mode, used only when api_key is absent
# password = "…"
# site = "default"
# insecure_tls = false        # skip TLS verification (self-signed controller certs)
# client_history_days = 30    # offline-client window; <= 0 → 30
```

- Env overrides file values: `UNIFI_HOST`, `UNIFI_API_KEY`, `UNIFI_USERNAME`,
  `UNIFI_PASSWORD`, `UNIFI_SITE`, `UNIFI_INSECURE_TLS`, `UNIFI_CLIENT_HISTORY_DAYS`.
- A missing file is fine when env supplies everything.
- Validation (same rules as homelab-api): `host` required; either `api_key` or both
  `username` and `password`. Messages name the key and its env var, e.g.
  `config: host is required (set host in <path> or UNIFI_HOST)`.
- If the file is group/other-readable, warn on stderr and continue.

## UniFi client

Ported from `adapters/unifi.go`.

- Blocking `reqwest`, rustls, 30 s timeout; `insecure_tls` →
  `danger_accept_invalid_certs(true)`.
- **API-key mode:** header `X-API-KEY`, path prefix `/proxy/network`. HTTP 401 →
  `unifi request: invalid API key (401)`.
- **Session mode:** cookie store; `POST https://{host}/api/login` with
  `{"username","password"}` on first request; no path prefix. On 401: re-login once and
  retry; second 401 → `unifi request: unauthorized after re-auth`. Session lives only
  for the process.
- Non-2xx → `unifi request: status <code>`; decode failures →
  `unifi parse response: <err>`.
- Endpoints (`{p}` = prefix, `{site}` from config):
  - `{p}/api/s/{site}/stat/device` — `{meta,data}` envelope
  - `{p}/api/s/{site}/stat/sta` — envelope
  - `{p}/api/s/{site}/rest/wlanconf` — envelope
  - `{p}/api/s/{site}/rest/networkconf` — envelope
  - `{p}/v2/api/site/{site}/clients/active?includeTrafficUsage=false&includeUnifiDevices=false` — bare array
  - `{p}/v2/api/site/{site}/clients/history?onlyNonBlocked=true&withinHours={days*24}` — bare array
- Not ported: `Ping`, `GetHealth` (server health-monitor only).

`UnifiApi` trait exposes exactly these six calls plus `all_clients(days)`
(active + history). Production uses `client.rs`; tests use a fixture-backed fake.

## Output model

One Rust type per spec schema in `schemas/network/`, minus `uri`. Idiomatic serde:

- `#[serde(rename_all = "camelCase")]`; field names match the spec.
- Presence rules copied from the generated Go struct tags (what actually produced
  the API output): `omitempty` pointer → `Option<T>` +
  `skip_serializing_if = "Option::is_none"`; required-nullable → `Option<T>`
  serialized as `null`.
- Discriminated unions (`NetworkDeviceDetail`, `NetworkClientDetail`,
  `TopologyNode`, `TopologyEdge`, `NetworkConnectionRef`) → `#[serde(tag = "…")]`
  enums using the spec's discriminator property and mapping values.
- Enums (`NetworkDeviceType`, `NetworkDeviceStatus`, `NetworkPortState`,
  `DevicePortVlanMode`, `DevicePortPoeMode`, `NetworkLinkSpeed`, `WifiBand`,
  `WifiSecurityProtocol`, `DhcpMode`, `WanStatus`, …) with the spec's string values.

`-o json` prints the model with `serde_json::to_writer_pretty` plus a trailing
newline. List responses keep their wrapper objects (e.g. `{"items": [...]}`) exactly
as the spec defines them.

## Services

Each `*_service.go` is ported function-for-function into `service/<resource>.rs`,
including helpers (`toKebab`, `clientName`, `clientSuffix`, `macHexPrefix`,
`normalizeMac`, VLAN id extraction for int/`""`/null, DHCP mode, DNS collection,
WAN iface resolution, gateway LAN ports, port VLAN config, LAG/SFP, offline-client
handling via v2 APIs, SSID broadcasting-AP collection and sorting). Composite-ID
parsing (`parseID`) is removed; lookups match the unprefixed ID directly.
`client_history_days` from config replaces the per-controller history map.

## Commands

```
unifictl [-o table|json] [--config PATH] <command>
  devices list
  devices get <id> [--all-ports]
  clients list [--status online|offline]
  clients get <id>
  topology [--include-clients] [--include-wireless]
  ports list [--device ID] [--mode trunk|access] [--state up|down|disabled | --all-ports] [--vlan N] [--wide]
  vlans list | vlans get <id>
  ssids list | ssids get <id>
  wans  list | wans  get <id>
  version
```

Enum flags use clap `ValueEnum`; `--all-ports` conflicts with `--state`.
`version` prints the crate version.

Client-side behaviour carried over from hlctl:

- `ports list`: default filter `state=up` unless `--state` or `--all-ports` is given
  (applies to JSON too — it is a query filter in hlctl). `--vlan 0` means no filter.
- `devices get --all-ports`: table only. Without it, switch/gateway port tables show
  only `up` ports; JSON always contains all ports.
- `topology`: `--include-wireless` implies `--include-clients`. Wireless edges are
  dropped from the table tree only (not from JSON) unless `--include-wireless`. Tree
  built from nodes/edges rooted at the gateway; no gateway → error
  `no gateway node found in topology`. Node/edge display strings as in
  `buildTopologyTree` (`Name (type) [N clients]`,
  `Name (client, <conn>, <status>)`, `[port N, <speed>]`, wireless `[ssid]` or `[ssid, -NN dBm]` when signal is present).
- Port decoration (`decoratePort`) shared by `ports list` and `devices get`:
  connected-to name, `-` placeholders, `%.1f W` PoE, LAG/SFP strings.

## Rendering

- One renderer per hlctl template (`devices_list`, `devices_get_{switch,gateway,
  accesspoint,unknown}` + `device_base`, `clients_list`,
  `clients_get_{wired,wireless}`, `topology`, `ports_list` (normal + `--wide`),
  `vlans_{list,get}`, `ssids_{list,get}`, `wans_{list,get}`).
- Writes tab-separated lines through `tabwriter::TabWriter` configured like Go's
  `tabwriter.NewWriter(w, 0, 0, 2, ' ', 0)`; calls `flush()` wherever the template
  calls `{{ flush }}` so sections get independent column widths.
- `format.rs` ports `FormatUptime`, `FormatBytes`, `FormatBytesPerSec` (binary
  units), `FormatLinkSpeed`, `FormatTime` (local TZ, RFC 3339).

## Errors and exit codes

`main` prints `Error: <message>` to stderr. Exit 0 success; 1 runtime error
(config, UniFi transport/auth, not found, invalid topology); 2 usage error (clap).

## Testing

1. **Service unit tests** — ported from `homelab-api/internal/network/service_test.go`
   and `ports_service_test.go`, using a `FakeUnifi` over `tests/fixtures/`. Expected
   IDs drop `unifi.`; `uri` assertions are removed. Services are implemented
   test-first against them.
2. **Golden equivalence tests** — `tools/golden/generate.sh` (run manually, output
   committed). It needs local clones of bwilczynski/homelab-api and
   bwilczynski/homelab-cli, found next to the unifictl checkout by default or via
   `HOMELAB_API_DIR` / `HOMELAB_CLI_DIR`. The script:
   - builds a fixture-backed homelab-api server from `tools/golden/server/main.go`,
     injected into the homelab-api module with `go build -overlay` (so it can use
     `internal/` packages) — **no changes to the homelab-api repo**. Unlike the
     existing `cmd/testserver`, it also serves the wlanconf/networkconf fixtures.
   - runs hlctl against it (auth disabled, `TZ=UTC`) for every list command and flag
     combination, and `get` for every ID returned by the lists (switch/gateway also
     with `--all-ports`), in both `-o json` and table mode;
   - normalizes (removes `uri`, strips the `unifi.` prefix from IDs and CLI args)
     and writes `tests/golden/<case>.{json,txt}` and `tests/golden/cases.txt`.
   - `tests/golden.rs` runs unifictl (with `TZ=UTC`) for each case against the same
     fixtures via a hidden `--fixtures <dir>` flag, compiled only with the
     `fixtures` cargo feature, and compares: JSON structurally (numbers normalized
     to f64); tables row-by-row with cells split on runs of ≥2 spaces.
3. **HTTP client tests** — `unifi/client.rs` against a local mock HTTP server:
   API-key header and prefix, session login + single retry on 401, site substitution,
   envelope vs bare-array decoding, error messages.
4. **Manual smoke test** — against the real controller, side-by-side with
   `hlctl network …`.

## Risks

- **`tabwriter` crate edge cases** (trailing cells, empty cells) may pad differently
  from Go. Acceptable under functional equivalence; golden table comparison ignores
  padding.
- **Struct-tag presence rules**: the YAML `required` lists and the Go structs could
  disagree; the Go structs win, and golden JSON tests catch mismatches.
- **Fixture coverage**: golden tests only cover shapes present in the fixtures; the
  ported unit tests cover the remaining edge cases.

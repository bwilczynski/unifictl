# `unifictl status`: one-screen site health (design)

Date: 2026-10-14
Status: draft

## Goal

"Is anything wrong with the network, and what?" takes five commands today
(`wans list`, `devices list`, `clients list`, `logs -p high`, and reading
firmware versions by eye). `unifictl status` answers it on one screen, for a
person glancing at it; the exit code is a bonus, not the main use.

Success looks like:

- One line per area (WAN, devices, Wi-Fi, wired, alarms, updates), always all
  of them, each with a coloured verdict and a short summary.
- When an area is not fine, the line below names the culprits: the offline
  device, the poorly connected clients, the latest high-severity event.
- One slow piece of data (system log, WAN history, health) never takes the
  whole screen down.

## Output

```
$ unifictl status
WAN      ok     WAN1  up 3d 4h  99.8% 24h  12 ms
WAN      error  WAN2  down 12m
                1 outage in 24h (4m 10s at 03:12)
DEVICES  error  8 of 9 online
                offline: ap-garage
WI-FI    warn   4 APs, 38 clients
                poor experience: roborock-0c04 (41), printer-0b02 (38)
WIRED    ok     3 switches, 41 clients
ALARMS   warn   2 high-severity events in 24h
                latest: security: intrusion attempt from 192.168.10.67 to 198.51.100.7 (2h ago)
UPDATES  info   ap-office 7.1.66 → 7.1.68
                us-8-60w 7.2.120 → 7.2.123
```

A healthy network:

```
$ unifictl status
WAN      ok     WAN1  up 3d 4h  99.98% 24h  11 ms
DEVICES  ok     9 online
WI-FI    ok     4 APs, 38 clients
WIRED    ok     3 switches, 41 clients
ALARMS   ok     no high-severity events in 24h
UPDATES  ok     all up to date
```

Verdict words and colours: `ok` green, `warn` yellow, `error` red, `info`
blue, `unknown` dim. Only the verdict word is coloured; names in detail lines
are blue like everywhere else. `--color` and `NO_COLOR` behave as for other
commands.

## Decisions

| Question | Decision |
|---|---|
| Command | `unifictl status`, no flags of its own; takes the global `-o json`, `--refresh`, `--color`, `--config` |
| Layout | Fixed rows in the order above; every row always shown |
| WAN rows | One per network with `purpose == "wan"`, built by the `wans list` builder |
| Verdict | The worse of the row's own checks and its `stat/health` subsystem status |
| Thresholds | Reused from `render/health.rs` (experience, availability, latency) |
| Alarms | High-severity system-log entries from the last 24 hours (the controller serves no alarm list; see below) |
| Updates | `upgradable` / `upgrade_to_firmware` from `stat/device`; informational, never a problem |
| Exit code | 1 if any row is `error`, otherwise 0; same for `-o json` |
| `-o ids` | Rejected like on other commands without IDs |
| Detail lists | At most 5 names, then `+N more` |
| `vpn` subsystem | Ignored: it reports `unknown` when no VPN is configured |

### Verdict rules

`stat/health` statuses map as `ok` → ok, `warning` → warn, `error` → error;
anything else (`unknown`, missing subsystem) adds nothing.

| Row | Data | error | warn | otherwise |
|---|---|---|---|---|
| WAN (per WAN) | `build_wan`, 24h `wan_history` outages; health `wan` and `www` | WAN disconnected | availability or latency graded Fair/Poor, or ≥1 outage in 24h | health status, else ok |
| DEVICES | `stat/device` `state` | any device in state 0 (offline), 6 (heartbeat missed), 9, 10 (adoption failed) or 11 (isolated) | any in state 2 (pending), 4 (upgrading), 5 (provisioning) or 7 (adopting) | ok |
| WI-FI | APs from `stat/device`; online wireless clients from v2 `clients/active`; health `wlan` | — | any client with experience < 50, or health `warning` | health status, else ok |
| WIRED | switches from `stat/device`; online wired clients; health `lan` | — | — | health status, else ok |
| ALARMS | system log, last 24h, severity high and above | any very-high entry | any high entry | ok |
| UPDATES | `stat/device` | — | — | `info` when any device is upgradable, else ok |

Health `error` on `wlan` or `lan` makes that row `error` even though our own
checks never do.

Summaries and details:

- WAN: `<name>  up <uptime>  <availability>% 24h  <latency> ms`, or `<name>
  down <downtime>`; a detail line when there were outages in the 24h window:
  `N outage(s) in 24h (<duration> at <HH:MM>)` for the most recent one.
- DEVICES: `N online`, or `N of M online`; details `offline: …` and
  `pending: …` / `upgrading: …` (device IDs).
- WI-FI: `N APs, M clients`; detail `poor experience: id (score), …`, worst
  first.
- WIRED: `N switches, M clients`.
- ALARMS: `N high-severity events in 24h`; detail `latest: <category>:
  <message> (<age> ago)`, rendered like a `logs` line.
- UPDATES: one detail-style line per upgradable device: `id from → to`, with
  the first on the verdict line.

## Partial failure

`stat/device` is required: without it there is nothing to say, and the
command fails like every other command (`Error: …`, exit 1).

Everything else is optional and fetched with `.ok()`, the way `wans get`
treats `wan_history`:

| Missing | Effect |
|---|---|
| `stat/health` | Verdicts come from our own checks only |
| v2 `clients/active` | WI-FI and WIRED rows `unknown`, summary `couldn't read clients` |
| `wan_history` | WAN rows without the outage check or detail line |
| system log | ALARMS row `unknown`, `couldn't read the system log` |

`unknown` never sets exit code 1.

## Alarms: endpoint findings

Observed on a UniFi OS gateway with UniFi Network 9.x, API key auth:

| Request | Result |
|---|---|
| `GET api/s/{site}/list/alarm` (with or without `?archived=false`) | 400 `api.err.InvalidObject` |
| `GET api/s/{site}/cnt/alarm` | 400 `api.err.InvalidObject` |
| `GET api/s/{site}/stat/alarm` | 404 |
| `GET v2/api/site/{site}/alarms` | 404 |

The system log (already used by `unifictl logs`) carries the same events,
with a severity, so ALARMS is built from it instead.

## `stat/health`

`GET {prefix}/api/s/{site}/stat/health`, the usual `{"meta", "data"}`
envelope, one entry per subsystem:

```json
{"meta": {"rc": "ok"}, "data": [
  {"subsystem": "wlan", "status": "ok", "num_ap": 4, "num_user": 38, "num_disconnected": 0},
  {"subsystem": "wan",  "status": "ok", "isp_name": "Example ISP"},
  {"subsystem": "www",  "status": "ok", "latency": 12, "drops": 0},
  {"subsystem": "lan",  "status": "ok", "num_sw": 3, "num_user": 41},
  {"subsystem": "vpn",  "status": "unknown"}
]}
```

Only `subsystem` and `status` are deserialised. Counts come from our own data
so they agree with `devices list` and `clients list`.

## Code

Following the existing layers:

- `unifi/types.rs`: `HealthSubsystem { subsystem, status }`; `Device` gains
  `upgradable: bool` and `upgrade_to_firmware: String`.
- `unifi/mod.rs`, `unifi/client.rs`: `UnifiApi::health()`, cached like the
  other reads.
- `unifi/fixtures.rs`: `FixtureApi.health: Option<Vec<HealthSubsystem>>` from
  `unifi-health.json`; `None` when the file is absent, like `wan_history`.
- `service/status.rs`: `Service::status(now_ms, tz) -> Result<SiteStatus>`.
  One `fetch!` for devices, active clients, network conf, health,
  `wan_history(86400)` and the system log for the last 24h at high severity
  and above. Reuses `build_wan`, `outages`, `network_client` and the log
  severity mapping rather than re-deriving them.
- `model.rs`: `SiteStatus { rows: Vec<StatusRow> }`; `StatusRow { area,
  verdict, summary, details }` plus typed per-area data for JSON (below);
  `Verdict { Ok, Warn, Error, Info, Unknown }`.
- `render/status.rs`: the table; verdict colour through `style`.
- `cli.rs`, `app.rs`: `Command::Status`; `app.rs` sets `Outcome.code` to 1
  when any row is `Error`.

### JSON

`-o json` prints `SiteStatus` (camelCase, like other models). Each row
carries its rendered `summary` and `details` and the data behind them, so a
script does not have to parse text:

```json
{
  "rows": [
    {"area": "wan", "verdict": "error", "summary": "WAN2  down 12m", "details": [],
     "wan": {"id": "wan2", "status": "disconnected", "downtime": 720, "…": "…"}, "outages24h": 0},
    {"area": "devices", "verdict": "error", "summary": "8 of 9 online",
     "details": ["offline: ap-garage"], "online": 8, "total": 9,
     "offline": ["ap-garage"], "transitioning": []},
    {"area": "wifi", "verdict": "warn", "summary": "4 APs, 38 clients",
     "details": ["poor experience: roborock-0c04 (41), printer-0b02 (38)"],
     "accessPoints": 4, "clients": 38,
     "poorClients": [{"id": "roborock-0c04", "experience": 41}, {"id": "printer-0b02", "experience": 38}]},
    {"area": "alarms", "verdict": "warn", "summary": "2 high-severity events in 24h",
     "details": ["…"], "count": 2, "latest": {"…": "a logs -o json entry"}},
    {"area": "updates", "verdict": "info", "summary": "ap-office 7.1.66 → 7.1.68",
     "details": ["us-8-60w 7.2.120 → 7.2.123"],
     "updates": [{"id": "ap-office", "from": "7.1.66", "to": "7.1.68"}, {"id": "us-8-60w", "from": "7.2.120", "to": "7.2.123"}]}
  ]
}
```

Times inside follow the existing models (seconds for durations, RFC 3339 in
the local offset for timestamps).

## Testing

- `service/status.rs` unit tests against `tests/fixtures`, with a new
  fictional `unifi-health.json` and one fixture device marked upgradable:
  each verdict rule, health worsening a row, health absent, clients absent,
  system log absent, `fail` (required data missing → error), the 5-name cap.
- `render/status.rs`: table text without colour, verdict colouring with.
- `tests/cli.rs`: `status` output and exit code 1 for a fixture with an
  offline device; `-o json` shape; `-o ids` rejected.
- README: `status` in the `## Use` block and a short section with the healthy
  example.

## Out of scope

- Flags (`--quiet`, per-area selection, `--since`). The window is 24h.
- VPN, port anomalies, Wi-Fi radio metrics (#37).
- `--watch`.

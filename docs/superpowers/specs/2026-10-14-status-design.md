# `unifictl status`: one-screen site health (design)

Date: 2026-10-14
Status: approved

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
WAN      ok     Internet 1  up 3d 4h 0m 0s  99.80% over 24h  12 ms
                1 outage in 24h (latest 4m 10s at 03:12)
WAN      error  Internet 2  down 12m 0s
DEVICES  error  8 of 9 online
                offline: ap-garage
WI-FI    warn   4 APs, 38 clients
                poor experience: roborock-0c04 (41), printer-0b02 (38)
WIRED    ok     3 switches, 41 clients
ALARMS   warn   2 high-severity events in 24h
                latest: Oct 14 09:12:40 security: A network intrusion attempt from 192.168.10.67 to 198.51.100.7 has been detected.
UPDATES  info   ap-office 7.1.66 → 7.1.68
                us-8-60w 7.2.120 → 7.2.123
```

A healthy network:

```
$ unifictl status
WAN      ok     Internet 1  up 3d 4h 0m 0s  99.98% over 24h  11 ms
WAN      idle   Internet 2  not connected since boot
DEVICES  ok     9 online
WI-FI    ok     4 APs, 38 clients
WIRED    ok     3 switches, 41 clients
ALARMS   ok     no high-severity events in 24h
UPDATES  ok     all up to date
```

Verdict words and colours: `ok` green, `warn` yellow, `error` red, `info`
blue, `idle` and `unknown` dim. Only the verdict word is coloured; names in detail lines
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
| Idle WAN | A disconnected WAN that has been down since the gateway booted (downtime within 5 minutes of the gateway's uptime, or no downtime reported) is `idle`, not `error`: a configured but unplugged backup WAN must not keep `status` red |
| WAN availability | The 24h `wan_history` figure, shown as `99.80% over 24h`, as in `wans list`. Falls back to the gateway's live figure over its own period when the history can't be read. (Revised 2026-10-20: the live period restarts when the WAN reconnects, so right after an outage it read `100.00% over 58s`.) |
| `-o ids` | Rejected like on other commands without IDs |
| Detail lists | At most 5 names, then `+N more` |
| `vpn` subsystem | Ignored: it reports `unknown` when no VPN is configured |

### Verdict rules

`stat/health` statuses map as `ok` → ok, `warning` → warn, `error` → error;
anything else (`unknown`, missing subsystem) adds nothing.

| Row | Data | error | warn | otherwise |
|---|---|---|---|---|
| WAN (per WAN) | `build_wan`, 24h `wan_history` outages; health `wan` and `www` | WAN disconnected after boot (down since boot is `idle`, and health is not consulted) | availability or latency graded Fair/Poor, or ≥1 outage in 24h | health status, else ok |
| DEVICES | `stat/device` `state` | any device in state 0 (offline), 6 (heartbeat missed), 9, 10 (adoption failed) or 11 (isolated) | any in state 2 (pending), 4 (upgrading), 5 (provisioning) or 7 (adopting) | ok |
| WI-FI | APs from `stat/device`; online wireless clients from v2 `clients/active`; health `wlan` | — | any client with experience < 50, or health `warning` | health status, else ok |
| WIRED | switches from `stat/device`; online wired clients; health `lan` | — | — | health status, else ok |
| ALARMS | system log, last 24h, severity high and above | — (a log entry is history; `error` is kept for what is wrong now) | any high or very-high entry | ok |
| UPDATES | `stat/device` | — | — | `info` when any device is upgradable, else ok |

Health `error` on `wlan` or `lan` makes that row `error` even though our own
checks never do.

Summaries and details:

- WAN: `<name>  up <uptime>  <availability>% over <period>  <latency> ms`,
  `<name>  down <downtime>`, or `<name>  not connected since boot`; a detail
  line when an outage overlaps the 24h window: `N outage(s) in 24h (latest
  <duration> at <HH:MM>)`.
- DEVICES: `N online`, or `N of M online`; details `offline: …` and
  `pending: …` / `upgrading: …` (device IDs).
- WI-FI: `N APs, M clients`; detail `poor experience: id (score), …`, worst
  first.
- WIRED: `N switches, M clients`.
- ALARMS: `N high-severity events in 24h`, or `no high-severity events in
  24h`; detail `latest: ` followed by the newest entry exactly as `logs`
  prints it.
- UPDATES: one detail-style line per upgradable device: `id from → to`, with
  the first on the verdict line.

## Partial failure

`stat/device` and `rest/networkconf` (which WANs exist) are required:
without them there is nothing to say, and the command fails like every other
command (`Error: …`, exit 1). Both are the same cached reads `devices list`
and `wans list` make.

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
- `model.rs`: `SiteStatus { rows: Vec<StatusRow> }`; `StatusRow { verdict,
  unavailable, area: StatusArea }` where `StatusArea` is an enum with the
  typed data of each area (JSON below); `Verdict { Ok, Warn, Error, Info,
  Idle, Unknown }`.
- `render/status.rs`: the table, including all summary and detail text;
  verdict colour through `style`.
- `cli.rs`, `app.rs`: `Command::Status`; `app.rs` sets `Outcome.code` to 1
  when any row is `Error`.

### JSON

`-o json` prints `SiteStatus` (camelCase, like other models). Each row
carries its `verdict`, its `area` and the data behind it; the summary and
detail text are built by the table renderer only, so a script reads data
rather than parsing text. A row that is `unknown` adds `unavailable` with the
reason.

```json
{
  "rows": [
    {"verdict": "error", "area": "wan",
     "wan": {"id": "internet-2", "name": "Internet 2", "status": "disconnected", "downtime": 720, "…": "the wans list item"},
     "outages24h": []},
    {"verdict": "error", "area": "devices", "online": 8, "total": 9,
     "offline": [{"kind": "device", "id": "ap-garage", "name": "AP Garage"}], "transitioning": []},
    {"verdict": "warn", "area": "wifi", "accessPoints": 4, "clients": 38,
     "poorClients": [{"id": "printer-0b02", "name": "Printer", "experience": 38}, {"id": "roborock-0c04", "name": "Roborock", "experience": 41}]},
    {"verdict": "unknown", "unavailable": "couldn't read clients", "area": "wired", "switches": 3, "clients": null},
    {"verdict": "warn", "area": "alarms", "count": 2, "latest": {"…": "a logs -o json entry"}},
    {"verdict": "info", "area": "updates",
     "updates": [{"kind": "device", "id": "ap-office", "name": "AP Office", "from": "7.1.66", "to": "7.1.68"}]}
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

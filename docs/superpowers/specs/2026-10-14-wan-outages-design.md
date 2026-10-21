# WAN outage history in `wans get` (design)

Date: 2026-10-14
Status: approved
Issue: #33

## Goal

When the internet drops, the only trace in unifictl is the system log, where a
"down" and a "restored" entry have to be paired by hand. `wans get` should
answer "how many outages did this WAN have this week, and how long were they?"

Success looks like:

- `unifictl wans get internet-1` lists the last 7 days' outages with start,
  end and duration, plus the controller's availability figure for that window.
- `--since -30d` widens the window.
- A controller without this data still prints `wans get` as today.

## Decisions

| Question | Decision |
|---|---|
| Source | `GET {prefix}/v2/api/site/{site}/aggregated-dashboard?historySeconds=N&widgets=wan_history` |
| Where | `wans get` only. `wans list` is unchanged; it already shows availability |
| Window | Last 7 days by default; `-S/--since WHEN` changes it, same forms as `logs --since` |
| `--until` | None: the endpoint only counts back from now |
| Window shown | The controller's own window from `dashboard_meta.start_timestamp`, never the requested one |
| Availability | `wan_history_details[].uptime` (percent over the window), graded like AVAILABILITY |
| Outage end | `timestamp + downtime_in_seconds` |
| Existing AVAILABILITY row | Unchanged: the gateway's live `uptime_stats` figure and its period. (Revised 2026-10-20: now the last 24h from a second, 24h history request; see the wan-uptime spec.) |
| Endpoint fails or no `wan_history` | Outage section left out; the rest of `wans get` prints as today |
| WAN without a history entry | Same as above (a WAN never up, e.g. unplugged WAN2, has none) |
| JSON timestamps | RFC 3339 in the local offset, like `logs` |

## Endpoint findings

Observed on a UCG-Fiber with UniFi Network 9.x, API key auth.

`aggregated-dashboard` is the web dashboard's endpoint. Unfiltered it returns
~20 widgets, 300–500 KB. `widgets=wan_history` (comma-separated for several)
cuts it to `dashboard_meta` and `wan_history`:

| historySeconds | Size | Time | Window returned | Buckets |
|---|---|---|---|---|
| 86400 (24h) | 42 KB | 0.07 s | from the next full hour | 5 min |
| 604800 (7d) | 24 KB | 0.07 s | from local midnight 6 days back | 1 h |
| 2592000 (30d) | 6 KB | 0.07 s | from local midnight 29 days back | 1 day |
| 7776000 (90d) | 13 KB | 0.06 s | only as far back as history goes (~75 days) | 1 day |

The returned window always starts at or after the requested one, which is why
the output names the controller's window start. Unknown query parameters are
ignored silently (`widget=`, `include=`, `fields=` all returned the full
payload), so a controller that does not know `widgets` returns the full
dashboard: slower, still correct.

```json
{
  "dashboard_meta": {"start_timestamp": 1791158400000, "end_timestamp": 1791720000000, "widgets": ["wan_history"]},
  "wan_history": {
    "wan_history_details": [
      {
        "network_group": "WAN",
        "isp_name": "Example ISP",
        "isp_asn": 64500,
        "uptime": 99.95,
        "downtime_history": [
          {"id": "…", "timestamp": 1791256365250, "downtime_in_seconds": 95, "wan_network_group": "WAN"}
        ],
        "health_history": [ … ]
      }
    ]
  }
}
```

`network_group` matches the network's `wan_networkgroup` (`WAN`, `WAN2`).
`health_history` (bucketed flags) is not deserialised.

Alternatives considered and rejected:

- `system-log/all`, pairing `NETWORK_WAN_FAILED` with `NETWORK_WAN_RESTORED`:
  no availability figure, fragile pairing at window edges and across reboots,
  `WAN1` vs `WAN` naming, depends on log retention.
- `stat/health` (WAN subsystem), `list/alarm`: current state only / no WAN
  outages.
- Cloud Site Manager `/ea/isp-metrics`: a different API and key, early access,
  bucketed metrics with no outage events.
- `wan/{group}/isp-status`: only the last ~24h, ignores time parameters.

## Output

### `wans get internet-1`

```
FIELD         VALUE
ID            internet-1
NAME          Internet 1
IP            203.0.113.1
UPTIME        1d 4h 12m 33s
STATUS        connected
AVAILABILITY  99.93% (24h)
LATENCY       11 ms
DNS           8.8.8.8, 8.8.4.4
OUTAGES       3 since Oct 05 00:00 (99.95% available)

START            END              DURATION
Oct 06 03:12:45  Oct 06 03:14:20  1m 35s
Oct 09 21:40:08  Oct 09 21:40:49  41s
Oct 10 14:05:30  Oct 10 14:05:39  9s
```

- Outages oldest first, local time, `Mon DD HH:MM:SS` like `logs`.
- Duration via `format_uptime` without leading zero parts (`1m 35s`, `9s`).
- The percentage is coloured with the AVAILABILITY grader.
- No outages: `OUTAGES  0 since Oct 05 00:00 (100.00% available)`, no table.
- No history: no OUTAGES row, no table.

### `wans get internet-1 --since -30d`

Same layout, the window and rows from a 30-day request.

### JSON

`wans get -o json` adds `outages` to the existing object (`null` when there is
no history):

```json
"outages": {
  "since": "2026-10-05T02:00:00+02:00",
  "availability": 99.95,
  "items": [
    {"start": "2026-10-06T05:12:45+02:00", "end": "2026-10-06T05:14:20+02:00", "durationSeconds": 95}
  ]
}
```

`wans list -o json` is unchanged; its items do not carry `outages`.

## `--since`

- Parsed by `timespec::parse` against now; `historySeconds = now − since`,
  rounded up to whole seconds.
- Default 7 days (604800).
- Spans are written as in `logs`: `-30d` or `"30d ago"`; a bare `30d` is
  rejected by `timespec` as there.
- A time at or after now is an error: `--since must be in the past`.
- Help: `Outages at or after WHEN [default: 7 days ago]: today, yesterday, -2h, "2h ago", YYYY-MM-DD [HH:MM[:SS]]`.

## Implementation

- `cli.rs`: `wans` gets its own subcommand enum (`List { wide }`,
  `Get { id, since }`) in place of the shared `LookupCommand`; completion and
  `-o ids` behave as before.
- `unifi/types.rs`: `WanHistory { dashboard_meta: { start_timestamp,
  end_timestamp }, wan_history: { wan_history_details: [{ network_group,
  uptime, downtime_history: [{ timestamp, downtime_in_seconds }] }] } }`, all
  tolerant of nulls and missing keys.
- `unifi/mod.rs` + `client.rs` + `fixtures.rs`: `fn wan_history(&self,
  history_seconds: i64) -> Result<WanHistory>`.
- `model.rs`: `WanDetail { #[serde(flatten)] wan: Wan, outages:
  Option<WanOutages> }`, `WanOutages { since, availability, items }`,
  `WanOutage { start, end, duration_seconds }`. `Wan` itself is unchanged, so
  list items stay as they are.
- `service/wans.rs`: `get_wan(id, history_seconds) -> WanDetail`; fetches
  history alongside networks and devices; a history error becomes
  `outages: None`, never a failed command.
- `render/wans.rs`: the OUTAGES row and the outage table.
- `tests/fixtures/`: a made-up `aggregated-dashboard` response (fixture WAN
  groups, ISP and timestamps).
- README: `wans get` outages and `--since`.

## Tests

- Types: decodes the fixture; tolerates nulls, missing `wan_history`, missing
  `dashboard_meta`.
- Client: request path and query (`historySeconds`, `widgets=wan_history`).
- Service: outages matched by `network_group`; end = start + duration; window
  start from `dashboard_meta`; WAN without an entry → `None`; history error →
  `None` with live fields intact; `list_item_equals_get` compares `Wan`.
- `--since`: default 604800; `-30d`; future time rejected.
- Render: OUTAGES row with and without outages, table order and formats, no
  section when `None`, colour on the percentage.
- CLI golden output for `wans get` and `wans get -o json`.

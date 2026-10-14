# Per-WAN uptime, availability and latency in `wans` (design)

Date: 2026-10-14
Status: approved
Follow-up: #33 (outage history in `wans get`)

## Goal

`wans list` and `wans get` show the gateway's uptime in the UPTIME column, not
the WAN's. After a short internet drop the gateway still reports days of
uptime, and a disconnected WAN shows the same figure as a connected one. Show
each WAN's own uptime instead, plus how reliable it has been over the last day.

Success looks like:

- Minutes after a drop, `unifictl wans list` shows that WAN's UPTIME in
  minutes, not the gateway's days.
- A disconnected WAN shows `-` for UPTIME, and `wans get` says how long it has
  been down.
- An AVAILABILITY column shows the last 24 hours as a percentage, coloured by
  health; `--wide` adds the average latency.

## Decisions

| Question | Decision |
|---|---|
| Source | The gateway's `uptime_stats` in `stat/device`, which `wans` already fetches. No new requests |
| UPTIME | `uptime_stats[group].uptime` while connected; `-` while disconnected |
| Without `uptime_stats` | UPTIME, AVAILABILITY, LATENCY print `-`. Gateway uptime is never used as a fallback |
| Disconnected WAN | `downtime` (seconds) from `uptime_stats[group].downtime`, shown by `wans get` and in JSON |
| AVAILABILITY | `uptime_stats[group].availability` over `time_period`, two decimals: `99.93%`; JSON keeps the controller's full value |
| Availability colour | On the displayed value: ≥ 99.9% green, 99–99.9% yellow, < 99% red |
| LATENCY | `uptime_stats[group].latency_average`, ms; `--wide` only in `list`, always in `get` |
| Latency colour | < 50 ms green, 50–99 yellow, ≥ 100 red |
| Status | Unchanged: `wanN.up` |
| JSON | `uptime` becomes nullable; `downtime`, `availability`, `availabilityPeriod`, `latency` added (nullable) |
| Per-monitor results | Not shown (ping.ui.com, 1.1.1.1, …); maybe later under `wans get` |

The `uptime` change is the one breaking change to the JSON: it was always a
number (the gateway's uptime) and becomes `null` for a WAN that is down or
reports no stats. The old number was wrong for every WAN, so keeping it would
keep the bug.

## The controller data

Observed on a UCG-Fiber running UniFi Network 9.x. `stat/device`, the gateway
entry (`type` `udm`, `ugw`, `uxg`, …), keyed by the network's
`wan_networkgroup` (`WAN`, `WAN2`):

```json
"uptime_stats": {
  "WAN": {
    "uptime": 264,
    "availability": 99.9306,
    "latency_average": 11,
    "time_period": 86400,
    "monitors": [{"target": "1.1.1.1", "type": "icmp", "availability": 100.0, "latency_average": 11}],
    "alerting_monitors": [ … ]
  },
  "WAN2": {
    "downtime": 2053545,
    "monitors": [{"target": "1.1.1.1", "type": "icmp", "availability": 0.0}],
    "alerting_monitors": [ … ]
  }
}
```

- `uptime`: seconds since this WAN last came up. Present only while it is up.
- `downtime`: seconds since it went down. Present only while it is down.
- `availability`, `latency_average`: over `time_period` seconds (86400 in
  every response seen). Absent for a WAN that is down.
- `wan1`/`wan2` carry neither `uptime` nor `downtime`.

The same object appears under the `wan` subsystem of `stat/health`; it is not
used, to avoid a second request.

## Output

### `wans list`

```
ID          NAME        IP           UPTIME   STATUS        AVAILABILITY
internet-1  Internet 1  203.0.113.1  4m 24s   connected     99.93%
internet-2  Internet 2               -        disconnected  -
```

`--wide` adds LATENCY before DNS:

```
… AVAILABILITY  LATENCY  DNS
… 99.93%        11 ms    8.8.8.8, 8.8.4.4
… -             -        -
```

### `wans get`

```
FIELD         VALUE
ID            internet-1
NAME          Internet 1
IP            203.0.113.1
UPTIME        4m 24s
STATUS        connected
AVAILABILITY  99.93% (24h)
LATENCY       11 ms
DNS           8.8.8.8, 8.8.4.4
```

A disconnected WAN shows `DOWNTIME  23d 18h 25m 45s` in place of UPTIME. The
period in `(24h)` comes from `time_period`, with zero parts left out and
days used only from two days up (`24h`, `7d`, `1h 11m 10s`).

### JSON

```json
{"id": "internet-1", "name": "Internet 1", "ipAddress": "203.0.113.1",
 "status": "connected", "uptime": 264, "downtime": null,
 "availability": 99.9306, "availabilityPeriod": 86400, "latency": 11,
 "dnsServers": ["8.8.8.8", "8.8.4.4"]}
```

## Implementation

- `unifi/types.rs`: `Device.uptime_stats: HashMap<String, WanUptimeStats>`
  (`uptime`, `downtime`, `availability`, `latency_average`, `time_period`;
  all `Option`). Monitors are not deserialised.
- `service/wans.rs`: `live_fields` reads `uptime_stats[n.wan_networkgroup]`
  and stops using `gw.uptime`.
- `model.rs`: `Wan` gains the fields above; `uptime: Option<i64>`.
- `render/style.rs`: `availability()` and `latency()` graders;
  `render/wans.rs`: the new columns and rows.
- `tests/fixtures/unifi-devices.json`: `uptime_stats` on the fixture gateway,
  one WAN up and one down, with made-up values.

## Tests

- Service: uptime comes from `uptime_stats`, not the gateway; a down WAN has
  `uptime` `None` and `downtime` set; a gateway without `uptime_stats` gives
  `None` throughout; `list_item_equals_get` still holds.
- Render: `-` cells, `--wide` LATENCY, `DOWNTIME` in `get`, colour thresholds
  at their boundaries.
- CLI golden output for `wans list`, `wans list --wide`, `wans get`.
- README: the `wans` columns.

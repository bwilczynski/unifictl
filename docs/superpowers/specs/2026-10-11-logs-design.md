# `unifictl logs`: the controller's system log (design)

Date: 2026-10-11
Status: draft, awaiting review

## Goal

Read the UniFi Network system log from the terminal the way `journalctl` reads
the journal: the same flags where they translate, one line per entry, and a
`-f` that streams new entries.

Success looks like:

- `unifictl logs` shows the 50 most recent entries, oldest at the top.
- `unifictl logs -p high --since yesterday` shows what went wrong since
  yesterday.
- `unifictl logs --client iphone-0003 --device uap-02` shows that phone's
  roams to and from UAP-02, with both IDs tab-completed.
- `unifictl logs -f -o json | jq` streams new entries as they arrive.

## Decisions

| Question | Decision |
|---|---|
| Endpoint | `POST /v2/api/site/{site}/system-log/all`; `/critical` is not used (it returned `[]` even with a `VERY_HIGH` entry in range) |
| Free-text search (`-g`) | Not provided; pipe to `grep` |
| Device/client filter | Matched in unifictl, against parameter MACs; the controller's `searchText` misses entries |
| `--follow` | In v1, polling every 5 s |
| Text format | journalctl `short`: time, category, message; severity shown by colour |
| JSON | One compact object per line, unifictl's own model with `DeviceRef`/`ClientRef` |
| Device and client lists | Fetched only for `-o json`, `--device` or `--client` |
| Time zone and parsing | New dependency `jiff` |

## The controller API

Observed on a UCG-Fiber running UniFi Network 9.x.

`POST {prefix}/v2/api/site/{site}/system-log/all` with a JSON body; `GET`
returns 405. Body fields (all optional):

| Field | Meaning |
|---|---|
| `timestampFrom`, `timestampTo` | Epoch milliseconds. Omitted: the whole retained log |
| `pageSize`, `pageNumber` | Paging; `pageNumber` is 0-based. 5000 per page works |
| `severities` | Any of `LOW, MEDIUM, HIGH, VERY_HIGH, WARNING, INFO` |
| `categories` | Any of `SECURITY, UNIFI_DEVICES, SOFTWARE_UPDATES, VPN, POWER, UNIFI_ETHERNET_PORTS, CLIENT_DEVICES, UNKNOWN, AUDIT, INTERNET_AND_WAN` |

Unknown fields are ignored; an unknown enum value is a 400 whose message lists
the accepted values.

The response is newest first:

```json
{"data": [ … ], "page_number": 0, "total_element_count": 1434, "total_page_count": 2}
```

An entry (fields unifictl ignores omitted):

```json
{
  "id": "65f000000000000000000001",
  "timestamp": 1791708195146,
  "severity": "LOW",
  "category": "CLIENT_DEVICES",
  "subcategory": "MONITORING_WIFI",
  "event": "CLIENT_ROAMED",
  "title_raw": "WiFi Client Roamed",
  "message_raw": "{CLIENT} roamed from {DEVICE_FROM} to {DEVICE_TO}. Connection Info: {WIFI_STATS}. …",
  "parameters": {
    "CLIENT": {"id": "a4:ee:ee:00:00:03", "name": "iPhone 00:03", "hostname": "iPhone"},
    "DEVICE_FROM": {"id": "aa:bb:cc:dd:02:01", "name": "UAP-02", "model": "UAP-AC-Lite"},
    "WIFI_STATS": {"id": "a4:ee:ee:00:00:03", "name": "Ch. 116 (5 GHz, 80 MHz), -68 dBm"}
  }
}
```

`title_raw` can contain placeholders too (`{WAN_ID} Internet Down`). A
parameter's `name` may be missing or `null`. Devices and clients appear as
parameters whose `id` is their MAC, under keys such as `DEVICE`,
`DEVICE_FROM`, `DEVICE_TO`, `DEVICE_WITH_PORT`, `OBJECT`, `CLIENT` and
`SRC_CLIENT`; other parameters of the same entry may reuse a client's MAC as
their `id` (`WIFI_STATS`, `CHANNEL`).

The API key works for this endpoint. Session auth (self-hosted controllers)
uses the same path without `/proxy/network`, as every other endpoint does.

## CLI

```
unifictl logs [-n N] [-S WHEN] [-U WHEN] [-p LEVEL] [-r] [-f]
              [--category CAT]... [--device ID]... [--client ID]...
              [-o table|json] [--color …] [--refresh]
```

| Flag | Meaning |
|---|---|
| `-n, --lines N` | Show the N most recent matching entries. Default 50 when neither `-S` nor `-U` is given; with either, every entry in the window. With `-f`, default 10 |
| `-S, --since WHEN` | Entries at or after WHEN |
| `-U, --until WHEN` | Entries at or before WHEN |
| `-p, --priority LEVEL` | `low`, `medium`, `high` or `very-high`: that level and above |
| `-r, --reverse` | Newest first |
| `-f, --follow` | Keep running and print new entries as they arrive |
| `--category CAT` | `security`, `unifi-devices`, `software-updates`, `vpn`, `power`, `unifi-ethernet-ports`, `client-devices`, `audit`, `internet-and-wan`, `unknown`. Repeatable, or comma-separated |
| `--device ID` | Entries that mention this device. Repeatable. Completes like `devices get` |
| `--client ID` | Entries that mention this client. Repeatable. Completes like `clients get` |

Rejected combinations, as clap usage errors (exit 2): `-f` with `-U`, and
`-f` with `-r` (as journalctl). `-o ids` returns the
existing `-o ids is only supported by …` error.

### Time expressions (`-S`, `-U`)

A subset of `systemd.time(7)`, in the local time zone:

- `now`, `today` (00:00), `yesterday` (00:00 the day before), `tomorrow`.
- Relative: `-2h`, `+30m`, `-1d12h`; units `s`, `m`, `h`, `d`, `w`. Also the
  journalctl spelling `2h ago`.
- Absolute: `YYYY-MM-DD`, `YYYY-MM-DD HH:MM`, `YYYY-MM-DD HH:MM:SS`, and the
  same with `T` instead of the space.

Anything else is an error naming the accepted forms.

`--since` after `--until` is an error.

### Severity order

`-p` and colour treat the levels as `info` = `low` < `warning` = `medium` <
`high` < `very-high`. `INFO` and `WARNING` have not been seen in a real log, so
this ranking is an assumption. `-p low` sends no `severities` filter; `-p
medium` sends `MEDIUM, WARNING, HIGH, VERY_HIGH`; and so on.

## Filtering

Each filter runs where it is cheapest and still exact:

| Filter | Where |
|---|---|
| `-S`, `-U` | Controller: `timestampFrom`, `timestampTo` |
| `-p` | Controller: `severities` |
| `--category` | Controller: `categories` |
| `--device`, `--client` | unifictl, after each page arrives |

Different flags narrow the result (AND); repeating one flag widens it (OR), as
journalctl does with fields. `--client a --device b` keeps entries that mention
client `a` and device `b`.

### Device and client matching

`--device ID` and `--client ID` turn the ID into a MAC through the same lookup
`devices get` and `clients get` use (devices list; online clients plus client
history). An unknown ID is the same error `get` gives. A client that has left
the client history (`client_history_days`) cannot be named by ID.

An entry mentions a MAC when any of its parameters has that MAC as its `id`
(compared lower-case). This finds config changes too: the controller's own
`searchText` field missed 7 of 1072 entries for one access point, all
`MADE_CHANGES_ONE_SETTING_OBJECT` with the device under `OBJECT`.

## Fetching

Log requests (POST) never use the response cache.

1. `now` is taken once, and `timestampTo` is `--until` or `now`. Pinning it
   keeps pages from shifting while new entries arrive.
2. Pages of 1000 are requested newest first (`pageNumber` 0, 1, …) until N
   matching entries are collected, or the last page (`total_page_count`) is
   read.
3. Entries are deduplicated by `id`, then ordered oldest first (or newest first
   with `-r`).

Without `--device` and `--client`, `-n 50` takes one request. With them,
unifictl may read further back: the whole log on the test controller (about
6600 entries) is 7 requests.

The device and client lists (3 GETs, in parallel through `fetch!`, through the
cache) are fetched only when `-o json`, `--device` or `--client` is given.
Plain text needs neither: the line shows the names the log already carries.

## `--follow`

1. Print the last N entries (default 10) as above.
2. Every 5 seconds, request entries with `timestampFrom` set to the newest
   timestamp printed. Skip entries whose `id` was already printed at that
   timestamp (several can share a millisecond), apply the device/client
   filter, print oldest first.
3. Flush stdout after every batch.

Failures while following:

- Network errors and 5xx: one warning to stderr per failure streak
  (`warning: controller unreachable, retrying: …`), then retry with backoff
  5 s, 10 s, 20 s, 40 s, 60 s, 60 s, …; a success resets it.
- Authentication errors: exit 1 with the usual message.

Ending:

- Ctrl-C ends the process through the default SIGINT handling; no signal
  handler or extra dependency.
- A closed stdout (`| head`) exits 0 without an error message. This applies to
  non-follow output too.

When an entry mentions a MAC the device/client map lacks (a new client), the
map is refetched, at most once a minute. Only relevant with `-o json`; the
filter MACs never change during a run.

The clock and the sleep are injected so tests run without waiting.

## Text output

journalctl's `short` format without the host column (the console name, the
same on every line), with the category in place of the identifier:

```
Oct 08 17:55:28 internet-and-wan: Internet connection WAN1 ExampleISP on port 5 is down.
Oct 10 23:33:32 security: A network intrusion attempt from 192.168.10.67 to 198.51.100.7 has been detected.
Oct 11 03:07:42 unifi-devices: UAP-02 moved to channel 11 from 1.
Oct 11 10:43:15 client-devices: iPhone 00:03 roamed from UAP-02 to UAP-01. Connection Info: Ch. 116 (5 GHz, 80 MHz), -68 dBm. Roaming Decision: -57 dBm to -68 dBm.
```

- Time: local, `%b %d %H:%M:%S`.
- Category: kebab-case, as the `--category` values.
- Message: `message_raw` with each `{KEY}` replaced by `parameters.KEY.name`.
  A missing or `null` value, or a key that has no parameter, becomes `?`:
  `Network Application made a change to ? in UAP-02 ? settings. Source IP: ?`.
- The title is not shown; every observed message stands on its own.
- No truncation or wrapping.
- Nothing printed (no header) when nothing matches.

Colour (existing `--color` rules; `src/render/style.rs`) is applied to the
message by severity, as journalctl colours by priority:

| Severity | Paint |
|---|---|
| very-high | new `BoldRed` (`1;31`) |
| high | `Red` |
| medium, warning | `Yellow` |
| low, info | none |

## JSON output

One compact object per line (JSON Lines), flushed per line, for every `logs`
invocation; unlike other commands there is no surrounding array, so `-f` and
the non-follow form produce the same shape. Expanded for reading:

```json
{
  "id": "65f000000000000000000001",
  "time": "2026-10-11T10:43:15.146+02:00",
  "severity": "low",
  "category": "clientDevices",
  "subcategory": "monitoringWifi",
  "event": "clientRoamed",
  "title": "WiFi Client Roamed",
  "message": "iPhone 00:03 roamed from UAP-02 to UAP-01. Connection Info: Ch. 116 (5 GHz, 80 MHz), -68 dBm. Roaming Decision: -57 dBm to -68 dBm.",
  "devices": [
    {"kind": "device", "id": "uap-02", "name": "UAP-02"},
    {"kind": "device", "id": "uap-01", "name": "UAP-01"}
  ],
  "clients": [
    {"kind": "client", "id": "iphone-0003", "name": "iPhone"}
  ],
  "details": {
    "channel": "116",
    "channelWidth": "80",
    "previousChannel": "48",
    "previousSignalStrength": "-57",
    "radioBand": "na",
    "signalStrength": "-68",
    "wifiStats": "Ch. 116 (5 GHz, 80 MHz), -68 dBm",
    "wlan": "home-wifi"
  }
}
```

| Field | Source |
|---|---|
| `id` | `id` |
| `time` | `timestamp`, RFC 3339 with milliseconds and the local offset |
| `severity`, `category`, `subcategory`, `event` | The controller's `SCREAMING_SNAKE` value in camelCase (`VERY_HIGH` → `veryHigh`) |
| `title`, `message` | `title_raw`, `message_raw` with placeholders filled as in text output |
| `devices` | `DeviceRef` for each distinct MAC under a device key (`DEVICE`, `DEVICE_FROM`, `DEVICE_TO`, `DEVICE_WITH_PORT`, `OBJECT`) that is a known device, in key order |
| `clients` | `ClientRef` for each distinct MAC under a client key (`CLIENT`, `SRC_CLIENT`, `DST_CLIENT`). A MAC missing from the client list gets `"id": null` and the log's name |
| `details` | Every other parameter as `camelCaseKey: name`, as strings: all but the ones that produced a reference, `CONSOLE_NAME` and those without a name. Other parameters reusing a device's or client's MAC as their `id` (`CHANNEL`, `WIFI_STATS`) stay here |

Keys are matched explicitly because other parameters reuse those MACs as
their `id`. Parameters are read in key order (the JSON object is not
order-preserving). `devices` and `clients` are always present (possibly empty), `details` always
an object. Not carried over: `status` (the UI's read state), `show_on_dashboard`,
fingerprint fields, the versioned `key` (`CLIENT_ROAMED_2`), raw templates.

`ClientRef.id` is a `String` today; the log model uses its own
`LogClientRef { kind, id: Option<String>, name }` rather than changing the
shared type.

## Code layout

- `src/cli.rs`: `Command::Logs(LogsArgs)`, completers from `completion::devices`
  and `completion::clients`.
- `src/unifi/types.rs`: `SystemLogPage`, `SystemLogEntry`, `LogParam`
  (`id`, `name: Option<String>`, other fields ignored).
- `src/unifi/client.rs`: a POST path sharing `fetch`'s auth and 401 handling
  (refactored to take the request to send), never cached; `system_log(&LogQuery)`
  on `UnifiApi`.
- `src/unifi/fixtures.rs`: `unifi-system-log.json` loaded when present;
  `system_log` applies the time window, severities, categories and paging, so
  the service is tested against realistic behaviour.
- `src/service/logs.rs`: ID → MAC, paging, MAC filter, ordering, entry → model,
  the follow loop (clock and sleep injected).
- `src/timespec.rs`: `--since`/`--until` parsing against an injected `now`.
- `src/model.rs`: `LogEntry`, `LogClientRef`.
- `src/render/logs.rs`: template filling, the text line, colour.
- `src/render/style.rs`: `BoldRed`.
- `src/app.rs`: dispatch, broken-pipe handling.
- `Cargo.toml`: `jiff`.
- `README.md`: usage line and a short `logs` section.

## Testing

Unit tests:

- `timespec`: every accepted form, `ago`, rejection messages, DST-safe
  arithmetic in a fixed zone.
- Template filling: all placeholders present, missing, `null`, and a
  placeholder in the title.
- MAC matching: under `OBJECT`; a client MAC reused by `WIFI_STATS` does not
  create a second client reference; AND across flags, OR within one.
- Paging: stops at N matches; reads every page when needed; pinned
  `timestampTo`; duplicates across pages removed.
- Follow: new entries printed once, same-millisecond entries not repeated,
  backoff sequence and reset, auth error stops, map refetch rate limit.
- JSON model: camelCase enums, `details` keys, unknown client with `id: null`.
- Colour: each severity's paint; uncoloured output has no escapes.

`httpmock` tests in `client.rs`: POST method, path with and without the proxy
prefix, body fields, API key header, session re-login on 401, the response
cache untouched.

Integration tests (`tests/cli.rs`), with `--fixtures`, a fixed clock (hidden
`UNIFICTL_NOW` read only in fixtures builds) and `TZ=UTC`: default output,
`-p`, `--category`, `--device`/`--client` with completion candidates,
`-r`, `-o json`, rejected flag combinations, `-o ids` error. Golden files are
generated from the Go implementation, which has no logs, so none are added.

The fixture `tests/fixtures/unifi-system-log.json` is built from a real log's
structure but carries no real data. Every identifying value is replaced with
the fictional ones the other fixtures use: MACs (`aa:bb:cc:dd:…`,
`…:ee:ee:00:00:…`), client and device names, admin names, SSIDs, ISP names,
entry and object IDs (`65f0…`), private IPs from the existing fixtures and
public IPs from the documentation ranges (`198.51.100.0/24`, `203.0.113.0/24`).
The same rule applies to examples in code, tests, docs and the README.

## Out of scope

- Free-text search (`-g`): use `grep`.
- The controller's `searchText` as a pre-filter for `--device`/`--client`;
  possible later if scanning gets slow.
- `/system-log/critical`.
- A configurable follow interval.
- Other journalctl output modes (`-o short-iso`, `-o cat`, …).
- Marking entries read or hiding them (the UI's `status`).

# `unifictl clients traffic`: internet activity per client (design)

Date: 2026-10-11
Status: draft, awaiting review
Issue: #25

## Goal

Show what the WebUI's traffic insights show: how much each client used the
internet over a time window, and which applications (YouTube, Netflix, …) the
traffic went to.

Success looks like:

- `unifictl clients traffic` ranks clients by traffic over the last 24 hours,
  with each client's top application.
- `unifictl clients traffic macbook-pro-0001` breaks that client's traffic down
  by application, with the same client IDs as `clients list`, tab-completed.
- `unifictl clients traffic --by-app --since yesterday` ranks applications
  across the whole site.
- `-o json` gives the same data for scripts.

## Decisions

| Question | Decision |
|---|---|
| Command | `clients traffic [ID]`, a third `clients` subcommand |
| Site-wide view | `--by-app`, from the same response |
| Endpoint | `GET /v2/api/site/{site}/traffic`; `app-traffic-rate` (a time series, `GET` returns 405) is not used |
| Time window | `-S/--since`, `-U/--until`, parsed like `logs`; default the last 24 hours |
| Application names | Integration API DPI catalogue, cached for 24 hours |
| Without an API key | Names fall back to `Unknown (cat:app)`, one hint on stderr |
| Client IDs | `clients list`'s ID for the client's MAC; else the `{kebab-name}-{mac-suffix}` scheme from the report |

## The controller API

Observed on a UCG-Fiber running UniFi Network 9.x.

### Traffic

`GET {prefix}/v2/api/site/{site}/traffic?start={ms}&end={ms}&includeUnidentified=true`,
epoch milliseconds. Fields unifictl ignores are omitted:

```json
{
  "client_usage_by_app": [
    {
      "client": {"mac": "a4:ee:ee:00:00:01", "name": "MacBook Pro", "hostname": "macbook", "is_wired": false},
      "usage_by_app": [
        {"category": 0, "application": 127, "bytes_received": 1200000, "bytes_transmitted": 40000,
         "total_bytes": 1240000, "activity_seconds": 3600}
      ]
    }
  ],
  "total_usage_by_app": [
    {"category": 0, "application": 127, "bytes_received": 1200000, "bytes_transmitted": 40000,
     "total_bytes": 1240000, "client_count": 1}
  ]
}
```

`client.name` may be empty; the client then falls back to its hostname, then
its MAC, as everywhere else. `bytes_received` is download from the client's
point of view.

### Application names

The traffic response carries only numbers. Names come from the Integration
API, which UniFi OS serves only with an API key:

- `GET /proxy/network/integration/v1/dpi/categories?offset=&limit=`
- `GET /proxy/network/integration/v1/dpi/applications?offset=&limit=`

Both return a page:

```json
{"offset": 0, "limit": 200, "count": 200, "totalCount": 2112,
 "data": [{"id": 65538, "name": "BitTorrent Series"}]}
```

On the observed controller there are 35 categories and 2112 applications; a
`limit` of 200 works. A category `id` is the traffic `category`. An
application `id` is `category << 16 | application`: BitTorrent (category 1,
application 2) is 65538. Category 255 with application 65535 is traffic DPI
could not identify.

## Command line

```
unifictl clients traffic [ID] [--by-app] [-S WHEN] [-U WHEN]
```

- `ID`: a client ID, completed like `clients get`. Conflicts with `--by-app`.
- `--by-app`: site-wide totals per application.
- `-S/--since WHEN`, `-U/--until WHEN`: the forms `logs` accepts. Defaults:
  24 hours before `--until`, and now. `--since` after `--until` is an error.

### Output

Rows are sorted by total bytes, largest first. Byte counts use the existing
`format_bytes`. On a terminal, client and application names are blue like
other names; fallback application names (`Unknown (…)`, `Unidentified`) are
dim, so unnamed traffic recedes.

Clients (no `ID`, no `--by-app`); `TOP APP` is the client's largest
application:

```
ID                NAME         DOWN      UP        TOTAL     TOP APP
macbook-pro-0001  MacBook Pro  1.2 GB    40.0 MB   1.2 GB    Facebook Messenger
```

One client (`ID`); `ACTIVE` is `activity_seconds` formatted like uptime:

```
APP                 CATEGORY            DOWN    UP       TOTAL   ACTIVE
Facebook Messenger  Instant messengers  1.2 GB  40.0 MB  1.2 GB  1h 0m
```

Applications (`--by-app`):

```
APP                 CATEGORY            CLIENTS  DOWN    UP       TOTAL
Facebook Messenger  Instant messengers  4        3.1 GB  90.0 MB  3.2 GB
```

An application missing from the catalogue shows as `Unknown (9:4242)`, its
category by name if known, else `Unknown (9)`. Unidentified traffic shows as
`Unidentified`. A client in the traffic response takes the ID and name
`clients list` gives its MAC, since the report's own name can differ or be
empty; only a client missing from the list gets an ID derived from the
report. A known client with no traffic in the window shows an empty table;
an ID found in neither is `Network client not found: <id>`, the same error
as `clients get`.

`-o json` serializes the same rows, camelCase, with the numeric IDs kept next
to the names:

```json
{"items": [{"id": "macbook-pro-0001", "name": "MacBook Pro", "mac": "a4:ee:ee:00:00:01",
            "bytesReceived": 1200000, "bytesTransmitted": 40000, "totalBytes": 1240000,
            "topApp": {"id": 127, "name": "Facebook Messenger", "category": {"id": 0, "name": "Instant messengers"}}}]}
```

The one-client view adds `activitySeconds` per application; `--by-app` adds
`clientCount`. `-o ids` prints client IDs for the clients view and is
unsupported for the other two.

## Components

Following the existing layers:

- `unifi/types.rs`: `TrafficReport`, `ClientUsage`, `TrafficClient`,
  `AppUsage`, `DpiPage<T>`, `DpiEntry {id, name}`.
- `UnifiApi`: `traffic(start_ms, end_ms)`, `dpi_categories()`,
  `dpi_applications()`. The HTTP client reads the first catalogue page, then
  the remaining pages in parallel with `fetch!`. `FixtureApi` loads
  `unifi-v2-traffic.json`, `unifi-dpi-categories.json` and
  `unifi-dpi-applications.json`, each optional like the system log.
- Caching: the traffic report is never cached, like the system log: every
  window is its own URL, so cached entries would pile up. The catalogue is
  near static, so its pages are read with a 24-hour freshness instead
  (`Cache::get` gains a max-age argument variant); `--refresh` skips it.
- `service/traffic.rs`: fetches traffic, categories, applications and the
  client list with
  `fetch!`; a `DpiNames` lookup resolves `(category, application)` with the
  fallbacks above; builds and sorts the three views. A catalogue failure is
  not fatal: names fall back and the service returns the reason once, so the
  app prints `note: application names unavailable (<reason>)` to stderr,
  `(needs an API key)` without one.
- `model.rs`: `ClientTrafficList`, `ClientAppTrafficList`,
  `AppTrafficList` and their rows; `Ids` for the clients view.
- `render/traffic.rs`: the three tables, names styled with `style::name`.
- `cli.rs`, `app.rs`: the subcommand and its flags. `ID` reuses the
  existing `completion::clients` candidates (every client `clients list`
  shows), so `completion.rs` is unchanged. Completion cannot see the window
  flags, so it may offer a client with no traffic in the window; that shows an
  empty table.

## Testing

Fixtures use fictional clients, MACs and names matching the existing client
fixtures (no real network data).

- Unit: application ID decoding; name fallbacks (known, unknown app, unknown
  category, unidentified); sorting; top application; client ID matching;
  catalogue paging; window defaults and `--since` after `--until`.
- Exact output: the three tables (render tests) and their JSON (CLI tests).
- CLI: `--by-app` with an `ID` is rejected; an unknown ID is not found;
  `-o ids` for the clients view.

## Out of scope

- `app-traffic-rate` and any per-interval or chart view.
- Device types from `fingerprint_devices`.
- Filtering by application or category.

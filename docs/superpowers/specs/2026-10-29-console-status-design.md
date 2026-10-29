# `status`: controller header and UniFi OS updates (design)

Date: 2026-10-29
Status: approved

## Goal

The UniFi app sent a push notification saying a UniFi OS update was available
for the console. `unifictl status` said `UPDATES ok all up to date`. The
`UPDATES` row only reads per-device firmware (`upgradable` /
`upgrade_to_firmware` from `stat/device`). UniFi OS updates the console's
operating system itself, and the Network app never reports that.

Success looks like:

- When UniFi OS reports a pending OS update, `status` lists it on the
  `UPDATES` row, and the row turns `info`.
- `status` opens with one line naming the controller and its versions, so it
  is clear which console the report is about.
- Both show up in `-o json` too.

## Output

Fictional names and versions:

```
$ unifictl status
console-01  UniFi OS 4.3.6 · Network 9.4.19

WAN      ok    Internet 1  up 7d 22h 57m 33s  100.00% over 24h  14 ms
DEVICES  ok    7 online
WI-FI    ok    2 APs, 17 clients
WIRED    ok    4 switches, 6 clients
ALARMS   ok    no high-severity events in 24h
UPDATES  info  console-01 UniFi OS 4.3.6 → update available
               usw-office 7.1.68 → 7.1.72
```

With no OS update pending, there is no console line and the row reads as it
does today (`all up to date` when nothing else is pending). On a legacy
(non-UniFi OS) controller the header leaves out the UniFi OS part:

```
controller-01  Network 9.4.19
```

JSON (`-o json`) gets a top-level `controller` object, and the `updates` area
gets an optional `console` object:

```json
{
  "controller": { "name": "console-01", "osVersion": "4.3.6", "networkVersion": "9.4.19" },
  "rows": [
    ...,
    {
      "verdict": "info",
      "area": "updates",
      "updates": [ { "id": "usw-office", ..., "from": "7.1.68", "to": "7.1.72" } ],
      "console": { "name": "console-01", "version": "4.3.6", "updateAvailable": true }
    }
  ]
}
```

`controller` is left out when `stat/sysinfo` can't be read, and so is
`osVersion` when it isn't known. `console` is left out unless an OS update is
pending. Existing fields don't change.

## Data sources

Checked against a live UniFi OS console using an API key:

| Call | Auth | Gives |
|---|---|---|
| `GET {prefix}/api/s/{site}/stat/sysinfo` | API key and session | `data[0].name` (controller name), `data[0].version` (Network app version), `data[0].console_display_version` (UniFi OS version; UniFi OS only) |
| `GET /api/system` (no `/proxy/network` prefix) | API key only | `deviceState`: `"updateAvailable"` while an OS update is pending; `name` |
| `GET /api/firmware/update` | session only (401 with an API key) | probably the target version; **not used** |

Notes:

- `sysinfo`'s `update_available` refers to the Network app, not UniFi OS.
  Don't use it.
- In this codebase, API key mode means UniFi OS (`/proxy/network` prefix) and
  session mode means a legacy controller (no prefix, `/api/login`). So
  `/api/system` is only called in API key mode, and session mode never shows
  a console update.
- `deviceState` values other than `"updateAvailable"` (for example `"ready"`,
  `"updating"`) mean no pending update. Only the exact string counts.

## Decisions

| Question | Decision |
|---|---|
| Target version | Not shown. The API key can't read it. The text says `update available`. |
| Login (session) auth for OS updates | Out of scope. |
| Where the OS update goes | On the `UPDATES` row, before the device updates. The row is `info` when there's any device update or a console update. |
| Console line when up to date | Not shown. |
| Name on the console line | `/api/system`'s `name`, falling back to `sysinfo`'s `name`. *(Claude's call.)* |
| Version on the console line | `console_display_version` from `sysinfo`. If it's missing, the line reads `console-01 UniFi OS update available`. *(Claude's call.)* |
| Header placement | One plain line above the table, followed by a blank line. No verdict, because it's identity, not health. |
| Header contents | `<name>  UniFi OS <os> · Network <network>`, leaving out any part that's unknown. No header at all when `sysinfo` can't be read or has no name. *(Claude's call.)* |
| `/api/system` or `sysinfo` fails (401, 404, timeout, legacy controller) | That piece is silently left out: no console line, or no header. Never an error, never makes the `UPDATES` row `unknown`, never changes the exit code. Same as the optional reads `status` already makes (`health`, clients, etc.). *(Claude's call.)* |
| Other commands | Only `status` changes. `devices` and the rest stay as they are. |
| Caching | Same policy as `stat/health` and `stat/device` for both new reads, so they're cached for the same time. *(Claude's call; the builder can follow the existing cache rules.)* |
| Colour | The name is styled like other names (`style::name`); versions are passed through `style::printable`. The header has no verdict colour. |

## Testing

- Unit tests for the parsing: `deviceState` values, a `sysinfo` with and
  without `console_display_version`.
- Service tests through `FixtureApi`: an update pending, no update pending,
  `/api/system` failing, and `sysinfo` failing. Check the `UPDATES` verdict and
  that device updates still show.
- Renderer tests for the header (all parts, no OS part, none) and the console
  line, with and without device updates.
- A CLI test through the `fixtures` feature for both the table and `-o json`.
  New fixture files (`unifi-system.json`, `unifi-sysinfo.json`) use fictional
  values.
- HTTP client tests (mock server): `/api/system` is requested **without** the
  `/proxy/network` prefix and with the `X-API-KEY` header, and is never
  requested in session mode.

All names, MACs, IPs and versions in tests, fixtures and docs are fictional,
never the live controller's.

## Non-goals

- The target UniFi OS version, or anything that needs a session login on
  UniFi OS.
- Network app (`sysinfo.update_available`) or other UniFi OS app updates.
- Triggering or scheduling updates.
- Showing the controller header in other commands.

## Left to the builder

- Type and trait method names (for example `UnifiApi::sysinfo()` and
  `UnifiApi::system()`, where `system()` returns `Ok(None)` or an error in
  session mode).
- How `SiteStatus` carries the header (for example
  `controller: Option<Controller>` skipped when `None`).
- Updating the README's `status` section and example output. The CHANGELOG
  comes through the usual release flow.

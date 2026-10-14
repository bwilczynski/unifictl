# Per-AP radio health in `radios list` (design)

Date: 2026-10-14
Status: approved
Issue: #37

## Goal

`logs --client` shows *that* a client keeps roaming between APs, but nothing in
unifictl shows *why*. The usual reasons are channel overlap, high channel
utilisation, interference and retries: radio-level facts the controller already
reports. Show them, one row per radio of every access point.

Success looks like:

- `unifictl radios list` shows, for every AP radio, its band, channel, width,
  TX power mode, utilisation, interference, retry rate and client count.
- Two APs on the same busy 2.4 GHz channel with high interference stand out at
  a glance: UTIL, INTERF and RETRY are coloured green, yellow or red.
- `devices get <ap>` shows the same radios for that one AP.
- No new controller requests: everything comes from `stat/device`.

## Output

```
$ unifictl radios list
AP              BAND  CH   WIDTH  TX    UTIL  INTERF  RETRY  CLIENTS
AP-Living-Room  2.4G  6    20     auto  68%   35%     12%    1
AP-Living-Room  5G    149  80     auto  18%   2%      2%     3
AP-Office       2.4G  6    20     auto  71%   38%     14%    1
AP-Office       5G    36   80     auto  22%   4%      3%     2

$ unifictl radios list --wide
AP              BAND  CH   WIDTH  TX    TX POWER  MIN RSSI  UTIL  INTERF  RETRY  CLIENTS  EXPERIENCE
AP-Living-Room  2.4G  6    20     auto  17 dBm    -         68%   35%     12%    1        91%
AP-Living-Room  5G    149  80     auto  20 dBm    -75 dBm   18%   2%      2%     3        98%
AP-Office       2.4G  6    20     auto  17 dBm    -         71%   38%     14%    1        88%
AP-Office       5G    36   80     auto  20 dBm    -         22%   4%      3%     2        97%

$ unifictl radios list -o ids
ap-living-room/2.4g
ap-living-room/5g
ap-office/2.4g
ap-office/5g
```

`devices get <ap>` gains a section after the device's own fields, with the same
columns as `radios list --wide` minus AP:

```
--- RADIOS ---
BAND  CH   WIDTH  TX    TX POWER  MIN RSSI  UTIL  INTERF  RETRY  CLIENTS  EXPERIENCE
2.4G  6    20     auto  17 dBm    -         71%   38%     14%    1        88%
5G    36   80     auto  20 dBm    -         22%   4%      3%     2        97%
```

## Decisions

| Question | Decision |
|---|---|
| Source | `radio_table` and `radio_table_stats` of each `uap` entry in `stat/device`. No new requests |
| Rows | One per `radio_table` entry; stats joined on the radio `name` (`wifi0`, `wifi1`, …) |
| AP | The device name (blue), like DEVICE in `ports list` |
| Order | Table rows sorted by AP name, then band (2.4G, 5G, 6G, other), then radio name; JSON in controller order |
| BAND | `radio`: `ng` → `2.4G`, `na` → `5G`, `6e` → `6G`; anything else is shown as the raw code |
| CH | `radio_table_stats.channel`, else `radio_table.channel` |
| WIDTH | `radio_table.ht` (MHz), as a number |
| TX | `radio_table.tx_power_mode` (`auto`, `high`, `medium`, `low`, `custom`); `-` when absent |
| TX POWER | `radio_table_stats.tx_power`, dBm (`--wide`) |
| MIN RSSI | `radio_table.min_rssi` dBm when `min_rssi_enabled`, else `-` (`--wide`) |
| UTIL | `cu_total`, percent of airtime the channel is busy |
| INTERF | `cu_total − cu_self_rx − cu_self_tx`, floored at 0: airtime used by others |
| RETRY | `tx_retries_pct`, rounded to a whole percent |
| CLIENTS | `num_sta` |
| EXPERIENCE | `satisfaction`, graded like the other experience columns; `-` when absent or negative (`--wide`) |
| Missing stats | A radio with no matching `radio_table_stats` entry is still listed; its stats columns are `-` |
| Offline AP | Its stats are stale, so they are dropped (like its experience score): stats columns `-`, CH from `radio_table` |
| Odd field types | Radio numbers are read leniently (int, float or numeric string; anything else is `null`), so one odd field never fails `stat/device` for every command |
| UTIL colour | < 50% green, 50–79% yellow, ≥ 80% red |
| INTERF colour | < 10% green, 10–24% yellow, ≥ 25% red |
| RETRY colour | < 10% green, 10–19% yellow, ≥ 20% red |
| `--device ID` | Only that AP's radios; completes AP device IDs. An unknown or non-AP ID gives an empty list, like `ports list --device` |
| ID | `<device-id>/<band>` lowercased (`ap-office/2.4g`). If an AP has two radios on one band, both use `<device-id>/<radio name>` (`ap-office/wifi1`) |
| `-o ids` | One radio ID per line |
| Live TX/RX traffic | Not shown: the controller reports only cumulative byte counters per radio, not rates |

## JSON

```json
{
  "items": [
    {
      "id": "ap-office/2.4g",
      "device": {"kind": "device", "id": "ap-office", "name": "AP-Office"},
      "name": "wifi0",
      "band": "2.4G",
      "channel": 6,
      "width": 20,
      "txPowerMode": "auto",
      "txPower": 17,
      "minRssi": null,
      "utilisation": 71,
      "interference": 38,
      "retries": 14.2,
      "clients": 1,
      "satisfaction": 88
    }
  ]
}
```

Every stats field is nullable. `retries` keeps the controller's decimal value;
the table rounds it. `devices get -o json` adds `radios` (the same objects
without `device`) to access-point details.

## Code shape

- `unifi/types.rs`: `RadioEntry` (`radio_table`) and `RadioStats`
  (`radio_table_stats`) on `Device`.
- `model.rs`: `Radio`, `NetworkRadio` (radio + `DeviceRef`, flattened like
  `NetworkPort`) and `NetworkRadioList`; `radios` on the access-point detail.
- `service/radios.rs`: `build_radios(&Device)` shared with `devices get`, and
  `Service::list_radios(&RadioFilter)`.
- `render/radios.rs`: `list_table` and a section writer reused by
  `render/devices.rs`.
- `render/health.rs`: `utilisation`, `interference`, `retries` graders.
- `cli.rs` / `app.rs`: `radios list [--device ID] [--wide]`; `completion.rs`:
  AP device candidates.
- Fixtures: `docs/demo/fixtures/unifi-devices.json` gains radio data for the two
  demo APs, so the example above runs against the demo.
- README: a `radios list` section next to `ports list`.

## Testing

- Service: band mapping, stats join by radio name, interference floor at 0,
  missing stats → `None`, duplicate-band ID fallback, `--device` filter, against
  `tests/fixtures/unifi-devices.json`.
- Health: boundary tests for the three new graders.
- Render: narrow/wide headers, sort order, `-` for absent values, colour codes
  on UTIL/INTERF/RETRY only when colour is on.
- CLI (`tests/cli.rs`): `radios list`, `--wide`, `--device`, `-o ids`, `-o json`
  and the `devices get <ap>` RADIOS section.

## Out of scope

- `radios neighbours` from `stat/rogueap` (nearby foreign networks per channel).
- Folding radio health into `unifictl status`.
- Re-recording the demo GIFs.

# Health at a glance in lists, and `--wide` everywhere (design)

Date: 2026-10-11
Status: draft, awaiting review
Issue: #14 (supersedes its column proposal for `devices list` and `clients list`)

## Goal

Make `devices list` and `clients list` good for spotting problems quickly:
a client with weak signal, a device whose cable negotiated 100 Mbps, an
access point with poor experience. Keep the default tables lean and put
everything else behind `--wide`, which every list command gains.

Success looks like:

- `unifictl devices list` and `unifictl clients list` show an EXPERIENCE
  column, coloured green/yellow/red; MAC is gone from the default view.
- `--wide` adds the identity and link metrics (MAC, uplink, signal, link
  speed, live traffic, …), with signal and link speed coloured by health.
- Download and upload figures are cyan and magenta in every table that shows
  traffic.
- `clients list` and `clients get` return the same client shape;
  `devices get` is `devices list`'s item plus type-specific sections.

## Decisions

| Question | Decision |
|---|---|
| Default columns | Lean: today's columns minus MAC, plus EXPERIENCE |
| `--wide` | A per-command flag on every list command; adds columns after the default ones |
| Experience source | Devices: `satisfaction`, only while connected; clients: v2 `wifi_experience_score`, only while online. Wired clients and `-1` print `-` |
| Experience colour | ≥ 80% green, 50–79% yellow, < 50% red |
| Signal colour | ≥ −67 dBm green, −68 to −75 yellow, < −75 red |
| Device uplink colour | Relative to the link's capability, min(own `max_speed`, upstream port's `media` max): at it green, below yellow, 10 Mbps or half duplex red |
| Wired client link colour | Absolute (no `max_speed` known): ≥ 1 GbE green, 100 Mbps yellow, 10 Mbps red |
| Wi-Fi PHY rate colour | None; signal already says it |
| Traffic colour | Download/RX cyan, upload/TX magenta, from each row's own point of view; TOTAL uncoloured |
| Extra fetches | None. Lists read only what they fetch today |
| JSON | Always carries the new fields; `--wide` only changes the table; additions only, nothing renamed or removed |
| Model | List item = detail whenever no extra fetch is needed: one `NetworkClient`, one `Vlan`, one `Wan`. Otherwise the detail flattens the list item: `NetworkDevice` (today's `DeviceBase` + `experience`), `Ssid` (+ `securityProtocol`) |
| `clients get` source | v2 active + history (as `clients list`); v1 `stat/sta` is no longer used by it |

This reverses the output-polish design's non-goal "colouring traffic".
Colours stay basic 16-colour ANSI SGR, so the terminal theme supplies the
shades, and follow the existing `--color` rules.

## The controller data

Observed on a UCG-Fiber running UniFi Network 9.x. Only fields unifictl newly
reads are listed.

`stat/device` (already fetched by `devices list`):

| Field | Use |
|---|---|
| `satisfaction` | Experience, 0–100; `-1` (gateways, Flex switches) means none |
| `uplink.speed`, `uplink.max_speed` | Negotiated speed and this device's uplink port maximum, Mbps |
| `port_table[].media` of the upstream device | The upstream port's maximum: `FE` 100, `GE` 1000, `2.5GE` 2500, `5GE` 5000, `10GE` 10000 |
| `uplink.full_duplex` | Half duplex is graded red |
| `uplink.uplink_remote_port` | Upstream port number |

v2 `clients/active` and `clients/history` (already fetched by `clients list`):

| Field | Active | History | Use |
|---|---|---|---|
| `wifi_experience_score` | wireless | – | Experience |
| `signal` | wireless | – | Signal, dBm |
| `tx_rate` | wireless | – | Wi-Fi PHY rate, kbps → Mbps |
| `wired_rate_mbps` | wired | wired | Wired link speed |
| `rx_bytes-r`, `tx_bytes-r` | yes | – | Live rates |
| `essid` | wireless | – | SSID |
| `last_uplink_name` | yes | yes | Upstream device; its ID is `to_kebab(name)` |
| `sw_port` / `last_uplink_remote_port` | wired | wired | Upstream port |
| `uptime`, `last_seen` | yes | `last_seen` | Uptime, last seen |

The upstream port matters: a USW Flex 2.5G with a 10G uplink port in a
2.5G gateway port reports `speed 2500, max_speed 10000`; it runs at what the
link allows, so it is green. The effective maximum is the smaller of the two
port maxima; when either is unknown, the known one is used, and with neither
the wired-client absolute thresholds apply.

Client byte counters are kept from the network's side: a wired client's `tx`
bytes track its switch port's `tx` bytes (checked on the live controller). So
the client's download is `tx_bytes-r` and its upload `rx_bytes-r`;
`NetworkClient.traffic` swaps them, so `rxBytesPerSec` is what the client
received.

## Output

### `devices list`

```
ID          NAME        IP           TYPE          STATUS     EXPERIENCE
uap-01      UAP-01      10.0.0.21    accessPoint   connected  98%
cgf-01      CGF-01      10.0.0.1     gateway       connected  -
```

`--wide` adds MAC, MODEL, FIRMWARE, UPTIME, UPLINK, DOWN, UP:

- UPLINK: negotiated uplink speed (`1 GbE`), coloured as above; `-` with no
  upstream device (the gateway).
- DOWN / UP: `traffic.rxBytesPerSec` / `txBytesPerSec` (the uplink's rates;
  the gateway's WAN rates), formatted like `ports list` RX/S.

### `clients list`

```
ID                NAME         IP          STATUS   CONNECTION  EXPERIENCE
macbook-pro-0001  MacBook Pro  10.0.0.50   online   wireless    92%
nas-1-0002        NAS 1        10.0.0.10   online   wired       -
```

`--wide` adds MAC, UPLINK, SSID, SIGNAL, LINK, DOWN, UP, UPTIME, LAST SEEN:

- UPLINK: upstream AP or switch name, with ` #port` for wired clients.
- SIGNAL: `-69 dBm`; LINK: wired speed (`1 GbE`) or Wi-Fi rate (`866 Mbps`).
- LAST SEEN: UTC `YYYY-MM-DD HH:MM` (a relative time would break golden tests).
- Offline clients print `-` for SSID, SIGNAL, LINK rate, DOWN, UP, UPTIME.

### Other lists

`--wide` as proposed in #14:

| Command | Extra columns |
|---|---|
| `vlans list` | GATEWAY IP, DHCP MODE, DHCP RANGE (`-` unless DHCP server) |
| `ssids list` | SECURITY |
| `wans list` | DNS |

`ports list --wide` is unchanged apart from colour.

### Detail views

- `devices get`: an EXPERIENCE row; UPLINK coloured by health; TRAFFIC RX
  cyan, TX magenta; port table RX cyan, TX magenta; access-point CLIENTS
  section SIGNAL coloured.
- `clients get`: EXPERIENCE, LINK SPEED / SIGNAL (coloured), WI-FI RATE,
  DOWN / UP, LAST SEEN rows where present.

### Traffic colour

| Where | Cyan | Magenta |
|---|---|---|
| `clients traffic` (all three tables) | DOWN | UP |
| `devices list --wide`, `clients list --wide` | DOWN | UP |
| `ports list --wide` | RX/S | TX/S |
| `devices get` | TRAFFIC RX, port RX | TRAFFIC TX, port TX |

## JSON and model

### Devices

`DeviceBase` is renamed `NetworkDevice` and gains `experience`. `devices list`
returns these; `SwitchDetail`, `GatewayDetail`, `AccessPointDetail` keep
flattening it and adding `ports` / `wans` / `connectedClients` (those need the
`clients` and `networkconf` fetches, which stay detail-only).

`Connection` gains optional `maxLinkSpeed` (the effective maximum above) and
`fullDuplex`, set for device uplinks and omitted for wired clients. The list
builds `uplink` from a MAC lookup over the device list itself, not `PortIndex`
(which needs clients).

```json
{"id": "uap-01", "name": "UAP-01", "mac": "…", "ip": "10.0.0.21", "type": "accessPoint",
 "status": "connected", "model": "U6LR", "firmwareVersion": "…", "uptime": 86400,
 "experience": 98,
 "traffic": {"rxBytesTotal": 0, "txBytesTotal": 0, "rxBytesPerSec": 0, "txBytesPerSec": 0},
 "uplink": {"device": {"kind": "device", "id": "us-8-60w", "name": "US 8 60W"}, "port": 5,
            "linkSpeed": "fe", "maxLinkSpeed": "gbe1", "fullDuplex": true}}
```

`devices list` JSON gains these fields; `devices get` JSON gains `experience`
and the two uplink fields.

### Clients

One type replaces `NetworkClient`, `WiredClientDetail`,
`WirelessClientDetail` and `NetworkClientDetail`: `NetworkClient`, untagged
and discriminated on `connectionType`, as `NetworkClientDetail` is today.

- Shared: `id`, `name`, `mac`, `ip`, `connectionType`, `status`,
  `experience`, `traffic` (rates only, `rxBytesPerSec` / `txBytesPerSec`),
  `uptime`, `lastSeen` (epoch seconds).
- Wired `connectedTo`: `Connection` (`device`, `port`, `linkSpeed`).
- Wireless `connectedTo`: `WirelessConnection` (`device`, `ssid`,
  `signalStrength`, `rateMbps`).

Absent values are omitted. `clients list` items gain these fields;
`clients get` JSON gains `experience`, `traffic`, `lastSeen`, `rateMbps`.
`-o ids` is unchanged.

`get_client` becomes a lookup by ID over v2 active, then history (fetched in
parallel, as today), dropping its `devices` and v1 `stat/sta` fetches. The
upstream device ID comes from `last_uplink_name`, so a renamed device's old
name can appear for long-offline clients, as it already does in `clients list`.

### VLANs, WANs, SSIDs

- `Vlan` and `VlanDetail` merge into `Vlan` (today's `VlanDetail` fields):
  `vlans list` items gain `gatewayIp`, `broadcastIp`, `dhcpMode`, `dhcpRange`,
  `relayServer`, `dnsServers`. Both commands map through `build_vlan`.
- `Wan` and `WanDetail` merge into `Wan`: `wans list` items gain `dnsServers`.
- `Ssid` gains `securityProtocol`; `SsidDetail` flattens `Ssid` and adds
  `clients` and `broadcastingAps` (which need the devices fetch).

`vlans get`, `wans get` and `ssids get` JSON is unchanged.

## Components

- `unifi/types.rs`: `Device.satisfaction`; `Uplink.max_speed`, `full_duplex`;
  `PortEntry.media`;
  `ClientV2` gains `wifi_experience_score`, `signal` (exists), `tx_rate`,
  `wired_rate_mbps`, `rx_bytes_r`, `tx_bytes_r`, `sw_port`,
  `last_uplink_remote_port`.
- `model.rs`: the merged types above.
- `service/devices.rs`: `device_to_list` builds the full `NetworkDevice`;
  `device_uplink` fills the new fields, taking the effective maximum from the
  upstream device's port entry.
- `service/{vlans,wans,ssids}.rs`: the merges above.
- `service/clients.rs`: one `client_from_v2` for list and get; v1
  `client_to_detail` is removed.
- `render/health.rs` (new): `Health { Good, Fair, Poor }` and pure graders
  `experience(pct)`, `signal(dbm)`, `uplink(speed, effective_max, full_duplex)`,
  `wired_link(speed)`.
- `render/style.rs`: `Paint::Cyan`, `Paint::Magenta`; `health(text, Health)`,
  `down(text)`, `up(text)`.
- `render/{devices,clients,vlans,ssids,wans}.rs`: `list(…, wide, color)`;
  `render/{ports,traffic}.rs`: traffic colour.
- `cli.rs`, `app.rs`: `--wide` on each list subcommand.

## Testing

Fixtures stay fictional (no real network data). Values are chosen so every
grade appears: an AP at 100 Mbps on a gigabit port, a switch with a 10G
uplink port at 2.5G in a 2.5G upstream port (green), a half-duplex link, a
−80 dBm client, a 45% experience score, an offline client, a device with
`satisfaction: -1`.

- Unit: each grader at its boundaries (80/79/50/49; −67/−68/−75/−76;
  speed = max, below max, 10 Mbps, half duplex, missing max); kbps → Mbps.
- Service: `devices list` and `clients list` mapping, including `-1` and
  missing fields; effective maximum from the upstream port's `media`;
  `vlans list` / `wans list` items equal their `get`; `get_client` for active wired, active wireless, offline,
  and not found.
- Render: default and `--wide` headers for every list; `-` for wired and
  offline clients; coloured output with escapes stripped equals uncoloured.
- CLI golden: new default and wide tables; JSON for `devices list`,
  `clients list`, `clients get`; `--color always -o json` has no escapes.

## Out of scope

- Configurable thresholds or colours.
- Experience for wired clients (needs v1 `stat/sta`).
- Uplink error counters (`rx_errors`, `tx_errors`) and `rate_imbalance`.
- Sorting or filtering lists by health.

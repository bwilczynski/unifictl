# `clients list --self`: the clients that are this machine (design)

Date: 2026-10-29
Status: approved
Issue: #58

## Goal

Sitting at a computer, you want to know which switch port or access point it
is attached to and how that connection is doing (EXPERIENCE, signal, link
speed). Today that means finding your own MAC, scanning `clients list` for it,
then running `clients get <id>`.

The Omarchy bar widget built on unifictl wants a "This device" row for the same
reason. Deciding which local interfaces count, and matching them to controller
clients, belongs in unifictl next to the rest of the client logic, not in the
widget.

Success looks like:

- `unifictl clients list --self` shows only the rows that are this machine.
- A laptop docked on Ethernet with Wi-Fi still on shows **both** rows, wired
  first.
- `--wide` shows uplink (switch + port, or AP), SSID, signal and link speed
  for those rows, exactly as for any other client.
- `-o json` is the normal `clients list` JSON, so the widget parses nothing
  new.

## Output

Same table as `clients list`, filtered. Example with the test fixtures' names
and a made-up local MAC mapping:

```
$ unifictl clients list --self
ID                NAME         IP              STATUS  CONNECTION  EXPERIENCE
nas-1-0002        NAS-1        192.168.10.20   online  wired       -
macbook-pro-0001  MacBook Pro  192.168.10.67   online  wireless    96%

$ unifictl clients list --self --wide
ID                NAME         ...  UPLINK       SSID     SIGNAL   LINK   ...
nas-1-0002        NAS-1        ...  US 8 60W #3  -        -        1 GbE  ...
macbook-pro-0001  MacBook Pro  ...  UAP-01       HomeNet  -58 dBm  -      ...

$ unifictl clients list --self      # off-site / VPN only
ID  NAME  IP  STATUS  CONNECTION  EXPERIENCE
$ echo $?
0
```

## Decisions

| Question | Decision |
|---|---|
| Where does it live? | A `--self` flag on `clients list`. No `clients get self`, no new subcommand, no top-level `whoami`. Chosen over those because a machine can be two clients at once, and `get` returns one. |
| Several matches (wired + Wi-Fi)? | Show all of them. |
| Order of matches | Wired rows before wireless; otherwise the list's existing order. *(Claude's call.)* |
| No match (off-site, VPN only, unknown interface) | Empty list, exit 0, like any other filter that matches nothing. No error, no stderr note. |
| Local interfaces can't be read (no `/sys/class/net`, `getifaddrs` fails, sandbox) | An error: non-zero exit and a clear message such as `cannot read local network interfaces: <cause>`, through the existing `src/error.rs` path. "Couldn't look" must stay distinguishable from "looked, no match". Reading successfully but finding zero usable interfaces is **not** an error; it is the no-match case. |
| Combining with other filters | `--self` is one more filter; it ANDs with `--status`, `--connection`, `--device`, `--ssid`, and works with `--all` and `--wide`. The default "online only" still applies. *(Claude's call.)* |
| Output formats | Unchanged: table, `--wide`, `-o json`, `-o ids` all behave as for an unfiltered list. |
| `clients get` | Unchanged. The issue's `get self` idea is dropped. |
| Shell completion | Nothing to add; clap completes the flag. |
| Marking your row in the normal list (`*`) | Out of scope. |

## Matching this machine

From the issue, kept as written:

- Match on **MAC address**, never IP. VPN, Docker and bridge interfaces add
  addresses, and IPs move between leases.
- Read each interface's **current** MAC, not its permanent one, so
  NetworkManager's per-SSID MAC randomisation still matches.
- Consider only interfaces that are **up** and have a **link-layer
  (Ethernet-style, 6-byte) address**. Skip by name: `lo`, `docker*`, `br-*`,
  `veth*`, `tailscale*`, `wg*`, `tun*`, `tap*`. Skip all-zero MACs.
- Compare the **full** MAC after normalising case and separators (both sides
  through the same normaliser; the `-` → `:` mapping is applied on the
  `--self` side only, leaving the shared `normalize_mac` unchanged). No suffix
  matching; see #47.
- Compare against the client's MAC as the controller reports it (the
  `NetworkClient.mac` field).

Platforms: unifictl ships for Linux and macOS (x86_64 and aarch64), so both
need a reader.

- **"Up" means the link is live, not just admin-enabled** *(Claude's call)*.
  An unplugged NIC is usually still admin-up, and the controller keeps a just-
  undocked wired client "online" for a few minutes; counting it would show a
  stale wired row first.
- **Linux:** `/sys/class/net/<if>/address` for the MAC. Up when `operstate` is
  `up`, or when it is `unknown` and `carrier` reads `1` (some drivers never
  report `up`).
- **macOS:** `getifaddrs`, taking `AF_LINK` entries with a 6-byte address and
  `IFF_UP`; the link is live when `SIOCGIFMEDIA` reports `IFM_ACTIVE` (what
  `ifconfig` shows as `status: active`), falling back to `IFF_RUNNING` when
  the interface has no media status. macOS keeps `IFF_RUNNING` set on an
  unplugged Ethernet port, so the flags alone are not enough. macOS virtual interfaces to skip as well:
  `lo0`, `utun*`, `awdl*`, `llw*`, `bridge*`, `gif*`, `stf*`, `anpi*`, `ap*`.
- Reading local interfaces needs no controller access and no new config.

## Testing

The local-interface reader must be swappable so tests do not depend on the
machine running them:

- Unit tests for the interface filter (names skipped, down interfaces skipped,
  zero MACs skipped) and MAC normalisation.
- Service-level tests that pass a fixed set of local MACs into the client
  filter and assert which fixture clients come back, including both a wired
  and a wireless match and the no-match case.
- A test where the stubbed reader fails, asserting the error and non-zero
  exit (so the reader returns a `Result`).
- A CLI test through the `fixtures` feature. Following the `UNIFICTL_NOW`
  pattern, fixture builds could read the local MACs from an env var (for
  example `UNIFICTL_SELF_MACS=68:ee:ee:00:00:02,3c:ee:ee:00:00:01`) instead
  of the real interfaces. *(Claude's call; the mechanism is the builder's.)*

All MACs, names and IPs in tests, docs and examples are fictional fixture
values, never the live controller's.

## Non-goals

- `clients get self` or any single-client "self" shortcut.
- A new subcommand or top-level command.
- Marking this machine's row in the unfiltered list.
- Exiting non-zero when nothing matches (only a failure to read interfaces errors).
- Searching or sorting in the widget (stays in the widget).
- Windows.

## Left to the builder

- Module layout for the local-interface reader, and whether to use `libc`
  directly on macOS or a small crate. Prefer no new dependency if `libc` (or
  std) is enough; justify any crate added.
- Exact flag help text, plus updating the README's `clients list` section and
  the CHANGELOG through the usual release flow.

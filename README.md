# unifictl

A command-line client for UniFi network controllers: devices, clients, ports,
VLANs, SSIDs, WANs, a topology tree and the system log, straight from the
controller with no server in between.

![Diagnosing a robot vacuum that keeps dropping off Wi-Fi: unifictl clients list, then clients get roborock-0c04 shows a weak -77 dBm signal, then logs --client roborock-0c04 shows it roaming between AP-Office and AP-Living-Room every few minutes](docs/demo/roaming.gif)

A misbehaving client in three commands: find it in `clients list`, check its
signal with `clients get`, then read its story from the controller's log with
`logs --client`. Here a robot vacuum sitting between two access points on a
weak 2.4 GHz signal ping-pongs from one to the other every few minutes.

## Install

Prebuilt binaries for macOS (Apple Silicon and Intel) and Linux (x86_64 and
arm64) are attached to every [GitHub release](https://github.com/bwilczynski/unifictl/releases).
Install them with [mise](https://mise.jdx.dev); the command is the same on both
platforms:

```sh
mise use -g github:bwilczynski/unifictl    # latest release
unifictl --version
```

mise picks the archive matching your OS and CPU and puts `unifictl` on your
`PATH`; append `@<version>` to pin a release, and `mise upgrade unifictl` moves
to a newer one. Nothing else is needed on macOS. On Linux the binary needs
glibc 2.35 or newer (Arch, Ubuntu 22.04+, Debian 12+, Fedora 36+).

### From source

```sh
mise install
cargo install --path .
```

## Configure

Run the guided setup; it asks for the controller address, an API key (or
username and password) and the site, writes the config file readable only by
you, and checks that everything works:

```
unifictl config init
```

For scripts, pass everything as flags (`--host`, `--api-key` or
`--username`/`--password`, `--site`, `--insecure-tls`); secrets can come from
`UNIFI_API_KEY` / `UNIFI_PASSWORD` instead, which keeps them out of shell
history. `--force` overwrites an existing file; `--no-check` skips contacting
the controller.

When something stops working, `unifictl config check` walks through the
config, file permissions, DNS, TCP, TLS, authentication and site, and says
which step fails and what to change. It exits non-zero on failure.

The file lives at `~/.config/unifictl/config.toml` (or
`$XDG_CONFIG_HOME/unifictl/config.toml`, or `--config PATH`, or
`UNIFICTL_CONFIG`). Every key:

```toml
host = "192.168.1.1"      # host[:port]; always https
api_key = "…"             # UniFi OS (UCG, UDM, …): Settings → Control Plane → Integrations
# username = "admin"      # self-hosted Network application instead of api_key
# password = "…"
# site = "default"
# insecure_tls = true     # UniFi gateways ship self-signed certificates
# client_history_days = 30
# cache_ttl = 15          # seconds to reuse controller responses; 0 disables
```

Every key can be overridden by an environment variable:
`UNIFI_HOST`, `UNIFI_API_KEY`, `UNIFI_USERNAME`, `UNIFI_PASSWORD`,
`UNIFI_SITE`, `UNIFI_INSECURE_TLS`, `UNIFI_CLIENT_HISTORY_DAYS`,
`UNIFI_CACHE_TTL`.

Controller responses are cached for `cache_ttl` seconds (15 by default) in
`$XDG_CACHE_HOME/unifictl` (or `~/.cache/unifictl`), readable only by you,
so a `list` followed by a few `get`s asks the controller once. Pass
`--refresh` to any command to skip the cache.

## Use

```
unifictl status
unifictl devices list [--wide] | get <id> [--all-ports]
unifictl clients list [--status online|offline] [--wide] | get <id> | traffic [<id> | --by-app] [-S WHEN] [-U WHEN]
unifictl topology [--include-clients] [--include-wireless]
unifictl ports list [--device ID] [--mode trunk|access] [--state up|down|disabled | --all-ports] [--vlan N] [--wide]
unifictl vlans|ssids|wans list [--wide] | get <id>
unifictl wans get <id> [-S WHEN]
unifictl logs [-n N] [-S WHEN] [-U WHEN] [-p LEVEL] [-r] [-f] [--category CAT] [--device ID] [--client ID]
```

Every command takes `-o json` and `--refresh`. `devices`, `clients`, `vlans`, `ssids` and `wans list`,
and `clients traffic`, also take `-o ids`, printing one ID per line for scripts (`unifictl clients list --status offline -o ids | xargs -n1 unifictl clients get`).
IDs are kebab-case names (`us-8-60w`); clients add the last two MAC bytes (`nas-1-0002`).

`devices list` and `clients list` show an EXPERIENCE column (UniFi's score)
for spotting trouble quickly; `wans list` shows each WAN's own uptime and its
availability over the last 24 hours. `--wide` on any list adds more columns,
such as MAC, uplink, signal, link speed, live traffic and WAN latency.

`wans get` also lists the WAN's outages over the last 7 days, with start, end
and duration, and the controller's availability for that window; `--since`
widens or narrows it (`--since -30d`). The window shown is the one the
controller covered, which starts on a bucket boundary and goes back at most as
far as its history.

On a terminal, status cells are coloured (green healthy, red disconnected, dim
offline/down, yellow disabled), health metrics are green, yellow or red
(experience, Wi-Fi signal, link speed against what the link allows, and WAN
availability and latency),
download is cyan and upload magenta, and names are blue, in your terminal's own
palette. `--color always|never` overrides that; `NO_COLOR` turns it off.

Every device, then zooming in on a switch: its port table with PoE draw,
per-port traffic and who's on the other end of each cable, and its VLAN
config. Then the whole network as a tree, with the wireless clients under the
access point each one is on.

![unifictl devices list, then unifictl devices get switch-core --all-ports, then unifictl topology --include-wireless](docs/demo/devices.gif)

### Status

`unifictl status` answers "is anything wrong?" on one screen: a line per
WAN, then devices, Wi-Fi, wired, alarms and firmware updates, each with a
verdict, and the culprits named below the line:

```sh
$ unifictl status
WAN      ok     Internet 1  up 3d 4h 0m 0s  99.98% over 3d 4h  11 ms
WAN      idle   Internet 2  not connected since boot
DEVICES  error  8 of 9 online
                offline: ap-garage
WI-FI    warn   4 APs, 38 clients
                poor experience: printer-0b02 (38), roborock-0c04 (41)
WIRED    ok     3 switches, 41 clients
ALARMS   ok     no high-severity events in 24h
UPDATES  info   ap-office 7.1.66 → 7.1.68
```

Verdicts combine unifictl's own checks (the same thresholds as the list
colours) with the controller's health report. A WAN that has been down since
the gateway started, such as an unplugged backup, is `idle` rather than an
error. Alarms are the high-severity system log entries of the last 24 hours
(`unifictl logs -p high -S -24h` lists them all). The command exits 1 when a
row is `error`. A row whose data cannot be read says so and shows `unknown`
without failing the command.

### Traffic

`unifictl clients traffic` shows how much each client used the internet over
the last 24 hours, largest first, with the application that took the most.
Give a client ID for its traffic by application, or `--by-app` for the whole
site:

```sh
$ unifictl clients traffic macbook-pro-0001 --since today
APP                 CATEGORY                  DOWN      UP       TOTAL     ACTIVE
Example Video       Media streaming services  2.0 GB    50.0 MB  2.0 GB    1h 30m 0s
Facebook Messenger  Instant messengers        100.0 MB  20.0 MB  120.0 MB  30m 0s
```

`-S`/`--since` and `-U`/`--until` take the same forms as `logs`. Application
names come from the controller's DPI catalogue, which UniFi OS serves only to
an API key; with a username and password they show as `Unknown (category:app)`.

### Logs

`unifictl logs` reads the controller's system log the way `journalctl` reads
the journal: the 50 most recent entries, oldest first, one per line.

```sh
$ unifictl logs -p high --since yesterday
Oct 10 23:33:32 security: A network intrusion attempt from 192.168.10.67 to 198.51.100.7 has been detected.
Oct 11 08:12:40 internet-and-wan: Internet connection WAN1 ExampleISP on port 5 is down.
```

- `-n N` shows the last N entries; `-S`/`--since` and `-U`/`--until` take
  `now`, `today`, `yesterday`, `-2h`, `"2h ago"` or `YYYY-MM-DD [HH:MM[:SS]]`
  in local time, and without `-n` show the whole window.
- `-p low|medium|high|very-high` keeps that severity and above; on a terminal
  the message is coloured by severity.
- `--category` (`security`, `client-devices`, `internet-and-wan`, `audit`, …),
  `--device ID` and `--client ID` narrow it down. Repeating a flag widens the
  match; different flags narrow it, so `--client iphone-0c01 --device ap-office`
  shows that phone's roams to and from AP-Office. IDs complete on TAB.
- `-r` puts the newest first; `-f` keeps running and prints new entries as they
  arrive (checking every 5 seconds).
- `-o json` prints one JSON object per line, with the device and client IDs
  each entry mentions, so `jq` can feed them back into `devices get` or
  `clients get`.

## Shell completions

`unifictl completions <bash|zsh|fish|powershell|elvish>` prints a script that
completes subcommands, flags and IDs: `devices get <TAB>`, `clients get
<TAB>`, `vlans|ssids|wans get <TAB>`, `ports list --device <TAB>` and `logs
--device|--client <TAB>`. Load it
from your shell's startup file:

```sh
source <(unifictl completions bash)              # ~/.bashrc
source <(unifictl completions zsh)               # ~/.zshrc, after compinit
unifictl completions fish | source               # ~/.config/fish/config.fish
unifictl completions powershell | Out-String | Invoke-Expression   # $PROFILE
eval (unifictl completions elvish | slurp)       # ~/.config/elvish/rc.elv
```

The script calls back into `unifictl` on every TAB, so it keeps up with new
versions without being regenerated. IDs come from the controller through the
response cache, so only the first TAB after `cache_ttl` waits on it, for at
most 3 seconds. Completion reads the config from `UNIFICTL_CONFIG` or the
default path (not `--config`) and completes no IDs if the controller cannot be
reached.

## Relationship to hlctl

unifictl started as a port of `hlctl network …`
([bwilczynski/homelab-cli](https://github.com/bwilczynski/homelab-cli)) and the
network domain of [bwilczynski/homelab-api](https://github.com/bwilczynski/homelab-api),
with its output model taken from
[bwilczynski/homelab-api-spec](https://github.com/bwilczynski/homelab-api-spec).
It has since diverged. Among other differences, IDs have no controller prefix,
client IDs end in the last two MAC bytes rather than the first (which same-named
clients from one vendor share), and there is no `uri` field.

## Develop

```sh
mise run test   # unit and CLI tests
mise run lint   # clippy + rustfmt
```

The images in this README are recorded with [VHS](https://github.com/charmbracelet/vhs)
from the tapes in `docs/demo`, against a made-up network in
`docs/demo/fixtures` (no controller needed). `mise run demo` re-renders
them; it needs `ffmpeg` on your `PATH`.

## Release

Releases are cut from [conventional commits](https://www.conventionalcommits.org)
(PR titles and commits are checked by commitlint):

1. On every push to `main`, [release-plz](https://release-plz.dev) opens or
   updates a release PR that bumps the version in `Cargo.toml` and adds the
   changes to `CHANGELOG.md`.
2. Merging that PR tags `vX.Y.Z`.
3. The tag triggers [dist](https://opensource.axo.dev/cargo-dist/)
   (`.github/workflows/release.yml`), which builds each target on a native
   runner and publishes the archives and checksums as a GitHub release.

release-plz needs a `RELEASE_PLZ_TOKEN` repository secret: a fine-grained PAT
with *Contents* and *Pull requests* read/write on this repository. A tag
pushed with the default `GITHUB_TOKEN` would not trigger the release workflow.
`release.yml` is generated: change `dist-workspace.toml` and run
`dist generate` (dist is pinned in `mise.toml`).

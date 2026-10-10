# unifictl

A command-line client for UniFi network controllers: devices, clients, ports,
VLANs, SSIDs, WANs and a topology tree, straight from the controller with no
server in between.

```sh
$ unifictl topology --include-wireless
UCG-Fiber (gateway)
└── Switch-Core (switch) [port 1, 10 GbE]
    ├── NAS (client, wired, online) [port 1, 10 GbE]
    └── AP-Office (accessPoint) [3 clients] [port 3, 2.5 GbE]
        └── MacBook-Pro (client, wireless, online) [HomeWiFi, -48 dBm]
```

## Install

Prebuilt binaries for macOS (Apple Silicon and Intel) and Linux (x86_64 and
arm64) are attached to every [GitHub release](https://github.com/bwilczynski/unifictl/releases).
Install them with [mise](https://mise.jdx.dev); the command is the same on both
platforms:

```sh
mise use -g github:bwilczynski/unifictl          # latest release
mise use -g github:bwilczynski/unifictl@0.1.0    # or pin a version
unifictl --version
```

mise picks the archive matching your OS and CPU and puts `unifictl` on your
`PATH`; `mise upgrade unifictl` moves to a newer release. Nothing else is
needed on macOS. On Linux the binary needs glibc 2.35 or newer (Arch,
Ubuntu 22.04+, Debian 12+, Fedora 36+).

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
unifictl devices list | get <id> [--all-ports]
unifictl clients list [--status online|offline] | get <id>
unifictl topology [--include-clients] [--include-wireless]
unifictl ports list [--device ID] [--mode trunk|access] [--state up|down|disabled | --all-ports] [--vlan N] [--wide]
unifictl vlans|ssids|wans list | get <id>
```

Every command takes `-o json` and `--refresh`. `devices`, `clients`, `vlans`, `ssids` and `wans list`
also take `-o ids`, printing one ID per line for scripts (`unifictl clients list --status offline -o ids | xargs -n1 unifictl clients get`).
IDs are kebab-case names (`us-8-60w`); clients add the last two MAC bytes (`nas-1-0002`).

On a terminal, status cells are coloured (green healthy, red disconnected, dim
offline/down, yellow disabled) and names are blue, in your terminal's own palette. `--color
always|never` overrides that; `NO_COLOR` turns it off.

## Shell completions

`unifictl completions <bash|zsh|fish|powershell|elvish>` prints a script that
completes subcommands, flags and IDs: `devices get <TAB>`, `clients get
<TAB>`, `vlans|ssids|wans get <TAB>` and `ports list --device <TAB>`. Load it
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

unifictl is a port of `hlctl network …`
([bwilczynski/homelab-cli](https://github.com/bwilczynski/homelab-cli)) and the
network domain of [bwilczynski/homelab-api](https://github.com/bwilczynski/homelab-api),
with its output model taken from
[bwilczynski/homelab-api-spec](https://github.com/bwilczynski/homelab-api-spec).
Output is functionally equivalent, except that IDs have no controller prefix,
client IDs end in the last two MAC bytes rather than the first (which same-named
clients from one vendor share), and there is no `uri` field.

`mise run test` checks that equivalence against golden files in `tests/golden`.
`mise run golden` regenerates them from the Go code. It needs local clones of
homelab-api and homelab-cli: by default it looks for them next to this
repository, or you can point `HOMELAB_API_DIR` and `HOMELAB_CLI_DIR` at them.

## Develop

```sh
mise run test   # unit, CLI and golden tests
mise run lint   # clippy + rustfmt
```

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

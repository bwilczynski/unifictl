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

Requires [mise](https://mise.jdx.dev).

```sh
mise install
cargo install --path .
```

## Configure

`~/.config/unifictl/config.toml` (or `$XDG_CONFIG_HOME/unifictl/config.toml`,
or `--config PATH`, or `UNIFICTL_CONFIG`):

```toml
host = "192.168.1.1"      # host[:port]; always https
api_key = "…"             # UniFi OS (UCG, UDM, …): Settings → Control Plane → Integrations
# username = "admin"      # self-hosted Network application instead of api_key
# password = "…"
# site = "default"
# insecure_tls = true     # UniFi gateways ship self-signed certificates
# client_history_days = 30
```

`chmod 600` the file. Every key can be overridden by an environment variable:
`UNIFI_HOST`, `UNIFI_API_KEY`, `UNIFI_USERNAME`, `UNIFI_PASSWORD`,
`UNIFI_SITE`, `UNIFI_INSECURE_TLS`, `UNIFI_CLIENT_HISTORY_DAYS`.

## Use

```
unifictl devices list | get <id> [--all-ports]
unifictl clients list [--status online|offline] | get <id>
unifictl topology [--include-clients] [--include-wireless]
unifictl ports list [--device ID] [--mode trunk|access] [--state up|down|disabled | --all-ports] [--vlan N] [--wide]
unifictl vlans|ssids|wans list | get <id>
```

Every command takes `-o json`. IDs are kebab-case names (`us-8-60w`); clients
add the first MAC byte (`nas-1-68`).

## Relationship to hlctl

unifictl is a port of `hlctl network …`
([bwilczynski/homelab-cli](https://github.com/bwilczynski/homelab-cli)) and the
network domain of [bwilczynski/homelab-api](https://github.com/bwilczynski/homelab-api),
with its output model taken from
[bwilczynski/homelab-api-spec](https://github.com/bwilczynski/homelab-api-spec).
Output is functionally equivalent, except that IDs have no controller prefix
and there is no `uri` field.

`mise run test` checks that equivalence against golden files in `tests/golden`.
`mise run golden` regenerates them from the Go code. It needs local clones of
homelab-api and homelab-cli: by default it looks for them next to this
repository, or you can point `HOMELAB_API_DIR` and `HOMELAB_CLI_DIR` at them.

## Develop

```sh
mise run test   # unit, CLI and golden tests
mise run lint   # clippy + rustfmt
```

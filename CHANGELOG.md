# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.3](https://github.com/bwilczynski/unifictl/compare/v0.3.2...v0.3.3) - 2026-10-14

### Added

- add WAN outage history to wans get

### Fixed

- show each WAN's own uptime in wans, plus availability and latency

## [0.3.2](https://github.com/bwilczynski/unifictl/compare/v0.3.1...v0.3.2) - 2026-10-13

### Fixed

- show client last-seen time in local time instead of UTC

## [0.3.1](https://github.com/bwilczynski/unifictl/compare/v0.3.0...v0.3.1) - 2026-10-11

### Added

- --wide on vlans, ssids and wans list; list items carry the detail fields
- clients list experience column and --wide
- devices list experience column and --wide
- grade link health and colour traffic directions
- add unifictl clients traffic

## [0.3.0](https://github.com/bwilczynski/unifictl/compare/v0.2.2...v0.3.0) - 2026-10-11

### Added

- [**breaking**] drop the version subcommand and tidy ports list --vlan ([#23](https://github.com/bwilczynski/unifictl/pull/23))

## [0.2.2](https://github.com/bwilczynski/unifictl/compare/v0.2.1...v0.2.2) - 2026-10-11

### Added

- add unifictl logs with journalctl-style flags and --follow
- render log entries as journalctl-style lines and JSON Lines
- query, filter and follow the system log
- parse journalctl-style --since and --until times
- read the controller's system log

## [0.2.1](https://github.com/bwilczynski/unifictl/compare/v0.2.0...v0.2.1) - 2026-10-10

### Added

- colour names blue, with bold device names in topology
- add -o ids to list commands for scripting
- colour status cells on a terminal, with --color auto|always|never

## [0.2.0](https://github.com/bwilczynski/unifictl/compare/v0.1.0...v0.2.0) - 2026-10-10

### Added

- add `config init` for guided first-run setup
- *(setup)* gather host, auth and site for `config init`
- add `config check` to diagnose setup step by step
- *(setup)* run setup checks step by step and render the report
- *(setup)* add DNS and TCP probes
- *(unifi)* list sites and classify connection failures
- *(config)* point a missing config at `unifictl config init`
- *(config)* write config files owner-only via temp file and rename
- shell completions, including device/client IDs ([#5](https://github.com/bwilczynski/unifictl/pull/5))
- cache controller responses for a few seconds

### Fixed

- *(setup)* honour --username over env API key, bracket IPv6 hosts, keep secrets out of parse errors
- [**breaking**] make client IDs unique by suffixing the last two MAC bytes

### Performance

- fetch independent controller endpoints in parallel

## [0.1.0](https://github.com/bwilczynski/unifictl/releases/tag/v0.1.0) - 2026-10-10

### Added

- add unifictl, a standalone CLI for UniFi network controllers

### Other

- add release pipeline with release-plz and dist ([#1](https://github.com/bwilczynski/unifictl/pull/1))

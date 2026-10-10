# Guided setup: `config init` and `config check` (design)

Date: 2026-10-10
Status: draft, awaiting review
Issue: [#6](https://github.com/bwilczynski/unifictl/issues/6)

## Goal

A first-time user goes from nothing to a working config without reading the
README, and anyone whose setup is broken gets a report naming the step that
failed (config, permissions, DNS, TCP, TLS, auth, site) instead of whichever
request happened to fail first.

Success looks like:

- `unifictl config init` on a fresh machine prompts for what it needs, writes a
  `0600` `config.toml` (creating the directory), and ends with a passing check.
- `unifictl config check` on a broken setup prints every step with a pass/fail
  mark, a hint for the failing one, and exits non-zero.
- Any command run with no config at all says to run `unifictl config init`.

## Decisions

| Question | Decision |
|---|---|
| Name of the check command | `config check` (grouped with `config init`), no top-level `doctor` |
| Prompting | Hand-rolled line prompts on stdin/stderr; `rpassword` for hidden secrets |
| Site selection in `init` | Connect, list the controller's sites, pick one; free-text fallback if that fails |
| Secrets for scripted setup | `--api-key` / `--password` flags, falling back to `UNIFI_API_KEY` / `UNIFI_PASSWORD` |

## CLI

```
unifictl config init [--host HOST] [--api-key KEY | --username USER [--password PASS]]
                     [--site SITE] [--insecure-tls] [--force] [--no-check]
unifictl config check
```

Both honour the global `--config PATH` / `UNIFICTL_CONFIG` for the file
location, resolved exactly as for every other command (`config::resolve_path`).
`config check` honours `-o json`; `config init` honours it for the final check
report. `--refresh` is accepted (it is global) and ignored: neither command uses
the response cache.

`--api-key` conflicts with `--username` / `--password` (clap `conflicts_with`).

## Behaviour

### Missing config error

`config::load` gains one case: the resolved file does not exist, was not named
explicitly, **and** no `UNIFI_HOST` is set. Instead of
`config: host is required (…)` it returns:

```
Error: config: no config file at ~/.config/unifictl/config.toml; run `unifictl config init`
```

(The path is the resolved one, printed as-is.) Env-only setups keep working, and
their validation messages are unchanged.

### `config check`

Runs these checks in order. Each produces one result:
`Pass`, `Warn`, `Fail` or `Skip`, a one-line detail, and an optional hint.
A `Fail` turns every later check that depends on it into `Skip` ("skipped:
<earlier step> failed").

| # | Step | Pass | Warn / Fail |
|---|---|---|---|
| 1 | config | path of the file in effect (or "none, env only"), then each `UNIFI_*` var that overrides a key, secrets masked | Fail: load error (bad TOML, unknown key, missing host/auth): the `Error::Config` text; hint `run unifictl config init` when no file exists |
| 2 | permissions | `0600` | Warn: group/other bits set, hint `chmod 600 <path>`. Skip when there is no file or on non-Unix |
| 3 | dns | `<host> → <addr>, <addr>` | Fail: resolution error |
| 4 | tcp | `connected to <addr>:<port>` (5 s timeout per address, first success wins) | Fail: refused / timed out, hint names the port |
| 5 | tls | `certificate valid` | Fail: certificate error with hint `set insecure_tls = true (UniFi gateways ship self-signed certificates)`. Warn instead of Fail when `insecure_tls` is already on: `certificate not verified (insecure_tls = true)` |
| 6 | auth | API key: `API key accepted`; session: `logged in as <user>` | Fail: 401/403 → `API key rejected` or `login failed (status N)`; for an API key against a 404 at `/proxy/network`, hint that the host may be a self-hosted Network application that needs username/password |
| 7 | site | `site "<name>" found (<desc>)` | Fail: `site "<name>" not found`, hint `available: default (Default), lab (Lab)` |

Dependencies: 2 needs a file; 3–7 need 1; each of 4–7 needs the step before
it. With `insecure_tls` off, 6–7 run only after 5 passes; with it on, 5 is a
Warn and they proceed.

Port: the port in `host` if present, else 443. A bracketed IPv6 host
(`[::1]:8443`) is handled; `normalize_host` already accepts it.

Secret masking: an API key shows its first 4 characters then `…` (or just `…`
when it is 8 characters or shorter); a password always shows `****`.

Exit code: 1 if any check is `Fail`, else 0. Warnings do not fail.

Table output (stdout):

```
✓ config       /home/u/.config/unifictl/config.toml; UNIFI_SITE overrides site
✓ permissions  0600
✓ dns          unifi.lan → 192.168.1.1
✓ tcp          connected to 192.168.1.1:443
✗ tls          certificate not trusted: UnknownIssuer
               hint: set insecure_tls = true (UniFi gateways ship self-signed certificates)
– auth         skipped: tls failed
– site         skipped: tls failed
```

JSON output: `{"ok": false, "checks": [{"step": "tls", "status": "fail",
"detail": "…", "hint": "…"}, …]}`. `hint` is omitted when absent.

A config that fails to load still produces the report (step 1 fails, rest
skip). It does not go through `app::main`'s `Error: …` path.

### `config init`

1. Resolve the path. If the file exists and `--force` was not given, fail with
   `config: <path> already exists; pass --force to overwrite` before prompting.
2. Collect values, in this precedence per field: flag, then (secrets only)
   `UNIFI_API_KEY` / `UNIFI_PASSWORD`, then prompt.
   - host: `Controller host (e.g. 192.168.1.1):`. Validated with
     `normalize_host`; re-prompt on error.
   - auth method: skipped if a flag/env decided it, otherwise
     `Auth method — [1] API key (UniFi OS: UCG, UDM, …)  [2] username/password [1]:`.
   - API key or username + password: secrets read hidden via `rpassword`.
3. Unless `--no-check`: probe TLS strictly. If it fails with a certificate
   error and `--insecure-tls` was not given, ask
   `Controller certificate is not trusted (<reason>). Accept it anyway (insecure_tls)? [y/N]`.
   Then fetch sites. If `--site` was given, keep it. Otherwise, with one
   site, use it and say so; with several, show a numbered list and default to
   `default`. If fetching fails, print the reason and prompt free-text
   `Site [default]:`.
   With `--no-check`, sites are not fetched and site falls back to free-text
   prompt / `--site` / `default`.
4. Write the file (see Writing). Print `wrote <path>` to stderr.
5. Unless `--no-check`, run `config check` against the written file and print
   its report to stdout. Exit with its code; the file is kept on failure.

Non-interactive: if stdin is not a TTY and a required value has no flag/env,
fail with `config: <field> is required when not running interactively (pass
--<flag>)` and write nothing. EOF or Ctrl-C during prompts writes nothing.

Prompts and progress go to stderr; stdout carries only the check report.

### Writing

`config::write(path, &FileConfig)`:

- creates the parent directory (`create_dir_all`, mode `0700` on Unix for a
  directory it creates);
- writes to a temp file in the same directory, created with mode `0600`
  (`OpenOptions::mode`), then renames over the target, so the file is never
  briefly world-readable and a crash leaves no partial config;
- serializes only the keys that were set: `host`, `api_key` **or**
  `username` + `password`, `site`, and `insecure_tls` only when true. Keys
  with defaults (`client_history_days`, `cache_ttl`) are left out.

`FileConfig` derives `Serialize` with `skip_serializing_if = "Option::is_none"`.

## Components

| Unit | Responsibility |
|---|---|
| `src/cli.rs` | `Command::Config { command: ConfigCommand }`, `ConfigCommand::{Init(InitArgs), Check}` |
| `src/config.rs` | missing-file error; `write`; `FileConfig: Serialize`; a `load_parts` split so `check` can report path, file and env overrides separately; `mask` helper |
| `src/unifi/client.rs` | `sites()` → `Vec<Site>` (`GET {prefix}/api/self/sites`, `data[].name`/`desc`); a probe entry point that returns a classified failure instead of a flat string |
| `src/unifi/types.rs` | `Site { name, desc }` |
| `src/setup/mod.rs` | module root |
| `src/setup/probe.rs` | DNS (`ToSocketAddrs`), TCP (`TcpStream::connect_timeout`), and `classify(&reqwest::Error) -> ProbeError { Dns, Connect, Timeout, Certificate(String), Other(String) }` by walking `source()` |
| `src/setup/check.rs` | `run(path, explicit, env) -> Report`; the step table and skip cascade |
| `src/setup/init.rs` | the `init` flow over a `Prompter` trait (`line`, `secret`, `confirm`, `choose`, `interactive`) |
| `src/setup/prompt.rs` | `TerminalPrompter` (stdin/stderr + `rpassword`, `IsTerminal` for `interactive`) |
| `src/render/check.rs` | table rendering of a `Report` |
| `src/app.rs` | dispatches `Command::Config` before `build_service`, like `Version`; maps a failing report to exit code 1 |

`app::run` returns `Result<String>` today; `config check` needs "print this and
exit 1". It is handled by a small return type (`Outcome { stdout, code }`) for
`run`, with existing commands always producing `code = 0`.

## Dependencies

- `rpassword` (hidden input). Nothing else; `IsTerminal` is in std.

## Testing

- `config.rs` unit tests: `write` creates the directory and sets `0600`;
  `write` → `load` round-trips; missing default file with no `UNIFI_HOST` gives
  the `config init` message; env-only config still loads.
- `setup::check` against `httpmock` (via a `with_base_url` seam so the HTTP
  steps run over plain HTTP): API key accepted / 401; session login 200 /
  400; site found; site missing lists available sites; skip cascade after a
  failure; DNS failure on an unresolvable `*.invalid` host; TCP refused on a
  port bound then dropped.
- `setup::probe::classify`: unit tests on synthetic `std::error::Error`
  chains for certificate / DNS / connect / timeout text. A real certificate
  failure is verified manually against a controller with a self-signed cert.
- `setup::init` with a scripted `Prompter`: flags only; env secret fallback;
  prompting for missing fields; refuse to overwrite without `--force`;
  non-interactive with a missing field is an error and writes nothing.
- `tests/cli.rs`: `config init --no-check --host h --api-key k` into a temp
  `XDG_CONFIG_HOME` writes a `0600` file with those keys; a second run without
  `--force` exits 1; `config check` with no config exits 1 and mentions
  `config init`; `devices list` with no config mentions `config init`.

## Docs

README "Configure" leads with `unifictl config init` and `unifictl config
check`; the hand-written TOML example stays as the reference for all keys.

## Out of scope

- `config show` / `config path`, editing one key of an existing config.
- A top-level `doctor` alias.
- Checking that the API key has sufficient permissions beyond listing sites.

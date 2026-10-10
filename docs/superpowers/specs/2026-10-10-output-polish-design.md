# Output polish: colour and `-o ids` (design)

Date: 2026-10-10
Status: draft, awaiting review
Issue: [#7](https://github.com/bwilczynski/unifictl/issues/7)
(`--wide` on every list moved to [#14](https://github.com/bwilczynski/unifictl/issues/14))

## Goal

Table output that is quicker to scan on a terminal, and list output that pipes
into other commands without `jq`.

Success looks like:

- On a terminal, status cells stand out at a glance (green healthy, red broken,
  dim for normal-but-inactive, yellow disabled), in the terminal's own palette.
- `unifictl clients list --status offline -o ids | xargs -n1 unifictl clients get`
  works.
- Golden tests stay colourless.

## Decisions

| Question | Decision |
|---|---|
| IDs-only spelling | `-o ids`, a third value of the global `--output` |
| Colour control | Global `--color auto\|always\|never`, default `auto` |
| Colour codes | Basic 16-colour ANSI SGR only, so the terminal theme (e.g. Catppuccin) supplies the actual colours |

## CLI

```
unifictl [--color auto|always|never] [-o table|json|ids] ...
```

`--color` is global, like `--output`.

## Colour

### Resolution

`--color always` → on; `never` → off; `auto` → on only when stdout is a
terminal (`std::io::IsTerminal`) **and** `NO_COLOR` is unset or empty (per
no-color.org, any non-empty value disables). Resolved once in `app::run` by a
pure function taking the mode, an `is_tty` bool and the env lookup, so it is
unit-testable. `-o json` and `-o ids` never colour.

### Styling

New `src/render/style.rs`:

- `Paint` enum: `Green` (`32`), `Red` (`31`), `Yellow` (`33`), `Dim` (`2`).
- `fn paint(text: &str, p: Paint, on: bool) -> String`: wraps in
  `\x1b[<n>m … \x1b[0m` when `on`, returns `text` unchanged otherwise.
- One mapping function per status type, so the same word can differ by context:

| Value | Paint | Reason |
|---|---|---|
| device / WAN `connected`, client `online`, port `up` | Green | healthy |
| device / WAN `disconnected` | Red | a real problem |
| client `offline`, port `down` | Dim | normal state; red would be noise |
| port `disabled` | Yellow | deliberately off |
| `config check` ✓ / ! / ✗ / – | Green / Yellow / Red / Dim | |

`render::Table` builds its `TabWriter` with `.ansi(true)` so escape sequences do
not count toward column widths (the `ansi_formatting` crate feature is
deprecated in favour of this builder method).

### Where colour applies

- `devices list` / `devices get`: STATUS; the PORTS section's STATE column.
- `clients list` / `clients get`: STATUS.
- `ports list`: STATE.
- `wans list` / `wans get`: STATUS.
- `topology`: the client status word, e.g. `host-02 (client, wired, offline)`
  (device nodes carry no status).
- `config check`: the mark column.

Only stdout is coloured; `Error: …` and warnings on stderr stay plain.

### Threading

Renderers that colour gain a `color: bool` parameter, e.g.
`devices::list(list, color)`, `devices::get(detail, all_ports, color)`,
`ports::list_table(list, wide, color)`. Renderers with no status cells (vlans,
ssids) are unchanged.

## `-o ids`

`OutputFormat::Ids` prints each item's `id`, one per line; an empty list prints
nothing (no header). Supported on `devices`, `clients`, `vlans`, `ssids` and
`wans list`, implemented with a small trait over the list types:

```rust
pub trait Ids { fn ids(&self) -> Vec<&str>; }
```

Every other command that renders output (`ports list`, which has no per-port
ID; every `get`; `topology`; `config check`) returns
`Error: -o ids is only supported by devices, clients, vlans, ssids and wans list` with exit code 1. `version`
and `completions` ignore `--output`, as today.

## Testing

Unit tests:

- `style`: each mapping, and that `on = false` returns the text unchanged.
- Colour resolution: `always`/`never`, `auto` with and without a TTY, `NO_COLOR`
  empty vs. set.
- Renderers: coloured output still aligns (visible widths equal to the
  uncoloured output).

Integration tests (`tests/cli.rs`, against the fixtures). The golden cases in
`tests/golden/` are generated from the Go implementation by
`tools/golden/generate.sh`, so new behaviour is not added there.

- `-o ids` for the five ID lists, an empty list, IDs fed back into `get`, and
  the error for `ports list`, `get`, `topology` and `config check`.
- `--color always` for each coloured renderer: escape codes present, and the
  output with codes stripped equals the uncoloured output.
- `--color always -o json` has no escape codes.

Existing goldens are unchanged: the harness captures stdout through a pipe, so
`auto` resolves to off.

## Out of scope

- `--wide` on every list command ([#14](https://github.com/bwilczynski/unifictl/issues/14)).
- Configurable colours or a colour theme in `config.toml`.
- Colouring anything other than status cells (headers, IDs, traffic).
- A port ID for `ports list -o ids`.

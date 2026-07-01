# Configuration

tock reads an optional TOML configuration file at startup. It lets you set
defaults once — urgency weights, focus timers, TUI keys, user-defined
attributes, and more — instead of passing flags every time.

## File location

The file is resolved in this order (first match wins):

1. `--config <path>` on the command line
2. the `TOCK_CONFIG` environment variable
3. `$XDG_CONFIG_HOME/tock/config.toml`
4. `~/.config/tock/config.toml`

If no file exists, tock runs entirely on built-in defaults — the config file is
never required.

Print the resolved path any time with:

```sh
tock config path
```

## Precedence

Settings are layered from lowest to highest priority:

```text
built-in defaults  →  config file  →  environment variables  →  CLI flags
```

So a flag always wins over the file, and the file always wins over the
built-in defaults. Every section and key in the file is optional; anything you
omit falls back to the default.

## Getting started

Write a fully documented sample file to the resolved location:

```sh
tock config init          # writes the sample if none exists
tock config init --force  # overwrite an existing file
```

Then inspect the **effective** (merged) configuration — defaults plus whatever
your file overrides:

```sh
tock config show          # rendered as TOML
tock config show --json   # same data as JSON
```

## Sections

The sample written by `tock config init` documents every key. The sections that
change behavior today are:

### `[general]`

```toml
[general]
default_view = "today"   # the view `tock` shows when run with no arguments
```

Running a bare `tock` prints this built-in view (e.g. `today`, `inbox`,
`upcoming`, `anytime`, `someday`, `logbook`).

### `[urgency]`

Overrides the urgency-score coefficients. These weights are threaded through the
storage layer, so a task's cached urgency is recomputed against your values on
its next change.

```toml
[urgency]
deadline   = 1.0
start_date = 0.8
priority   = 0.6
age        = 0.5
tag        = 0.4   # general weight applied per tag on a task
project    = 0.4
next       = 0.7
blocked    = -5.0
waiting    = -2.0

# Additive per-tag boosts, applied on top of the general `tag` weight:
[urgency.tags]
urgent  = 4.0
someday = -3.0
```

Inspect a single task's breakdown (now reflecting your weights) with
`tock urgency <sid>`.

### `[focus]`

Default Pomodoro timings for `tock focus start`. Durations accept an integer
number of minutes or a duration string (`"25m"`, `"1h"`, `"90s"`). Explicit
`tock focus start` flags (`--work`, `--short-break`, `--long-break`,
`--cycles`) still override these.

```toml
[focus]
length      = "25m"
short_break = "5m"
long_break  = "15m"
cycles      = 4
```

### `[uda.<key>]`

Declare user-defined attributes in config. These merge with the attributes
stored in the vault database and appear in `tock uda list`; on a key conflict,
the config declaration wins.

```toml
[uda.effort]
type  = "int"
label = "Effort (1-5)"

[uda.energy]
type   = "enum"
values = ["low", "medium", "high"]
```

Supported `type` values: `string`, `int`/`number`, `float`, `bool`, `enum`,
`duration`, `date`, `task_ref`.

### `[notifications]`

```toml
[notifications]
enabled = true   # set to false to silence all notifications
```

Turning this off suppresses the focus/timer notifications globally.

### `[keys]`

Remap the interactive TUI (`tock tui`). Each value is a single character or a
named key: `tab`, `backtab`, `enter`, `esc`, `up`, `down`, `left`, `right`,
`space`, `home`, `end`. Unset actions keep their defaults, and the built-in
navigation fallbacks (arrow keys, number keys `1`/`2`/`3`, `Ctrl-C` to quit)
always remain available.

```toml
[keys]
quit      = "q"
next_pane = "tab"
prev_pane = "backtab"
down      = "j"
up        = "k"
top       = "g"
bottom    = "G"
activate  = "enter"
complete  = "d"
delete    = "x"
reload    = "r"
help      = "?"
```

## Recognized but not yet active

To mirror the full schema in the architecture document, the config model also
parses these sections and shows them in `tock config show`, but they do **not**
change behavior yet: `[locale]`, `[tasks]`, `[habits]`, `[time]`, `[contexts]`,
`[reports.<name>]`, and `[hooks]`. They are reserved so the file format is
stable as those wirings land. Setting them is harmless — they are simply
ignored for now.

## Example

```toml
[general]
default_view = "inbox"

[urgency]
deadline = 2.0

[urgency.tags]
urgent = 5.0

[focus]
length = "50m"
short_break = "10m"

[notifications]
enabled = false

[keys]
complete = "c"
```

Verify the merge took effect:

```sh
tock config show
```

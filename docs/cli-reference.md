# CLI reference

!!! note "Generated"

    This page is generated from the CLI's own `--help` by
    `s/docs-cli-reference`, so it cannot drift from the real command
    surface. Do not edit it by hand. If it disagrees with your build,
    trust your build.

## Global options

Accepted by every command.

```text
Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

`--format json` changes only the **result**. Progress, warnings, and
errors always go to stderr as plain text, so piping stdout into `jq` is
safe even for commands that print several progress lines.

## Commands

### `cortex session`

Hold a persistent connection to the device, serving other commands [alias: connect].

```text
Hold a persistent connection to the device, serving other commands.

The device grants its USB interface exclusively, so exactly one process can own it. This is that process: it performs the handshake ONCE and every other command then talks to it over a socket instead of connecting for itself.

That matters for more than speed. A held session SUBSCRIBES to device state, which is how the unit reports edits you make on the hardware - so what `cortex` reports can stay true while you play, rather than being a snapshot from whenever the last command ran.

Runs in the foreground. Stop it with Ctrl-C or `--stop`. Report whether a connection is running, and whether the device is answering. Ask a running connection to shut down, announcing the disconnect to the device first.

Usage: cortex session [OPTIONS] <COMMAND>

Commands:
  start   Open the session and serve other commands. Runs in the foreground
  status  Report whether a session is running, and whether the device answers
  stop    Ask a running session to shut down, announcing the disconnect first
  help    Print this message or the help of the given subcommand(s)

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex session start`

Open the session and serve other commands. Runs in the foreground.

```text
Open the session and serve other commands. Runs in the foreground

Usage: cortex session start [OPTIONS]

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex session start
  cortex session start   # foreground; append & to background it
```

#### `cortex session status`

Report whether a session is running, and whether the device answers.

```text
Report whether a session is running, and whether the device answers

Usage: cortex session status [OPTIONS]

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex session status
```

#### `cortex session stop`

Ask a running session to shut down, announcing the disconnect first.

```text
Ask a running session to shut down, announcing the disconnect first

Usage: cortex session stop [OPTIONS]

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex session stop
```

### `cortex preset`

Presets: list a setlist, show one, or load one onto the unit.

```text
Presets: list a setlist, show one, or load one onto the unit

Usage: cortex preset [OPTIONS] <COMMAND>

Commands:
  list    List the presets in a setlist, in slot order
  show    Recall a slot and dump the preset it loads
  recall  Recall a preset by slot, making it the one loaded on the grid
  help    Print this message or the help of the given subcommand(s)

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex preset list`

List the presets in a setlist, in slot order.

```text
List the presets in a setlist, in slot order.

Read-only: this does NOT change what is loaded on the grid.

Usage: cortex preset list [OPTIONS]

Options:
      --setlist <PATH>
          Absolute device path of the setlist, e.g. `/media/p4/Presets/My Presets`. Run `cortex setlist list` to list them
          
          [default: "/media/p4/Presets/My Presets"]

      --include-empty
          Include empty slots, so a free slot can be found

      --timeout <SECONDS>
          Seconds to wait for the listing. Delivery is lazy; a timeout means "ask again", not "the setlist is empty"
          
          [default: 25]

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex preset list
  cortex preset list --include-empty
```

#### `cortex preset show`

Recall a slot and dump the preset it loads.

```text
Recall a slot and dump the preset it loads.

CHANGES WHAT IS HEARD: there is no side-effect-free way to read a STORED preset - the device only emits a preset when it recalls one. Use `cortex probe` if you want the live grid without recalling.

Usage: cortex preset show [OPTIONS] --slot <BANK+LETTER>

Options:
      --slot <BANK+LETTER>
          Slot name: bank number then letter, e.g. `1A`, `28C`. Bank is 1-32, letter A-H

      --setlist <PATH>
          Absolute device path of the setlist. `cortex setlist list` lists them
          
          [default: "/media/p4/Presets/My Presets"]

      --factory
          Mark the setlist as the read-only factory library

      --params
          Also show each block's stored parameter values

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex preset show --slot 1B
  cortex preset show --slot 1B --params
```

#### `cortex preset recall`

Recall a preset by slot, making it the one loaded on the grid.

```text
Recall a preset by slot, making it the one loaded on the grid.

CHANGES WHAT IS HEARD. Nothing is saved and no stored preset is modified, but the grid is replaced and any unsaved edits are lost.

Usage: cortex preset recall [OPTIONS] --slot <BANK+LETTER>

Options:
      --slot <BANK+LETTER>
          Slot name: bank number then letter, e.g. `1A`, `12H`, `28C`. Bank is 1-32 and letter is A-H, giving 256 slots per setlist

      --setlist <PATH>
          Absolute device path of the setlist, e.g. `/media/p4/Presets/My Presets`. Run `cortex setlist list` to list them
          
          [default: "/media/p4/Presets/My Presets"]

      --factory
          Mark the setlist as the read-only factory library. Needed for paths under /opt/neuraldsp/Factory Library

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex preset recall --slot 2B
```

### `cortex setlist`

Setlists: the folders of presets the unit holds.

```text
Setlists: the folders of presets the unit holds

Usage: cortex setlist [OPTIONS] <COMMAND>

Commands:
  list  List every folder the device knows: setlists, captures, IR libraries
  help  Print this message or the help of the given subcommand(s)

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex setlist list`

List every folder the device knows: setlists, captures, IR libraries.

```text
List every folder the device knows: setlists, captures, IR libraries.

A single `File` READ makes the device enumerate all its folders, which arrive over ten to twenty seconds. There is no total-count field on the wire, so this always waits the full window.

Read-only.

Usage: cortex setlist list [OPTIONS]

Options:
      --window <SECONDS>
          Seconds to gather folder announcements
          
          [default: 20]

      --show-empty
          Also list folders holding no presets.
          
          The unit reports hundreds of them, nearly all empty, which buries the two or three you actually use.

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex setlist list
  cortex setlist list --show-empty
```

### `cortex grid`

The signal grid that is loaded right now.

```text
The signal grid that is loaded right now

Usage: cortex grid [OPTIONS] <COMMAND>

Commands:
  show  Show the LIVE grid: what is loaded right now, unsaved edits included
  help  Print this message or the help of the given subcommand(s)

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex grid show`

Show the LIVE grid: what is loaded right now, unsaved edits included.

```text
Show the LIVE grid: what is loaded right now, unsaved edits included.

Read-only and side-effect free. This is the command to use while editing: `cortex preset --slot X` reads a STORED slot and can only do so by recalling it, which discards unsaved edits and resets the active scene.

Usage: cortex grid show [OPTIONS]

Options:
      --timeout <SECONDS>
          Seconds to wait for the grid
          
          [default: 15]

      --params
          Also show each block's stored parameter values, which is how to check an edit landed

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex grid show
  cortex grid show --params
```

### `cortex block`

One block in the grid: its parameters, bypass, model, or removal.

```text
One block in the grid: its parameters, bypass, model, or removal

Usage: cortex block [OPTIONS] <COMMAND>

Commands:
  param   Set a block parameter on the grid
  bypass  Bypass or enable a block on the grid
  set     Place a model in a grid cell, creating or replacing a block
  remove  Remove the block at a grid cell
  help    Print this message or the help of the given subcommand(s)

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex block param`

Set a block parameter on the grid.

```text
Set a block parameter on the grid.

CHANGES THE WORKING GRID. Nothing is saved: the edit lives on the grid until you save the preset (not yet implemented) or recall another, which discards it.

Rows are given as the unit LABELS them, 1-4, not the zero-based wire index. Use `cortex preset --slot <slot>` to see what is where.

Usage: cortex block param [OPTIONS] --row <1-4> --column <0-7>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --column <0-7>
          Grid column, 0-7, left to right

      --param <NAME>
          Parameter by NAME, e.g. `GAIN`. Resolved through the device catalog, so it needs --model or a block already in the cell. Safer than --index: indices are positional and not every one is a visible knob

      --index <N>
          Parameter by raw wire index. Prefer --param

      --value <0.0-1.0>
          Normalised value, 0.0-1.0, which is what the wire carries

      --real <N>
          Value in the parameter's OWN units (dB, ms, Hz). Converted using the catalog range, so it needs --param

      --text <TEXT>
          String value, for parameters that take one such as a cabinet's microphone selection

      --scene <0-7>
          Apply to this scene (0-7) rather than the active one. Sends three messages and LEAVES THE UNIT ON THAT SCENE

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex block param --row 1 --column 2 --param GAIN --real 7.5
  cortex block param --row 1 --column 2 --index 0 --value 0.75
  cortex block param --row 1 --column 2 --param GAIN --value 0.9 --scene 2
```

#### `cortex block bypass`

Bypass or enable a block on the grid.

```text
Bypass or enable a block on the grid.

CHANGES THE WORKING GRID. Nothing is saved.

Usage: cortex block bypass [OPTIONS] --row <1-4> --column <0-7>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --column <0-7>
          Grid column, 0-7

      --bypass
          Bypass the block. Omit to enable it

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex block bypass --row 1 --column 2 --bypass   # bypass it
  cortex block bypass --row 1 --column 2             # enable it
```

#### `cortex block set`

Place a model in a grid cell, creating or replacing a block.

```text
Place a model in a grid cell, creating or replacing a block.

CHANGES THE WORKING GRID. Nothing is saved.

Verifies the device accepted it: a placement refused for want of DSP capacity is accepted on the wire and simply absent afterwards, with no error, so this waits for the device's echo naming the cell.

Usage: cortex block set [OPTIONS] --row <1-4> --column <0-7> --model <ID>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --column <0-7>
          Grid column, 0-7

      --model <ID>
          Model by NUMERIC id. Find one with `cortex catalog --search`

      --no-verify
          Send without waiting for the echo. A DSP refusal then looks identical to success

      --timeout <SECONDS>
          Seconds to wait for the device's echo
          
          [default: 5]

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex block set --row 1 --column 2 --model 1001   # find ids with: cortex catalog --search
```

#### `cortex block remove`

Remove the block at a grid cell.

```text
Remove the block at a grid cell.

CHANGES THE WORKING GRID. Nothing is saved.

Usage: cortex block remove [OPTIONS] --row <1-4> --column <0-7>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --column <0-7>
          Grid column, 0-7

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex block remove --row 1 --column 2
```

### `cortex row`

A grid row: its input, its output, and where it splits.

```text
A grid row: its input, its output, and where it splits

Usage: cortex row [OPTIONS] <COMMAND>

Commands:
  input   Re-point a grid row's input
  output  Re-point a grid row's output
  split   Set a row's split and mix points, activating a parallel branch
  help    Print this message or the help of the given subcommand(s)

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex row input`

Re-point a grid row's input.

```text
Re-point a grid row's input.

CHANGES THE WORKING GRID. Nothing is saved.

Usage: cortex row input [OPTIONS] --row <1-4> --port <ID>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --port <ID>
          Input port id. 1 = Input 1, 2 = Input 2, 4 = Return 1, 5 = Return 2. Note the ids are NOT 1/2/3/4 - combined ports are interleaved

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex row input --row 1 --port 0
```

#### `cortex row output`

Re-point a grid row's output.

```text
Re-point a grid row's output.

CHANGES THE WORKING GRID. Nothing is saved.

Not every id is a physical destination: 16-18 are internal row-to-row routing, while 19 (MULTIPLE) is a real output. The device does not validate this field, so a meaningless id is stored rather than rejected and reads back cleanly.

Usage: cortex row output [OPTIONS] --row <1-4> --port <ID>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --port <ID>
          Output port id

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex row output --row 1 --port 0
```

#### `cortex row split`

Set a row's split and mix points, activating a parallel branch.

```text
Set a row's split and mix points, activating a parallel branch.

CHANGES THE WORKING GRID. Nothing is saved.

Only screen rows 1 and 3 can branch; their parallel lane is the row below. Rows 2 and 4 have no splitter and are refused.

Usage: cortex row split [OPTIONS] --row <1|3> --split <COL>

Options:
      --row <1|3>
          Grid row as shown on the unit. Must be 1 or 3

      --split <COL>
          Column at which the row branches. -1 clears the branch

      --mix <COL>
          Column at which the branch rejoins. -1 means it never does
          
          [default: -1]

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex row split --row 1 --split 3 --mix 6   # branch at 3, rejoin at 6
  cortex row split --row 1 --split -1 --mix -1  # clear the branch
```

### `cortex device`

The unit itself: firmware, DSP load, and a connection probe.

```text
The unit itself: firmware, DSP load, and a connection probe

Usage: cortex device [OPTIONS] <COMMAND>

Commands:
  version  Read the device firmware version (CorOS, app, bootloader, zencoder)
  cpu      Show the unit's live DSP load
  probe    Run the connect handshake and report the state the device pushes back
  help     Print this message or the help of the given subcommand(s)

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex device version`

Read the device firmware version (CorOS, app, bootloader, zencoder).

```text
Read the device firmware version (CorOS, app, bootloader, zencoder)

Usage: cortex device version [OPTIONS]

Options:
      --session
          Read via the session layer (background RX thread + correlated request) instead of the one-shot synchronous transport.
          
          Both paths are valid: a `Version` READ is answered without the connect handshake. This flag exists to exercise the session layer against hardware. Both paths are now verified; see spec/140-session/spec.md.

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex device version
  cortex device version --format json
```

#### `cortex device cpu`

Show the unit's live DSP load.

```text
Show the unit's live DSP load.

The device pushes this about once a second, but only to a client that has subscribed - so this needs a running `cortex session start`. A one-shot command uses a minimal handshake and never asks the device to push it.

Usage: cortex device cpu [OPTIONS]

Options:
      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex device cpu   # needs a running: cortex session start
```

#### `cortex device probe`

Run the connect handshake and report the state the device pushes back.

```text
Run the connect handshake and report the state the device pushes back.

This is the hardware smoke test for the session layer. It performs the full handshake, holds the session open for a window so device pushes can arrive, prints a tally of what came back, then disconnects.

Read-only: the handshake sends READs and a connect announcement. It never writes preset data and never saves.

Usage: cortex device probe [OPTIONS]

Options:
      --listen <SECONDS>
          Extra seconds to hold the session open after the handshake before reading. The handshake already waits for the device to go quiet, so 0 is usually fine; raise it if reads time out
          
          [default: 0]

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex device probe
```

### `cortex scene`

Switch the active scene.

```text
Switch the active scene.

CHANGES WHAT IS HEARD. Nothing is saved.

Usage: cortex scene [OPTIONS] --index <0-7>

Options:
      --index <0-7>
          Scene number, 0-7 ZERO-BASED: 0 is scene A and 7 is scene H. The unit labels them A-H, so scene C is `--index 2`

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex scene --index 2
```

### `cortex catalog`

Fetch the device model catalog and write the raw payload to a file.

```text
Fetch the device model catalog and write the raw payload to a file.

The catalog is what turns the integer model ids stored in a preset into names. It comes from the device, so it covers this unit's purchased plugins and the player's own Neural Captures.

Read-only.

Usage: cortex catalog [OPTIONS]

Options:
      --search <TEXT>
          Case-insensitive substring to match against a model's name AND the gear it is based on, e.g. `marshall`, `tape echo`

      --model <ID>
          Show one model in full by NUMERIC id, e.g. `--model 1001`. Use --search to find an id from a name

      --dump <FILE>
          File path to write the raw payload to, for inspection

      --from-file <FILE>
          Parse a payload previously saved with --dump, instead of asking the device. Needs no unit connected

      --timeout <SECONDS>
          Seconds to wait. The payload is ~47 KB over several hundred reports, so allow generously
          
          [default: 40]

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex catalog --search plexi
  cortex catalog --model 1001
  cortex catalog --dump repo.bin
```

### `cortex decode-trace`

Decode a USB capture into Cortex Control messages.

```text
Decode a USB capture into Cortex Control messages.

Reads `tshark` field output on standard input and prints one line per reassembled message, in the same shape as `CORTEX_TRACE`. Use `s/usb-decode`, which supplies the right `tshark` invocation.

Works on a capture of ANY client, which is the point: it is how a recording of Cortex Control gets read against our own schema.

Usage: cortex decode-trace [OPTIONS]

Options:
      --quiet
          Print only the summary line, not every message

      --verbose
          Also describe each message's protobuf fields.
          
          Generic, so it works on message types we do not model - which is the case that matters when reading a capture of another client.

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  s/usb-decode traces/capture.pcapng          # the usual way
  s/usb-decode --live                          # watch it happen
```

### `cortex completions`

Generate or install shell completions.

```text
Generate or install shell completions.

`cortex completions install` is the one to use: it detects your shell, writes the completion file to the standard location, and prints any one-time setup still needed. It never edits your shell startup files.

`cortex completions <shell>` prints the script to stdout instead, which is the stable interface for packagers and unusual setups.

Usage: cortex completions [OPTIONS] <TARGET>

Arguments:
  <TARGET>
          A shell to generate for, or `install` to install for your own

          Possible values:
          - install:    Detect the current shell and install into its standard directory
          - bash
          - elvish
          - fish
          - powershell
          - zsh

Options:
      --shell <SHELL>
          Override the shell detected from $SHELL when using `install`
          
          [possible values: bash, elvish, fish, powershell, zsh]

      --dir <DIR>
          Directory to write the correctly-named file into, instead of stdout (or instead of the standard location, with `install`)

      --format <FORMAT>
          Output format. `text` is for humans; `json` is for scripts and agents, and is stable enough to parse.
          
          Only the RESULT changes format. Progress, warnings, and errors always go to stderr as plain text, so `cortex preset list --format json | jq` gets clean JSON regardless.

          Possible values:
          - text: Human-readable, the default
          - json: Machine-readable JSON
          
          [default: text]

      --zero-based
          Take `--row` as 0-3 rather than the 1-4 shown on the unit.
          
          The unit labels its rows 1-4 and the wire numbers them 0-3, so the default matches what a player sees. Scripts and agents generally have a zero-based index already, and converting it back by hand is exactly the sort of arithmetic that silently edits the wrong row.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex completions install
```

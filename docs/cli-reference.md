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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

The protocol requires one effective USB owner, although a damaging second open may succeed. This process claims ownership, performs the handshake ONCE, and serves every other command through local IPC.

That matters for more than speed. A held session SUBSCRIBES to device state, which is how the unit reports edits you make on the hardware - so what `cortex` reports can stay true while you play, rather than being a snapshot from whenever the last command ran.

`start` runs it in the background and returns once it is serving; `status` reports on it; `stop` ends it, announcing the disconnect to the device first.

Usage: cortex session [OPTIONS] <COMMAND>

Commands:
  start   Open the session and serve other commands
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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex session start`

Open the session and serve other commands.

```text
Open the session and serve other commands.

Runs in the BACKGROUND by default, detached from the terminal, so closing the terminal does not take the session with it. The log goes beside the socket in `$XDG_RUNTIME_DIR`.

It waits for the session to start serving before returning, so a handshake that fails is reported here rather than discovered by the next command.

Usage: cortex session start [OPTIONS]

Options:
      --foreground
          Stay in the foreground, logging to the terminal.

          This is what the background mode runs internally. Useful when a handshake is misbehaving and you want to watch it happen.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex session start                # background, detached
  cortex session start --foreground   # stay attached and watch it
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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex session stop
```

### `cortex preset`

Presets: list, inspect, recall, prepare/save, or delete.

```text
Presets: list, inspect, recall, prepare/save, or delete

Usage: cortex preset [OPTIONS] <COMMAND>

Commands:
  copy          Copy a stored preset through destination preparation, source recall, and save
  delete        Delete a preset from a setlist, by name
  move          Move a preset to an empty slot in the same setlist
  prepare-save  Prepare a save destination before editing the working grid
  save          Commit the working grid to a destination prepared before editing
  list          List the presets in a setlist, in slot order
  show          Recall a slot and dump the preset it loads
  recall        Recall a preset by slot, making it the one loaded on the grid
  help          Print this message or the help of the given subcommand(s)

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex preset copy`

Copy a stored preset through destination preparation, source recall, and save.

```text
Copy a stored preset through destination preparation, source recall, and save.

WRITES TO THE UNIT and changes what is loaded. The destination is recalled and backed up before the source is recalled.

Usage: cortex preset copy [OPTIONS] --from <BANK+LETTER> --to <BANK+LETTER>

Options:
      --from-setlist <PATH>
          Source setlist path

          [default: "/media/p4/Presets/My Presets"]

      --from <BANK+LETTER>
          Source slot

      --to-setlist <PATH>
          Destination setlist path

          [default: "/media/p4/Presets/My Presets"]

      --to <BANK+LETTER>
          Destination slot

      --name <NAME>
          Destination name. Defaults to the recalled source preset's name

      --instrument <INSTRUMENT>
          Preferred-instrument metadata

          [default: guitar]

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex preset delete`

Delete a preset from a setlist, by name.

```text
Delete a preset from a setlist, by name.

WRITES TO THE UNIT, and there is no undo on the device.

Addressed by NAME, not slot - the opposite of `save`. Use the name the device reports in `cortex preset list`, which a save may have altered: on a name collision the unit de-duplicates with a `_N` suffix.

The factory library is refused.

Usage: cortex preset delete [OPTIONS] --name <NAME>

Options:
      --name <NAME>
          The preset's stored name, exactly as `cortex preset list` shows it

      --setlist <PATH>
          Absolute device path of the setlist

          [default: "/media/p4/Presets/My Presets"]

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex preset delete --name "SCRATCH"
```

#### `cortex preset move`

Move a preset to an empty slot in the same setlist.

```text
Move a preset to an empty slot in the same setlist.

WRITES TO THE UNIT. The command requests a fresh complete listing and refuses an empty source, an occupied destination, a no-op move, the factory library, and malformed slots.

Usage: cortex preset move [OPTIONS] --from <BANK+LETTER> --to <BANK+LETTER>

Options:
      --from <BANK+LETTER>
          Occupied source slot: bank number then letter, e.g. `2A`

      --to <BANK+LETTER>
          Empty destination slot: bank number then letter, e.g. `2B`

      --setlist <PATH>
          Absolute device path of the setlist

          [default: "/media/p4/Presets/My Presets"]

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex preset list --include-empty
  cortex preset move --from 2A --to 2B --dry-run
  cortex preset move --from 2A --to 2B
```

#### `cortex preset prepare-save`

Prepare a save destination before editing the working grid.

```text
Prepare a save destination before editing the working grid

Usage: cortex preset prepare-save [OPTIONS] --slot <BANK+LETTER>

Options:
      --slot <BANK+LETTER>
          Target slot: bank number then letter, e.g. `7A`

      --setlist <PATH>
          Absolute device path of the setlist

          [default: "/media/p4/Presets/My Presets"]

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex session start
  cortex preset prepare-save --slot 7A
```

#### `cortex preset save`

Commit the working grid to a destination prepared before editing.

```text
Commit the working grid to a destination prepared before editing.

WRITES TO THE UNIT, and there is no undo on the device. It overwrites whatever is in the slot.

What gets saved is the working grid - whatever `cortex grid show` reports - not a preset you name. Omit --name to keep the slot's existing name; give one to save into an empty slot or rename an occupied one.

The factory library is refused.

Usage: cortex preset save [OPTIONS] --token <TOKEN>

Options:
      --token <TOKEN>
          Opaque token returned by `preset prepare-save`

      --name <NAME>
          Name to save under. Omit to keep the slot's existing name

      --instrument <INSTRUMENT>
          Preferred-instrument metadata

          [default: guitar]

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex preset save --token save-1 --dry-run
  cortex preset save --token save-1
  cortex preset save --token save-1 --name "Lead Tone"
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
          Seconds to wait for the complete 256-slot listing. A timeout means no answer arrived, not that the setlist is empty

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

CHANGES WHAT IS HEARD: there is no side-effect-free way to read a STORED preset - the device only emits a preset when it recalls one. Use `cortex grid show` if you want the live grid without recalling.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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
  create     Create a new USER setlist as a sibling of My Presets
  delete     Delete a USER setlist and all presets it contains
  duplicate  Duplicate a USER setlist through create plus recall/save per preset
  list       List every folder the device knows: setlists, captures, IR libraries
  help       Print this message or the help of the given subcommand(s)

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex setlist create`

Create a new USER setlist as a sibling of My Presets.

```text
Create a new USER setlist as a sibling of My Presets

Usage: cortex setlist create [OPTIONS] --name <NAME>

Options:
      --name <NAME>
          Single setlist name, not a path

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex setlist delete`

Delete a USER setlist and all presets it contains.

```text
Delete a USER setlist and all presets it contains

Usage: cortex setlist delete [OPTIONS] --name <NAME>

Options:
      --name <NAME>
          Single setlist name, not a path. My Presets is always refused

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex setlist duplicate`

Duplicate a USER setlist through create plus recall/save per preset.

```text
Duplicate a USER setlist through create plus recall/save per preset

Usage: cortex setlist duplicate [OPTIONS] --source <NAME> --destination <NAME>

Options:
      --source <NAME>
          Source setlist name under the USER root

      --destination <NAME>
          New destination setlist name under the USER root

      --limit <LIMIT>
          Copy at most this many occupied presets

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex grid show`

Show the LIVE grid: what is loaded right now, unsaved edits included.

```text
Show the LIVE grid: what is loaded right now, unsaved edits included.

Read-only and side-effect free. This is the command to use while editing: `cortex preset show --slot X` reads a STORED slot and can only do so by recalling it, which discards unsaved edits and resets the active scene.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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
  move    Move one block to an empty grid cell and verify by live read-back
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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex block param`

Set a block parameter on the grid.

```text
Set a block parameter on the grid.

CHANGES THE WORKING GRID. Nothing is saved: the edit lives on the grid until you save the preset or recall another, which discards it. A complete live-grid read must confirm the requested value before this command reports success.

Rows are given as the unit LABELS them, 1-4, not the zero-based wire index. Use `cortex grid show` to see what is where.

Usage: cortex block param [OPTIONS] --row <1-4> --column <0-7>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --column <0-7>
          Grid column, 0-7, left to right

      --param <NAME>
          Parameter by NAME, e.g. `GAIN`. The model is read from the cell and resolved through the device catalog. Safer than --index: indices are positional and not every one is a visible knob

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

CHANGES THE WORKING GRID. Nothing is saved. Success requires a complete live-grid read confirming the active-scene bypass state.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

Verifies the device accepted it: a placement refused for want of DSP capacity is accepted on the wire and simply absent afterwards, with no error, so this waits for the device's echo and falls back to a live grid read-back when the echo is late.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

CHANGES THE WORKING GRID. Nothing is saved. Success requires a complete live-grid read confirming that the cell is empty.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex block remove --row 1 --column 2
```

#### `cortex block move`

Move one block to an empty grid cell and verify by live read-back.

```text
Move one block to an empty grid cell and verify by live read-back.

CHANGES THE WORKING GRID. Nothing is saved. A cross-row move lets the device create or adjust a parallel path.

Usage: cortex block move [OPTIONS] --from-row <1-4> --from-column <0-7> --to-row <1-4> --to-column <0-7>

Options:
      --from-row <1-4>
          Source row as shown on the unit, 1-4

      --from-column <0-7>
          Source column, 0-7

      --to-row <1-4>
          Destination row as shown on the unit, 1-4

      --to-column <0-7>
          Empty destination column, 0-7

      --timeout <SECONDS>
          Seconds to wait for each live-grid read

          [default: 15]

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex block move --from-row 1 --from-column 2 --to-row 2 --to-column 6
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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex row input`

Re-point a grid row's input.

```text
Re-point a grid row's input.

CHANGES THE WORKING GRID. Nothing is saved. Success requires a complete live-grid read confirming the typed route.

Usage: cortex row input [OPTIONS] --row <1-4> --port <PORT>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --port <PORT>
          Typed input destination, e.g. input1, return1, usb5, previous_row

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex row input --row 1 --port input1
```

#### `cortex row output`

Re-point a grid row's output.

```text
Re-point a grid row's output.

CHANGES THE WORKING GRID. Nothing is saved. Success requires a complete live-grid read confirming the typed route.

Not every destination is physical: next_row3, next_row4 and next_row34 route internally, while multiple is a real device-selected output.

Usage: cortex row output [OPTIONS] --row <1-4> --port <PORT>

Options:
      --row <1-4>
          Grid row as shown on the unit, 1-4

      --port <PORT>
          Typed output destination, e.g. xlr12, out3, usb5, next_row3

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex row output --row 1 --port xlr12
```

#### `cortex row split`

Set a row's split and mix points, activating a parallel branch.

```text
Set a row's split and mix points, activating a parallel branch.

CHANGES THE WORKING GRID. Nothing is saved. Success requires a complete live-grid read confirming both control points.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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
  probe    Probe the core session read paths and report their state
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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex device cpu   # needs a running: cortex session start
```

#### `cortex device probe`

Probe the core session read paths and report their state.

```text
Probe the core session read paths and report their state.

Without a held daemon this performs a subscribed handshake, reads the core scene/current-preset/preset-list paths, then disconnects. With a held daemon it probes those paths through the existing owner and leaves that session running.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex device probe
```

### `cortex scene`

Switch, label, recolour, copy, or swap scenes.

```text
Switch, label, recolour, copy, or swap scenes.

CHANGES WHAT IS HEARD. Nothing is saved.

Usage: cortex scene [OPTIONS] [COMMAND]

Commands:
  switch   Switch the active scene. Changes what is heard; does not save
  label    Set a scene label on the unsaved working copy
  unlabel  Clear a scene label on the unsaved working copy
  color    Set a scene colour as `0xAARRGGBB`, `#RRGGBB`, or decimal
  copy     Copy one scene onto another, including its label and colour
  swap     Exchange two scenes, including their labels and colours
  help     Print this message or the help of the given subcommand(s)

Options:
      --index <0-7>
          Scene number, 0-7 ZERO-BASED: 0 is scene A and 7 is scene H. The unit labels them A-H, so scene C is `--index 2`. Compatibility shorthand for `cortex scene switch --index N`

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex scene switch --index 2
  cortex scene label --index 2 --label 'Wide Lead'
  cortex scene color --index 2 --color '#FF02C2'
  cortex scene copy --from 1 --to 3
  cortex scene swap --first 1 --second 3

Compatibility:
  cortex scene --index 2
```

#### `cortex scene switch`

Switch the active scene. Changes what is heard; does not save.

```text
Switch the active scene. Changes what is heard; does not save

Usage: cortex scene switch [OPTIONS] --index <0-7>

Options:
      --index <0-7>


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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex scene label`

Set a scene label on the unsaved working copy.

```text
Set a scene label on the unsaved working copy

Usage: cortex scene label [OPTIONS] --index <0-7> --label <LABEL>

Options:
      --index <0-7>


      --label <LABEL>


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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex scene unlabel`

Clear a scene label on the unsaved working copy.

```text
Clear a scene label on the unsaved working copy

Usage: cortex scene unlabel [OPTIONS] --index <0-7>

Options:
      --index <0-7>


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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex scene color`

Set a scene colour as `0xAARRGGBB`, `#RRGGBB`, or decimal.

```text
Set a scene colour as `0xAARRGGBB`, `#RRGGBB`, or decimal

Usage: cortex scene color [OPTIONS] --index <0-7> --color <COLOR>

Options:
      --index <0-7>


      --color <COLOR>


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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex scene copy`

Copy one scene onto another, including its label and colour.

```text
Copy one scene onto another, including its label and colour

Usage: cortex scene copy [OPTIONS] --from <0-7> --to <0-7>

Options:
      --from <0-7>


      --to <0-7>


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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

#### `cortex scene swap`

Exchange two scenes, including their labels and colours.

```text
Exchange two scenes, including their labels and colours

Usage: cortex scene swap [OPTIONS] --first <0-7> --second <0-7>

Options:
      --first <0-7>


      --second <0-7>


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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

### `cortex catalog`

Search or inspect the device model catalog, or save its raw payload.

```text
Search or inspect the device model catalog, or save its raw payload.

The catalog is what turns the integer model ids stored in a preset into names. It comes from the device, so it reflects installed model content rather than a hard-coded factory table.

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

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

  -n, --dry-run
          Print the operation plan without changing device or local state. Read-only commands accept and ignore this flag

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  cortex completions install
```

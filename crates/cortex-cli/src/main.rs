// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The standalone `cortex` command-line surface.
//!
//! A thin wrapper over [`cortex_rs`]: all protocol and domain behaviour lives
//! in the library so the MCP server and the Tauri backend can reuse it
//! without repetition. The binary adds only argument parsing, shell
//! completions, and the version command.
//!
//! @see spec/200-cli/spec.md
//! @see spec/200-cli/design.md

use std::process::ExitCode;
use std::time::Duration;

mod connect;
mod decode;

// The preset view types now live in the crate, so the GUI and the MCP server
// get the same shapes rather than reimplementing them. Aliased to their old
// names here to keep the printers below unchanged.
use cortex_rs::view::{CpuLoad, DeviceVersion, ParamValueKind, Preset as PresetOut, PresetSlot};

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use cortex_rs::proto::VersionMessage;
use cortex_rs::proto::cortex_message_type::Enum as MessageType;
use cortex_rs::proto::message_action::Enum as MessageAction;
use cortex_rs::{DeviceKind, Transport};

/// The `cortex` CLI: an unofficial, Linux-first command-line surface for the
/// Neural DSP Quad Cortex over USB HID. Nano Cortex support is planned but its
/// transport compatibility is unverified.
///
/// Not affiliated with or endorsed by Neural DSP. "Neural DSP", "Quad Cortex",
/// and "Nano Cortex" are trademarks of Neural DSP Technologies. See the README
/// for the full trademark and reverse-engineering-for-interoperability
/// statement.
#[derive(Parser, Debug)]
#[command(
    name = "cortex",
    version,
    about,
    long_about = None,
    propagate_version = true,
    subcommand_required = false,
    arg_required_else_help = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Output format. `text` is for humans; `json` is for scripts and
    /// agents, and is stable enough to parse.
    ///
    /// Only the RESULT changes format. Progress, warnings, and errors always
    /// go to stderr as plain text, so `cortex preset list --format json | jq`
    /// gets clean JSON regardless.
    #[arg(long, global = true, value_enum, default_value = "text")]
    format: Format,

    /// Take `--row` as 0-3 rather than the 1-4 shown on the unit.
    ///
    /// The unit labels its rows 1-4 and the wire numbers them 0-3, so the
    /// default matches what a player sees. Scripts and agents generally have
    /// a zero-based index already, and converting it back by hand is exactly
    /// the sort of arithmetic that silently edits the wrong row.
    #[arg(long, global = true)]
    zero_based: bool,

    /// Print the operation plan without changing device or local state.
    /// Read-only commands accept and ignore this flag.
    #[arg(short = 'n', long, global = true)]
    dry_run: bool,
}

/// How to render a command's result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
enum Format {
    /// Human-readable, the default.
    #[default]
    Text,
    /// Machine-readable JSON.
    Json,
}

/// A complete local plan returned before any side-effect boundary is crossed.
#[derive(Debug, serde::Serialize)]
struct DryRunPlan {
    dry_run: bool,
    action: &'static str,
    effect: &'static str,
    target: serde_json::Value,
    checks_on_execute: Vec<&'static str>,
}

/// Print a result: JSON when asked for, otherwise the caller's text form.
///
/// Results go to STDOUT. Everything else - progress, warnings, hints - goes
/// to stderr, so a caller piping stdout into `jq` never gets it corrupted by
/// a diagnostic.
fn emit<T: serde::Serialize>(value: &T, format: Format, text: impl FnOnce(&T)) -> Result<()> {
    match format {
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(value)?);
        }
        Format::Text => text(value),
    }
    Ok(())
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Hold a persistent connection to the device, serving other commands.
    ///
    /// The protocol requires one effective USB owner, although a damaging
    /// second open may succeed. This process claims ownership, performs the
    /// handshake ONCE, and serves every other command through local IPC.
    ///
    /// That matters for more than speed. A held session SUBSCRIBES to device
    /// state, which is how the unit reports edits you make on the hardware -
    /// so what `cortex` reports can stay true while you play, rather than
    /// being a snapshot from whenever the last command ran.
    ///
    /// `start` runs it in the background and returns once it is serving;
    /// `status` reports on it; `stop` ends it, announcing the disconnect to
    /// the device first.
    #[command(visible_alias = "connect", alias = "s")]
    Session {
        #[command(subcommand)]
        command: SessionCmd,
    },
    /// Presets: list, inspect, recall, prepare/save, or delete.
    #[command(alias = "p")]
    Preset {
        #[command(subcommand)]
        command: PresetCmd,
    },
    /// Setlists: the folders of presets the unit holds.
    #[command(alias = "sl")]
    Setlist {
        #[command(subcommand)]
        command: SetlistCmd,
    },
    /// The signal grid that is loaded right now.
    #[command(alias = "g")]
    Grid {
        #[command(subcommand)]
        command: GridCmd,
    },
    /// One block in the grid: its parameters, bypass, model, or removal.
    #[command(alias = "b")]
    Block {
        #[command(subcommand)]
        command: BlockCmd,
    },
    /// A grid row: its input, its output, and where it splits.
    #[command(alias = "r")]
    Row {
        #[command(subcommand)]
        command: RowCmd,
    },
    /// The unit itself: firmware, DSP load, and a connection probe.
    #[command(alias = "d")]
    Device {
        #[command(subcommand)]
        command: DeviceCmd,
    },

    /// Switch the active scene.
    ///
    /// CHANGES WHAT IS HEARD. Nothing is saved.
    #[command(alias = "sc")]
    #[command(after_help = "Examples:\n  cortex scene --index 2")]
    Scene {
        /// Scene number, 0-7 ZERO-BASED: 0 is scene A and 7 is scene H.
        /// The unit labels them A-H, so scene C is `--index 2`.
        #[arg(long, value_name = "0-7")]
        index: u32,
    },
    /// Search or inspect the device model catalog, or save its raw payload.
    ///
    /// The catalog is what turns the integer model ids stored in a preset
    /// into names. It comes from the device, so it reflects installed model
    /// content rather than a hard-coded factory table.
    ///
    /// Read-only.
    #[command(alias = "c")]
    #[command(
        after_help = "Examples:\n  cortex catalog --search plexi\n  cortex catalog --model 1001\n  cortex catalog --dump repo.bin"
    )]
    Catalog {
        /// Case-insensitive substring to match against a model's name AND
        /// the gear it is based on, e.g. `marshall`, `tape echo`.
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,
        /// Show one model in full by NUMERIC id, e.g. `--model 1001`.
        /// Use --search to find an id from a name.
        #[arg(long, value_name = "ID")]
        model: Option<u32>,
        /// File path to write the raw payload to, for inspection.
        #[arg(long, value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        dump: Option<std::path::PathBuf>,
        /// Parse a payload previously saved with --dump, instead of asking
        /// the device. Needs no unit connected.
        #[arg(long, value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        from_file: Option<std::path::PathBuf>,
        /// Seconds to wait. The payload is ~47 KB over several hundred
        /// reports, so allow generously.
        #[arg(long, value_name = "SECONDS", default_value = "40")]
        timeout: u64,
    },
    /// Decode a USB capture into Cortex Control messages.
    ///
    /// Reads `tshark` field output on standard input and prints one line per
    /// reassembled message, in the same shape as `CORTEX_TRACE`. Use
    /// `s/usb-decode`, which supplies the right `tshark` invocation.
    ///
    /// Works on a capture of ANY client, which is the point: it is how a
    /// recording of Cortex Control gets read against our own schema.
    #[command(
        after_help = "Examples:\n  s/usb-decode traces/capture.pcapng          # the usual way\n  s/usb-decode --live                          # watch it happen"
    )]
    DecodeTrace {
        /// Print only the summary line, not every message.
        #[arg(long)]
        quiet: bool,
        /// Also describe each message's protobuf fields.
        ///
        /// Generic, so it works on message types we do not model - which is
        /// the case that matters when reading a capture of another client.
        #[arg(long, conflicts_with = "quiet")]
        verbose: bool,
    },
    /// Generate or install shell completions.
    ///
    /// `cortex completions install` is the one to use: it detects your shell,
    /// writes the completion file to the standard location, and prints any
    /// one-time setup still needed. It never edits your shell startup files.
    ///
    /// `cortex completions <shell>` prints the script to stdout instead,
    /// which is the stable interface for packagers and unusual setups.
    #[command(after_help = "Examples:\n  cortex completions install")]
    Completions {
        /// A shell to generate for, or `install` to install for your own.
        #[arg(value_enum)]
        target: CompletionTarget,
        /// Override the shell detected from $SHELL when using `install`.
        #[arg(long, value_enum)]
        shell: Option<clap_complete::Shell>,
        /// Directory to write the correctly-named file into, instead of
        /// stdout (or instead of the standard location, with `install`).
        #[arg(long, value_name = "DIR", value_hint = clap::ValueHint::DirPath)]
        dir: Option<std::path::PathBuf>,
    },
}

/// What to do with the held session.
#[derive(Subcommand, Debug)]
enum SessionCmd {
    /// Open the session and serve other commands.
    ///
    /// Runs in the BACKGROUND by default, detached from the terminal, so
    /// closing the terminal does not take the session with it. The log goes
    /// beside the socket in `$XDG_RUNTIME_DIR`.
    ///
    /// It waits for the session to start serving before returning, so a
    /// handshake that fails is reported here rather than discovered by the
    /// next command.
    #[command(
        after_help = "Examples:\n  cortex session start                # background, detached\n  cortex session start --foreground   # stay attached and watch it"
    )]
    Start {
        /// Stay in the foreground, logging to the terminal.
        ///
        /// This is what the background mode runs internally. Useful when a
        /// handshake is misbehaving and you want to watch it happen.
        #[arg(long)]
        foreground: bool,
    },
    /// Report whether a session is running, and whether the device answers.
    #[command(after_help = "Examples:\n  cortex session status")]
    Status,
    /// Ask a running session to shut down, announcing the disconnect first.
    #[command(after_help = "Examples:\n  cortex session stop")]
    Stop,
}

/// Commands acting on presets.
#[derive(Subcommand, Debug)]
enum PresetCmd {
    /// Delete a preset from a setlist, by name.
    ///
    /// WRITES TO THE UNIT, and there is no undo on the device.
    ///
    /// Addressed by NAME, not slot - the opposite of `save`. Use the name the
    /// device reports in `cortex preset list`, which a save may have altered:
    /// on a name collision the unit de-duplicates with a `_N` suffix.
    ///
    /// The factory library is refused.
    #[command(after_help = "Examples:\n  cortex preset delete --name \"SCRATCH\"")]
    Delete {
        /// The preset's stored name, exactly as `cortex preset list` shows it.
        #[arg(long, value_name = "NAME")]
        name: String,
        /// Absolute device path of the setlist.
        #[arg(long, value_name = "PATH", default_value = cortex_rs::client::USER_SETLIST)]
        setlist: String,
    },
    /// Move a preset to an empty slot in the same setlist.
    ///
    /// WRITES TO THE UNIT. The command requests a fresh complete listing and
    /// refuses an empty source, an occupied destination, a no-op move, the
    /// factory library, and malformed slots.
    #[command(
        after_help = "Examples:\n  cortex preset list --include-empty\n  cortex preset move --from 2A --to 2B --dry-run\n  cortex preset move --from 2A --to 2B"
    )]
    Move {
        /// Occupied source slot: bank number then letter, e.g. `2A`.
        #[arg(long, value_name = "BANK+LETTER")]
        from: String,
        /// Empty destination slot: bank number then letter, e.g. `2B`.
        #[arg(long, value_name = "BANK+LETTER")]
        to: String,
        /// Absolute device path of the setlist.
        #[arg(long, value_name = "PATH", default_value = cortex_rs::client::USER_SETLIST)]
        setlist: String,
    },
    /// Prepare a save destination before editing the working grid.
    #[command(
        after_help = "Examples:\n  cortex session start\n  cortex preset prepare-save --slot 7A"
    )]
    PrepareSave {
        /// Target slot: bank number then letter, e.g. `7A`.
        #[arg(long, value_name = "BANK+LETTER")]
        slot: String,
        /// Absolute device path of the setlist.
        #[arg(long, value_name = "PATH", default_value = cortex_rs::client::USER_SETLIST)]
        setlist: String,
    },
    /// Commit the working grid to a destination prepared before editing.
    ///
    /// WRITES TO THE UNIT, and there is no undo on the device. It overwrites
    /// whatever is in the slot.
    ///
    /// What gets saved is the working grid - whatever `cortex grid show`
    /// reports - not a preset you name. Omit --name to keep the slot's
    /// existing name; give one to save into an empty slot or rename an
    /// occupied one.
    ///
    /// The factory library is refused.
    #[command(
        after_help = "Examples:\n  cortex preset save --token save-1 --dry-run\n  cortex preset save --token save-1\n  cortex preset save --token save-1 --name \"Lead Tone\""
    )]
    Save {
        /// Opaque token returned by `preset prepare-save`.
        #[arg(long, value_name = "TOKEN")]
        token: String,
        /// Name to save under. Omit to keep the slot's existing name.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
    /// List the presets in a setlist, in slot order.
    ///
    /// Read-only: this does NOT change what is loaded on the grid.
    #[command(after_help = "Examples:\n  cortex preset list\n  cortex preset list --include-empty")]
    List {
        /// Absolute device path of the setlist, e.g.
        /// `/media/p4/Presets/My Presets`. Run `cortex setlist list` to list them.
        #[arg(long, value_name = "PATH", default_value = cortex_rs::client::USER_SETLIST)]
        setlist: String,
        /// Include empty slots, so a free slot can be found.
        #[arg(long)]
        include_empty: bool,
        /// Seconds to wait for the complete 256-slot listing. A timeout means
        /// no answer arrived, not that the setlist is empty.
        #[arg(long, value_name = "SECONDS", default_value = "25")]
        timeout: u64,
    },
    /// Recall a slot and dump the preset it loads.
    ///
    /// CHANGES WHAT IS HEARD: there is no side-effect-free way to read a
    /// STORED preset - the device only emits a preset when it recalls one.
    /// Use `cortex grid show` if you want the live grid without recalling.
    #[command(
        after_help = "Examples:\n  cortex preset show --slot 1B\n  cortex preset show --slot 1B --params"
    )]
    Show {
        /// Slot name: bank number then letter, e.g. `1A`, `28C`.
        /// Bank is 1-32, letter A-H.
        #[arg(long, value_name = "BANK+LETTER")]
        slot: String,
        /// Absolute device path of the setlist. `cortex setlist list` lists them.
        #[arg(long, value_name = "PATH", default_value = cortex_rs::client::USER_SETLIST)]
        setlist: String,
        /// Mark the setlist as the read-only factory library.
        #[arg(long)]
        factory: bool,
        /// Also show each block's stored parameter values.
        #[arg(long)]
        params: bool,
    },
    /// Recall a preset by slot, making it the one loaded on the grid.
    ///
    /// CHANGES WHAT IS HEARD. Nothing is saved and no stored preset is
    /// modified, but the grid is replaced and any unsaved edits are lost.
    #[command(after_help = "Examples:\n  cortex preset recall --slot 2B")]
    Recall {
        /// Slot name: bank number then letter, e.g. `1A`, `12H`, `28C`.
        /// Bank is 1-32 and letter is A-H, giving 256 slots per setlist.
        #[arg(long, value_name = "BANK+LETTER")]
        slot: String,
        /// Absolute device path of the setlist, e.g.
        /// `/media/p4/Presets/My Presets`. Run `cortex setlist list` to list them.
        #[arg(long, value_name = "PATH", default_value = cortex_rs::client::USER_SETLIST)]
        setlist: String,
        /// Mark the setlist as the read-only factory library. Needed for
        /// paths under /opt/neuraldsp/Factory Library.
        #[arg(long)]
        factory: bool,
    },
}

/// Commands acting on setlists.
#[derive(Subcommand, Debug)]
enum SetlistCmd {
    /// List every folder the device knows: setlists, captures, IR libraries.
    ///
    /// A single `File` READ makes the device enumerate all its folders, which
    /// arrive over ten to twenty seconds. There is no total-count field on the
    /// wire, so this always waits the full window.
    ///
    /// Read-only.
    #[command(after_help = "Examples:\n  cortex setlist list\n  cortex setlist list --show-empty")]
    List {
        /// Seconds to gather folder announcements.
        #[arg(long, value_name = "SECONDS", default_value = "20")]
        window: u64,
        /// Also list folders holding no presets.
        ///
        /// The unit reports hundreds of them, nearly all empty, which buries
        /// the two or three you actually use.
        #[arg(long)]
        show_empty: bool,
    },
}

/// Commands acting on the grid as a whole.
#[derive(Subcommand, Debug)]
enum GridCmd {
    /// Show the LIVE grid: what is loaded right now, unsaved edits included.
    ///
    /// Read-only and side-effect free. This is the command to use while
    /// editing: `cortex preset show --slot X` reads a STORED slot and can only do
    /// so by recalling it, which discards unsaved edits and resets the
    /// active scene.
    #[command(after_help = "Examples:\n  cortex grid show\n  cortex grid show --params")]
    Show {
        /// Seconds to wait for the grid.
        #[arg(long, value_name = "SECONDS", default_value = "15")]
        timeout: u64,
        /// Also show each block's stored parameter values, which is how to
        /// check an edit landed.
        #[arg(long)]
        params: bool,
    },
}

/// Commands acting on one block in the grid.
#[derive(Subcommand, Debug)]
enum BlockCmd {
    /// Set a block parameter on the grid.
    ///
    /// CHANGES THE WORKING GRID. Nothing is saved: the edit lives on the
    /// grid until you save the preset or recall another, which discards it.
    ///
    /// Rows are given as the unit LABELS them, 1-4, not the zero-based wire
    /// index. Use `cortex grid show` to see what is where.
    #[command(
        after_help = "Examples:\n  cortex block param --row 1 --column 2 --param GAIN --real 7.5\n  cortex block param --row 1 --column 2 --index 0 --value 0.75\n  cortex block param --row 1 --column 2 --param GAIN --value 0.9 --scene 2"
    )]
    Param {
        /// Grid row as shown on the unit, 1-4.
        #[arg(long, value_name = "1-4")]
        row: u32,
        /// Grid column, 0-7, left to right.
        #[arg(long, value_name = "0-7")]
        column: u32,
        /// Parameter by NAME, e.g. `GAIN`. The model is read from the cell
        /// and resolved through the device catalog.
        /// Safer than --index: indices are positional and not every one is
        /// a visible knob.
        #[arg(long, value_name = "NAME", conflicts_with = "index")]
        param: Option<String>,
        /// Parameter by raw wire index. Prefer --param.
        #[arg(long, value_name = "N", conflicts_with = "param")]
        index: Option<u32>,
        /// Normalised value, 0.0-1.0, which is what the wire carries.
        #[arg(long, value_name = "0.0-1.0", conflicts_with_all = ["real", "text"])]
        value: Option<f32>,
        /// Value in the parameter's OWN units (dB, ms, Hz). Converted using
        /// the catalog range, so it needs --param.
        #[arg(long, value_name = "N", conflicts_with_all = ["value", "text"])]
        real: Option<f64>,
        /// String value, for parameters that take one such as a cabinet's
        /// microphone selection.
        #[arg(long, value_name = "TEXT", conflicts_with_all = ["value", "real"])]
        text: Option<String>,
        /// Apply to this scene (0-7) rather than the active one. Sends three
        /// messages and LEAVES THE UNIT ON THAT SCENE.
        #[arg(long, value_name = "0-7")]
        scene: Option<u32>,
    },
    /// Bypass or enable a block on the grid.
    ///
    /// CHANGES THE WORKING GRID. Nothing is saved.
    #[command(
        after_help = "Examples:\n  cortex block bypass --row 1 --column 2 --bypass   # bypass it\n  cortex block bypass --row 1 --column 2             # enable it"
    )]
    Bypass {
        /// Grid row as shown on the unit, 1-4.
        #[arg(long, value_name = "1-4")]
        row: u32,
        /// Grid column, 0-7.
        #[arg(long, value_name = "0-7")]
        column: u32,
        /// Bypass the block. Omit to enable it.
        #[arg(long)]
        bypass: bool,
    },
    /// Place a model in a grid cell, creating or replacing a block.
    ///
    /// CHANGES THE WORKING GRID. Nothing is saved.
    ///
    /// Verifies the device accepted it: a placement refused for want of DSP
    /// capacity is accepted on the wire and simply absent afterwards, with
    /// no error, so this waits for the device's echo and falls back to a live
    /// grid read-back when the echo is late.
    #[command(
        after_help = "Examples:\n  cortex block set --row 1 --column 2 --model 1001   # find ids with: cortex catalog --search"
    )]
    Set {
        /// Grid row as shown on the unit, 1-4.
        #[arg(long, value_name = "1-4")]
        row: u32,
        /// Grid column, 0-7.
        #[arg(long, value_name = "0-7")]
        column: u32,
        /// Model by NUMERIC id. Find one with `cortex catalog --search`.
        #[arg(long, value_name = "ID")]
        model: u32,
        /// Send without waiting for the echo. A DSP refusal then looks
        /// identical to success.
        #[arg(long)]
        no_verify: bool,
        /// Seconds to wait for the device's echo.
        #[arg(long, value_name = "SECONDS", default_value = "5")]
        timeout: u64,
    },
    /// Remove the block at a grid cell.
    ///
    /// CHANGES THE WORKING GRID. Nothing is saved.
    #[command(after_help = "Examples:\n  cortex block remove --row 1 --column 2")]
    Remove {
        /// Grid row as shown on the unit, 1-4.
        #[arg(long, value_name = "1-4")]
        row: u32,
        /// Grid column, 0-7.
        #[arg(long, value_name = "0-7")]
        column: u32,
    },
}

/// Commands acting on a grid row.
#[derive(Subcommand, Debug)]
enum RowCmd {
    /// Re-point a grid row's input.
    ///
    /// CHANGES THE WORKING GRID. Nothing is saved.
    #[command(after_help = "Examples:\n  cortex row input --row 1 --port 1")]
    Input {
        /// Grid row as shown on the unit, 1-4.
        #[arg(long, value_name = "1-4")]
        row: u32,
        /// Input port id. 1 = Input 1, 2 = Input 2, 4 = Return 1, 5 = Return 2.
        /// Note the ids are NOT 1/2/3/4 - combined ports are interleaved.
        #[arg(long, value_name = "ID")]
        port: u32,
    },
    /// Re-point a grid row's output.
    ///
    /// CHANGES THE WORKING GRID. Nothing is saved.
    ///
    /// Not every id is a physical destination: 16-18 are internal row-to-row
    /// routing, while 19 (MULTIPLE) is a real output. The device does not
    /// validate this field, so a meaningless id is stored rather than
    /// rejected and reads back cleanly.
    #[command(after_help = "Examples:\n  cortex row output --row 1 --port 0")]
    Output {
        /// Grid row as shown on the unit, 1-4.
        #[arg(long, value_name = "1-4")]
        row: u32,
        /// Output port id.
        #[arg(long, value_name = "ID")]
        port: u32,
    },
    /// Set a row's split and mix points, activating a parallel branch.
    ///
    /// CHANGES THE WORKING GRID. Nothing is saved.
    ///
    /// Only screen rows 1 and 3 can branch; their parallel lane is the row
    /// below. Rows 2 and 4 have no splitter and are refused.
    #[command(
        after_help = "Examples:\n  cortex row split --row 1 --split 3 --mix 6   # branch at 3, rejoin at 6\n  cortex row split --row 1 --split -1 --mix -1  # clear the branch"
    )]
    Split {
        /// Grid row as shown on the unit. Must be 1 or 3.
        #[arg(long, value_name = "1|3")]
        row: u32,
        /// Column at which the row branches. -1 clears the branch.
        #[arg(long, value_name = "COL", allow_negative_numbers = true)]
        split: i32,
        /// Column at which the branch rejoins. -1 means it never does.
        #[arg(
            long,
            value_name = "COL",
            default_value = "-1",
            allow_negative_numbers = true
        )]
        mix: i32,
    },
}

/// Commands reporting on the unit itself.
#[derive(Subcommand, Debug)]
enum DeviceCmd {
    /// Read the device firmware version (CorOS, app, bootloader, zencoder).
    #[command(
        after_help = "Examples:\n  cortex device version\n  cortex device version --format json"
    )]
    Version {
        /// Read via the session layer (background RX thread + correlated
        /// request) instead of the one-shot synchronous transport.
        ///
        /// Both paths are valid: a `Version` READ is answered without the
        /// connect handshake. This flag exists to exercise the session layer
        /// against hardware. Both paths are now verified; see
        /// spec/140-session/spec.md.
        #[arg(long)]
        session: bool,
    },
    /// Show the unit's live DSP load.
    ///
    /// The device pushes this about once a second, but only to a client that
    /// has subscribed - so this needs a running `cortex session start`. A one-shot
    /// command uses a minimal handshake and never asks the device to push it.
    #[command(
        after_help = "Examples:\n  cortex device cpu   # needs a running: cortex session start"
    )]
    Cpu,
    /// Probe the core session read paths and report their state.
    ///
    /// Without a held daemon this performs a subscribed handshake, reads the
    /// core scene/current-preset/preset-list paths, then disconnects. With a
    /// held daemon it probes those paths through the existing owner and leaves
    /// that session running.
    ///
    /// Read-only: the handshake sends READs and a connect announcement. It
    /// never writes preset data and never saves.
    #[command(after_help = "Examples:\n  cortex device probe")]
    Probe {
        /// Extra seconds to hold the session open after the handshake before
        /// reading. The handshake already waits for the device to go quiet,
        /// so 0 is usually fine; raise it if reads time out.
        #[arg(long, value_name = "SECONDS", default_value = "0")]
        listen: u64,
    },
}

fn main() -> ExitCode {
    // Reset SIGPIPE on Unix so output pipes cleanly into `head`/`less`.
    #[cfg(unix)]
    unsafe {
        libc_sigpipe_reset();
    }

    let cli = Cli::parse();

    // Only worth installing for commands that open a device. Parsing and
    // completions have nothing to release.
    install_signal_handler();

    let result = run(cli);

    // Belt and braces: a command that returned early via `?` may have left
    // the session in the slot. Releasing here means the device is told even
    // on an error path, without relying on drop order.
    release_session();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cortex: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let fmt = cli.format;
    // Recorded rather than threaded through every row-taking command. Six
    // signatures would otherwise grow an argument that none of them decide.
    let _ = ZERO_BASED.set(cli.zero_based);

    if cli.dry_run {
        if let Some(plan) = dry_run_plan(cli.command.as_ref())? {
            return emit(&plan, fmt, |plan| {
                println!("dry-run {}: {}", plan.action, plan.target);
                for check in &plan.checks_on_execute {
                    println!("  on execution: {check}");
                }
            });
        }
    }

    match cli.command {
        Some(Command::Session { command }) => match command {
            SessionCmd::Start { foreground } => {
                if foreground {
                    cmd_connect(false, false, fmt)
                } else {
                    connect::start_detached()
                }
            }
            SessionCmd::Status => cmd_connect(true, false, fmt),
            SessionCmd::Stop => cmd_connect(false, true, fmt),
        },
        Some(Command::Preset { command }) => match command {
            PresetCmd::Delete { name, setlist } => cmd_preset_delete(&name, &setlist, fmt),
            PresetCmd::Move { from, to, setlist } => cmd_preset_move(&from, &to, &setlist, fmt),
            PresetCmd::PrepareSave { slot, setlist } => {
                cmd_preset_prepare_save(&slot, &setlist, fmt)
            }
            PresetCmd::Save { token, name } => cmd_preset_save(&token, name.as_deref(), fmt),
            PresetCmd::List {
                setlist,
                include_empty,
                timeout,
            } => cmd_presets(&setlist, include_empty, timeout, fmt),
            PresetCmd::Show {
                slot,
                setlist,
                factory,
                params,
            } => cmd_preset(&slot, &setlist, factory, params, fmt),
            PresetCmd::Recall {
                slot,
                setlist,
                factory,
            } => cmd_recall(&slot, &setlist, factory, fmt),
        },
        Some(Command::Setlist { command }) => match command {
            SetlistCmd::List { window, show_empty } => cmd_folders(window, show_empty, fmt),
        },
        Some(Command::Grid { command }) => match command {
            GridCmd::Show { timeout, params } => cmd_grid(timeout, params, fmt),
        },
        Some(Command::Block { command }) => match command {
            BlockCmd::Param {
                row,
                column,
                param,
                index,
                value,
                real,
                text,
                scene,
            } => cmd_set_param(
                row,
                column,
                param.as_deref(),
                index,
                value,
                real,
                text.as_deref(),
                scene,
                fmt,
            ),
            BlockCmd::Bypass {
                row,
                column,
                bypass,
            } => cmd_set_bypass(row, column, bypass, fmt),
            BlockCmd::Set {
                row,
                column,
                model,
                no_verify,
                timeout,
            } => cmd_set_block(row, column, model, no_verify, timeout, fmt),
            BlockCmd::Remove { row, column } => cmd_remove_block(row, column, fmt),
        },
        Some(Command::Row { command }) => match command {
            RowCmd::Input { row, port } => cmd_set_routing(row, Some(port), None, fmt),
            RowCmd::Output { row, port } => cmd_set_routing(row, None, Some(port), fmt),
            RowCmd::Split { row, split, mix } => cmd_set_split(row, split, mix, fmt),
        },
        Some(Command::Device { command }) => match command {
            DeviceCmd::Version { session: true } => cmd_version_via_session(fmt),
            DeviceCmd::Version { session: false } => cmd_version(fmt),
            DeviceCmd::Cpu => cmd_cpu(fmt),
            DeviceCmd::Probe { listen } => cmd_probe(listen, fmt),
        },
        Some(Command::Scene { index }) => cmd_scene(index, fmt),
        Some(Command::Catalog {
            search,
            model,
            dump,
            from_file,
            timeout,
        }) => cmd_catalog(
            search.as_deref(),
            model,
            dump.as_deref(),
            from_file.as_deref(),
            timeout,
            fmt,
        ),
        Some(Command::DecodeTrace { quiet, verbose }) => {
            decode::decode_stream(std::io::stdin().lock(), quiet, verbose)
        }
        Some(Command::Completions { target, shell, dir }) => {
            cmd_completions(target, shell, dir.as_deref())
        }
        None => {
            Cli::command().print_help()?;
            Ok(())
        }
    }
}

fn plan(
    action: &'static str,
    effect: &'static str,
    target: serde_json::Value,
    checks_on_execute: &[&'static str],
) -> DryRunPlan {
    DryRunPlan {
        dry_run: true,
        action,
        effect,
        target,
        checks_on_execute: checks_on_execute.to_vec(),
    }
}

fn validate_slot(slot: &str) -> Result<()> {
    if cortex_rs::client::slot_to_position_checked(slot).is_none() {
        anyhow::bail!(
            "{slot} is not a slot. Slots are a bank number 1-32 then a letter A-H, e.g. 2B"
        );
    }
    Ok(())
}

fn validate_cell(row: u32, column: u32) -> Result<cortex_rs::Row> {
    let row = wire_row(row)?;
    if column > 7 {
        anyhow::bail!("column {column} is out of range: columns are 0-7");
    }
    Ok(row)
}

/// Classify every command before dispatch. Exhaustive matching makes a new
/// command fail to compile until its dry-run behavior is deliberately chosen.
fn dry_run_plan(command: Option<&Command>) -> Result<Option<DryRunPlan>> {
    let Some(command) = command else {
        return Ok(None);
    };
    let plan = match command {
        Command::Session { command } => match command {
            SessionCmd::Start { foreground } => Some(plan(
                "session start",
                "local process and device session",
                serde_json::json!({ "foreground": foreground }),
                &[
                    "claim the local endpoint",
                    "open and subscribe to the device",
                ],
            )),
            SessionCmd::Status => None,
            SessionCmd::Stop => Some(plan(
                "session stop",
                "local process and device session",
                serde_json::json!({}),
                &[
                    "announce disconnect",
                    "stop the daemon and remove its endpoint",
                ],
            )),
        },
        Command::Preset { command } => match command {
            PresetCmd::Delete { name, setlist } => {
                if cortex_rs::client::is_factory_setlist(setlist) {
                    anyhow::bail!("{setlist} is the factory library and is not writable");
                }
                Some(plan(
                    "preset delete",
                    "persistent device storage",
                    serde_json::json!({ "name": name, "setlist": setlist }),
                    &["resolve the stored file path", "delete the preset"],
                ))
            }
            PresetCmd::Move { from, to, setlist } => {
                if cortex_rs::client::is_factory_setlist(setlist) {
                    anyhow::bail!("{setlist} is the factory library and is not writable");
                }
                validate_slot(from)?;
                validate_slot(to)?;
                Some(plan(
                    "preset move",
                    "persistent device storage",
                    serde_json::json!({ "from": from, "to": to, "setlist": setlist }),
                    &[
                        "verify the source is occupied",
                        "verify the destination is empty",
                        "move and wait for listing convergence",
                    ],
                ))
            }
            PresetCmd::PrepareSave { slot, setlist } => {
                if cortex_rs::client::is_factory_setlist(setlist) {
                    anyhow::bail!("{setlist} is the factory library and is not writable");
                }
                validate_slot(slot)?;
                Some(plan(
                    "preset prepare-save",
                    "working grid and daemon preparation registry",
                    serde_json::json!({ "slot": slot, "setlist": setlist }),
                    &[
                        "recall and back up the exact target",
                        "retain an opaque preparation token",
                    ],
                ))
            }
            PresetCmd::Save { token, name } => Some(plan(
                "preset save",
                "persistent device storage",
                serde_json::json!({ "token": token, "name": name }),
                &[
                    "validate and consume the prepared target",
                    "commit the working grid",
                ],
            )),
            PresetCmd::List { .. } => None,
            PresetCmd::Show {
                slot,
                setlist,
                factory,
                params,
            } => {
                validate_slot(slot)?;
                Some(plan(
                    "preset show",
                    "working grid and audible state",
                    serde_json::json!({ "slot": slot, "setlist": setlist, "factory": factory, "params": params }),
                    &[
                        "recall the stored preset",
                        "replace unsaved grid state and reset the scene",
                    ],
                ))
            }
            PresetCmd::Recall {
                slot,
                setlist,
                factory,
            } => {
                validate_slot(slot)?;
                Some(plan(
                    "preset recall",
                    "working grid and audible state",
                    serde_json::json!({ "slot": slot, "setlist": setlist, "factory": factory }),
                    &[
                        "recall the stored preset",
                        "replace unsaved grid state and reset the scene",
                    ],
                ))
            }
        },
        Command::Setlist { command } => match command {
            SetlistCmd::List { .. } => None,
        },
        Command::Grid { command } => match command {
            GridCmd::Show { .. } => None,
        },
        Command::Block { command } => match command {
            BlockCmd::Param {
                row,
                column,
                param,
                index,
                value,
                real,
                text,
                scene,
            } => {
                let row = validate_cell(*row, *column)?;
                if scene.is_some_and(|scene| scene > 7) {
                    anyhow::bail!("scene must be 0-7");
                }
                if value.is_none() && real.is_none() && text.is_none() {
                    anyhow::bail!("give a value: --value (0.0-1.0), --real (own units), or --text");
                }
                Some(plan(
                    "block param",
                    "working grid",
                    serde_json::json!({ "wire_row": row.wire(), "screen_row": row.screen(), "column": column, "param": param, "index": index, "value": value, "real": real, "text": text, "scene": scene }),
                    &[
                        "resolve and type-check the parameter against the live block catalog",
                        "write the parameter and read back device state",
                    ],
                ))
            }
            BlockCmd::Bypass {
                row,
                column,
                bypass,
            } => {
                let row = validate_cell(*row, *column)?;
                Some(plan(
                    "block bypass",
                    "working grid",
                    serde_json::json!({ "wire_row": row.wire(), "screen_row": row.screen(), "column": column, "bypass": bypass }),
                    &["write bypass state"],
                ))
            }
            BlockCmd::Set {
                row,
                column,
                model,
                no_verify,
                timeout,
            } => {
                let row = validate_cell(*row, *column)?;
                Some(plan(
                    "block set",
                    "working grid",
                    serde_json::json!({ "wire_row": row.wire(), "screen_row": row.screen(), "column": column, "model": model, "verify": !no_verify, "timeout_seconds": timeout }),
                    &[
                        "place or replace the model",
                        "verify the device accepted the block when requested",
                    ],
                ))
            }
            BlockCmd::Remove { row, column } => {
                let row = validate_cell(*row, *column)?;
                Some(plan(
                    "block remove",
                    "working grid",
                    serde_json::json!({ "wire_row": row.wire(), "screen_row": row.screen(), "column": column }),
                    &["remove the block"],
                ))
            }
        },
        Command::Row { command } => match command {
            RowCmd::Input { row, port } => {
                let row = wire_row(*row)?;
                Some(plan(
                    "row input",
                    "working grid",
                    serde_json::json!({ "wire_row": row.wire(), "screen_row": row.screen(), "port": port }),
                    &["write the row input routing"],
                ))
            }
            RowCmd::Output { row, port } => {
                let row = wire_row(*row)?;
                Some(plan(
                    "row output",
                    "working grid",
                    serde_json::json!({ "wire_row": row.wire(), "screen_row": row.screen(), "port": port }),
                    &["write the row output routing"],
                ))
            }
            RowCmd::Split { row, split, mix } => {
                let row = wire_row(*row)?;
                cortex_rs::grid::set_split(row, *split, *mix)?;
                Some(plan(
                    "row split",
                    "working grid",
                    serde_json::json!({ "wire_row": row.wire(), "screen_row": row.screen(), "split": split, "mix": mix }),
                    &["write the row split and mix points"],
                ))
            }
        },
        Command::Device { command } => match command {
            DeviceCmd::Version { .. } => std::env::var("CORTEX_DUMP_VERSION").ok().map(|path| {
                plan(
                    "device version dump",
                    "local filesystem",
                    serde_json::json!({ "path": path }),
                    &["read the device version", "write the raw response payload"],
                )
            }),
            DeviceCmd::Cpu | DeviceCmd::Probe { .. } => None,
        },
        Command::Scene { index } => {
            if *index > 7 {
                anyhow::bail!("scene must be 0-7");
            }
            Some(plan(
                "scene",
                "audible state",
                serde_json::json!({ "index": index, "display": char::from_u32(65 + index).unwrap_or('?').to_string() }),
                &["switch the active scene"],
            ))
        }
        Command::Catalog { dump, .. } => dump.as_ref().map(|path| {
            plan(
                "catalog dump",
                "local filesystem",
                serde_json::json!({ "path": path }),
                &["read or load the model catalog", "write the raw payload"],
            )
        }),
        Command::DecodeTrace { .. } => None,
        Command::Completions { target, shell, dir } => {
            if matches!(target, CompletionTarget::Install) || dir.is_some() {
                Some(plan(
                    "completions install",
                    "local filesystem",
                    serde_json::json!({ "target": format!("{target:?}").to_lowercase(), "shell": shell.map(|shell| shell.to_string()), "dir": dir }),
                    &[
                        "create the destination directory if absent",
                        "write the completion script",
                    ],
                ))
            } else {
                None
            }
        }
    };
    Ok(plan)
}

/// Enumerate every folder the device knows about.
///
/// Exercises the session's `collect` primitive: one READ provokes many pushes
/// rather than one reply, so a single-shot waiter would see only the first.
fn cmd_folders(window: u64, show_empty: bool, fmt: Format) -> Result<()> {
    eprintln!("gathering folder announcements for {window}s ...");
    if let Some(result) = connect::request(&cortex_host::Request::ListFolders {
        window_seconds: window,
    }) {
        let folders: Vec<cortex_rs::client::Folder> = serde_json::from_value(result?)?;
        return emit_folders(folders, show_empty, fmt);
    }

    let session = open_device()?;
    session.connect(Duration::from_secs(10), Duration::from_secs(2))?;
    let qc = cortex_rs::QuadCortex::new(session.clone());

    let folders = qc.list_folders(Duration::from_secs(window))?;

    qc.disconnect();
    session.stop();

    emit_folders(folders, show_empty, fmt)
}

/// Filter and render folder announcements identically on direct and daemon paths.
fn emit_folders(
    folders: Vec<cortex_rs::client::Folder>,
    show_empty: bool,
    fmt: Format,
) -> Result<()> {
    // Hide the empty ones unless asked. The device reports 399 folders and
    // all but a handful hold nothing, so the default listing was mostly
    // noise obscuring its own useful lines.
    let total = folders.len();
    let folders: Vec<_> = folders
        .into_iter()
        .filter(|f| show_empty || f.occupied > 0)
        .collect();
    let hidden = total - folders.len();

    emit(&folders, fmt, move |folders| {
        for f in folders {
            println!(
                "{:>4}/{:<4} {}{}",
                f.occupied,
                f.slots,
                f.key,
                if f.is_factory { "  [factory]" } else { "" }
            );
        }
        if hidden > 0 {
            eprintln!("{hidden} empty folders hidden; --show-empty lists them");
        }
    })
}

/// Run the connect handshake, then exercise the read paths that depend on it.
///
/// The device will not push state to a client that has merely opened the pipe,
/// so a successful read here is the real proof the handshake landed - more
/// meaningful than counting inbound frames.
///
/// Read-only. Nothing here writes preset data or saves.
fn cmd_probe(listen: u64, fmt: Format) -> Result<()> {
    use std::io::Write;

    if connect::is_running() {
        return cmd_probe_held(listen, fmt);
    }

    // Each step prints before it runs, so a hang is attributable rather than
    // just a silent stall. Progress goes to stderr, results to stdout.
    macro_rules! step {
        ($($arg:tt)*) => {{
            eprint!("{} ... ", format!($($arg)*));
            let _ = std::io::stderr().flush();
        }};
    }

    step!("opening device");
    let session = open_device()?;
    eprintln!("ok");

    let started = std::time::Instant::now();
    eprintln!("connect handshake:");
    session.connect_with_progress(
        cortex_rs::ConnectMode::Subscribed,
        Duration::from_secs(10),
        Duration::from_secs(2),
        |s| eprintln!("  {s} ..."),
    )?;
    let handshake = started.elapsed().as_secs_f32();
    eprintln!("  done ({handshake:.1}s)");

    let qc = cortex_rs::QuadCortex::new(session.clone());

    // Keep listening for an explicit observation window when requested.
    if listen > 0 {
        step!("listening {listen}s for pushes");
        std::thread::sleep(Duration::from_secs(listen));
        eprintln!("ok");
    }

    // --- reads that require the handshake --------------------------------
    // Each read is attempted even if an earlier one failed: a probe that
    // stops at the first failure tells you less than one that reports which
    // paths work and which do not.
    let mut out = ProbeOut {
        handshake_seconds: handshake,
        active_scene: None,
        current_preset: None,
        current_preset_chains: None,
        preset_count: None,
        presets: Vec::new(),
        failures: Vec::new(),
    };

    step!("active_scene");
    match qc.active_scene(Duration::from_secs(10)) {
        Ok(scene) => {
            eprintln!("ok");
            out.active_scene = Some(scene);
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            out.failures.push(format!("active_scene: {e}"));
        }
    }

    step!("read_current_preset");
    match qc.read_current_preset(Duration::from_secs(15)) {
        Ok(preset) => {
            eprintln!("ok");
            out.current_preset = Some(
                preset
                    .name
                    .as_ref()
                    .map_or("<unnamed>", |n| {
                        let cortex_rs::proto::binary_preset::Name::Name(v) = n;
                        v.as_str()
                    })
                    .to_string(),
            );
            out.current_preset_chains = Some(preset.chains.len());
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            out.failures.push(format!("read_current_preset: {e}"));
        }
    }

    step!("list_presets");
    match qc.list_presets(
        cortex_rs::client::USER_SETLIST,
        Duration::from_secs(25),
        false,
    ) {
        Ok(entries) => {
            eprintln!("ok ({} occupied)", entries.len());
            out.preset_count = Some(entries.len());
            out.presets = entries.iter().map(PresetSlot::from).collect();
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            out.failures.push(format!("list_presets: {e}"));
        }
    }

    step!("disconnect");
    qc.disconnect();
    session.stop();
    eprintln!("ok");

    emit_probe(out, fmt)
}

fn cmd_probe_held(listen: u64, fmt: Format) -> Result<()> {
    if listen > 0 {
        eprintln!("listening {listen}s for pushes ...");
        std::thread::sleep(Duration::from_secs(listen));
    }
    let mut out = ProbeOut {
        handshake_seconds: 0.0,
        active_scene: None,
        current_preset: None,
        current_preset_chains: None,
        preset_count: None,
        presets: Vec::new(),
        failures: Vec::new(),
    };

    match connect::request(&cortex_host::Request::ActiveScene) {
        Some(Ok(value)) => match serde_json::from_value(value) {
            Ok(scene) => out.active_scene = Some(scene),
            Err(error) => out.failures.push(format!("active_scene: {error}")),
        },
        Some(Err(error)) => out.failures.push(format!("active_scene: {error}")),
        None => out.failures.push("active_scene: daemon disappeared".into()),
    }
    match connect::request(&cortex_host::Request::CurrentPreset {
        with_params: false,
        timeout_seconds: 15,
    }) {
        Some(Ok(value)) => match serde_json::from_value::<cortex_rs::view::Preset>(value) {
            Ok(preset) => {
                out.current_preset = Some(preset.name);
                out.current_preset_chains = Some(preset.chains);
            }
            Err(error) => out.failures.push(format!("read_current_preset: {error}")),
        },
        Some(Err(error)) => out.failures.push(format!("read_current_preset: {error}")),
        None => out
            .failures
            .push("read_current_preset: daemon disappeared".into()),
    }
    match connect::request(&cortex_host::Request::ListPresets {
        setlist: cortex_rs::client::USER_SETLIST.into(),
        include_empty: false,
        timeout_seconds: 25,
    }) {
        Some(Ok(value)) => match serde_json::from_value::<Vec<PresetSlot>>(value) {
            Ok(presets) => {
                out.preset_count = Some(presets.len());
                out.presets = presets;
            }
            Err(error) => out.failures.push(format!("list_presets: {error}")),
        },
        Some(Err(error)) => out.failures.push(format!("list_presets: {error}")),
        None => out.failures.push("list_presets: daemon disappeared".into()),
    }

    emit_probe(out, fmt)
}

fn emit_probe(out: ProbeOut, fmt: Format) -> Result<()> {
    let failed = out.failures.len();
    emit(&out, fmt, |o| {
        if let Some(scene) = o.active_scene {
            println!("active_scene: {scene}");
        }
        if let Some(name) = &o.current_preset {
            println!("current_preset: {name}");
        }
        if let Some(n) = o.current_preset_chains {
            println!("current_preset_chains: {n}");
        }
        if let Some(n) = o.preset_count {
            println!("preset_count: {n}");
        }
        for e in o.presets.iter().take(10) {
            println!("  {:>4}  {}", e.slot, e.name);
        }
        if o.presets.len() > 10 {
            println!("  ... and {} more", o.presets.len() - 10);
        }
    })?;

    if failed > 0 {
        anyhow::bail!("{failed} read path(s) failed - see stderr above");
    }
    Ok(())
}

/// Read the version via the session layer: opens a `Session` (spawning the
/// background RX and keepalive threads) but does NOT run the connect
/// handshake, because a `Version` READ is answered without one.
///
/// This mirrors pyquadcortex's `_open_unconnected()`, which exists for the
/// same reason: the handshake announces our own version, and that announce
/// would race a caller's `Version` READ reply (READ replies carry no
/// `request_id` to disambiguate).
fn cmd_version_via_session(fmt: Format) -> Result<()> {
    let session = open_device()?;
    let qc = cortex_rs::QuadCortex::new(session.clone());

    let result = qc.version(Duration::from_secs(10));

    // Stop the background threads before dropping, so the HID handle is not
    // closed while the RX thread is still inside read().
    session.stop();

    emit(&DeviceVersion::from(&result?), fmt, print_device_version)
}

fn cmd_version(fmt: Format) -> Result<()> {
    // Ask the held session if there is one. This command is the natural
    // "is my unit connected?" check, so refusing it whenever a daemon runs
    // would break it exactly when the answer is most obviously yes - and it
    // cannot open the device for itself, because doing so alongside a held
    // session wedges both.
    if let Some(result) = connect::request(&cortex_host::Request::Version) {
        let parsed: DeviceVersion = serde_json::from_value(result?)?;
        return emit(&parsed, fmt, print_device_version);
    }

    // Still guarded: a daemon could have started between the probe above and
    // the open below.
    let _claim = claim_device()?;
    let transport = Transport::open(DeviceKind::QuadCortex)?;

    // Build a VersionMessage with action = READ. The version command works
    // without the full connect handshake - a plain Version READ gets a reply.
    let request = VersionMessage {
        action: MessageAction::Read as i32,
        ..Default::default()
    };
    let payload = prost::Message::encode_to_vec(&request);

    let reply = transport.request(
        MessageType::Version as u16,
        &payload,
        Duration::from_secs(10),
    )?;

    if let Ok(path) = std::env::var("CORTEX_DUMP_VERSION") {
        let _ = std::fs::write(path, reply.body.as_ref());
    }
    let version: VersionMessage = prost::Message::decode(reply.body.as_ref())
        .map_err(|e| anyhow::anyhow!("protobuf decode error: {e}"))?;

    emit(&DeviceVersion::from(&version), fmt, print_device_version)
}

/// Reset SIGPIPE to SIG_DFL so output pipes into `head`/`less` without a
/// panic on a closed pipe. See house-style rust-cli.md.
#[cfg(unix)]
unsafe fn libc_sigpipe_reset() {
    // SAFETY: `signal(SIGPIPE, SIG_DFL)` is a thread-safe libc call with no
    // preconditions; we ignore the return value (the previous handler).
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;
    let _ = unsafe { signal(SIGPIPE, SIG_DFL) };
}

/// List the presets in a setlist.
fn cmd_presets(setlist: &str, include_empty: bool, timeout: u64, fmt: Format) -> Result<()> {
    // Prefer a held session. A listing is the most expensive read the CLI
    // does, and the daemon has already paid for the handshake.
    if let Some(result) = connect::request(&cortex_host::Request::ListPresets {
        setlist: setlist.to_string(),
        include_empty,
        timeout_seconds: timeout,
    }) {
        let entries: Vec<PresetSlot> = serde_json::from_value(result?)?;
        return emit(&entries, fmt, print_preset_entries);
    }

    let session = open_device()?;
    session.connect(Duration::from_secs(10), Duration::from_secs(2))?;
    let qc = cortex_rs::QuadCortex::new(session.clone());

    let result = qc.list_presets(setlist, Duration::from_secs(timeout), include_empty);

    qc.disconnect();
    session.stop();

    let entries: Vec<PresetSlot> = result?.iter().map(PresetSlot::from).collect();
    emit(&entries, fmt, print_preset_entries)
}

/// Print a setlist listing. Shared so the routed and direct paths cannot
/// drift into two formats.
fn print_preset_entries(entries: &Vec<PresetSlot>) {
    for e in entries {
        println!(
            "{:>4}  {}",
            e.slot,
            if e.name.is_empty() { "-" } else { &e.name }
        );
    }
}

/// The session currently held by a running command, so a signal handler can
/// announce the disconnect before the process dies.
///
/// A single slot is enough: commands are sequential and only ever hold one
/// session at a time.
static ACTIVE_SESSION: std::sync::Mutex<Option<std::sync::Arc<cortex_rs::Session>>> =
    std::sync::Mutex::new(None);
static ACTIVE_DEVICE_CLAIM: std::sync::Mutex<Option<cortex_host::LocalClaim>> =
    std::sync::Mutex::new(None);

/// Tell the device we are going away, if a session is open.
///
/// Called from the signal handler and on the normal path. Idempotent: it
/// takes the session out of the slot, so a second call finds nothing.
fn release_session() {
    let held = ACTIVE_SESSION.lock().ok().and_then(|mut g| g.take());
    if let Some(session) = held {
        session.disconnect();
        session.stop();
    }
    if let Ok(mut claim) = ACTIVE_DEVICE_CLAIM.lock() {
        claim.take();
    }
}

/// Announce the disconnect on SIGINT/SIGTERM rather than just dying.
///
/// This matters more than it looks. Terminating without telling the device
/// leaves it pushing state to a client that has gone, and the NEXT session
/// then contends with that backlog - which surfaces as reads timing out
/// after an apparently successful handshake. Destructors do not run on a
/// signal, so `Drop` cannot cover this case.
fn install_signal_handler() {
    let result = ctrlc::set_handler(|| {
        eprintln!();
        eprintln!("interrupted: telling the device we are going away ...");
        release_session();
        // 130 is the conventional "terminated by SIGINT".
        std::process::exit(130);
    });
    if let Err(e) = result {
        // Not fatal: the tool still works, it is just less polite about
        // being interrupted.
        eprintln!("warning: could not install the interrupt handler ({e})");
    }
}

/// Open a connected session. Every device-touching command needs the
/// handshake, so this is the shared preamble.
fn connected() -> Result<(std::sync::Arc<cortex_rs::Session>, cortex_rs::QuadCortex)> {
    // Minimal by default. A one-shot command sends its own READ, which the
    // device answers without a subscription - and subscribing makes it dump
    // its entire state first, which is most of the handshake cost.
    open_session(cortex_rs::ConnectMode::Minimal)
}

/// Refuse to touch the device directly while a daemon holds it.
///
/// Hardware-verified, the hard way: running a command that opened the device
/// for itself while `cortex session start` held a session left every later read on
/// that held session timing out. Nothing errored at the point of the
/// collision - the damage only showed up on the next request - which is what
/// makes it worth refusing loudly here rather than hoping.
///
/// The device also went silent at the same moment. That looked like weak
/// evidence for a while, because our own sessions were falling silent for
/// unrelated reasons (too slow a keepalive, since fixed). The read failures
/// are the sound part regardless.
///
/// This is the CLI-side expression of the exclusive-access invariant in
/// AGENTS.md: one owning process per device, not one connection per call.
fn claim_device() -> Result<cortex_host::LocalClaim> {
    cortex_host::LocalClaim::acquire(&cortex_host::LocalEndpoint::daemon()).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AddrInUse {
            anyhow::anyhow!(
                "another cortex process is already holding the device.\n\
                 Opening it again here would wedge both sessions - the device \
                 stops answering both of them.\n\
                 If it is a held session, use it or stop it with `cortex session stop`."
            )
        } else {
            anyhow::Error::new(error).context("claiming exclusive Cortex device access")
        }
    })
}

/// Open the device for a one-shot command, refusing if a daemon holds it.
fn open_device() -> Result<std::sync::Arc<cortex_rs::Session>> {
    let claim = claim_device()?;
    let session = std::sync::Arc::new(cortex_rs::Session::open(DeviceKind::QuadCortex)?);
    *ACTIVE_DEVICE_CLAIM
        .lock()
        .map_err(|_| anyhow::anyhow!("exclusive device claim lock was poisoned"))? = Some(claim);
    Ok(session)
}

fn open_session(
    mode: cortex_rs::ConnectMode,
) -> Result<(std::sync::Arc<cortex_rs::Session>, cortex_rs::QuadCortex)> {
    let session = open_device()?;
    // Make it reachable from the signal handler before the handshake, since
    // an interrupt during the handshake is exactly the case that leaves the
    // device in a bad state.
    if let Ok(mut slot) = ACTIVE_SESSION.lock() {
        *slot = Some(session.clone());
    }
    // Progress to stderr: the handshake is several seconds of silence
    // otherwise, and the device's state dump can stretch it further, which
    // reads as a hang.
    session.connect_with_progress(mode, Duration::from_secs(10), Duration::from_secs(2), |s| {
        eprintln!("connecting: {s} ...");
    })?;
    let qc = cortex_rs::QuadCortex::new(session.clone());
    Ok((session, qc))
}

/// Recall a preset by slot.
fn cmd_recall(slot: &str, setlist: &str, factory: bool, fmt: Format) -> Result<()> {
    // Through the daemon when there is one. It keeps its session, so the
    // settling sleep the direct path needs is unnecessary there.
    if let Some(result) = connect::request(&cortex_host::Request::RecallPreset {
        setlist: setlist.to_string(),
        slot: slot.to_string(),
        factory,
    }) {
        result?;
        let out = ActionOut {
            action: "recall".into(),
            detail: format!("{slot} in {setlist}"),
        };
        return emit(&out, fmt, |o| println!("{}: {}", o.action, o.detail));
    }

    let (session, qc) = connected()?;
    let result = qc.recall_preset(setlist, slot, factory, Duration::from_secs(40));
    qc.disconnect();
    session.stop();
    result?;
    let out = ActionOut {
        action: "recall".into(),
        detail: format!("{slot} in {setlist}"),
    };
    emit(&out, fmt, |o| println!("{}: {}", o.action, o.detail))
}

/// Prepare one target while the working grid is known clean.
fn cmd_preset_prepare_save(slot: &str, setlist: &str, fmt: Format) -> Result<()> {
    if cortex_rs::client::is_factory_setlist(setlist) {
        anyhow::bail!("{setlist} is the factory library and is not writable");
    }
    if cortex_rs::client::slot_to_position_checked(slot).is_none() {
        anyhow::bail!(
            "{slot} is not a slot. Slots are a bank number 1-32 then a letter A-H, e.g. 2B"
        );
    }
    let Some(result) = connect::request(&cortex_host::Request::PrepareSave {
        setlist: setlist.to_string(),
        slot: slot.to_string(),
        recall_consent: cortex_rs::RecallConsent::RequireClean,
        timeout_seconds: 40,
    }) else {
        anyhow::bail!(
            "saving requires `cortex session start`: preparation must survive until after editing"
        )
    };
    let prepare_result: cortex_host::PrepareSaveResult = serde_json::from_value(result?)?;
    emit(&prepare_result, fmt, |result| {
        println!(
            "prepared {} in {}",
            result.view.target.slot, result.view.target.setlist
        );
        println!("token: {}", result.token);
    })
}

/// Commit a token prepared before working-grid edits.
fn cmd_preset_save(token: &str, name: Option<&str>, fmt: Format) -> Result<()> {
    let Some(result) = connect::request(&cortex_host::Request::CommitSave {
        token: token.to_string(),
        confirmed: true,
        name: name.map(str::to_string),
        timeout_seconds: 40,
    }) else {
        anyhow::bail!("saving requires `cortex session start`; prepare the target before editing")
    };
    let view: cortex_rs::SavePreparationView = serde_json::from_value(result?)?;
    report_edit(
        "save",
        format!("{} in {}", view.target.slot, view.target.setlist),
        fmt,
    )
}

/// Delete a preset by name.
fn cmd_preset_delete(name: &str, setlist: &str, fmt: Format) -> Result<()> {
    if cortex_rs::client::is_factory_setlist(setlist) {
        anyhow::bail!("{setlist} is the factory library and is not writable");
    }
    let detail = format!("{name:?} from {setlist}");

    if let Some(result) = connect::request(&cortex_host::Request::DeletePreset {
        setlist: setlist.to_string(),
        name: name.to_string(),
    }) {
        result?;
        return report_edit("delete", detail, fmt);
    }

    let (session, qc) = connected()?;
    let result = qc.delete_preset(setlist, name, Duration::from_secs(20));
    qc.disconnect();
    session.stop();
    result?;
    report_edit("delete", detail, fmt)
}

/// Move a preset from one slot to an empty slot in the same setlist.
fn cmd_preset_move(from_slot: &str, to_slot: &str, setlist: &str, fmt: Format) -> Result<()> {
    if cortex_rs::client::is_factory_setlist(setlist) {
        anyhow::bail!("{setlist} is the factory library and is not writable");
    }
    for slot in [from_slot, to_slot] {
        if cortex_rs::client::slot_to_position_checked(slot).is_none() {
            anyhow::bail!(
                "{slot} is not a slot. Slots are a bank number 1-32 then a letter A-H, e.g. 2B"
            );
        }
    }
    let detail = format!("{from_slot} to {to_slot} in {setlist}");
    if let Some(result) = connect::request(&cortex_host::Request::MovePreset {
        setlist: setlist.to_string(),
        from_slot: from_slot.to_string(),
        to_slot: to_slot.to_string(),
        confirmed: true,
    }) {
        result?;
        return report_edit("move", detail, fmt);
    }

    let (session, qc) = connected()?;
    let policy = cortex_rs::SavePolicy::new(
        setlist,
        vec![
            cortex_rs::ScratchRange::new(from_slot, from_slot)?,
            cortex_rs::ScratchRange::new(to_slot, to_slot)?,
        ],
    )?;
    let result = qc.move_preset(
        &policy,
        setlist,
        from_slot,
        to_slot,
        Duration::from_secs(30),
    );
    qc.disconnect();
    session.stop();
    result?;
    report_edit("move", detail, fmt)
}

/// Switch the active scene.
fn cmd_scene(index: u32, fmt: Format) -> Result<()> {
    // Prefer a held connection. It has already paid the handshake, so this
    // is a socket round trip rather than a fresh session - and it avoids
    // contending for a device interface the daemon already owns.
    if let Some(result) = connect::request(&cortex_host::Request::SwitchScene { scene: index }) {
        result?;
        return report_edit("scene", index.to_string(), fmt);
    }

    let (session, qc) = connected()?;
    let result = qc.switch_scene(index);
    std::thread::sleep(Duration::from_millis(500));
    qc.disconnect();
    session.stop();
    result?;
    let out = ActionOut {
        action: "scene".into(),
        detail: index.to_string(),
    };
    emit(&out, fmt, |o| println!("{}: {}", o.action, o.detail))
}

/// Recall a slot and dump the preset it loads, naming each block.
fn cmd_preset(slot: &str, setlist: &str, factory: bool, params: bool, fmt: Format) -> Result<()> {
    if let Some(result) = connect::request(&cortex_host::Request::ReadPreset {
        setlist: setlist.to_string(),
        slot: slot.to_string(),
        factory,
        with_params: params,
        timeout_seconds: 40,
    }) {
        let preset: PresetOut = serde_json::from_value(result?)?;
        return emit(&preset, fmt, print_preset);
    }

    let (session, qc) = connected()?;

    // Fetch the catalog in the same session: without it a block is just an
    // opaque integer. Failure here is not fatal - a preset with numeric
    // blocks still beats no preset at all - so it degrades rather than
    // aborting.
    let catalog = match qc.fetch_model_repo(Duration::from_secs(40)) {
        Ok(payload) => cortex_rs::Catalog::parse(&payload).ok(),
        Err(e) => {
            eprintln!("warning: could not fetch the model catalog ({e}); blocks will show ids");
            None
        }
    };

    let result = qc.read_preset(setlist, slot, factory, Duration::from_secs(40));
    qc.disconnect();
    session.stop();

    let preset = result?;
    let out = PresetOut::from_binary(&preset, catalog.as_ref(), slot, setlist, params);
    emit(&out, fmt, print_preset)
}

/// Fetch, parse, and query the device model catalog.
fn cmd_catalog(
    search: Option<&str>,
    model: Option<u32>,
    dump: Option<&std::path::Path>,
    from_file: Option<&std::path::Path>,
    timeout: u64,
    fmt: Format,
) -> Result<()> {
    // Reading a dumped payload keeps catalog work testable without tying up
    // the device, and without a 2 s handshake per iteration.
    let payload = if let Some(path) = from_file {
        std::fs::read(path)?
    } else if let Some(result) = connect::request(&cortex_host::Request::Catalog {
        timeout_seconds: timeout,
    }) {
        // The daemon already holds this from its handshake, so this is a
        // socket read rather than another 46 KB transfer off the device.
        let value = result?;
        serde_json::from_value(
            value
                .get("payload")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("daemon returned no catalog payload"))?,
        )?
    } else {
        let (session, qc) = connected()?;
        let result = qc.fetch_model_repo(Duration::from_secs(timeout));
        qc.disconnect();
        session.stop();
        result?
    };

    if let Some(path) = dump {
        std::fs::write(path, &payload)?;
        eprintln!("raw payload written to {}", path.display());
    }

    let catalog = cortex_rs::Catalog::parse(&payload)?;

    if let Some(id) = model {
        let Some(m) = catalog.get(id) else {
            anyhow::bail!("no model with id {id}; try --search to find one");
        };
        return emit(&ModelOut::from(m), fmt, print_model);
    }

    if let Some(needle) = search {
        let found: Vec<ModelOut> = catalog
            .search(needle)
            .into_iter()
            .map(ModelOut::from)
            .collect();
        // Say when nothing matched. Printing nothing at all is
        // indistinguishable from a broken command, and the catalog is full of
        // renamed models - the unit has no "Carvin", it has a "Solo 100" that
        // is a Soldano - so a search finding nothing is a normal outcome that
        // needs to look like one.
        //
        // The message goes to stderr and stdout stays empty, so a script
        // piping this still gets nothing rather than prose.
        if found.is_empty() {
            eprintln!(
                "no model matches '{needle}'. Names are Neural DSP's own, but the \
                 manufacturer they are based on is searchable too - try a maker \
                 ('marshall', 'soldano') or a family ('plexi', 'od')."
            );
        }
        return emit(&found, fmt, |found| {
            for m in found {
                print_model_line(m);
            }
        });
    }

    let models = catalog.models();
    let mut categories: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for m in &models {
        *categories.entry(m.category.clone()).or_default() += 1;
    }
    let summary = CatalogSummaryOut {
        models: models.len(),
        with_attribution: models.iter().filter(|m| m.based_on.is_some()).count(),
        categories,
    };
    emit(&summary, fmt, |s| {
        println!("models: {}", s.models);
        println!("with_attribution: {}", s.with_attribution);
        println!("categories: {}", s.categories.len());
        for (name, count) in &s.categories {
            println!("{count:>5}  {name}");
        }
    })
}

/// One line per model, for search results.
fn print_model_line(m: &ModelOut) {
    print!("{:>6}  {:<28} {}", m.id, m.name, m.category);
    if let Some(tm) = &m.based_on {
        print!("  [{tm}]");
    }
    println!();
}

/// A model in full, with its parameters in wire index order.
fn print_model(m: &ModelOut) {
    println!("id: {}", m.id);
    println!("name: {}", m.name);
    println!("category: {}", m.category);
    // Neural DSP's own wording about other companies' marks - printed
    // verbatim, never paraphrased.
    if let Some(tm) = &m.based_on {
        println!("attribution: {tm}");
    }
    println!("parameters: {}", m.parameters.len());
    for p in &m.parameters {
        print!(
            "{:>4}  {:<20} {:<8} {}..{}",
            p.index, p.name, p.kind, p.min, p.max
        );
        if !p.units.is_empty() {
            print!(" {}", p.units);
        }
        if !p.step_names.is_empty() {
            print!("  [{}]", p.step_names.join(", "));
        }
        if p.read_only {
            print!("  (read-only meter)");
        }
        println!();
    }
}

/// What `cortex completions` was asked to do: emit a script for a named
/// shell, or install for the shell the user is actually running.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum CompletionTarget {
    /// Detect the current shell and install into its standard directory.
    Install,
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
}

impl CompletionTarget {
    fn as_shell(self) -> Option<clap_complete::Shell> {
        use clap_complete::Shell;
        match self {
            Self::Install => None,
            Self::Bash => Some(Shell::Bash),
            Self::Elvish => Some(Shell::Elvish),
            Self::Fish => Some(Shell::Fish),
            Self::Powershell => Some(Shell::PowerShell),
            Self::Zsh => Some(Shell::Zsh),
        }
    }
}

/// The file name a shell expects for a completion script.
fn completion_file_name(shell: clap_complete::Shell) -> String {
    use clap_complete::Shell;
    match shell {
        Shell::Bash => "cortex".into(),
        Shell::Zsh => "_cortex".into(),
        Shell::Fish => "cortex.fish".into(),
        Shell::PowerShell => "cortex.ps1".into(),
        Shell::Elvish => "cortex.elv".into(),
        other => format!("cortex.{other}"),
    }
}

/// The conventional per-user completion directory for a shell.
fn completion_dir(shell: clap_complete::Shell) -> Option<std::path::PathBuf> {
    use clap_complete::Shell;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Some(match shell {
        Shell::Bash => data.join("bash-completion/completions"),
        // ~/.zfunc rather than an XDG path: it is what several widely-used
        // Rust CLIs (rustup among them) tell people to add to their fpath,
        // so a user who already has one gets every tool in one directory
        // instead of a new fpath entry per tool. House-style convention.
        Shell::Zsh => home.join(".zfunc"),
        Shell::Fish => config.join("fish/completions"),
        Shell::Elvish => config.join("elvish/lib"),
        Shell::PowerShell => return None,
        _ => return None,
    })
}

/// Guess the running shell from $SHELL.
fn detect_shell() -> Option<clap_complete::Shell> {
    use clap_complete::Shell;
    let shell = std::env::var("SHELL").ok()?;
    let name = std::path::Path::new(&shell).file_name()?.to_str()?;
    match name {
        "bash" => Some(Shell::Bash),
        "zsh" => Some(Shell::Zsh),
        "fish" => Some(Shell::Fish),
        "elvish" => Some(Shell::Elvish),
        "pwsh" | "powershell" => Some(Shell::PowerShell),
        _ => None,
    }
}

/// Anything the user still has to do once by hand. We deliberately do NOT
/// edit shell startup files: silently rewriting someone's rc file is a
/// surprising side effect, and a wrong guess is painful to undo.
fn post_install_hint(shell: clap_complete::Shell, dir: &std::path::Path) -> Option<String> {
    use clap_complete::Shell;
    match shell {
        Shell::Zsh => Some(format!(
            "Ensure this is on your fpath, before `compinit`, in ~/.zshrc:\n\n  fpath=({} $fpath)\n  autoload -Uz compinit && compinit",
            dir.display()
        )),
        Shell::Bash => Some(
            "Most distributions source this directory automatically via bash-completion.\nIf yours does not, add to ~/.bashrc:\n\n  source /usr/share/bash-completion/bash_completion"
                .into(),
        ),
        Shell::Elvish => Some(format!(
            "Add to ~/.config/elvish/rc.elv:\n\n  use {}",
            completion_file_name(shell).trim_end_matches(".elv")
        )),
        // fish scans its completions directory with no extra setup.
        _ => None,
    }
}

/// Generate or install shell completions.
fn cmd_completions(
    target: CompletionTarget,
    shell_override: Option<clap_complete::Shell>,
    dir: Option<&std::path::Path>,
) -> Result<()> {
    use clap::CommandFactory;

    let mut command = Cli::command();

    // A named shell with no --dir is the packager path: script to stdout,
    // nothing touched on disk.
    if let Some(shell) = target.as_shell() {
        if dir.is_none() {
            clap_complete::generate(shell, &mut command, "cortex", &mut std::io::stdout());
            return Ok(());
        }
    }

    let shell = match target.as_shell().or(shell_override).or_else(detect_shell) {
        Some(s) => s,
        None => anyhow::bail!(
            "could not determine your shell from $SHELL; pass one explicitly, \n             e.g. `cortex completions install --shell zsh`"
        ),
    };

    let target_dir = match dir {
        Some(d) => d.to_path_buf(),
        None => completion_dir(shell).ok_or_else(|| {
            anyhow::anyhow!(
                "no conventional completion directory is known for {shell}; \n                 pass one with --dir, or redirect `cortex completions {shell}` to a file"
            )
        })?,
    };

    std::fs::create_dir_all(&target_dir)?;
    let path = target_dir.join(completion_file_name(shell));
    let mut file = std::fs::File::create(&path)?;
    clap_complete::generate(shell, &mut command, "cortex", &mut file);

    // Result to stdout, guidance to stderr, so this stays scriptable.
    println!("{}", path.display());
    eprintln!("Installed {shell} completions.");
    if let Some(hint) = post_install_hint(shell, &target_dir) {
        eprintln!();
        eprintln!("{hint}");
    }
    eprintln!();
    eprintln!("Start a new shell to pick them up.");
    Ok(())
}

/// Whether `--row` arrives zero-based, from the global `--zero-based` flag.
///
/// A process-wide read-once value rather than an argument on every command
/// that takes a row: it is a property of how the caller talks to us, not a
/// decision any individual command makes.
static ZERO_BASED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Resolve a `--row` argument into the zero-based wire row.
///
/// Defaults to the 1-4 printed on the unit, because the CLI is for players.
/// `--zero-based` switches to the wire's own 0-3, which is what a script
/// already holds.
///
/// The conversion has to be explicit: a wrong row is accepted, reads back
/// correctly, and changes the wrong thing.
fn wire_row(row: u32) -> Result<cortex_rs::Row> {
    if *ZERO_BASED.get().unwrap_or(&false) {
        if row > 3 {
            anyhow::bail!("row {row} is out of range: --zero-based rows are 0-3");
        }
        return Ok(cortex_rs::Row::try_from_wire(row)?);
    }
    Ok(cortex_rs::Row::from_screen(row)?)
}

/// Render a stored parameter value for a human.
///
/// The view keeps these as `f64` so an int value survives intact, but the
/// exact decimal expansion of a float is noise to read - `0.14583329856395721`
/// where `0.145833` says the same thing. JSON output is unaffected and keeps
/// full precision.
fn number(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".into() } else { s.into() }
}

/// Report a grid edit, in whichever format was asked for.
fn report_edit(action: &str, detail: String, fmt: Format) -> Result<()> {
    let out = ActionOut {
        action: action.into(),
        detail,
    };
    emit(&out, fmt, |o| println!("{}: {}", o.action, o.detail))
}

/// Set a block parameter.
#[allow(clippy::too_many_arguments)]
fn cmd_set_param(
    row: u32,
    column: u32,
    param: Option<&str>,
    index: Option<u32>,
    value: Option<f32>,
    real: Option<f64>,
    text: Option<&str>,
    scene: Option<u32>,
    fmt: Format,
) -> Result<()> {
    let row = wire_row(row)?;
    let target = if let Some(name) = param {
        cortex_rs::ParameterTarget::Name(name.to_string())
    } else {
        cortex_rs::ParameterTarget::Index(index.unwrap_or_default())
    };
    let input = match (value, real, text) {
        (Some(value), None, None) => cortex_rs::ParameterInput::Normalised(value),
        (None, Some(value), None) => cortex_rs::ParameterInput::Real(value),
        (None, None, Some(value)) => cortex_rs::ParameterInput::Text(value.to_string()),
        (None, None, None) => {
            anyhow::bail!("give a value: --value (0.0-1.0), --real (own units), or --text")
        }
        _ => unreachable!("clap prevents more than one parameter value"),
    };

    if let Some(result) = connect::request(&cortex_host::Request::SetParam {
        row: row.wire(),
        column,
        target: target.clone(),
        input: input.clone(),
        scene,
        promote: scene.is_some(),
        timeout_seconds: 40,
    }) {
        let applied: cortex_rs::ParameterWrite = serde_json::from_value(result?)?;
        return report_edit(
            "set_param",
            parameter_detail(row, column, &applied, scene),
            fmt,
        );
    }

    let (session, qc) = connected()?;
    let outcome = qc.set_parameter(
        row,
        column,
        target,
        input,
        scene,
        scene.is_some(),
        Duration::from_secs(40),
    );

    qc.disconnect();
    session.stop();
    let applied = outcome?;
    report_edit(
        "set_param",
        parameter_detail(row, column, &applied, scene),
        fmt,
    )
}

/// Describe the resolved parameter write identically on direct and daemon paths.
fn parameter_detail(
    row: cortex_rs::Row,
    column: u32,
    applied: &cortex_rs::ParameterWrite,
    scene: Option<u32>,
) -> String {
    let shown = format!("{:?}", applied.value);
    match scene {
        Some(scene) => format!(
            "row {} column {column} param {} = {shown} on scene {scene} \
             (the unit is now sitting on that scene)",
            row.screen(),
            applied.index
        ),
        None => format!(
            "row {} column {column} param {} = {shown} on the active scene",
            row.screen(),
            applied.index
        ),
    }
}

/// Bypass or enable a block.
fn cmd_set_bypass(row: u32, column: u32, bypass: bool, fmt: Format) -> Result<()> {
    let row = wire_row(row)?;
    let detail = format!(
        "row {} column {column} {}",
        row.screen(),
        if bypass { "bypassed" } else { "enabled" }
    );

    if let Some(result) = connect::request(&cortex_host::Request::SetBypass {
        row: row.wire(),
        column,
        bypass,
    }) {
        result?;
        return report_edit("set_bypass", detail, fmt);
    }

    let (session, qc) = connected()?;
    let result = qc.set_bypass(row, column, bypass);
    qc.disconnect();
    session.stop();
    result?;
    report_edit("set_bypass", detail, fmt)
}

/// Place a model in a grid cell.
fn cmd_set_block(
    row: u32,
    column: u32,
    model: u32,
    no_verify: bool,
    timeout: u64,
    fmt: Format,
) -> Result<()> {
    let row = wire_row(row)?;
    if let Some(result) = connect::request(&cortex_host::Request::SetBlock {
        row: row.wire(),
        column,
        model,
        verify: !no_verify,
        timeout_seconds: timeout,
    }) {
        let placement: cortex_rs::Placement = serde_json::from_value(result?)?;
        return report_block_placement(row, column, model, placement, fmt);
    }

    let (session, qc) = connected()?;
    let result = if no_verify {
        qc.set_block_unverified(row, column, model)
            .map(|()| cortex_rs::Placement::Unverified)
    } else {
        qc.set_block(row, column, model, Duration::from_secs(timeout))
    };
    qc.disconnect();
    session.stop();
    report_block_placement(row, column, model, result?, fmt)
}

/// Report how a block placement was confirmed.
fn report_block_placement(
    row: cortex_rs::Row,
    column: u32,
    model: u32,
    placement: cortex_rs::Placement,
    fmt: Format,
) -> Result<()> {
    // Say which check actually confirmed it. Reporting "echo confirmed" when
    // the echo timed out and a read-back rescued it would be a small lie
    // about how much the device actually told us.
    let how = match placement {
        cortex_rs::Placement::EchoConfirmed => " (echo confirmed)",
        cortex_rs::Placement::ReadBackConfirmed => {
            " (no echo in time; confirmed by reading the grid back)"
        }
        cortex_rs::Placement::Unverified => " (unverified)",
    };
    report_edit(
        "set_block",
        format!("model {model} at row {} column {column}{how}", row.screen()),
        fmt,
    )
}

/// Remove the block at a grid cell.
fn cmd_remove_block(row: u32, column: u32, fmt: Format) -> Result<()> {
    let row = wire_row(row)?;
    let detail = format!("row {} column {column}", row.screen());

    if let Some(result) = connect::request(&cortex_host::Request::RemoveBlock {
        row: row.wire(),
        column,
    }) {
        result?;
        return report_edit("remove_block", detail, fmt);
    }

    let (session, qc) = connected()?;
    let result = qc.remove_block(row, column);
    qc.disconnect();
    session.stop();
    result?;
    report_edit("remove_block", detail, fmt)
}

/// Human-readable rendering of a preset's grid.
fn print_preset(o: &PresetOut) {
    println!("slot: {}", o.slot);
    println!("name: {}", o.name);
    println!("chains: {}", o.chains);
    for row in 0..o.chains {
        let routing = o
            .rows
            .iter()
            .find(|r| r.row == row)
            .map_or_else(String::new, |r| {
                let mut parts = Vec::new();
                if let Some(p) = r.in_port {
                    parts.push(format!("in {p}"));
                }
                if let Some(p) = r.out_port {
                    parts.push(format!("out {p}"));
                }
                if let Some(c) = r.split_at {
                    parts.push(match r.mix_at {
                        Some(m) => format!("split {c} rejoin {m}"),
                        None => format!("split {c}, no rejoin"),
                    });
                }
                if parts.is_empty() {
                    String::new()
                } else {
                    format!("  [{}]", parts.join(", "))
                }
            });
        println!("row {row} (screen row {}):{routing}", row + 1);
        for b in o.blocks.iter().filter(|b| b.row == row) {
            match (&b.name, &b.category) {
                (Some(name), Some(cat)) => {
                    print!("  col {}: {name} ({cat})", b.column);
                    if let Some(tm) = &b.based_on {
                        print!("  [{tm}]");
                    }
                    println!();
                }
                _ => println!("  col {}: model {}", b.column, b.model_id),
            }
            // Bypass on the ACTIVE scene is scene_bypass[0]; show the whole
            // stored set so a per-scene difference is visible.
            if let Some(bp) = &b.bypass {
                if bp.scenes.iter().any(|&x| x) {
                    let per_scene: String = bp
                        .scenes
                        .iter()
                        .map(|&x| if x { 'x' } else { '.' })
                        .collect();
                    println!("      bypass: {per_scene}  (x = bypassed, scenes A-H)");
                }
            }
            for p in &b.params {
                let label = p
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("param {}", p.index));
                match &p.value {
                    ParamValueKind::Number(v) => {
                        print!("      {:>3}  {label:<18} {}", p.index, number(*v));
                    }
                    ParamValueKind::Text(v) => print!("      {:>3}  {label:<18} {v:?}", p.index),
                }
                if !p.per_scene.is_empty() {
                    let scenes: Vec<String> = p
                        .per_scene
                        .iter()
                        .map(|v| match v {
                            ParamValueKind::Number(n) => number(*n),
                            ParamValueKind::Text(t) => format!("{t:?}"),
                        })
                        .collect();
                    print!("   per-scene A-H: [{}]", scenes.join(", "));
                }
                println!();
            }
        }
    }
}

/// Dump the LIVE grid, naming each block through the catalog.
///
/// Uses `read_current_preset`, which has no side effects - unlike reading a
/// stored slot, which the device can only do by recalling it. This is the
/// command to use while editing.
fn cmd_grid(timeout: u64, params: bool, fmt: Format) -> Result<()> {
    // Use the held session when one exists. This still performs a
    // side-effect-free live-grid read; the subscribed push-maintained cache
    // is separate work tracked by PROT-008.6.5.
    if let Some(result) = connect::request(&cortex_host::Request::CurrentPreset {
        with_params: params,
        timeout_seconds: timeout,
    }) {
        let preset: PresetOut = serde_json::from_value(result?)?;
        return emit(&preset, fmt, print_preset);
    }

    let (session, qc) = connected()?;
    let catalog = match qc.fetch_model_repo(Duration::from_secs(40)) {
        Ok(payload) => cortex_rs::Catalog::parse(&payload).ok(),
        Err(e) => {
            eprintln!("warning: could not fetch the model catalog ({e}); blocks will show ids");
            None
        }
    };
    let result = qc.read_current_preset(Duration::from_secs(timeout));
    qc.disconnect();
    session.stop();

    let preset = result?;
    let out = PresetOut::from_binary(
        &preset,
        catalog.as_ref(),
        "(live grid)",
        "(live grid)",
        params,
    );
    emit(&out, fmt, print_preset)
}

/// Re-point a row's input or output.
fn cmd_set_routing(row: u32, input: Option<u32>, output: Option<u32>, fmt: Format) -> Result<()> {
    let row = wire_row(row)?;
    let (which, port) = match (input, output) {
        (Some(port), None) => ("input", port),
        (None, Some(port)) => ("output", port),
        _ => unreachable!("clap gives exactly one of input or output"),
    };
    let detail = format!("row {} {which} = port {port}", row.screen());

    if let Some(result) = connect::request(&cortex_host::Request::SetRouting {
        row: row.wire(),
        input,
        output,
    }) {
        result?;
        return report_edit(&format!("set_{which}"), detail, fmt);
    }

    let (session, qc) = connected()?;
    let result = match (input, output) {
        (Some(port), None) => qc.set_chain_input(row, port),
        (None, Some(port)) => qc.set_chain_output(row, port),
        _ => unreachable!("clap gives exactly one of input or output"),
    };
    qc.disconnect();
    session.stop();
    result?;
    report_edit(&format!("set_{which}"), detail, fmt)
}

/// Set a row's split and mix points.
fn cmd_set_split(row: u32, split: i32, mix: i32, fmt: Format) -> Result<()> {
    let row = wire_row(row)?;
    let detail = if split < 0 {
        format!("row {} branch cleared", row.screen())
    } else if mix < 0 {
        format!(
            "row {} branches at column {split}, never rejoins",
            row.screen()
        )
    } else {
        format!(
            "row {} branches at column {split}, rejoins at {mix}",
            row.screen()
        )
    };
    if let Some(result) = connect::request(&cortex_host::Request::SetSplit {
        row: row.wire(),
        split,
        mix,
    }) {
        result?;
        return report_edit("set_split", detail, fmt);
    }

    let (session, qc) = connected()?;
    let result = qc.set_split(row, split, mix);
    qc.disconnect();
    session.stop();
    result?;
    report_edit("set_split", detail, fmt)
}

/// Run, query, or stop the persistent connection.
fn cmd_connect(status: bool, stop: bool, fmt: Format) -> Result<()> {
    if status {
        let Some(result) = connect::request(&cortex_host::Request::Status) else {
            // Not an error: "no connection running" is a legitimate answer to
            // "what is the status", and a caller scripting against this
            // should not have to parse stderr to find out.
            let out = serde_json::json!({ "running": false });
            return emit(&out, fmt, |_| println!("not running"));
        };
        let value = result?;
        return emit(&value, fmt, |v| {
            println!("running: true");
            if let Some(uptime) = v.get("uptime_seconds") {
                println!("uptime_seconds: {uptime}");
            }
            if let Some(device) = v.get("device") {
                println!("device: {}", device.get("state").unwrap_or(device));
                for (label, key) in [("serial", "serial"), ("coros_version", "coros_version")] {
                    if let Some(value) = device.get(key).and_then(serde_json::Value::as_str) {
                        println!("{label}: {value}");
                    }
                }
                if let Some(since) = device.get("last_message_seconds") {
                    println!("last_message_seconds: {since}");
                }
                if let Some(attempts) = device.get("attempts") {
                    println!("reconnect_attempts: {attempts}");
                }
                if let Some(error) = device.get("last_error").or_else(|| device.get("error")) {
                    println!("device_error: {error}");
                }
            }
            if let Some(cache) = v.get("cache") {
                for (label, key) in [
                    ("cache_phase", "phase"),
                    ("cache_generation", "generation"),
                    ("cache_revision", "revision"),
                    ("cache_storage_revision", "storage_revision"),
                    ("cached_catalog", "catalog"),
                    ("cached_current_preset", "current_preset"),
                    ("cached_active_scene", "active_scene"),
                    ("cached_preset_dirty", "preset_dirty"),
                    ("cached_preset_location", "preset_location"),
                    ("cache_messages_seen", "messages_seen"),
                    ("cache_messages_applied", "pushes_applied"),
                    ("cache_messages_rejected", "messages_rejected"),
                    ("cache_stream_gaps", "stream_gaps"),
                ] {
                    if let Some(value) = cache.get(key) {
                        println!("{label}: {value}");
                    }
                }
                if let Some(error) = cache.get("last_rejection") {
                    if !error.is_null() {
                        println!("cache_last_rejection: {error}");
                    }
                }
            }
        });
    }

    if stop {
        let Some(result) = connect::request(&cortex_host::Request::Shutdown) else {
            anyhow::bail!("no connection is running");
        };
        result?;
        return emit(&serde_json::json!({ "stopped": true }), fmt, |_| {
            println!("stopped")
        });
    }

    connect::run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        // Catches conflicting args, bad defaults, and duplicate names at test
        // time rather than on first run.
        Cli::command().debug_assert();
    }

    #[test]
    fn commands_execute_by_default_and_dry_run_is_explicit() {
        let move_cli =
            Cli::try_parse_from(["cortex", "preset", "move", "--from", "2A", "--to", "2B"])
                .unwrap();
        assert!(!move_cli.dry_run);
        assert!(dry_run_plan(move_cli.command.as_ref()).unwrap().is_some());

        let save_cli =
            Cli::try_parse_from(["cortex", "preset", "save", "--token", "save-1", "--dry-run"])
                .unwrap();
        assert!(save_cli.dry_run);
        assert!(dry_run_plan(save_cli.command.as_ref()).unwrap().is_some());

        assert!(
            Cli::try_parse_from([
                "cortex", "preset", "move", "--from", "2A", "--to", "2B", "--yes"
            ])
            .is_err()
        );
    }

    #[test]
    fn every_side_effect_class_has_a_dry_run_plan() {
        let cases: &[&[&str]] = &[
            &["cortex", "session", "start", "--dry-run"],
            &["cortex", "session", "stop", "--dry-run"],
            &[
                "cortex",
                "preset",
                "delete",
                "--name",
                "Fictional",
                "--dry-run",
            ],
            &[
                "cortex",
                "preset",
                "move",
                "--from",
                "2A",
                "--to",
                "2B",
                "--dry-run",
            ],
            &[
                "cortex",
                "preset",
                "prepare-save",
                "--slot",
                "2A",
                "--dry-run",
            ],
            &["cortex", "preset", "save", "--token", "save-1", "--dry-run"],
            &["cortex", "preset", "show", "--slot", "2A", "--dry-run"],
            &["cortex", "preset", "recall", "--slot", "2A", "--dry-run"],
            &["cortex", "scene", "--index", "2", "--dry-run"],
            &[
                "cortex",
                "block",
                "param",
                "--row",
                "1",
                "--column",
                "2",
                "--param",
                "GAIN",
                "--real",
                "7.5",
                "--dry-run",
            ],
            &[
                "cortex",
                "block",
                "bypass",
                "--row",
                "1",
                "--column",
                "2",
                "--dry-run",
            ],
            &[
                "cortex",
                "block",
                "set",
                "--row",
                "1",
                "--column",
                "2",
                "--model",
                "1001",
                "--dry-run",
            ],
            &[
                "cortex",
                "block",
                "remove",
                "--row",
                "1",
                "--column",
                "2",
                "--dry-run",
            ],
            &[
                "cortex",
                "row",
                "input",
                "--row",
                "1",
                "--port",
                "1",
                "--dry-run",
            ],
            &[
                "cortex",
                "row",
                "output",
                "--row",
                "1",
                "--port",
                "0",
                "--dry-run",
            ],
            &[
                "cortex",
                "row",
                "split",
                "--row",
                "1",
                "--split",
                "3",
                "--mix",
                "6",
                "--dry-run",
            ],
            &[
                "cortex",
                "catalog",
                "--dump",
                "/tmp/fictional-catalog",
                "--dry-run",
            ],
            &["cortex", "completions", "install", "--dry-run"],
        ];
        for args in cases {
            let cli =
                Cli::try_parse_from(*args).unwrap_or_else(|error| panic!("{args:?}: {error}"));
            assert!(cli.dry_run, "{args:?} did not set the global flag");
            assert!(
                dry_run_plan(cli.command.as_ref()).unwrap().is_some(),
                "{args:?} is side-effecting but has no plan"
            );
        }
    }

    /// Every `after_help` example must name a real command and real flags.
    ///
    /// Examples are the part of the help a reader copies verbatim, so a stale
    /// one is worse than none - it fails in their terminal, not ours. They
    /// cannot be generated, so this checks them instead.
    #[test]
    fn every_help_example_uses_a_real_command_and_flags() {
        let root = Cli::command();

        /// Longs accepted everywhere, which clap attaches at parse time
        /// rather than listing on each subcommand.
        const GLOBALS: [&str; 4] = ["--format", "--zero-based", "--dry-run", "--help"];

        /// Walk as far as the tokens name subcommands, then stop.
        ///
        /// Stopping matters: `cortex completions install` takes `install` as
        /// a positional VALUE, not a subcommand, and a resolver insisting on
        /// consuming every token would call a valid example broken.
        fn resolve<'a>(root: &'a clap::Command, path: &[&str]) -> (&'a clap::Command, usize) {
            let mut cmd = root;
            let mut used = 0;
            for step in path {
                match cmd
                    .get_subcommands()
                    .find(|c| c.get_name() == *step || c.get_all_aliases().any(|a| a == *step))
                {
                    Some(next) => {
                        cmd = next;
                        used += 1;
                    }
                    None => break,
                }
            }
            (cmd, used)
        }

        fn walk(root: &clap::Command, cmd: &clap::Command, failures: &mut Vec<String>) {
            if let Some(after) = cmd.get_after_help() {
                for line in after.to_string().lines() {
                    let line = line.split('#').next().unwrap_or_default().trim();
                    let Some(rest) = line.strip_prefix("cortex ") else {
                        continue;
                    };
                    // Shell operators are not part of the command.
                    let tokens: Vec<&str> = rest
                        .split_whitespace()
                        .take_while(|t| !matches!(*t, "&" | "|" | ">" | "&&"))
                        .collect();
                    let path: Vec<&str> = tokens
                        .iter()
                        .take_while(|t| !t.starts_with('-'))
                        .copied()
                        .collect();
                    let (target, used) = resolve(root, &path);
                    if used == 0 && !path.is_empty() {
                        failures.push(format!("no such command: cortex {}", path.join(" ")));
                        continue;
                    }
                    let longs: Vec<String> = target
                        .get_arguments()
                        .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
                        .collect();
                    for flag in tokens.iter().filter(|t| t.starts_with("--")) {
                        let flag = flag.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
                        if !GLOBALS.contains(&flag) && !longs.iter().any(|l| l == flag) {
                            failures.push(format!("cortex {}: no {flag}", path.join(" ")));
                        }
                    }
                }
            }
            for sub in cmd.get_subcommands() {
                walk(root, sub, failures);
            }
        }

        let mut failures = Vec::new();
        walk(&root, &root, &mut failures);
        assert!(failures.is_empty(), "stale help examples: {failures:#?}");
    }

    #[test]
    fn completions_generate_for_every_supported_shell() {
        for target in [
            CompletionTarget::Bash,
            CompletionTarget::Elvish,
            CompletionTarget::Fish,
            CompletionTarget::Powershell,
            CompletionTarget::Zsh,
        ] {
            let shell = target.as_shell().expect("a named shell resolves");
            let mut out = Vec::new();
            let mut command = Cli::command();
            clap_complete::generate(shell, &mut command, "cortex", &mut out);
            let script = String::from_utf8(out).expect("completion script is UTF-8");
            assert!(!script.is_empty(), "{shell} produced an empty script");
            // A representative command, so a script that generates but omits
            // the actual surface still fails.
            assert!(
                script.contains("catalog"),
                "{shell} script does not mention the `catalog` command"
            );
        }
    }

    #[test]
    fn install_target_resolves_to_no_fixed_shell() {
        // `install` means "work it out", so it must not map to a shell.
        assert!(CompletionTarget::Install.as_shell().is_none());
    }

    #[test]
    fn completion_file_names_follow_shell_convention() {
        use clap_complete::Shell;
        // Getting these wrong installs a file the shell will never load,
        // which fails silently.
        assert_eq!(completion_file_name(Shell::Bash), "cortex");
        assert_eq!(completion_file_name(Shell::Zsh), "_cortex");
        assert_eq!(completion_file_name(Shell::Fish), "cortex.fish");
        assert_eq!(completion_file_name(Shell::PowerShell), "cortex.ps1");
        assert_eq!(completion_file_name(Shell::Elvish), "cortex.elv");
    }

    #[test]
    fn zsh_completions_go_to_zfunc() {
        // Deliberate convention, not an XDG path: ~/.zfunc is what several
        // widely-used Rust CLIs tell people to put on their fpath, so a user
        // gets one directory for all of them rather than one per tool.
        let home = std::path::PathBuf::from(std::env::var("HOME").expect("HOME is set"));
        assert_eq!(
            completion_dir(clap_complete::Shell::Zsh),
            Some(home.join(".zfunc"))
        );
    }

    #[test]
    fn completion_dirs_are_under_the_users_home() {
        use clap_complete::Shell;
        let home = std::path::PathBuf::from(std::env::var("HOME").expect("HOME is set"));
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Elvish] {
            let dir = completion_dir(shell).expect("a directory is known");
            assert!(
                dir.starts_with(&home),
                "{shell} would install outside HOME: {}",
                dir.display()
            );
        }
        // No conventional per-user directory exists for PowerShell, and
        // guessing one is worse than saying so.
        assert!(completion_dir(Shell::PowerShell).is_none());
    }
}

// ---------------------------------------------------------------------------
// Output types
//
// These exist so `--format json` has a stable, documented shape rather than
// serialising whatever the protobuf types happen to look like. A prost type
// is a wire representation; this is an interface.
// ---------------------------------------------------------------------------

/// The result of a `probe` run.
#[derive(serde::Serialize)]
struct ProbeOut {
    handshake_seconds: f32,
    active_scene: Option<u32>,
    current_preset: Option<String>,
    current_preset_chains: Option<usize>,
    preset_count: Option<usize>,
    presets: Vec<PresetSlot>,
    /// Read paths that failed, with the reason. Empty on a clean run.
    failures: Vec<String>,
}

/// A model catalog entry.
#[derive(serde::Serialize)]
struct ModelOut {
    id: u32,
    name: String,
    category: String,
    /// Neural DSP's own attribution, verbatim.
    based_on: Option<String>,
    parameters: Vec<ParameterOut>,
}

/// One parameter of a model.
#[derive(serde::Serialize)]
struct ParameterOut {
    /// The WIRE index. Positional, so `empty` and `meter` entries occupy one.
    index: usize,
    name: String,
    kind: String,
    min: f64,
    max: f64,
    default: f64,
    units: String,
    step_names: Vec<String>,
    /// A meter is a live measurement, not a setting; writing to it is
    /// meaningless.
    read_only: bool,
}

impl From<&cortex_rs::Parameter> for ParameterOut {
    fn from(p: &cortex_rs::Parameter) -> Self {
        Self {
            index: p.index,
            name: p.name.clone(),
            kind: format!("{:?}", p.kind).to_lowercase(),
            min: p.min,
            max: p.max,
            default: p.default,
            units: p.units.clone(),
            step_names: p.step_names.clone(),
            read_only: p.kind.is_read_only(),
        }
    }
}

impl From<&cortex_rs::Model> for ModelOut {
    fn from(m: &cortex_rs::Model) -> Self {
        Self {
            id: m.id,
            name: m.name.clone(),
            category: m.category.clone(),
            based_on: m.based_on.clone(),
            parameters: m.parameters.iter().map(ParameterOut::from).collect(),
        }
    }
}

/// A catalog summary.
#[derive(serde::Serialize)]
struct CatalogSummaryOut {
    models: usize,
    with_attribution: usize,
    categories: std::collections::BTreeMap<String, usize>,
}

/// Confirmation of an action that changed device state.
#[derive(serde::Serialize)]
struct ActionOut {
    action: String,
    detail: String,
}

/// Show the unit's live DSP load.
fn cmd_cpu(fmt: Format) -> Result<()> {
    let Some(result) = connect::request(&cortex_host::Request::CpuLoad) else {
        anyhow::bail!(
            "no `cortex session start` session is running.\n\
             The device only pushes CPU load to a subscribed client, so this \
             needs a held session: start one with `cortex session start`."
        );
    };
    let parsed: CpuLoad = serde_json::from_value(result?)?;
    emit(&parsed, fmt, |c| {
        if let Some(total) = c.total {
            println!("total: {total:.1}%");
        }
        for (i, chain) in c.chains.iter().enumerate() {
            if chain.is_empty() {
                continue;
            }
            let cells: Vec<String> = chain
                .iter()
                .map(|col| format!("{:>5.1}{}", col.load, if col.on_core2 { "*" } else { " " }))
                .collect();
            println!("row {}: {}", i + 1, cells.join(" "));
        }
        if c.chains.iter().any(|ch| ch.iter().any(|c| c.on_core2)) {
            println!("(* = second DSP core)");
        }
    })
}

/// Human-readable rendering of the device version.
fn print_device_version(d: &DeviceVersion) {
    let row = |label: &str, value: &Option<String>| {
        if let Some(v) = value {
            println!("{label:<26} {v}");
        }
    };
    row("device_type", &d.device_type);
    row("custom_name", &d.custom_name);
    row("serial_number", &d.serial_number);
    row("coros_version", &d.coros_version);
    row("app_firmware", &d.app_firmware);
    row("bootloader_firmware", &d.bootloader_firmware);
    row("zencoder_app", &d.zencoder_app);
    row("zencoder_bootloader", &d.zencoder_bootloader);
    row("wireless_fw_checksum", &d.wireless_firmware_checksum);
    row("linux_kernel", &d.linux_kernel);
    row("uboot", &d.uboot);
    row("mac_address", &d.mac_address);
    if let Some(ess) = d.is_ess {
        println!("{:<26} {ess}", "is_ess");
    }
}

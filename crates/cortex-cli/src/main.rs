// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The standalone `cortex` command-line surface.
//!
//! A thin wrapper over [`cortex_rs`]: all protocol and domain behaviour lives
//! in the library so the MCP server and the Tauri backend can reuse it
//! without repetition. The binary adds only argument parsing, shell
//! completions, and the version command.

use std::process::ExitCode;
use std::time::Duration;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use cortex_rs::proto::VersionMessage;
use cortex_rs::proto::cortex_message_type::Enum as MessageType;
use cortex_rs::proto::message_action::Enum as MessageAction;
use cortex_rs::proto::version_message::{
    AppFwVersion, BootloaderFwVersion, CommsVersion, CortexControlVersion, CustomName,
    DeviceSerialNumber, DeviceTypeOneOf, IsEss, LinuxKernelVersion, MacAddress, UbootVersion,
    ZencoderFwApp, ZencoderFwBootloader, ZenosGitHash, ZenwirelessFwVersion,
};
use cortex_rs::{DeviceKind, Transport};

/// The `cortex` CLI: an unofficial, Linux-first command-line surface for the
/// Neural DSP Quad Cortex (and Nano Cortex) over USB HID.
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
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Read the device firmware version (CorOS, app, bootloader, zencoder).
    Version,
    /// Print shell completions to stdout.
    Completions {
        /// The shell to generate completions for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

fn main() -> ExitCode {
    // Reset SIGPIPE on Unix so output pipes cleanly into `head`/`less`.
    #[cfg(unix)]
    unsafe {
        libc_sigpipe_reset();
    }

    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cortex: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Command::Version) => cmd_version(),
        Some(Command::Completions { shell }) => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
        None => {
            Cli::command().print_help()?;
            Ok(())
        }
    }
}

fn cmd_version() -> Result<()> {
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

    let version: VersionMessage = prost::Message::decode(reply.body.as_ref())
        .map_err(|e| anyhow::anyhow!("protobuf decode error: {e}"))?;

    print_version(&version);
    Ok(())
}

/// Print the version fields as YAML-like text to stdout. Data on stdout,
/// hints on stderr (house-style rust-cli.md).
fn print_version(v: &VersionMessage) {
    if let Some(name) = message_action_name(v.action) {
        println!("action: {name}");
    }
    str_field(
        "linux_kernel_version",
        &v.linux_kernel_version,
        LinuxKernelVersion::LinuxKernelVersion,
    );
    str_field(
        "zenos_git_hash",
        &v.zenos_git_hash,
        ZenosGitHash::ZenosGitHash,
    );
    str_field(
        "zenwireless_fw_version",
        &v.zenwireless_fw_version,
        ZenwirelessFwVersion::ZenwirelessFwVersion,
    );
    str_field(
        "uboot_version",
        &v.uboot_version,
        UbootVersion::UbootVersion,
    );
    str_field(
        "app_fw_version",
        &v.app_fw_version,
        AppFwVersion::AppFwVersion,
    );
    str_field(
        "bootloader_fw_version",
        &v.bootloader_fw_version,
        BootloaderFwVersion::BootloaderFwVersion,
    );
    str_field(
        "device_serial_number",
        &v.device_serial_number,
        DeviceSerialNumber::DeviceSerialNumber,
    );
    if let Some(CommsVersion::CommsVersion(ref bytes)) = v.comms_version {
        println!("comms_version: {:?}", bytes);
    }
    if let Some(DeviceTypeOneOf::DeviceType(dt)) = v.device_type {
        let dt = cortex_rs::proto::version_message::DeviceType::try_from(dt)
            .unwrap_or(cortex_rs::proto::version_message::DeviceType::Qc);
        println!("device_type: {}", device_type_name(dt));
    }
    if let Some(IsEss::IsEss(b)) = v.is_ess {
        println!("is_ess: {b}");
    }
    str_field("custom_name", &v.custom_name, CustomName::CustomName);
    str_field("mac_address", &v.mac_address, MacAddress::MacAddress);
    str_field(
        "zencoder_fw_app",
        &v.zencoder_fw_app,
        ZencoderFwApp::ZencoderFwApp,
    );
    str_field(
        "zencoder_fw_bootloader",
        &v.zencoder_fw_bootloader,
        ZencoderFwBootloader::ZencoderFwBootloader,
    );
    str_field(
        "cortex_control_version",
        &v.cortex_control_version,
        CortexControlVersion::CortexControlVersion,
    );
}

/// Extract and print a oneof-wrapped string field, if present.
fn str_field<T>(label: &str, field: &Option<T>, variant: fn(::prost::alloc::string::String) -> T)
where
    T: std::fmt::Debug,
{
    // We can't pattern-match generically over the oneof enum, so we use the
    // Debug format and strip the wrapper: `AppFwVersion("d14e")` -> `d14e`.
    // The variant fn is unused but documents which wrapper this is.
    let _ = variant;
    if let Some(value) = field {
        let s = format!("{value:?}");
        if let Some(start) = s.find('(') {
            if let Some(end) = s.rfind(')') {
                let inner = &s[start + 1..end];
                let inner = inner.trim_matches('"');
                println!("{label}: {inner}");
                return;
            }
        }
        println!("{label}: {s}");
    }
}

fn message_action_name(action: i32) -> Option<&'static str> {
    match action {
        0 => Some("CREATE"),
        1 => Some("UPDATE"),
        2 => Some("DELETE"),
        3 => Some("READ"),
        4 => Some("MOVE"),
        5 => Some("COPY"),
        6 => Some("UPLOAD"),
        7 => Some("DOWNLOAD"),
        8 => Some("SWAP"),
        _ => None,
    }
}

fn device_type_name(dt: cortex_rs::proto::version_message::DeviceType) -> &'static str {
    match dt {
        cortex_rs::proto::version_message::DeviceType::Qc => "QC",
        cortex_rs::proto::version_message::DeviceType::Atma => "ATMA",
    }
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

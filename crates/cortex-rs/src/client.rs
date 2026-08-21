// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The ergonomic `QuadCortex` client API: the Rust equivalent of
//! `pyquadcortex`'s `QuadCortex` class. This is the layer the CLI, MCP server,
//! and Tauri backend all call.
//!
//! It holds a `Session` reference and builds protobuf messages, handing them
//! to the session's `send`/`request`/`await_broadcast`/`collect` primitives.
//! It knows nothing about hidapi, HID reports, framing, or the session state
//! machine.
//!
//! ## Domain traps
//!
//! The Quad Cortex protocol has several silent-no-op traps confirmed on
//! hardware. Every one is documented in the relevant method's rustdoc. The
//! most important:
//!
//! - Rows are 0-based in the API, 1-4 on screen. A wrong-row edit succeeds
//!   silently.
//! - A recalled preset carries no explicit `row`; writing it back wholesale
//!   does nothing. Use the keyed wrappers.
//! - `read_preset` RECALLS the slot (side effect); `read_current_preset` does
//!   not.
//! - `set_param(scene=)` is 3 messages: promote `scene_mode`, switch scene,
//!   write. The flag and a value cannot travel together.
//! - `set_block` can be refused for `DSP` capacity (no echo within timeout).
//! - `remove_block` uses action DELETE, not UPDATE with `hash: 0`.
//!
//! @see spec/150-client/spec.md [FR-1]
//! @see spec/150-client/design.md [DES-CLIENT]

#![allow(clippy::missing_panics_doc, clippy::needless_pass_by_value)]

use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "hid")]
use crate::DeviceKind;
use crate::catalog::{Parameter, ParameterKind};
use crate::grid::{Row, Value};
use crate::proto::cortex_message_type::Enum as MessageType;
use crate::proto::message_action::Enum as MessageAction;
use crate::proto::{
    AvailableModes, BinaryPreset, FileMessage, GeneralMidiMessage, GeneralMidiMessages,
    GeneralSettingsMessage, GlobalBypassRows, GlobalEqMessage, GlobalEqParameter,
    InputPortSettings, IoSettingsMessage, LooperMessage, MasterVolumeAssignmentOptions,
    MasterVolumeMessage, MidiMessageInfo, MidiPortSettings, MidiSettingsMessage, ModeMessage,
    OutputPortSettings, PinnedModelsMessage, PortSettings, RecallPresetMessage,
    RecentsFavoritesItem, RecentsFavoritesMessage, SceneColorMessage, SceneCopyMessage,
    SceneLabelMessage, SceneMessage, SetlistPositionMessage, ShowGigViewMessage, ShowTunerMessage,
    TunerMessage, UsbPortSettings, VersionMessage,
};
use crate::session::{InboundMessage, Session};

/// Where user setlists live on the device filesystem. They sit side by side
/// under this root, NOT nested inside "My Presets" - a folder created under My
/// Presets is not a setlist and the device ignores it.
pub const USER_SETLIST_ROOT: &str = "/media/p4/Presets";

/// The default user setlist path ("My Presets").
pub const USER_SETLIST: &str = "/media/p4/Presets/My Presets";

/// How the unit stores "this scene has no label": a single space, not an empty
/// string. Detect a blank scene with `label.trim().is_empty()`.
pub const SCENE_UNLABELLED: &str = " ";

/// The wire value the mixer, splitter, and lane-output LEVEL parameters hold
/// at 0 dB (unity). 10/13 on the -100..+30 dB span those controls cover.
#[allow(clippy::unreadable_literal)]
pub const UNITY_LEVEL: f64 = 0.76923077;

/// 32 banks of 8 slots = 256 total slots per setlist.
pub const BANKS: u32 = 32;
/// 8 slots per bank (A through H).
pub const SLOTS_PER_BANK: u32 = 8;
/// Whether a setlist path is the read-only factory library.
///
/// Matched on the path rather than a caller-supplied flag, because a flag is
/// something every surface has to remember to set, and this is the one place
/// that must not be got wrong.
#[must_use]
pub fn is_factory_setlist(setlist: &str) -> bool {
    setlist.starts_with("/opt/neuraldsp/")
}

/// Total slots per setlist (256).
pub const SETLIST_SLOTS: u32 = BANKS * SLOTS_PER_BANK;

/// Preferred-instrument metadata stored with a preset listing.
///
/// These are exact wire values, in the order shown by the unit's Preferred
/// Instrument picker. They are an enumeration, not bit flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum Instrument {
    /// No preferred-instrument tag.
    None = 0,
    /// Guitar.
    #[default]
    Guitar = 1,
    /// Bass.
    Bass = 2,
    /// Synthesizer.
    Synth = 3,
    /// Vocal.
    Vocal = 4,
    /// Other instrument.
    Other = 5,
}

impl TryFrom<i32> for Instrument {
    type Error = crate::Error;

    fn try_from(value: i32) -> crate::Result<Self> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Guitar),
            2 => Ok(Self::Bass),
            3 => Ok(Self::Synth),
            4 => Ok(Self::Vocal),
            5 => Ok(Self::Other),
            _ => Err(crate::Error::Decode(format!(
                "unknown ProductData.instrument value {value}"
            ))),
        }
    }
}

impl std::str::FromStr for Instrument {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "guitar" => Ok(Self::Guitar),
            "bass" => Ok(Self::Bass),
            "synth" => Ok(Self::Synth),
            "vocal" => Ok(Self::Vocal),
            "other" => Ok(Self::Other),
            _ => Err(format!(
                "unknown instrument {value:?}; use none, guitar, bass, synth, vocal, or other"
            )),
        }
    }
}

/// The device folder key for the complete Neural Capture library.
pub const CAPTURES_LIBRARY: &str = "local_nc_root";

/// The device folder key for the complete loadable IR library.
pub const IR_LIBRARY: &str = "local_ir_root";

/// Default model id for a Neural Capture block.
pub const DEFAULT_CAPTURE_MODEL: u32 = 14_000;

/// Wire index of a Neural Capture block's capture-reference string.
pub const CAPTURE_FILE_NAME_PARAM: u32 = 5;

/// First IR Loader model id.
pub const FIRST_IR_LOADER_MODEL: u32 = 29_001;

/// Last IR Loader model id.
pub const LAST_IR_LOADER_MODEL: u32 = 29_008;

const IR_PATH_PARAMS: [u32; 2] = [2, 10];
const IR_NAME_PARAMS: [u32; 2] = [22, 23];

/// An input-port entry accepted by `IOSettings`.
///
/// The ids are not consecutive jack numbers: combined entries occupy 3 and 6,
/// so Return 1 is 4 and Return 2 is 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum InputPort {
    /// Input 1.
    Input1 = 1,
    /// Input 2.
    Input2 = 2,
    /// Combined Input 1/2.
    Input12 = 3,
    /// Return 1.
    Return1 = 4,
    /// Return 2.
    Return2 = 5,
    /// Combined Return 1/2.
    Return12 = 6,
}

impl TryFrom<u32> for InputPort {
    type Error = crate::Error;

    fn try_from(value: u32) -> crate::Result<Self> {
        match value {
            1 => Ok(Self::Input1),
            2 => Ok(Self::Input2),
            3 => Ok(Self::Input12),
            4 => Ok(Self::Return1),
            5 => Ok(Self::Return2),
            6 => Ok(Self::Return12),
            _ => Err(crate::Error::InvalidParameter(format!(
                "unknown IOSettings input port id {value}"
            ))),
        }
    }
}

/// An output-port entry accepted by `IOSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum OutputPort {
    /// Paired XLR Outputs 1/2.
    Xlr12 = 1,
    /// Paired Outputs 3/4.
    Out34 = 2,
    /// Paired Sends 1/2.
    Send12 = 3,
    /// XLR Output 1.
    Xlr1 = 4,
    /// XLR Output 2.
    Xlr2 = 5,
    /// Output 3.
    Out3 = 6,
    /// Output 4.
    Out4 = 7,
    /// Send 1.
    Send1 = 8,
    /// Send 2.
    Send2 = 9,
}

impl TryFrom<u32> for OutputPort {
    type Error = crate::Error;

    fn try_from(value: u32) -> crate::Result<Self> {
        match value {
            1 => Ok(Self::Xlr12),
            2 => Ok(Self::Out34),
            3 => Ok(Self::Send12),
            4 => Ok(Self::Xlr1),
            5 => Ok(Self::Xlr2),
            6 => Ok(Self::Out3),
            7 => Ok(Self::Out4),
            8 => Ok(Self::Send1),
            9 => Ok(Self::Send2),
            _ => Err(crate::Error::InvalidParameter(format!(
                "unknown IOSettings output port id {value}"
            ))),
        }
    }
}

/// Sparse writable controls for one input port. Every selected field is sent alone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct InputPortPatch {
    /// Normalized input gain. Use [`input_level_db`] for the established dB scale.
    pub level: Option<f32>,
    /// Normalized input impedance-mode selection.
    pub impedance: Option<f32>,
    /// Normalized Instrument/Mic input-type selection.
    pub input_type: Option<f32>,
    /// Normalized ground-lift control.
    pub ground_lift: Option<f32>,
}

/// Sparse writable controls for one output port. Every selected field is sent alone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OutputPortPatch {
    /// Normalized output level. Its dB scale is not established.
    pub level: Option<f32>,
    /// Normalized ground-lift control.
    pub ground_lift: Option<f32>,
    /// Output mute state.
    pub mute: Option<bool>,
}

/// Sparse writable controls for USB audio. Every selected field is sent alone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsbPortPatch {
    /// Normalized USB output level.
    pub level: Option<f32>,
    /// Normalized USB headphone-source selection.
    pub headphone_source: Option<f32>,
    /// Normalized clean-DI/processed-audio selection.
    pub dry_wet: Option<f32>,
}

/// Sparse output-pairing controls. Each selected pairing is sent alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputPairingPatch {
    /// Pair or unpair XLR Outputs 1/2.
    pub xlr12: Option<bool>,
    /// Pair or unpair Outputs 3/4.
    pub out34: Option<bool>,
}

/// Safe, known-writable scalar fields of `GeneralSettings`.
///
/// Command-shaped and unsupported fields are structurally absent: callers cannot
/// represent power/reset operations, updater control, or the unwritable internal
/// MIDI clock. Nested replacement groups also use dedicated read-merge-write APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneralSettingsPatch {
    /// Screen brightness, 0-100. Device read-back is quantized.
    pub screen_brightness: Option<u8>,
    /// Normal LED brightness, 0-100. Device read-back is quantized.
    pub led_brightness: Option<u8>,
    /// Dimmed LED brightness, 0-100. Firmware may cap it below normal brightness.
    pub dimmed_led_brightness: Option<u8>,
    /// Whether STOMP mode auto-assigns blocks.
    pub stomp_mode_auto_assign: Option<bool>,
    /// Whether Tempo and Tuner hold access are swapped.
    pub swap_tempo_tuner_access: Option<bool>,
    /// Dynamic delay compensation.
    pub enable_dynamic_delay_compensation: Option<bool>,
    /// Gig View access from STOMP mode.
    pub gig_view_stomp_access_enabled: Option<bool>,
    /// Device MIDI receive channel, 1-16.
    pub midi_channel: Option<u8>,
    /// MIDI-over-USB setting.
    pub midi_over_usb: Option<bool>,
    /// External MIDI clock input.
    pub midi_clock_in_enabled: Option<bool>,
    /// Ignore duplicate MIDI program changes.
    pub ignore_duplicate_pc: Option<bool>,
    /// Disable the internet connectivity probe.
    pub disable_internet_connection_check: Option<bool>,
    /// Dim inactive preset footswitch LEDs.
    pub enable_preset_dimmed: Option<bool>,
    /// Dim inactive scene footswitch LEDs.
    pub enable_scene_dimmed: Option<bool>,
    /// Dim inactive STOMP footswitch LEDs.
    pub enable_stomp_dimmed: Option<bool>,
}

/// How scene-local block bypass edits overwrite stored scene state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum SceneBypassBehavior {
    /// Every bypass edit is retained in the active scene.
    AlwaysOverwrite = 0,
    /// STOMP presses are not retained; touchscreen edits are retained.
    NonStompOverwrite = 1,
    /// No bypass edit is retained.
    NeverOverwrite = 2,
}

/// Complete Master Volume output assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Mirrors one indivisible protobuf replacement group.
pub struct MasterVolumeAssignment {
    /// Outputs 1/2.
    pub out12: bool,
    /// Outputs 3/4.
    pub out34: bool,
    /// Sends 1/2.
    pub send12: bool,
    /// Headphones.
    pub headphones: bool,
}

/// Partial intent for a read-merge-write Master Volume assignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MasterVolumeAssignmentPatch {
    /// New Outputs 1/2 assignment, or unchanged.
    pub out12: Option<bool>,
    /// New Outputs 3/4 assignment, or unchanged.
    pub out34: Option<bool>,
    /// New Sends 1/2 assignment, or unchanged.
    pub send12: Option<bool>,
    /// New headphone assignment, or unchanged.
    pub headphones: Option<bool>,
}

/// Four grid-row bypass flags.
pub type GlobalBypassRowsState = [bool; 4];

/// Complete Cab and IR Loader global-bypass state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalBypassState {
    /// Cab bypass by grid row.
    pub cab: GlobalBypassRowsState,
    /// IR Loader bypass by grid row.
    pub ir: GlobalBypassRowsState,
}

/// Partial intent for read-merge-write global Cab/IR bypass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GlobalBypassPatch {
    /// Complete replacement Cab row group, or unchanged.
    pub cab: Option<GlobalBypassRowsState>,
    /// Complete replacement IR Loader row group, or unchanged.
    pub ir: Option<GlobalBypassRowsState>,
}

/// One Global EQ band's sparse controls. Numeric values are normalized 0-1.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlobalEqBandPatch {
    /// Gain, where 0.5 is 0 dB.
    pub gain: Option<f32>,
    /// Frequency control.
    pub frequency: Option<f32>,
    /// Q control.
    pub q: Option<f32>,
    /// Filter shape.
    pub filter_type: Option<GlobalEqFilter>,
    /// Whether this band is active. `false` bypasses the band.
    pub enabled: Option<bool>,
}

/// Global EQ filter shape, mapped to the five-option wire control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum GlobalEqFilter {
    /// Peak/bell.
    Peak = 0,
    /// High-pass.
    HighPass = 1,
    /// Low-pass.
    LowPass = 2,
    /// High shelf.
    HighShelf = 3,
    /// Low shelf.
    LowShelf = 4,
}

/// Sparse controls on the Global EQ OUT tab.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlobalEqOutputPatch {
    /// Normalized output level. No dB conversion is claimed.
    pub level: Option<f32>,
    /// Route EQ to Outputs 1/2.
    pub out12: Option<bool>,
    /// Route EQ to Outputs 3/4.
    pub out34: Option<bool>,
}

/// A valid base or hybrid footswitch-mode slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum FootswitchModeSlot {
    /// Preset mode.
    Preset = 0,
    /// Scene mode.
    Scene = 1,
    /// STOMP mode.
    Stomp = 2,
    /// Preset on A-D, Scene on E-H.
    PresetScene = 3,
    /// Preset on A-D, STOMP on E-H.
    PresetStomp = 4,
    /// Scene on A-D, Preset on E-H.
    ScenePreset = 5,
    /// Scene on A-D, STOMP on E-H.
    SceneStomp = 6,
    /// STOMP on A-D, Preset on E-H.
    StompPreset = 7,
    /// STOMP on A-D, Scene on E-H.
    StompScene = 8,
}

impl FootswitchModeSlot {
    const fn is_hybrid(self) -> bool {
        self as u32 >= 3
    }
}

impl TryFrom<u32> for FootswitchModeSlot {
    type Error = crate::Error;

    fn try_from(value: u32) -> crate::Result<Self> {
        match value {
            0 => Ok(Self::Preset),
            1 => Ok(Self::Scene),
            2 => Ok(Self::Stomp),
            3 => Ok(Self::PresetScene),
            4 => Ok(Self::PresetStomp),
            5 => Ok(Self::ScenePreset),
            6 => Ok(Self::SceneStomp),
            7 => Ok(Self::StompPreset),
            8 => Ok(Self::StompScene),
            9 => Err(crate::Error::InvalidParameter(
                "mode 9 is stored but disables the footswitches and is never sent".into(),
            )),
            _ => Err(crate::Error::InvalidParameter(format!(
                "mode {value} is out of range; valid slots are 0-8"
            ))),
        }
    }
}

/// Inputs demonstrated to be accepted by the tuner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum TunerInput {
    /// Input 1.
    Input1 = 1,
    /// Input 2.
    Input2 = 2,
    /// Combined Input 1/2.
    Input12 = 3,
    /// Return 1.
    Return1 = 4,
    /// Return 2.
    Return2 = 5,
    /// USB input 5.
    Usb5 = 8,
    /// USB input 6.
    Usb6 = 9,
}

/// Supported preset-local controls from the unit's Tempo menu.
///
/// The discriminants are the positional parameter indices stored by model
/// 25000. Index 1 (`TYPE` in the catalog) is deliberately absent: changing the
/// menu's MODE control produced no wire traffic, so no positive evidence links
/// that index to MODE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum TempoParameter {
    /// Preset tempo.
    Tempo = 0,
    /// Tempo footswitch LED (`LED LIGHT` on the unit).
    LedLight = 2,
    /// Metronome playback volume.
    Volume = 3,
    /// Metronome mute, despite the catalog naming this positional slot `START`.
    Mute = 4,
    /// Metronome pan.
    Pan = 5,
    /// Time signature.
    TimeSignature = 6,
    /// Rhythmic subdivision (`SUBDIVISIONS` on screen, `NOTELENGTH` in the catalog).
    Subdivisions = 7,
    /// Metronome sound.
    Sound = 8,
    /// Metronome output routing.
    Routing = 9,
}

impl TempoParameter {
    /// Positional wire index in `tempo_program_data[0].params`.
    #[must_use]
    pub const fn index(self) -> u32 {
        self as u32
    }

    const fn option_count(self) -> Option<u32> {
        match self {
            Self::TimeSignature => Some(21),
            Self::Subdivisions => Some(4),
            Self::Sound => Some(6),
            Self::Routing => Some(5),
            _ => None,
        }
    }
}

macro_rules! checked_tempo_enum {
    ($name:ident, $count:expr, [$($value:ident = $index:literal),+ $(,)?]) => {
        impl $name {
            const COUNT: u32 = $count;

            const fn option(self) -> u32 {
                self as u32
            }

            #[allow(clippy::cast_precision_loss)]
            const fn normalised(self) -> f32 {
                self.option() as f32 / (Self::COUNT - 1) as f32
            }
        }

        impl TryFrom<u32> for $name {
            type Error = crate::Error;

            fn try_from(value: u32) -> crate::Result<Self> {
                match value {
                    $($index => Ok(Self::$value),)+
                    _ => Err(crate::Error::InvalidParameter(format!(
                        "{} option {value} is out of range 0-{}",
                        stringify!($name),
                        Self::COUNT - 1
                    ))),
                }
            }
        }
    };
}

/// Metronome rhythmic subdivision, in the unit's displayed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum TempoSubdivision {
    /// 1/4.
    Quarter = 0,
    /// 1/8.
    Eighth = 1,
    /// 1/8 triplet.
    EighthTriplet = 2,
    /// 1/16.
    Sixteenth = 3,
}
checked_tempo_enum!(
    TempoSubdivision,
    4,
    [Quarter = 0, Eighth = 1, EighthTriplet = 2, Sixteenth = 3]
);

/// A STOMP-mode footswitch, numbered A-H on the unit and 0-7 on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum Footswitch {
    /// Footswitch A.
    A = 0,
    /// Footswitch B.
    B = 1,
    /// Footswitch C.
    C = 2,
    /// Footswitch D.
    D = 3,
    /// Footswitch E.
    E = 4,
    /// Footswitch F.
    F = 5,
    /// Footswitch G.
    G = 6,
    /// Footswitch H.
    H = 7,
}

impl Footswitch {
    const fn wire(self) -> u32 {
        self as u32
    }
}

impl TryFrom<u32> for Footswitch {
    type Error = crate::Error;

    fn try_from(value: u32) -> crate::Result<Self> {
        match value {
            0 => Ok(Self::A),
            1 => Ok(Self::B),
            2 => Ok(Self::C),
            3 => Ok(Self::D),
            4 => Ok(Self::E),
            5 => Ok(Self::F),
            6 => Ok(Self::G),
            7 => Ok(Self::H),
            _ => Err(crate::Error::InvalidParameter(format!(
                "footswitches are numbered 0-7 (A-H), got {value}"
            ))),
        }
    }
}

/// One of the two expression-pedal inputs, numbered as on the rear panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(i32)]
pub enum ExpressionPedal {
    /// EXP 1.
    One = 1,
    /// EXP 2.
    Two = 2,
}

impl ExpressionPedal {
    const fn wire(self) -> i32 {
        self as i32
    }
}

impl TryFrom<u32> for ExpressionPedal {
    type Error = crate::Error;

    fn try_from(value: u32) -> crate::Result<Self> {
        match value {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            _ => Err(crate::Error::InvalidParameter(format!(
                "expression pedals are numbered 1 or 2, got {value}"
            ))),
        }
    }
}

/// How an expression pedal controls block bypass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum ExpressionBypassMode {
    /// Stop mode.
    Stop = 0,
    /// External switch mode.
    Switch = 1,
    /// Heel-toe sweep mode.
    HeelToe = 2,
}

/// A source for per-preset MIDI output.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[repr(u32)]
pub enum MidiSource {
    /// Footswitch A.
    FootswitchA = 0,
    /// Footswitch B.
    FootswitchB = 1,
    /// Footswitch C.
    FootswitchC = 2,
    /// Footswitch D.
    FootswitchD = 3,
    /// Footswitch E.
    FootswitchE = 4,
    /// Footswitch F.
    FootswitchF = 5,
    /// Footswitch G.
    FootswitchG = 6,
    /// Footswitch H.
    FootswitchH = 7,
    /// Expression pedal 1.
    Expression1 = 8,
    /// Expression pedal 2.
    Expression2 = 9,
}

impl MidiSource {
    const fn wire(self) -> u32 {
        self as u32
    }
}

impl TryFrom<u32> for MidiSource {
    type Error = crate::Error;

    fn try_from(value: u32) -> crate::Result<Self> {
        match value {
            0 => Ok(Self::FootswitchA),
            1 => Ok(Self::FootswitchB),
            2 => Ok(Self::FootswitchC),
            3 => Ok(Self::FootswitchD),
            4 => Ok(Self::FootswitchE),
            5 => Ok(Self::FootswitchF),
            6 => Ok(Self::FootswitchG),
            7 => Ok(Self::FootswitchH),
            8 => Ok(Self::Expression1),
            9 => Ok(Self::Expression2),
            _ => Err(crate::Error::InvalidParameter(format!(
                "MIDI output sources are numbered 0-9, got {value}"
            ))),
        }
    }
}

/// Wire type of one per-preset MIDI output message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum MidiOutType {
    /// Control Change, either one value or an expression range.
    ControlChange = 1,
    /// Control Change alternating between two values.
    ControlChangeToggle = 2,
    /// Program Change with bank-select bytes.
    ProgramChange = 3,
}

impl TryFrom<u32> for MidiOutType {
    type Error = crate::Error;

    fn try_from(value: u32) -> crate::Result<Self> {
        match value {
            1 => Ok(Self::ControlChange),
            2 => Ok(Self::ControlChangeToggle),
            3 => Ok(Self::ProgramChange),
            _ => Err(crate::Error::Decode(format!(
                "unknown preset MIDI output type {value}"
            ))),
        }
    }
}

/// One checked per-preset MIDI output message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MidiOut {
    /// Message type.
    pub message_type: MidiOutType,
    /// MIDI channel, 1-16.
    pub channel: u32,
    /// Type-specific first 7-bit value.
    pub param1: u32,
    /// Type-specific second 7-bit value.
    pub param2: u32,
    /// Type-specific third 7-bit value.
    pub param3: u32,
}

impl MidiOut {
    pub(crate) fn from_proto(message: &MidiMessageInfo) -> crate::Result<Option<Self>> {
        if message.r#type == 0
            && message.channel == 0
            && message.param1 == 0
            && message.param2 == 0
            && message.param3 == 0
        {
            return Ok(None);
        }
        Ok(Some(Self {
            message_type: MidiOutType::try_from(message.r#type)?,
            channel: message.channel,
            param1: message.param1,
            param2: message.param2,
            param3: message.param3,
        }))
    }

    fn checked(
        message_type: MidiOutType,
        channel: u32,
        param1: u32,
        param2: u32,
        param3: u32,
    ) -> crate::Result<Self> {
        if !(1..=16).contains(&channel) {
            return Err(crate::Error::InvalidParameter(format!(
                "MIDI channels are numbered 1-16, got {channel}"
            )));
        }
        if let Some(value) = [param1, param2, param3]
            .into_iter()
            .find(|value| *value > 127)
        {
            return Err(crate::Error::InvalidParameter(format!(
                "MIDI data bytes are 7-bit values 0-127, got {value}"
            )));
        }
        Ok(Self {
            message_type,
            channel,
            param1,
            param2,
            param3,
        })
    }

    /// Construct a footswitch Control Change with one value.
    ///
    /// # Errors
    ///
    /// Returns an invalid parameter error unless the channel is 1-16 and both
    /// data values are 0-127.
    pub fn cc(channel: u32, cc: u32, value: u32) -> crate::Result<Self> {
        Self::checked(MidiOutType::ControlChange, channel, cc, value, 0)
    }

    /// Construct a Control Change Toggle with minimum and maximum values.
    ///
    /// # Errors
    ///
    /// Returns an invalid parameter error unless the channel is 1-16 and all
    /// data values are 0-127.
    pub fn cc_toggle(channel: u32, cc: u32, minimum: u32, maximum: u32) -> crate::Result<Self> {
        Self::checked(
            MidiOutType::ControlChangeToggle,
            channel,
            cc,
            minimum,
            maximum,
        )
    }

    /// Construct an expression-pedal Control Change sweep.
    ///
    /// # Errors
    ///
    /// Returns an invalid parameter error unless the channel is 1-16 and all
    /// data values are 0-127.
    pub fn expression_cc(channel: u32, cc: u32, minimum: u32, maximum: u32) -> crate::Result<Self> {
        Self::checked(MidiOutType::ControlChange, channel, cc, minimum, maximum)
    }

    /// Construct a Program Change with optional bank-select bytes.
    ///
    /// # Errors
    ///
    /// Returns an invalid parameter error unless the channel is 1-16 and the
    /// program and bank values are 0-127.
    #[allow(clippy::similar_names)]
    pub fn pc(channel: u32, program: u32, bank_msb: u32, bank_lsb: u32) -> crate::Result<Self> {
        Self::checked(
            MidiOutType::ProgramChange,
            channel,
            bank_msb,
            bank_lsb,
            program,
        )
    }

    const fn to_proto(self) -> MidiMessageInfo {
        MidiMessageInfo {
            r#type: self.message_type as u32,
            channel: self.channel,
            param1: self.param1,
            param2: self.param2,
            param3: self.param3,
        }
    }
}

/// Metronome sound, in the unit's displayed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum MetronomeSound {
    /// Blip.
    Blip = 0,
    /// Block.
    Block = 1,
    /// Cowbell.
    Cowbell = 2,
    /// Digital.
    Digital = 3,
    /// Drum kit.
    DrumKit = 4,
    /// Soft kit.
    SoftKit = 5,
}
checked_tempo_enum!(
    MetronomeSound,
    6,
    [
        Blip = 0,
        Block = 1,
        Cowbell = 2,
        Digital = 3,
        DrumKit = 4,
        SoftKit = 5
    ]
);

/// Metronome output routing, in the unit's displayed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum MetronomeRouting {
    /// Multi-Out.
    Multi = 0,
    /// Headphones.
    Headphones = 1,
    /// Outputs 1/2.
    Out12 = 2,
    /// Outputs 3/4.
    Out34 = 3,
    /// Sends 1/2.
    Send12 = 4,
}
checked_tempo_enum!(
    MetronomeRouting,
    5,
    [Multi = 0, Headphones = 1, Out12 = 2, Out34 = 3, Send12 = 4]
);

/// Metronome time signature and accent grouping, in displayed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum TimeSignature {
    /// 2/4.
    TwoFour = 0,
    /// 3/4.
    ThreeFour = 1,
    /// 4/4.
    FourFour = 2,
    /// 5/4.
    FiveFour = 3,
    /// 6/4.
    SixFour = 4,
    /// 7/4.
    SevenFour = 5,
    /// 8/4.
    EightFour = 6,
    /// 9/4.
    NineFour = 7,
    /// 10/4.
    TenFour = 8,
    /// 11/4.
    ElevenFour = 9,
    /// 12/4.
    TwelveFour = 10,
    /// 13/4.
    ThirteenFour = 11,
    /// 3/8.
    ThreeEight = 12,
    /// 6/8.
    SixEight = 13,
    /// 9/8.
    NineEight = 14,
    /// 12/8.
    TwelveEight = 15,
    /// 5/8 grouped 3+2.
    FiveEight32 = 16,
    /// 5/8 grouped 2+3.
    FiveEight23 = 17,
    /// 7/8 grouped 3+2+2.
    SevenEight322 = 18,
    /// 7/8 grouped 2+3+2.
    SevenEight232 = 19,
    /// 7/8 grouped 2+2+3.
    SevenEight223 = 20,
}
checked_tempo_enum!(
    TimeSignature,
    21,
    [
        TwoFour = 0,
        ThreeFour = 1,
        FourFour = 2,
        FiveFour = 3,
        SixFour = 4,
        SevenFour = 5,
        EightFour = 6,
        NineFour = 7,
        TenFour = 8,
        ElevenFour = 9,
        TwelveFour = 10,
        ThirteenFour = 11,
        ThreeEight = 12,
        SixEight = 13,
        NineEight = 14,
        TwelveEight = 15,
        FiveEight32 = 16,
        FiveEight23 = 17,
        SevenEight322 = 18,
        SevenEight232 = 19,
        SevenEight223 = 20
    ]
);

/// A capture or impulse-response entry identified by the library key the
/// device uses when loading it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryEntry {
    /// Stable device library key. For a capture this is its content hash.
    pub key: String,
    /// Display name shown in the device browser.
    pub name: String,
}

impl LibraryEntry {
    fn from_proto(product: &crate::proto::ProductData) -> Option<Self> {
        let crate::proto::product_data::Key::Key(key) = product.key.as_ref()?;
        let crate::proto::product_data::Name::Name(name) = product.name.as_ref()?;
        if key.is_empty() || name.is_empty() {
            return None;
        }
        Some(Self {
            key: key.clone(),
            name: name.clone(),
        })
    }

    fn validate_name(&self, kind: &str) -> crate::Result<()> {
        if self.name.trim().is_empty() {
            return Err(crate::Error::InvalidParameter(format!(
                "{kind} library entry has an empty display name"
            )));
        }
        Ok(())
    }

    fn validate_capture(&self) -> crate::Result<()> {
        self.validate_name("capture")?;
        if self.key.len() != 64 || !self.key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(crate::Error::InvalidParameter(
                "capture library key must be the device-returned 64-character content hash".into(),
            ));
        }
        Ok(())
    }

    fn validate_ir(&self) -> crate::Result<()> {
        self.validate_name("IR")?;
        if self.key.is_empty() || self.key.starts_with('/') {
            return Err(crate::Error::InvalidParameter(
                "IR library key must be the non-path key returned by list_irs()".into(),
            ));
        }
        Ok(())
    }
}

/// Floor on the read-back used to settle a `set_block` whose echo did not
/// arrive. Generous, because that read-back is the ground truth and a busy
/// device is exactly the case that got us here.
const READ_BACK_TIMEOUT: Duration = Duration::from_secs(20);

/// One entry in a setlist listing: a preset occupying a slot.
///
/// `index` is the LINEAR slot position (see [`slot_to_position`]), not the
/// bank/letter shown on the unit. Use [`position_to_slot`] to display it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PresetEntry {
    /// Linear slot index, 0..255.
    pub index: u32,
    /// The preset name as shown on the unit.
    pub name: String,
    /// The device filesystem key, e.g. `/media/p4/Presets/My Presets/Foo.pb`.
    pub key: Option<String>,
    /// The instrument tag, if set.
    pub instrument: Option<Instrument>,
}

impl PresetEntry {
    /// Build from a `ProductData` listing entry, or `None` if it has no name
    /// (which is how the device reports an EMPTY slot - every setlist always
    /// reports its full complement of 256 slots).
    fn from_proto(pd: &crate::proto::ProductData) -> crate::Result<Option<Self>> {
        use crate::proto::product_data;
        let Some(name) = pd.name.as_ref() else {
            return Ok(None);
        };
        let name = match name {
            product_data::Name::Name(n) if !n.is_empty() => n.clone(),
            product_data::Name::Name(_) => return Ok(None),
        };
        let index = match pd.index.as_ref() {
            Some(product_data::Index::Index(i)) => u32::try_from(*i).map_err(|_| {
                crate::Error::Decode(format!("preset listing index {i} is negative"))
            })?,
            None => return Ok(None),
        };
        let key = pd.key.as_ref().map(|k| {
            let product_data::Key::Key(v) = k;
            v.clone()
        });
        let instrument = pd
            .instrument
            .as_ref()
            .map(|i| {
                let product_data::Instrument::Instrument(v) = i;
                Instrument::try_from(*v)
            })
            .transpose()?;
        Ok(Some(Self {
            index,
            name,
            key,
            instrument,
        }))
    }
}

/// Verified result of copying one recalled preset into a prepared destination.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CopyPresetReceipt {
    /// Source setlist key.
    pub source_setlist: String,
    /// Source displayed slot.
    pub source_slot: String,
    /// Destination entry from a fresh complete listing. Its name is the name
    /// the device actually stored after any collision handling.
    pub stored: PresetEntry,
}

/// Honest result of a setlist duplication, including partial progress.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DuplicateSetlistReceipt {
    /// Freshly created destination folder.
    pub destination: Folder,
    /// Number of occupied source entries selected by `limit`.
    pub selected: usize,
    /// Copies proven by fresh destination listings.
    pub copied: Vec<CopyPresetReceipt>,
    /// Failure text when the destination was only partially populated.
    pub failure: Option<String>,
}

impl DuplicateSetlistReceipt {
    /// Whether every selected source entry was copied and verified.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.failure.is_none() && self.copied.len() == self.selected
    }
}

/// A folder the device knows about: a setlist, the Captures library, an IR
/// library, or a plugin artist folder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Folder {
    /// The device filesystem key, used to address the folder in requests.
    pub key: String,
    /// Display name.
    pub name: String,
    /// How many slots the listing reported (a setlist always reports 256).
    pub slots: usize,
    /// How many of those slots are occupied.
    pub occupied: usize,
    /// Whether this is the read-only factory setlist.
    pub is_factory: bool,
}

impl Folder {
    /// Build from a `FolderInfo`, or `None` if it carries no key (which is
    /// how a folder is addressed, so a keyless one is unusable).
    fn from_proto(info: &crate::proto::FolderInfo) -> Option<Self> {
        use crate::proto::folder_info;
        let key = match info.key.as_ref()? {
            folder_info::Key::Key(k) if !k.is_empty() => k.clone(),
            folder_info::Key::Key(_) => return None,
        };
        let name = info.name.as_ref().map_or_else(String::new, |n| {
            let folder_info::Name::Name(v) = n;
            v.clone()
        });
        let is_factory = info.is_factory.as_ref().is_some_and(|f| {
            let folder_info::IsFactory::IsFactory(v) = f;
            *v
        });
        let occupied = info
            .files
            .iter()
            .filter(|pd| {
                pd.name.as_ref().is_some_and(|name| {
                    let crate::proto::product_data::Name::Name(name) = name;
                    !name.is_empty()
                })
            })
            .count();
        Some(Self {
            key,
            name,
            slots: info.files.len(),
            occupied,
            is_factory,
        })
    }
}

/// Build a `SetlistPosition{UPDATE}` payload, optionally tagged with a
/// `request_id` so a resulting broadcast can be correlated back to it.
///
/// `position` is either a linear slot index or a slot name like `"28C"`.
fn build_recall(
    setlist_path: &str,
    position: &str,
    is_factory: bool,
    request_id: Option<u64>,
) -> crate::Result<Vec<u8>> {
    use crate::proto::setlist_position_message as spm;
    let pos = slot_to_position_checked(position)
        .ok_or_else(|| crate::Error::InvalidSlot(position.to_string()))?;
    let msg = SetlistPositionMessage {
        action: MessageAction::Update as i32,
        request_id: request_id.map(spm::RequestId::RequestId),
        folder_key: Some(spm::FolderKey::FolderKey(setlist_path.into())),
        position: Some(spm::Position::Position(pos)),
        is_factory: Some(spm::IsFactory::IsFactory(is_factory)),
        ..Default::default()
    };
    Ok(prost::Message::encode_to_vec(&msg))
}

/// Extract a folder's key, with any trailing slash normalised away.
///
/// Note the trailing-slash asymmetry this absorbs: recalls need the factory
/// path WITH its trailing slash (Cortex Control sends it that way), but the
/// device reports that same folder's listing key WITHOUT one. Comparing
/// normalised keys is what lets one setlist argument serve both.
fn folder_key(info: &crate::proto::FolderInfo) -> Option<&str> {
    let crate::proto::folder_info::Key::Key(key) = info.key.as_ref()?;
    Some(key.trim_end_matches('/'))
}

fn is_preset_file_message(message: &FileMessage) -> bool {
    message.r#type.as_ref().is_none_or(|kind| {
        let crate::proto::file_message::Type::Type(value) = kind;
        *value == 0
    })
}

fn complete_setlist_folder(folder: &crate::proto::FolderInfo, wanted: &str) -> bool {
    if folder_key(folder) != Some(wanted) || folder.files.len() != SETLIST_SLOTS as usize {
        return false;
    }
    let mut seen = vec![false; SETLIST_SLOTS as usize];
    for file in &folder.files {
        let Some(crate::proto::product_data::Index::Index(index)) = file.index else {
            return false;
        };
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        if index >= seen.len() || std::mem::replace(&mut seen[index], true) {
            return false;
        }
    }
    seen.into_iter().all(std::convert::identity)
}

fn matches_setlist_listing(message: &FileMessage, wanted: &str) -> bool {
    if message.action != MessageAction::Update as i32
        || !is_preset_file_message(message)
        || message.to_folder.is_some()
    {
        return false;
    }
    let Some(crate::proto::file_message::Folder::Folder(folder)) = message.folder.as_ref() else {
        return false;
    };
    complete_setlist_folder(folder, wanted)
}

fn matches_library_listing(message: &FileMessage, request_id: u64, wanted: &str) -> bool {
    if message.action != MessageAction::Update as i32 {
        return false;
    }
    if !matches!(
        message.request_id,
        Some(crate::proto::file_message::RequestId::RequestId(value)) if value == request_id
    ) {
        return false;
    }
    let Some(crate::proto::file_message::Folder::Folder(folder)) = message.folder.as_ref() else {
        return false;
    };
    folder_key(folder) == Some(wanted)
}

fn matches_favorite_echo(
    message: &RecentsFavoritesMessage,
    action: MessageAction,
    target: &RecentsFavoritesItem,
) -> bool {
    message.action == action as i32
        && message.is_favorites
        && message.items.as_slice() == std::slice::from_ref(target)
}

fn matches_save_ack(message: &FileMessage, setlist: &str, index: u32) -> bool {
    if message.action != MessageAction::Create as i32
        || !is_preset_file_message(message)
        || message.to_folder.is_some()
    {
        return false;
    }
    let Some(crate::proto::file_message::Folder::Folder(folder)) = message.folder.as_ref() else {
        return false;
    };
    if folder_key(folder) != Some(setlist) || folder.files.len() != 1 {
        return false;
    }
    matches!(
        folder.files[0].index,
        Some(crate::proto::product_data::Index::Index(value))
            if u32::try_from(value).ok() == Some(index)
    )
}

fn matches_delete_ack(message: &FileMessage, setlist: &str, key: &str) -> bool {
    if message.action != MessageAction::Delete as i32
        || !is_preset_file_message(message)
        || message.to_folder.is_some()
    {
        return false;
    }
    let Some(crate::proto::file_message::Folder::Folder(folder)) = message.folder.as_ref() else {
        return false;
    };
    if folder_key(folder) != Some(setlist) || folder.files.len() != 1 {
        return false;
    }
    matches!(
        folder.files[0].key.as_ref(),
        Some(crate::proto::product_data::Key::Key(value)) if value == key
    )
}

fn user_setlist_key(name: &str) -> crate::Result<String> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        return Err(crate::Error::UnsafeSave(format!(
            "setlist name {name:?} must be one safe path component"
        )));
    }
    let key = format!("{USER_SETLIST_ROOT}/{name}");
    if !key.starts_with(&format!("{USER_SETLIST_ROOT}/")) {
        return Err(crate::Error::UnsafeSave(
            "setlist path escaped the USER setlist root".into(),
        ));
    }
    Ok(key)
}

/// Validate one USER setlist name and resolve it beneath [`USER_SETLIST_ROOT`].
///
/// Names are single safe path components, never caller-supplied paths.
///
/// # Errors
///
/// Returns [`crate::Error::UnsafeSave`] for an empty, dot, nested, absolute,
/// control-character, or otherwise root-escaping name.
pub fn user_setlist_path(name: &str) -> crate::Result<String> {
    user_setlist_key(name)
}

fn setlist_name_from_key(key: &str) -> Option<&str> {
    let name = key.strip_prefix(&format!("{USER_SETLIST_ROOT}/"))?;
    if user_setlist_key(name).ok().as_deref() == Some(key) {
        Some(name)
    } else {
        None
    }
}

fn protected_setlist_name(name: &str) -> bool {
    name == "My Presets"
}

fn build_create_setlist(name: &str) -> crate::Result<FileMessage> {
    let key = user_setlist_key(name)?;
    Ok(FileMessage {
        action: MessageAction::Create as i32,
        r#type: Some(crate::proto::file_message::Type::Type(0)),
        folder: Some(crate::proto::file_message::Folder::Folder(
            crate::proto::FolderInfo {
                key: Some(crate::proto::folder_info::Key::Key(key)),
                name: Some(crate::proto::folder_info::Name::Name(name.to_string())),
                is_factory: Some(crate::proto::folder_info::IsFactory::IsFactory(false)),
                ..Default::default()
            },
        )),
        ..Default::default()
    })
}

fn build_delete_setlist(name: &str) -> crate::Result<FileMessage> {
    if protected_setlist_name(name) {
        return Err(crate::Error::UnsafeSave(
            "My Presets is the default USER setlist and cannot be deleted".into(),
        ));
    }
    let key = user_setlist_key(name)?;
    Ok(FileMessage {
        action: MessageAction::Delete as i32,
        r#type: Some(crate::proto::file_message::Type::Type(0)),
        folder: Some(crate::proto::file_message::Folder::Folder(
            crate::proto::FolderInfo {
                key: Some(crate::proto::folder_info::Key::Key(key)),
                name: Some(crate::proto::folder_info::Name::Name(name.to_string())),
                is_factory: Some(crate::proto::folder_info::IsFactory::IsFactory(false)),
                ..Default::default()
            },
        )),
        ..Default::default()
    })
}

fn direct_user_setlists(folders: Vec<Folder>) -> Vec<Folder> {
    folders
        .into_iter()
        .filter(|folder| setlist_name_from_key(&folder.key).is_some() && !folder.is_factory)
        .collect()
}

fn move_source(
    policy: &crate::safety::SavePolicy,
    setlist: &str,
    entries: &[PresetEntry],
    source: u32,
    destination: u32,
) -> crate::Result<(String, String)> {
    let entry = entries
        .iter()
        .find(|entry| entry.index == source)
        .ok_or_else(|| {
            crate::Error::NotFound(format!(
                "no preset occupies source slot {}",
                position_to_slot(source)
            ))
        })?;
    if source == destination {
        return Err(crate::Error::UnsafeMove(format!(
            "source and destination are both {}",
            position_to_slot(destination)
        )));
    }
    if let Some(occupied) = entries.iter().find(|entry| entry.index == destination) {
        return Err(crate::Error::UnsafeMove(format!(
            "destination {} is occupied by {:?}; moving onto occupied slots is not supported",
            position_to_slot(destination),
            occupied.name
        )));
    }
    if !policy.contains_scratch_slot(setlist, source)
        || !policy.contains_scratch_slot(setlist, destination)
    {
        return Err(crate::Error::UnsafeMove(format!(
            "both source {} and destination {} must be inside the configured scratch range",
            position_to_slot(source),
            position_to_slot(destination)
        )));
    }
    let key = entry.key.clone().ok_or_else(|| {
        crate::Error::Decode(format!(
            "listing entry for {:?} carried no device file path",
            entry.name
        ))
    })?;
    Ok((key, entry.name.clone()))
}

fn move_converged(entries: &[PresetEntry], name: &str, source: u32, destination: u32) -> bool {
    let wanted = name.trim().to_lowercase();
    entries
        .iter()
        .any(|entry| entry.index == destination && entry.name.trim().to_lowercase() == wanted)
        && entries.iter().all(|entry| entry.index != source)
}

fn stored_name_matches_request(requested: &str, stored: &str) -> bool {
    if stored == requested {
        return true;
    }
    let Some((base, suffix)) = stored.rsplit_once('_') else {
        return false;
    };
    let suffix_is_valid = suffix.parse::<u32>().is_ok_and(|value| value > 0);
    let Some(max_base_len) = 20usize.checked_sub(1 + suffix.chars().count()) else {
        return false;
    };
    let expected_base = requested.chars().take(max_base_len).collect::<String>();
    !base.is_empty() && stored.chars().count() <= 20 && suffix_is_valid && base == expected_base
}

fn build_move_preset(setlist: &str, source_key: &str, destination: u32) -> FileMessage {
    FileMessage {
        action: MessageAction::Move as i32,
        r#type: Some(crate::proto::file_message::Type::Type(0)),
        folder: Some(crate::proto::file_message::Folder::Folder(
            crate::proto::FolderInfo {
                key: Some(crate::proto::folder_info::Key::Key(setlist.to_string())),
                is_factory: Some(crate::proto::folder_info::IsFactory::IsFactory(false)),
                files: vec![crate::proto::ProductData {
                    key: Some(crate::proto::product_data::Key::Key(source_key.to_string())),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )),
        to_folder: Some(crate::proto::file_message::ToFolder::ToFolder(
            crate::proto::FolderInfo {
                key: Some(crate::proto::folder_info::Key::Key(setlist.to_string())),
                files: vec![crate::proto::ProductData {
                    index: Some(crate::proto::product_data::Index::Index(
                        i32::try_from(destination).unwrap_or(i32::MAX),
                    )),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

/// Convert one complete folder listing into the stable preset-entry view.
fn preset_entries(
    folder: &crate::proto::FolderInfo,
    include_empty: bool,
) -> crate::Result<Vec<PresetEntry>> {
    let mut entries: Vec<PresetEntry> = folder
        .files
        .iter()
        .filter_map(|entry| PresetEntry::from_proto(entry).transpose())
        .collect::<crate::Result<_>>()?;
    entries.sort_by_key(|entry| entry.index);
    if include_empty {
        let occupied: std::collections::HashMap<u32, PresetEntry> = entries
            .into_iter()
            .map(|entry| (entry.index, entry))
            .collect();
        let total = u32::try_from(folder.files.len()).unwrap_or(SETLIST_SLOTS);
        entries = (0..total)
            .map(|index| {
                occupied
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| PresetEntry {
                        index,
                        name: String::new(),
                        key: None,
                        instrument: None,
                    })
            })
            .collect();
    }
    Ok(entries)
}

/// The model id at a grid cell, if the cell is occupied.
fn model_id_at(preset: &BinaryPreset, row: Row, column: u32) -> Option<u32> {
    let model = model_at(preset, row, column)?;
    match model.hash {
        Some(crate::proto::model::Hash::Hash(id)) if id != 0 => Some(id),
        _ => None,
    }
}

/// The complete model payload at an occupied grid cell.
fn model_at(preset: &BinaryPreset, row: Row, column: u32) -> Option<&crate::proto::Model> {
    let model = crate::helpers::model_at(preset, row.wire(), column)?;
    match model.hash {
        Some(crate::proto::model::Hash::Hash(id)) if id != 0 => Some(model),
        _ => None,
    }
}

/// One complete chain, applying the wire's explicit-key-then-position rule.
fn chain_at(preset: &BinaryPreset, row: Row) -> Option<&crate::proto::Chain> {
    preset
        .chains
        .iter()
        .enumerate()
        .find(|(position, chain)| {
            let positional = u32::try_from(*position).ok();
            let stored = chain.row.as_ref().map(|value| {
                let crate::proto::chain::Row::Row(row) = value;
                *row
            });
            stored.or(positional) == Some(row.wire())
        })
        .map(|(_, chain)| chain)
}

/// One parameter, applying the wire's explicit-key-then-position rule.
fn parameter_at(
    preset: &BinaryPreset,
    row: Row,
    column: u32,
    index: u32,
) -> Option<&crate::proto::Param> {
    model_at(preset, row, column)?
        .params
        .iter()
        .enumerate()
        .find(|(position, parameter)| {
            let positional = u32::try_from(*position).ok();
            let stored = parameter.index.as_ref().map(|value| {
                let crate::proto::param::Index::Index(index) = value;
                *index
            });
            stored.or(positional) == Some(index)
        })
        .map(|(_, parameter)| parameter)
}

fn parameter_matches(
    parameter: &crate::proto::Param,
    expected: &Value,
    scene: u32,
) -> crate::Result<bool> {
    let follows_scenes = parameter.scene_mode.as_ref().is_some_and(|value| {
        let crate::proto::param::SceneMode::SceneMode(enabled) = value;
        *enabled
    });
    let value = if follows_scenes {
        parameter
            .param_values
            .get(usize::try_from(scene).ok().unwrap_or(usize::MAX))
    } else {
        parameter.param_values.first()
    };
    let Some(value) = value.and_then(|value| value.value.as_ref()) else {
        return Ok(false);
    };
    match (expected, value) {
        (Value::Normalised(expected), crate::proto::param_value::Value::FloatValue(actual)) => {
            crate::helpers::params_equal(*expected, *actual, None)
        }
        (Value::Normalised(expected), crate::proto::param_value::Value::IntValue(actual)) =>
        {
            #[allow(clippy::cast_precision_loss)]
            crate::helpers::params_equal(*expected, *actual as f32, None)
        }
        (Value::Text(expected), crate::proto::param_value::Value::StringValue(actual)) => {
            Ok(expected == actual)
        }
        _ => Ok(false),
    }
}

/// The complete model state at a cell, without its location key.
fn model_state_at(preset: &BinaryPreset, row: Row, column: u32) -> Option<crate::proto::Model> {
    let mut model = model_at(preset, row, column)?.clone();
    model.column = None;
    Some(model)
}

/// The bypass state exposed for a cell, independent of its row/column keys.
fn bypass_state_at(
    preset: &BinaryPreset,
    row: Row,
    column: u32,
) -> Option<(Vec<bool>, Option<bool>)> {
    for (bypass_index, entry) in preset.bypass.iter().enumerate() {
        let positional_row = u32::try_from(bypass_index).ok()?;
        let entry_row = entry.row.as_ref().map_or(positional_row, |value| {
            let crate::proto::bypass::Row::Row(row) = value;
            *row
        });
        if entry_row != row.wire() {
            continue;
        }
        for (column_index, value) in entry.col_bypass.iter().enumerate() {
            let positional_column = u32::try_from(column_index).ok()?;
            let entry_column = value.column.as_ref().map_or(positional_column, |entry| {
                let crate::proto::col_bypass::Column::Column(column) = entry;
                *column
            });
            if entry_column == column {
                let scene_mode = value.scene_mode.as_ref().map(|entry| {
                    let crate::proto::col_bypass::SceneMode::SceneMode(enabled) = entry;
                    *enabled
                });
                return Some((
                    value
                        .scene_bypass
                        .iter()
                        .map(|scene| scene.bypass)
                        .collect(),
                    scene_mode,
                ));
            }
        }
    }
    None
}

/// Validate a caller-provided wire value before the device can store it.
fn normalised_value(value: f32) -> crate::Result<Value> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(crate::Error::InvalidParameter(format!(
            "normalised values are 0.0-1.0, got {value}"
        )));
    }
    Ok(Value::Normalised(value))
}

fn validate_grid_cell(row: Row, column: u32) -> crate::Result<()> {
    if row.wire() > 3 {
        return Err(crate::Error::InvalidRow(format!(
            "wire rows are numbered 0-3, got {}",
            row.wire()
        )));
    }
    if column > 7 {
        return Err(crate::Error::InvalidParameter(format!(
            "grid columns are numbered 0-7, got {column}"
        )));
    }
    Ok(())
}

fn validate_parameter_writes(parameters: &[ParameterWrite]) -> crate::Result<()> {
    for parameter in parameters {
        if let Value::Normalised(value) = &parameter.value {
            normalised_value(*value)?;
        }
    }
    Ok(())
}

const fn is_capture_model(model_id: u32) -> bool {
    matches!(model_id, 14_000 | 14_001)
}

const fn is_ir_loader_model(model_id: u32) -> bool {
    model_id >= FIRST_IR_LOADER_MODEL && model_id <= LAST_IR_LOADER_MODEL
}

fn build_midi_settings(
    source: u32,
    messages: &[MidiOut],
    preset_load: bool,
) -> crate::Result<MidiSettingsMessage> {
    if messages.len() > 12 {
        return Err(crate::Error::InvalidParameter(format!(
            "a MIDI output source accepts at most 12 messages, got {}",
            messages.len()
        )));
    }
    let group = GeneralMidiMessages {
        messages: vec![GeneralMidiMessage {
            source: Some(crate::proto::general_midi_message::Source::Source(source)),
            msg: messages.iter().copied().map(MidiOut::to_proto).collect(),
        }],
    };
    let mut message = MidiSettingsMessage {
        action: MessageAction::Update as i32,
        ..Default::default()
    };
    if preset_load {
        message.preset_load_messages = Some(
            crate::proto::midi_settings_message::PresetLoadMessages::PresetLoadMessages(group),
        );
    } else {
        message.general_midi_messages = Some(
            crate::proto::midi_settings_message::GeneralMidiMessages::GeneralMidiMessages(group),
        );
    }
    Ok(message)
}

/// Resolve a named parameter's typed input into its wire value.
fn parameter_value(parameter: &Parameter, input: ParameterInput) -> crate::Result<Value> {
    match (parameter.kind, input) {
        (ParameterKind::Str, ParameterInput::Text(value)) => Ok(Value::Text(value)),
        (
            ParameterKind::Float
            | ParameterKind::Int
            | ParameterKind::Switch
            | ParameterKind::Fader,
            ParameterInput::Normalised(value),
        ) => normalised_value(value),
        (
            ParameterKind::Float
            | ParameterKind::Int
            | ParameterKind::Switch
            | ParameterKind::Fader,
            ParameterInput::Real(value),
        ) => {
            if !value.is_finite() {
                return Err(crate::Error::InvalidParameter(format!(
                    "real-unit values must be finite, got {value}"
                )));
            }
            let normalised = parameter.to_normalised(value).ok_or_else(|| {
                crate::Error::InvalidParameter(format!(
                    "{} has a placeholder range ({}..{}); use a normalised value",
                    parameter.name, parameter.min, parameter.max
                ))
            })?;
            #[allow(clippy::cast_possible_truncation)]
            normalised_value(normalised as f32)
        }
        (ParameterKind::Meter, _) => Err(crate::Error::InvalidParameter(format!(
            "{} is a live meter, not a setting",
            parameter.name
        ))),
        (ParameterKind::Str, _) => Err(crate::Error::InvalidParameter(format!(
            "{} is a string parameter; use text input",
            parameter.name
        ))),
        (
            ParameterKind::Float
            | ParameterKind::Int
            | ParameterKind::Switch
            | ParameterKind::Fader,
            ParameterInput::Text(_),
        ) => Err(crate::Error::InvalidParameter(format!(
            "{} is a numeric parameter; use a normalised or real-unit value",
            parameter.name
        ))),
        (ParameterKind::Empty | ParameterKind::Unknown, _) => {
            Err(crate::Error::InvalidParameter(format!(
                "{} has unsupported parameter type {:?}",
                parameter.name, parameter.kind
            )))
        }
    }
}

fn option_parameter_write(
    source: &BinaryPreset,
    row: Row,
    column: u32,
    target: &ParameterTarget,
    option: &str,
    catalog: Option<&crate::Catalog>,
) -> crate::Result<ParameterWrite> {
    let index = match target {
        ParameterTarget::Index(index) => *index,
        ParameterTarget::Name(name) => {
            if name.trim().is_empty() {
                return Err(crate::Error::InvalidParameter(
                    "parameter name cannot be empty".into(),
                ));
            }
            let model_id = model_id_at(source, row, column).ok_or_else(|| {
                crate::Error::NotFound(format!(
                    "source preset has no block at screen row {} column {column}",
                    row.screen()
                ))
            })?;
            let catalog = catalog.ok_or_else(|| {
                crate::Error::NotFound("the device model catalog is unavailable".into())
            })?;
            let model = catalog.get(model_id).ok_or_else(|| {
                crate::Error::NotFound(format!(
                    "model {model_id} from the source preset is not in this unit's catalog"
                ))
            })?;
            u32::try_from(
                model
                    .parameter(name)
                    .ok_or_else(|| {
                        crate::Error::NotFound(format!("{} has no parameter {name:?}", model.name))
                    })?
                    .index,
            )
            .map_err(|_| {
                crate::Error::InvalidParameter("parameter index does not fit on the wire".into())
            })?
        }
    };
    let options = crate::helpers::param_options(source, row.wire(), column, index);
    let value = crate::helpers::option_value(&options, option)?;
    Ok(ParameterWrite {
        index,
        value: Value::Normalised(value),
    })
}

/// How a block placement was confirmed.
///
/// Worth distinguishing, because the two carry different confidence: an echo
/// is the device telling us it accepted the cell, while a read-back is us
/// observing the grid afterwards. Both mean the block is there; only the
/// second survives a slow device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// The device echoed a `Grid` broadcast naming the cell.
    EchoConfirmed,
    /// No echo arrived in time, but a read-back found the block in place.
    ReadBackConfirmed,
    /// The write was sent without waiting for confirmation.
    Unverified,
}

/// How a caller addresses a block parameter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "by", content = "value", rename_all = "snake_case")]
pub enum ParameterTarget {
    /// Raw positional wire index.
    Index(u32),
    /// Display name, resolved case-insensitively through the device catalog.
    Name(String),
}

/// A parameter value before any catalog conversion.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ParameterInput {
    /// The wire's normalised 0..1 representation.
    Normalised(f32),
    /// A value in the parameter's displayed units, resolved through the catalog.
    Real(f64),
    /// A string value such as a microphone or capture selection.
    Text(String),
}

/// The concrete wire parameter write after name and unit resolution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParameterWrite {
    /// Positional wire index that was written.
    pub index: u32,
    /// Wire value after conversion.
    pub value: Value,
}

fn validate_scene(scene: u32) -> crate::Result<()> {
    if scene > 7 {
        return Err(crate::Error::InvalidScene(format!(
            "scene {scene} is out of range; scenes are 0-7 (A-H)"
        )));
    }
    Ok(())
}

fn validate_normalized(name: &str, value: f32) -> crate::Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(crate::Error::InvalidParameter(format!(
            "{name} must be finite and normalized to 0-1, got {value}"
        )));
    }
    Ok(())
}

fn io_update(settings: PortSettings) -> IoSettingsMessage {
    IoSettingsMessage {
        action: MessageAction::Update as i32,
        settings: Some(crate::proto::io_settings_message::Settings::Settings(
            settings,
        )),
        ..Default::default()
    }
}

/// Build input-port updates in level, impedance, type, ground-lift order.
/// Every message repeats the port key and contains exactly one writable field.
///
/// # Errors
///
/// Refuses an empty patch or any non-finite/non-normalized value.
pub fn build_input_port_updates(
    port: InputPort,
    patch: InputPortPatch,
) -> crate::Result<Vec<IoSettingsMessage>> {
    let fields = [
        ("input level", patch.level),
        ("input impedance", patch.impedance),
        ("input type", patch.input_type),
        ("input ground lift", patch.ground_lift),
    ];
    if fields.iter().all(|(_, value)| value.is_none()) {
        return Err(crate::Error::InvalidParameter(
            "InputPortPatch contains no change".into(),
        ));
    }
    for (name, value) in fields {
        if let Some(value) = value {
            validate_normalized(name, value)?;
        }
    }

    let keyed = || InputPortSettings {
        input_port_id: port as u32,
        ..Default::default()
    };
    let mut messages = Vec::new();
    if let Some(value) = patch.level {
        let mut field = keyed();
        field.level = Some(crate::proto::input_port_settings::Level::Level(value));
        messages.push(io_update(PortSettings {
            in_port: vec![field],
            ..Default::default()
        }));
    }
    if let Some(value) = patch.impedance {
        let mut field = keyed();
        field.input_zmode = Some(crate::proto::input_port_settings::InputZmode::InputZmode(
            value,
        ));
        messages.push(io_update(PortSettings {
            in_port: vec![field],
            ..Default::default()
        }));
    }
    if let Some(value) = patch.input_type {
        let mut field = keyed();
        field.input_type = Some(crate::proto::input_port_settings::InputType::InputType(
            value,
        ));
        messages.push(io_update(PortSettings {
            in_port: vec![field],
            ..Default::default()
        }));
    }
    if let Some(value) = patch.ground_lift {
        let mut field = keyed();
        field.ground_lift = Some(crate::proto::input_port_settings::GroundLift::GroundLift(
            value,
        ));
        messages.push(io_update(PortSettings {
            in_port: vec![field],
            ..Default::default()
        }));
    }
    Ok(messages)
}

/// Build output-port updates in level, ground-lift, mute order.
/// Every message repeats the port key and contains exactly one writable field.
///
/// # Errors
///
/// Refuses an empty patch or any non-finite/non-normalized value.
pub fn build_output_port_updates(
    port: OutputPort,
    patch: OutputPortPatch,
) -> crate::Result<Vec<IoSettingsMessage>> {
    if patch == OutputPortPatch::default() {
        return Err(crate::Error::InvalidParameter(
            "OutputPortPatch contains no change".into(),
        ));
    }
    if let Some(value) = patch.level {
        validate_normalized("output level", value)?;
    }
    if let Some(value) = patch.ground_lift {
        validate_normalized("output ground lift", value)?;
    }

    let keyed = || OutputPortSettings {
        output_port_id: port as u32,
        ..Default::default()
    };
    let mut messages = Vec::new();
    if let Some(value) = patch.level {
        let mut field = keyed();
        field.level = Some(crate::proto::output_port_settings::Level::Level(value));
        messages.push(io_update(PortSettings {
            out_port: vec![field],
            ..Default::default()
        }));
    }
    if let Some(value) = patch.ground_lift {
        let mut field = keyed();
        field.ground_lift = Some(crate::proto::output_port_settings::GroundLift::GroundLift(
            value,
        ));
        messages.push(io_update(PortSettings {
            out_port: vec![field],
            ..Default::default()
        }));
    }
    if let Some(value) = patch.mute {
        let mut field = keyed();
        field.mute = Some(crate::proto::output_port_settings::Mute::Mute(value));
        messages.push(io_update(PortSettings {
            out_port: vec![field],
            ..Default::default()
        }));
    }
    Ok(messages)
}

/// Build USB updates in level, headphone-source, dry/wet order, one field per message.
///
/// # Errors
///
/// Refuses an empty patch or any non-finite/non-normalized value.
pub fn build_usb_port_updates(patch: UsbPortPatch) -> crate::Result<Vec<IoSettingsMessage>> {
    let fields = [
        ("USB level", patch.level),
        ("USB headphone source", patch.headphone_source),
        ("USB dry/wet", patch.dry_wet),
    ];
    if fields.iter().all(|(_, value)| value.is_none()) {
        return Err(crate::Error::InvalidParameter(
            "UsbPortPatch contains no change".into(),
        ));
    }
    for (name, value) in fields {
        if let Some(value) = value {
            validate_normalized(name, value)?;
        }
    }

    let wrap = |port| {
        io_update(PortSettings {
            usb_port: Some(crate::proto::port_settings::UsbPort::UsbPort(port)),
            ..Default::default()
        })
    };
    let mut messages = Vec::new();
    if let Some(value) = patch.level {
        messages.push(wrap(UsbPortSettings {
            level: Some(crate::proto::usb_port_settings::Level::Level(value)),
            ..Default::default()
        }));
    }
    if let Some(value) = patch.headphone_source {
        messages.push(wrap(UsbPortSettings {
            hp_select: Some(crate::proto::usb_port_settings::HpSelect::HpSelect(value)),
            ..Default::default()
        }));
    }
    if let Some(value) = patch.dry_wet {
        messages.push(wrap(UsbPortSettings {
            dry_wet: Some(crate::proto::usb_port_settings::DryWet::DryWet(value)),
            ..Default::default()
        }));
    }
    Ok(messages)
}

/// Build the one-field MIDI Thru update. The protobuf stores this Boolean as a float.
#[must_use]
pub fn build_midi_thru_update(enabled: bool) -> IoSettingsMessage {
    io_update(PortSettings {
        midi_port: Some(crate::proto::port_settings::MidiPort::MidiPort(
            MidiPortSettings {
                midi_thru: Some(crate::proto::midi_port_settings::MidiThru::MidiThru(
                    f32::from(enabled),
                )),
            },
        )),
        ..Default::default()
    })
}

/// Build output-pairing updates in XLR 1/2 then Out 3/4 order, one field per message.
///
/// # Errors
///
/// Refuses an empty patch.
pub fn build_output_pairing_updates(
    patch: OutputPairingPatch,
) -> crate::Result<Vec<IoSettingsMessage>> {
    if patch == OutputPairingPatch::default() {
        return Err(crate::Error::InvalidParameter(
            "OutputPairingPatch contains no change".into(),
        ));
    }
    let mut messages = Vec::new();
    if let Some(value) = patch.xlr12 {
        messages.push(IoSettingsMessage {
            action: MessageAction::Update as i32,
            xlr1_2_linked: Some(crate::proto::io_settings_message::Xlr12Linked::Xlr12Linked(
                value,
            )),
            ..Default::default()
        });
    }
    if let Some(value) = patch.out34 {
        messages.push(IoSettingsMessage {
            action: MessageAction::Update as i32,
            out3_4_linked: Some(crate::proto::io_settings_message::Out34Linked::Out34Linked(
                value,
            )),
            ..Default::default()
        });
    }
    Ok(messages)
}

fn io_settings_are_complete(message: &IoSettingsMessage) -> bool {
    let Some(crate::proto::io_settings_message::Settings::Settings(settings)) =
        message.settings.as_ref()
    else {
        return false;
    };
    let mut input_ids = [false; 7];
    let inputs_complete = settings.in_port.len() == 4
        && settings.in_port.iter().all(|port| {
            let Ok(index) = usize::try_from(port.input_port_id) else {
                return false;
            };
            if InputPort::try_from(port.input_port_id).is_err() || input_ids[index] {
                return false;
            }
            input_ids[index] = true;
            let presence = (
                port.level.is_some(),
                port.input_zmode.is_some(),
                port.input_type.is_some(),
                port.ground_lift.is_some(),
            );
            match port.input_port_id {
                1 | 2 => presence == (true, true, true, true),
                4 | 5 => presence == (true, false, false, true),
                _ => false,
            }
        })
        && [1, 2, 4, 5].into_iter().all(|id| input_ids[id]);
    let mut output_ids = [false; 10];
    let outputs_complete = settings.out_port.len() == 8
        && settings.out_port.iter().all(|port| {
            let Ok(index) = usize::try_from(port.output_port_id) else {
                return false;
            };
            if OutputPort::try_from(port.output_port_id).is_err() || output_ids[index] {
                return false;
            }
            output_ids[index] = true;
            let presence = (
                port.level.is_some(),
                port.ground_lift.is_some(),
                port.mute.is_some(),
            );
            match port.output_port_id {
                1 | 4 | 5 => presence == (true, true, true),
                2 | 6 | 7 => presence == (true, false, true),
                8 | 9 => presence == (true, false, false),
                _ => false,
            }
        })
        && [1, 2, 4, 5, 6, 7, 8, 9]
            .into_iter()
            .all(|id| output_ids[id]);
    let usb_complete = settings.usb_port.as_ref().is_some_and(|port| {
        let crate::proto::port_settings::UsbPort::UsbPort(port) = port;
        port.level.is_some() && port.hp_select.is_some() && port.dry_wet.is_some()
    });
    let midi_complete = settings.midi_port.as_ref().is_some_and(|port| {
        let crate::proto::port_settings::MidiPort::MidiPort(port) = port;
        port.midi_thru.is_some()
    });
    inputs_complete
        && outputs_complete
        && usb_complete
        && midi_complete
        && message.xlr1_2_linked.is_some()
        && message.out3_4_linked.is_some()
}

/// Build one safe sparse `GeneralSettings{UPDATE}` message.
///
/// # Errors
///
/// Refuses an empty patch and an invalid MIDI channel before any device I/O.
#[allow(clippy::too_many_lines)] // Explicit field mapping keeps unsafe schema fields absent.
pub fn build_settings_update(
    patch: &GeneralSettingsPatch,
) -> crate::Result<GeneralSettingsMessage> {
    if patch == &GeneralSettingsPatch::default() {
        return Err(crate::Error::InvalidParameter(
            "GeneralSettingsPatch contains no change".into(),
        ));
    }
    for (name, value) in [
        ("screen brightness", patch.screen_brightness),
        ("LED brightness", patch.led_brightness),
        ("dimmed LED brightness", patch.dimmed_led_brightness),
    ] {
        if value.is_some_and(|value| value > 100) {
            return Err(crate::Error::InvalidParameter(format!(
                "{name} must be 0-100"
            )));
        }
    }
    if patch
        .midi_channel
        .is_some_and(|channel| !(1..=16).contains(&channel))
    {
        return Err(crate::Error::InvalidParameter(
            "MIDI channel must be 1-16".into(),
        ));
    }
    let mut message = GeneralSettingsMessage {
        action: MessageAction::Update as i32,
        ..Default::default()
    };
    if let Some(value) = patch.screen_brightness {
        message.screen_brightness = Some(
            crate::proto::general_settings_message::ScreenBrightness::ScreenBrightness(i32::from(
                value,
            )),
        );
    }
    if let Some(value) = patch.led_brightness {
        message.led_brightness = Some(
            crate::proto::general_settings_message::LedBrightness::LedBrightness(i32::from(value)),
        );
    }
    if let Some(value) = patch.dimmed_led_brightness {
        message.dimmed_led_brightness = Some(
            crate::proto::general_settings_message::DimmedLedBrightness::DimmedLedBrightness(
                i32::from(value),
            ),
        );
    }
    if let Some(value) = patch.stomp_mode_auto_assign {
        message.stomp_mode_auto_assign = Some(
            crate::proto::general_settings_message::StompModeAutoAssign::StompModeAutoAssign(value),
        );
    }
    if let Some(value) = patch.swap_tempo_tuner_access {
        message.swap_tempo_tuner_access = Some(
            crate::proto::general_settings_message::SwapTempoTunerAccess::SwapTempoTunerAccess(
                value,
            ),
        );
    }
    if let Some(value) = patch.enable_dynamic_delay_compensation {
        message.enable_dynamic_delay_compensation = Some(
            crate::proto::general_settings_message::EnableDynamicDelayCompensation::EnableDynamicDelayCompensation(value),
        );
    }
    if let Some(value) = patch.gig_view_stomp_access_enabled {
        message.gig_view_stomp_access_enabled = Some(
            crate::proto::general_settings_message::GigViewStompAccessEnabled::GigViewStompAccessEnabled(value),
        );
    }
    if let Some(value) = patch.midi_channel {
        message.midi_channel = Some(
            crate::proto::general_settings_message::MidiChannel::MidiChannel(i32::from(value)),
        );
    }
    if let Some(value) = patch.midi_over_usb {
        message.midi_over_usb =
            Some(crate::proto::general_settings_message::MidiOverUsb::MidiOverUsb(value));
    }
    if let Some(value) = patch.midi_clock_in_enabled {
        message.midi_clock_in_enabled = Some(
            crate::proto::general_settings_message::MidiClockInEnabled::MidiClockInEnabled(value),
        );
    }
    if let Some(value) = patch.ignore_duplicate_pc {
        message.ignore_duplicate_pc = Some(
            crate::proto::general_settings_message::IgnoreDuplicatePc::IgnoreDuplicatePc(value),
        );
    }
    if let Some(value) = patch.disable_internet_connection_check {
        message.disable_internet_connection_check = Some(
            crate::proto::general_settings_message::DisableInternetConnectionCheck::DisableInternetConnectionCheck(value),
        );
    }
    if let Some(value) = patch.enable_preset_dimmed {
        message.enable_preset_dimmed = Some(
            crate::proto::general_settings_message::EnablePresetDimmed::EnablePresetDimmed(value),
        );
    }
    if let Some(value) = patch.enable_scene_dimmed {
        message.enable_scene_dimmed = Some(
            crate::proto::general_settings_message::EnableSceneDimmed::EnableSceneDimmed(value),
        );
    }
    if let Some(value) = patch.enable_stomp_dimmed {
        message.enable_stomp_dimmed = Some(
            crate::proto::general_settings_message::EnableStompDimmed::EnableStompDimmed(value),
        );
    }
    Ok(message)
}

/// Build the checked HOLD timing update. The wire stores an index, not milliseconds.
///
/// # Errors
///
/// Refuses values other than 500, 600, 700, 800, 900, or 1000 ms.
pub fn build_hold_timing(milliseconds: u32) -> crate::Result<GeneralSettingsMessage> {
    if !(500..=1000).contains(&milliseconds) || milliseconds % 100 != 0 {
        return Err(crate::Error::InvalidParameter(format!(
            "hold timing must be 500-1000 ms in exact 100 ms steps, got {milliseconds}"
        )));
    }
    Ok(GeneralSettingsMessage {
        action: MessageAction::Update as i32,
        hold_timing: Some(
            crate::proto::general_settings_message::HoldTiming::HoldTiming(
                i32::try_from((milliseconds - 500) / 100).expect("validated HOLD index fits i32"),
            ),
        ),
        ..Default::default()
    })
}

fn master_volume_assignment(message: &GeneralSettingsMessage) -> Option<MasterVolumeAssignment> {
    let Some(
        crate::proto::general_settings_message::MasterVolumeAssignment::MasterVolumeAssignment(
            value,
        ),
    ) = message.master_volume_assignment.as_ref()
    else {
        return None;
    };
    Some(MasterVolumeAssignment {
        out12: value.out12,
        out34: value.out34,
        send12: value.send12,
        headphones: value.headphones,
    })
}

fn global_bypass_rows(rows: GlobalBypassRows) -> GlobalBypassRowsState {
    [rows.row1, rows.row2, rows.row3, rows.row4]
}

fn global_bypass_state(message: &GeneralSettingsMessage) -> Option<GlobalBypassState> {
    let Some(crate::proto::general_settings_message::GlobalBypassCab::GlobalBypassCab(cab)) =
        message.global_bypass_cab.as_ref()
    else {
        return None;
    };
    let Some(crate::proto::general_settings_message::GlobalBypassIr::GlobalBypassIr(ir)) =
        message.global_bypass_ir.as_ref()
    else {
        return None;
    };
    Some(GlobalBypassState {
        cab: global_bypass_rows(*cab),
        ir: global_bypass_rows(*ir),
    })
}

fn bypass_rows(rows: GlobalBypassRowsState) -> GlobalBypassRows {
    GlobalBypassRows {
        row1: rows[0],
        row2: rows[1],
        row3: rows[2],
        row4: rows[3],
    }
}

/// Build a complete nested Master Volume assignment update.
#[must_use]
pub fn build_master_volume_assignment(
    assignment: MasterVolumeAssignment,
) -> GeneralSettingsMessage {
    GeneralSettingsMessage {
        action: MessageAction::Update as i32,
        master_volume_assignment: Some(
            crate::proto::general_settings_message::MasterVolumeAssignment::MasterVolumeAssignment(
                MasterVolumeAssignmentOptions {
                    out12: assignment.out12,
                    out34: assignment.out34,
                    send12: assignment.send12,
                    headphones: assignment.headphones,
                },
            ),
        ),
        ..Default::default()
    }
}

/// Build a complete nested Cab and IR Loader global-bypass update.
#[must_use]
pub fn build_global_bypass(state: GlobalBypassState) -> GeneralSettingsMessage {
    GeneralSettingsMessage {
        action: MessageAction::Update as i32,
        global_bypass_cab: Some(
            crate::proto::general_settings_message::GlobalBypassCab::GlobalBypassCab(bypass_rows(
                state.cab,
            )),
        ),
        global_bypass_ir: Some(
            crate::proto::general_settings_message::GlobalBypassIr::GlobalBypassIr(bypass_rows(
                state.ir,
            )),
        ),
        ..Default::default()
    }
}

/// Build sparse Global EQ band writes, one indexed parameter per message.
///
/// # Errors
///
/// Refuses bands outside 1-5, empty patches, and non-normalized numeric values.
pub fn build_global_eq_band(
    band: u8,
    patch: GlobalEqBandPatch,
) -> crate::Result<Vec<GlobalEqMessage>> {
    if !(1..=5).contains(&band) {
        return Err(crate::Error::InvalidParameter(format!(
            "Global EQ band must be 1-5, got {band}"
        )));
    }
    let mut controls = Vec::new();
    for (offset, name, value) in [
        (0, "Global EQ gain", patch.gain),
        (1, "Global EQ frequency", patch.frequency),
        (2, "Global EQ Q", patch.q),
    ] {
        if let Some(value) = value {
            validate_normalized(name, value)?;
            controls.push((offset, value));
        }
    }
    if let Some(filter) = patch.filter_type {
        controls.push((3, f32::from(filter as u8) / 4.0));
    }
    if let Some(enabled) = patch.enabled {
        controls.push((4, f32::from(enabled)));
    }
    if controls.is_empty() {
        return Err(crate::Error::InvalidParameter(
            "GlobalEqBandPatch contains no change".into(),
        ));
    }
    let base = i32::from(band - 1) * 5;
    Ok(controls
        .into_iter()
        .map(|(offset, value)| GlobalEqMessage {
            action: MessageAction::Update as i32,
            parameters: vec![GlobalEqParameter {
                parameter_index: base + offset,
                value,
            }],
            ..Default::default()
        })
        .collect())
}

/// Build sparse Global EQ OUT-tab writes. Level stays normalized because its dB
/// mapping has not been established.
///
/// # Errors
///
/// Refuses an empty patch or a non-normalized level.
pub fn build_global_eq_output(patch: GlobalEqOutputPatch) -> crate::Result<Vec<GlobalEqMessage>> {
    let mut controls = Vec::new();
    if let Some(level) = patch.level {
        validate_normalized("Global EQ output level", level)?;
        controls.push((25, level));
    }
    if let Some(out12) = patch.out12 {
        controls.push((26, f32::from(out12)));
    }
    if let Some(out34) = patch.out34 {
        controls.push((27, f32::from(out34)));
    }
    if controls.is_empty() {
        return Err(crate::Error::InvalidParameter(
            "GlobalEqOutputPatch contains no change".into(),
        ));
    }
    Ok(controls
        .into_iter()
        .map(|(parameter_index, value)| GlobalEqMessage {
            action: MessageAction::Update as i32,
            parameters: vec![GlobalEqParameter {
                parameter_index,
                value,
            }],
            ..Default::default()
        })
        .collect())
}

/// Build a checked complete footswitch-mode cycle replacement.
///
/// # Errors
///
/// Refuses an empty cycle, more than one hybrid, and a hybrid as the sole slot.
pub fn build_mode_cycle(slots: &[FootswitchModeSlot]) -> crate::Result<ModeMessage> {
    if slots.is_empty() {
        return Err(crate::Error::InvalidParameter(
            "mode cycle must contain at least one base mode".into(),
        ));
    }
    let hybrids = slots.iter().filter(|slot| slot.is_hybrid()).count();
    if hybrids > 1 {
        return Err(crate::Error::InvalidParameter(
            "mode cycle may contain at most one hybrid slot".into(),
        ));
    }
    if hybrids == 1 && slots.len() == 1 {
        return Err(crate::Error::InvalidParameter(
            "a hybrid cannot be the sole mode-cycle slot".into(),
        ));
    }
    Ok(ModeMessage {
        action: MessageAction::Update as i32,
        available_modes: Some(crate::proto::mode_message::AvailableModes::AvailableModes(
            AvailableModes {
                modes: slots.iter().map(|slot| *slot as u32).collect(),
            },
        )),
        ..Default::default()
    })
}

/// The `QuadCortex` client: an ergonomic API over the session layer.
/// Holds an `Arc<Session>` and builds protobuf messages for each operation.
///
/// Construct with [`QuadCortex::connect`] for the full handshake, or
/// [`QuadCortex::new`] if you already have a `Session`.
pub struct QuadCortex {
    session: Arc<Session>,
}

impl QuadCortex {
    /// Construct a client around an existing `Session`. The caller owns the
    /// session lifecycle.
    #[must_use]
    pub fn new(session: Arc<Session>) -> Self {
        Self { session }
    }

    /// Clone this client's non-consuming device-state cache handle.
    #[must_use]
    pub fn state_cache(&self) -> crate::DeviceStateCache {
        self.session.state_cache()
    }

    /// Open a transport, start the session, run the connect handshake, and
    /// return a ready-to-use `QuadCortex`. This is the Rust equivalent of
    /// `pyquadcortex.connect()`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::DeviceNotFound`] if no matching device is on
    /// the bus, or [`crate::Error::ReadTimeout`] if the handshake reply does
    /// not arrive within `timeout`.
    #[cfg(feature = "hid")]
    pub fn connect(kind: DeviceKind, timeout: Duration, settle: Duration) -> crate::Result<Self> {
        let session = Arc::new(Session::open(kind)?);
        session.connect(timeout, settle)?;
        Ok(Self::new(session))
    }

    /// Read the device firmware version. Works WITHOUT the full connect
    /// handshake - a plain `Version` READ gets a reply.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no reply arrives within
    /// `timeout`.
    pub fn version(&self, timeout: Duration) -> crate::Result<VersionMessage> {
        let rid = self.session.next_request_id();
        let request = VersionMessage {
            action: MessageAction::Read as i32,
            request_id: Some(crate::proto::version_message::RequestId::RequestId(rid)),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);
        let reply = self
            .session
            .request(MessageType::Version, &payload, rid, timeout)?;
        decode_version(&reply)
    }

    /// Recall a preset within a setlist. `position` is either the linear slot
    /// index or a slot name like `"28C"`.
    ///
    /// This sends `SetlistPosition{UPDATE}` and waits for the correlated
    /// `RecallPreset` push. Returning before that push is unsafe: the device
    /// has accepted the write but may still expose the previous grid.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidSlot`] if `position` is not a valid slot
    /// name, or [`crate::Error::ReadTimeout`] if the matching recall push does
    /// not arrive within `timeout`.
    pub fn recall_preset(
        &self,
        setlist_path: &str,
        position: &str,
        is_factory: bool,
        timeout: Duration,
    ) -> crate::Result<()> {
        self.read_preset(setlist_path, position, is_factory, timeout)
            .map(drop)
    }

    /// Switch the active scene. Scenes are 0-based.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidScene`] when `scene` is outside 0-7.
    pub fn switch_scene(&self, scene: u32) -> crate::Result<()> {
        validate_scene(scene)?;
        let msg = SceneMessage {
            action: MessageAction::Update as i32,
            selected_scene: Some(crate::proto::scene_message::SelectedScene::SelectedScene(
                scene,
            )),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&msg);
        self.session.send(MessageType::Scene, &payload)
    }

    /// Copy one scene onto another, or exchange them when `swap` is true.
    /// Labels and colours travel with the scene's parameter and bypass state.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidScene`] when either index is outside 0-7.
    pub fn copy_scene(&self, from_scene: u32, to_scene: u32, swap: bool) -> crate::Result<()> {
        validate_scene(from_scene)?;
        validate_scene(to_scene)?;
        let msg = SceneCopyMessage {
            action: MessageAction::Update as i32,
            from_index: i32::try_from(from_scene).expect("validated scene fits i32"),
            to_index: i32::try_from(to_scene).expect("validated scene fits i32"),
            is_swap: swap,
            ..Default::default()
        };
        self.session
            .send(MessageType::SceneCopy, &prost::Message::encode_to_vec(&msg))
    }

    /// Set a scene label. `None` writes the unit's unlabelled value, one space.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidScene`] when `scene` is outside 0-7.
    pub fn set_scene_label(&self, scene: u32, label: Option<&str>) -> crate::Result<()> {
        validate_scene(scene)?;
        let msg = SceneLabelMessage {
            action: MessageAction::Update as i32,
            index: i32::try_from(scene).expect("validated scene fits i32"),
            label: label.unwrap_or(SCENE_UNLABELLED).to_string(),
            ..Default::default()
        };
        self.session.send(
            MessageType::SceneLabel,
            &prost::Message::encode_to_vec(&msg),
        )
    }

    /// Set a scene colour as an ARGB `u32`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidScene`] when `scene` is outside 0-7.
    pub fn set_scene_color(&self, scene: u32, color: u32) -> crate::Result<()> {
        validate_scene(scene)?;
        let msg = SceneColorMessage {
            action: MessageAction::Update as i32,
            index: i32::try_from(scene).expect("validated scene fits i32"),
            color,
            ..Default::default()
        };
        self.session.send(
            MessageType::SceneColor,
            &prost::Message::encode_to_vec(&msg),
        )
    }

    /// Read the LIVE grid: the current editing state, unsaved changes
    /// included.
    ///
    /// `RecallPreset{READ}` answers with the preset as it exists on the
    /// device right now. This read has **no side effects**: an unsaved edit
    /// survives it and the active scene is untouched.
    ///
    /// Contrast with [`QuadCortex::read_preset`], which reads a STORED slot
    /// and RECALLS it as a side effect (discarding unsaved edits and
    /// resetting the active scene). Use this method for inspection during
    /// editing - it is the only way to distinguish "my write never applied"
    /// from "it applied and was later reset".
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no reply arrives within
    /// `timeout`, or [`crate::Error::Decode`] if the reply body is not a
    /// valid `RecallPresetMessage`.
    pub fn read_current_preset(&self, timeout: Duration) -> crate::Result<BinaryPreset> {
        let rid = self.session.next_request_id();
        let request = RecallPresetMessage {
            action: MessageAction::Read as i32,
            request_id: Some(crate::proto::recall_preset_message::RequestId::RequestId(
                rid,
            )),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);
        let reply = self.session.await_broadcast(
            MessageType::RecallPreset,
            || self.session.send(MessageType::RecallPreset, &payload),
            timeout,
            move |m| m.request_id == Some(rid),
        )?;
        let decoded: RecallPresetMessage = prost::Message::decode(reply.body.as_ref())
            .map_err(|e| crate::Error::Decode(format!("RecallPresetMessage: {e}")))?;
        decoded
            .preset
            .map(|crate::proto::recall_preset_message::Preset::Preset(p)| p)
            .ok_or_else(|| crate::Error::Decode("RecallPreset reply carried no preset".into()))
    }

    /// Recall a stored preset slot and return its full `BinaryPreset`.
    ///
    /// **This has a side effect: it RECALLS the slot**, loading the preset
    /// onto the grid, discarding unsaved edits, and resetting the active
    /// scene to the preset's default. Use [`QuadCortex::read_current_preset`]
    /// for side-effect-free inspection.
    ///
    /// There is no host-initiated "read preset" request: a `Grid`/
    /// `RecallPreset` READ for a stored slot gets no reply. Instead the
    /// device BROADCASTS a `RecallPreset` push whenever a preset is recalled,
    /// by host or by the unit. So this recalls the slot and captures that
    /// push.
    ///
    /// Correlation matters here. The push a host recall triggers echoes that
    /// recall's `request_id`, while the unsolicited seed push (the connect
    /// handshake's grid state) carries none. Without matching on the id the
    /// waiter returns whatever `RecallPreset` arrives first - which lags by
    /// one recall when a prior push is still in flight. So this tags the
    /// recall with a fresh id and accepts only the push echoing it.
    ///
    /// The push is asynchronous, so callers provide the wait timeout.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no matching push arrives
    /// within `timeout`, or [`crate::Error::Decode`] on a malformed reply.
    pub fn read_preset(
        &self,
        setlist_path: &str,
        position: &str,
        is_factory: bool,
        timeout: Duration,
    ) -> crate::Result<BinaryPreset> {
        let rid = self.session.next_request_id();
        let recall = build_recall(setlist_path, position, is_factory, Some(rid))?;
        let reply = self.session.await_broadcast(
            MessageType::RecallPreset,
            || self.session.send(MessageType::SetlistPosition, &recall),
            timeout,
            move |m| m.request_id == Some(rid),
        )?;
        let decoded: RecallPresetMessage = prost::Message::decode(reply.body.as_ref())
            .map_err(|e| crate::Error::Decode(format!("RecallPresetMessage: {e}")))?;
        decoded
            .preset
            .map(|crate::proto::recall_preset_message::Preset::Preset(p)| p)
            .ok_or_else(|| crate::Error::Decode("RecallPreset push carried no preset".into()))
    }

    /// Which scene the unit is on right now. Scenes are 0-based.
    ///
    /// Several writes apply to "the active scene" (`set_bypass` on a
    /// scene-mode block, `set_param` scene values), and a recall changes it
    /// out from under you - this makes the assumption checkable rather than
    /// tracked by hand.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no reply arrives within
    /// `timeout`, or [`crate::Error::Decode`] on a malformed reply.
    pub fn active_scene(&self, timeout: Duration) -> crate::Result<u32> {
        let rid = self.session.next_request_id();
        let request = SceneMessage {
            action: MessageAction::Read as i32,
            request_id: Some(crate::proto::scene_message::RequestId::RequestId(rid)),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);
        let reply = self
            .session
            .request(MessageType::Scene, &payload, rid, timeout)?;
        let decoded: SceneMessage = prost::Message::decode(reply.body.as_ref())
            .map_err(|e| crate::Error::Decode(format!("SceneMessage: {e}")))?;
        decoded
            .selected_scene
            .map(|crate::proto::scene_message::SelectedScene::SelectedScene(s)| s)
            .ok_or_else(|| crate::Error::Decode("Scene reply carried no selected_scene".into()))
    }

    fn read_state<T>(
        &self,
        message_type: MessageType,
        timeout: Duration,
        predicate: impl Fn(&T) -> bool + Send + Sync + 'static,
    ) -> crate::Result<T>
    where
        T: prost::Message + Default,
    {
        let reply = self.session.await_broadcast(
            message_type,
            || self.session.send(message_type, &[0x08, 0x03]),
            timeout,
            move |message| {
                prost::Message::decode(message.body.as_ref())
                    .is_ok_and(|decoded: T| predicate(&decoded))
            },
        )?;
        prost::Message::decode(reply.body.as_ref())
            .map_err(|error| crate::Error::Decode(format!("{message_type:?}: {error}")))
    }

    fn list_library(
        &self,
        folder: &str,
        file_type: i32,
        timeout: Duration,
    ) -> crate::Result<Vec<LibraryEntry>> {
        let request_id = self.session.next_request_id();
        let request = FileMessage {
            action: MessageAction::Read as i32,
            request_id: Some(crate::proto::file_message::RequestId::RequestId(request_id)),
            r#type: Some(crate::proto::file_message::Type::Type(file_type)),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);
        let wanted = folder.trim_end_matches('/').to_string();
        let match_key = wanted.clone();
        let reply = self.session.await_broadcast(
            MessageType::File,
            || self.session.send(MessageType::File, &payload),
            timeout,
            move |message| {
                prost::Message::decode(message.body.as_ref()).is_ok_and(|file: FileMessage| {
                    matches_library_listing(&file, request_id, &match_key)
                })
            },
        )?;
        let decoded: FileMessage = prost::Message::decode(reply.body.as_ref())
            .map_err(|error| crate::Error::Decode(format!("FileMessage: {error}")))?;
        let Some(crate::proto::file_message::Folder::Folder(folder)) = decoded.folder else {
            return Err(crate::Error::Decode("File reply carried no folder".into()));
        };
        Ok(folder
            .files
            .iter()
            .filter_map(LibraryEntry::from_proto)
            .collect())
    }

    /// List the Neural Captures in the device library.
    ///
    /// A correlated `File{READ, type: 2}` reply is accepted only when it echoes
    /// the request id and names [`CAPTURES_LIBRARY`]. An empty correlated folder
    /// is a successful empty listing. The protocol exposes no item count or end
    /// marker, so this returns the contents of that one response without claiming
    /// that firmware could not have omitted later entries.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no correlated listing arrives, or a decode error for
    /// a malformed matched reply.
    pub fn captures(&self, timeout: Duration) -> crate::Result<Vec<LibraryEntry>> {
        self.list_library(CAPTURES_LIBRARY, 2, timeout)
    }

    /// List loadable impulse responses from `folder`, or [`IR_LIBRARY`] when
    /// `folder` is `None`.
    ///
    /// IR listings use `FileMessage.type = 1`. Both request id and normalized
    /// folder identity must match, so an unrelated or delayed listing cannot
    /// satisfy this read. An empty correlated folder is a valid empty result.
    /// There is no total count or completion marker; the returned vector is the
    /// complete content of one correlated response, not proof that no additional
    /// entries exist elsewhere or could arrive in another response.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no correlated listing arrives, or a decode error for
    /// a malformed matched reply.
    pub fn list_irs(
        &self,
        folder: Option<&str>,
        timeout: Duration,
    ) -> crate::Result<Vec<LibraryEntry>> {
        self.list_library(folder.unwrap_or(IR_LIBRARY), 1, timeout)
    }

    /// Read the recent-presets state.
    ///
    /// Recents and Favorites share a message type and replies do not identify
    /// which list they contain. A plain READ requests Recents. Empty pushes are
    /// ignored because they are also observed transiently before the real list;
    /// consequently this method cannot represent an empty Recents list without
    /// further hardware evidence that distinguishes it from a partial push.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no non-empty Recents push arrives, or a decode error
    /// for a malformed matched reply.
    pub fn recents(&self, timeout: Duration) -> crate::Result<RecentsFavoritesMessage> {
        self.read_state(
            MessageType::RecentsFavorites,
            timeout,
            |message: &RecentsFavoritesMessage| !message.items.is_empty(),
        )
    }

    /// Read Favorites, retrying a dropped first request and preserving an empty
    /// list as a successful answer.
    ///
    /// The reply omits `is_favorites`, so only the echoed request id identifies
    /// it. `timeout` is the total budget divided across at least one attempt.
    ///
    /// # Errors
    ///
    /// Returns a timeout after all attempts receive no correlated reply, or a
    /// decode error for a malformed matched reply.
    pub fn favorites(
        &self,
        timeout: Duration,
        attempts: usize,
    ) -> crate::Result<Vec<RecentsFavoritesItem>> {
        let attempts = attempts.max(1).min(u32::MAX as usize);
        let attempt_timeout = timeout / u32::try_from(attempts).unwrap_or(u32::MAX);
        let mut last_timeout = None;
        for _ in 0..attempts {
            let request_id = self.session.next_request_id();
            let request = RecentsFavoritesMessage {
                action: MessageAction::Read as i32,
                is_favorites: true,
                request_id: Some(
                    crate::proto::recents_favorites_message::RequestId::RequestId(request_id),
                ),
                ..Default::default()
            };
            let payload = prost::Message::encode_to_vec(&request);
            match self.session.await_broadcast(
                MessageType::RecentsFavorites,
                || self.session.send(MessageType::RecentsFavorites, &payload),
                attempt_timeout,
                move |message| message.request_id == Some(request_id),
            ) {
                Ok(reply) => {
                    let decoded: RecentsFavoritesMessage =
                        prost::Message::decode(reply.body.as_ref()).map_err(|error| {
                            crate::Error::Decode(format!("RecentsFavoritesMessage: {error}"))
                        })?;
                    return Ok(decoded.items);
                }
                Err(error @ crate::Error::ReadTimeout(_)) => last_timeout = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last_timeout.unwrap_or(crate::Error::ReadTimeout(timeout)))
    }

    /// Read model ids pinned to the top of their categories.
    ///
    /// The repeated field has no presence marker, so an empty `PinnedModels`
    /// response is accepted as a valid empty list.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no response arrives, or a decode error for a
    /// malformed response.
    pub fn pinned_models(&self, timeout: Duration) -> crate::Result<Vec<u32>> {
        let request_id = self.session.next_request_id();
        let request = PinnedModelsMessage {
            action: MessageAction::Read as i32,
            request_id: Some(crate::proto::pinned_models_message::RequestId::RequestId(
                request_id,
            )),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);
        let reply = self.session.await_broadcast(
            MessageType::PinnedModels,
            || self.session.send(MessageType::PinnedModels, &payload),
            timeout,
            move |message| message.request_id == Some(request_id),
        )?;
        let decoded: PinnedModelsMessage = prost::Message::decode(reply.body.as_ref())
            .map_err(|error| crate::Error::Decode(format!("PinnedModelsMessage: {error}")))?;
        Ok(decoded.models)
    }

    /// Pin one model to the top of its category.
    ///
    /// Pinning uses the protobuf default action (`CREATE`), which is omitted on
    /// the wire. `UPDATE` is silently ignored. The device appends rather than
    /// replacing or de-duplicating, so pinning an already pinned model creates
    /// another copy.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn pin_model(&self, model_id: u32) -> crate::Result<()> {
        let message = PinnedModelsMessage {
            models: vec![model_id],
            ..Default::default()
        };
        self.session.send(
            MessageType::PinnedModels,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Unpin one model, removing every occurrence of its id.
    ///
    /// Unlike pinning, unpinning requires `DELETE`. The request contains
    /// exactly one model id.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn unpin_model(&self, model_id: u32) -> crate::Result<()> {
        let message = PinnedModelsMessage {
            action: MessageAction::Delete as i32,
            models: vec![model_id],
            ..Default::default()
        };
        self.session.send(
            MessageType::PinnedModels,
            &prost::Message::encode_to_vec(&message),
        )
    }

    fn favorite(
        &self,
        item: &RecentsFavoritesItem,
        action: MessageAction,
        timeout: Duration,
    ) -> crate::Result<()> {
        if item.name.is_empty() || item.folder_key.is_empty() {
            return Err(crate::Error::InvalidParameter(
                "a favorite requires the device entry's name and folder_key".into(),
            ));
        }
        let message = RecentsFavoritesMessage {
            action: action as i32,
            is_favorites: true,
            items: vec![item.clone()],
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&message);
        let target = item.clone();
        self.session.await_broadcast(
            MessageType::RecentsFavorites,
            || self.session.send(MessageType::RecentsFavorites, &payload),
            timeout,
            move |inbound| {
                prost::Message::decode(inbound.body.as_ref()).is_ok_and(
                    |echo: RecentsFavoritesMessage| matches_favorite_echo(&echo, action, &target),
                )
            },
        )?;
        Ok(())
    }

    /// Add exactly one preset entry to Favorites and wait for its exact echo.
    ///
    /// Pass an item supplied by the device, such as one from [`Self::recents`].
    /// The name, folder identity, factory/plugin flags, and operation must all
    /// match the echo; the device silently ignores invented or mismatched
    /// metadata. Only presets can be favorited.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidParameter`] for an item without a name or
    /// folder key, or a timeout if the device does not echo this exact add.
    pub fn add_favorite(
        &self,
        item: &RecentsFavoritesItem,
        timeout: Duration,
    ) -> crate::Result<()> {
        self.favorite(item, MessageAction::Create, timeout)
    }

    /// Remove exactly one preset entry from Favorites and wait for its exact
    /// `DELETE` echo.
    ///
    /// The item must be the device's own complete metadata, preferably read
    /// from [`Self::favorites`] or [`Self::recents`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidParameter`] for an item without a name or
    /// folder key, or a timeout if the device does not echo this exact removal.
    pub fn remove_favorite(
        &self,
        item: &RecentsFavoritesItem,
        timeout: Duration,
    ) -> crate::Result<()> {
        self.favorite(item, MessageAction::Delete, timeout)
    }

    /// Read Master Volume state. The normalized `volume` field must be present.
    /// This is intentionally read-only: upstream hardware testing found writes
    /// accepted but ignored.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no volume-bearing push arrives, or a decode error
    /// for a malformed push.
    pub fn master_volume(&self, timeout: Duration) -> crate::Result<MasterVolumeMessage> {
        self.read_state(
            MessageType::MasterVolume,
            timeout,
            |message: &MasterVolumeMessage| message.volume.is_some(),
        )
    }

    /// Read Looper X state, ignoring partial pushes without `status`.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no status-bearing push arrives, or a decode error
    /// for a malformed push.
    pub fn looper(&self, timeout: Duration) -> crate::Result<LooperMessage> {
        self.read_state(MessageType::Looper, timeout, |message: &LooperMessage| {
            message.status.is_some()
        })
    }

    /// Read tuner settings, ignoring partial pushes without `input_port_id`.
    ///
    /// `frequency` is the reference-frequency offset from 440 Hz, not detected
    /// pitch. The tuner needle was not observed streaming to the upstream host.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no input-bearing push arrives, or a decode error for
    /// a malformed push.
    pub fn tuner(&self, timeout: Duration) -> crate::Result<TunerMessage> {
        self.read_state(MessageType::Tuner, timeout, |message: &TunerMessage| {
            message.input_port_id.is_some()
        })
    }

    /// Read input, output, headphone, USB, MIDI, and expression-port settings.
    /// Partial pushes without at least one input-port entry are ignored.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no complete-enough settings push arrives, or a
    /// decode error for a malformed push.
    pub fn io_settings(&self, timeout: Duration) -> crate::Result<IoSettingsMessage> {
        self.read_state(
            MessageType::IoSettings,
            timeout,
            |message: &IoSettingsMessage| {
                message.settings.as_ref().is_some_and(|settings| {
                    let crate::proto::io_settings_message::Settings::Settings(settings) = settings;
                    !settings.in_port.is_empty()
                })
            },
        )
    }

    /// Read a restoration-grade I/O snapshot matching the measured per-port capabilities.
    ///
    /// Read-only `plugged`, headphone, and expression-pedal fields are deliberately
    /// not part of the completion rule. Inapplicable fields legitimately absent
    /// from Return, Out 3/4, and Send entries are capability-aware. This always
    /// issues an explicit READ and rejects sparse or structurally incomplete pushes.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no complete snapshot arrives, or a decode error.
    pub fn io_settings_complete(&self, timeout: Duration) -> crate::Result<IoSettingsMessage> {
        self.read_state(MessageType::IoSettings, timeout, io_settings_are_complete)
    }

    /// Poll fresh complete I/O reads until `matches` accepts one.
    ///
    /// I/O writes are dispatch-only and immediate reads may be stale. This method
    /// is the confirmation pattern: it never treats successful dispatch as device
    /// acceptance and returns only a complete snapshot satisfying the predicate.
    ///
    /// # Errors
    ///
    /// Returns the final read/decode error, or a timeout if state does not converge.
    pub fn poll_io_settings(
        &self,
        timeout: Duration,
        interval: Duration,
        matches: impl Fn(&IoSettingsMessage) -> bool,
    ) -> crate::Result<IoSettingsMessage> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(crate::Error::ReadTimeout(timeout));
            }
            let current = self.io_settings_complete(remaining)?;
            if matches(&current) {
                return Ok(current);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(crate::Error::ReadTimeout(timeout));
            }
            std::thread::sleep(interval.min(remaining));
        }
    }

    fn send_io_updates(&self, messages: Vec<IoSettingsMessage>) -> crate::Result<()> {
        for message in messages {
            self.session.send(
                MessageType::IoSettings,
                &prost::Message::encode_to_vec(&message),
            )?;
        }
        Ok(())
    }

    /// Dispatch sparse input-port changes, one keyed field per message.
    ///
    /// Return 1 is id 4, not 3; [`InputPort`] prevents that jack-number trap.
    /// A successful return means only that all writes were dispatched. Use
    /// [`QuadCortex::poll_io_settings`] for fresh read-back confirmation.
    ///
    /// # Errors
    ///
    /// Refuses empty or invalid intent before I/O, or returns a send error.
    pub fn set_input_port(&self, port: InputPort, patch: InputPortPatch) -> crate::Result<()> {
        self.send_io_updates(build_input_port_updates(port, patch)?)
    }

    /// Dispatch sparse output-port changes, one keyed field per message.
    ///
    /// A successful return is not confirmation; poll a fresh complete read.
    ///
    /// # Errors
    ///
    /// Refuses empty or invalid intent before I/O, or returns a send error.
    pub fn set_output_port(&self, port: OutputPort, patch: OutputPortPatch) -> crate::Result<()> {
        self.send_io_updates(build_output_port_updates(port, patch)?)
    }

    /// Dispatch sparse USB-audio changes, one field per message.
    ///
    /// A successful return is not confirmation; poll a fresh complete read.
    ///
    /// # Errors
    ///
    /// Refuses empty or invalid intent before I/O, or returns a send error.
    pub fn set_usb_port(&self, patch: UsbPortPatch) -> crate::Result<()> {
        self.send_io_updates(build_usb_port_updates(patch)?)
    }

    /// Dispatch a MIDI Thru toggle. The wire represents the Boolean as 0.0/1.0.
    ///
    /// A successful return is not confirmation; poll a fresh complete read.
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn set_midi_thru(&self, enabled: bool) -> crate::Result<()> {
        self.send_io_updates(vec![build_midi_thru_update(enabled)])
    }

    /// Dispatch output-pairing changes, one pairing flag per message.
    ///
    /// Pairing may change which member-port values are active. Poll the pairing,
    /// then restore and verify both member ports rather than assuming their state.
    /// A successful return is dispatch only, not confirmation.
    ///
    /// # Errors
    ///
    /// Refuses empty intent before I/O, or returns a send error.
    pub fn set_output_pairing(&self, patch: OutputPairingPatch) -> crate::Result<()> {
        self.send_io_updates(build_output_pairing_updates(patch)?)
    }

    /// Set normalized input gain as a one-field update.
    ///
    /// # Errors
    ///
    /// Refuses an invalid value before I/O, or returns a send error.
    pub fn set_input_level(&self, port: InputPort, level: f32) -> crate::Result<()> {
        self.set_input_port(
            port,
            InputPortPatch {
                level: Some(level),
                ..Default::default()
            },
        )
    }

    /// Set normalized output level as a one-field update.
    ///
    /// # Errors
    ///
    /// Refuses an invalid value before I/O, or returns a send error.
    pub fn set_output_level(&self, port: OutputPort, level: f32) -> crate::Result<()> {
        self.set_output_port(
            port,
            OutputPortPatch {
                level: Some(level),
                ..Default::default()
            },
        )
    }

    /// Mute or unmute one output using the required one-field message.
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn set_output_mute(&self, port: OutputPort, muted: bool) -> crate::Result<()> {
        self.set_output_port(
            port,
            OutputPortPatch {
                mute: Some(muted),
                ..Default::default()
            },
        )
    }

    /// Read global device settings. The wide generated protobuf is returned so
    /// firmware-defined fields are not hidden. Partial pushes without
    /// `scene_block_bypass` are ignored.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no scene-bypass-bearing push arrives, or a decode
    /// error for a malformed push.
    pub fn settings(&self, timeout: Duration) -> crate::Result<GeneralSettingsMessage> {
        self.read_state(
            MessageType::GeneralSettings,
            timeout,
            |message: &GeneralSettingsMessage| message.scene_block_bypass.is_some(),
        )
    }

    /// Read Global EQ state, ignoring partial pushes without `bypassed`.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no bypass-bearing push arrives, or a decode error
    /// for a malformed push.
    pub fn global_eq(&self, timeout: Duration) -> crate::Result<GlobalEqMessage> {
        self.read_state(
            MessageType::GlobalEq,
            timeout,
            |message: &GlobalEqMessage| message.bypassed.is_some(),
        )
    }

    /// Read the active footswitch-mode slot.
    ///
    /// Mode pushes are partial. This requires `mode` presence and does not claim
    /// that the same push contains the configured cycle.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no mode-bearing push arrives, or a decode error for
    /// a malformed push.
    pub fn mode(&self, timeout: Duration) -> crate::Result<ModeMessage> {
        self.read_state(MessageType::Mode, timeout, |message: &ModeMessage| {
            message.mode.is_some()
        })
    }

    /// Read configured footswitch-mode slots in cycle order.
    ///
    /// This deliberately requires a present, non-empty `available_modes` list;
    /// accepting a mode-only partial push would misreport the cycle as empty.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no cycle-bearing push arrives, or a decode error for
    /// a malformed push.
    pub fn mode_cycle(&self, timeout: Duration) -> crate::Result<Vec<u32>> {
        let state = self.read_state(MessageType::Mode, timeout, |message: &ModeMessage| {
            message.available_modes.as_ref().is_some_and(|available| {
                let crate::proto::mode_message::AvailableModes::AvailableModes(available) =
                    available;
                !available.modes.is_empty()
            })
        })?;
        let Some(crate::proto::mode_message::AvailableModes::AvailableModes(available)) =
            state.available_modes
        else {
            return Err(crate::Error::Decode(
                "Mode reply carried no available_modes".into(),
            ));
        };
        Ok(available.modes)
    }

    /// Read a complete global-settings snapshot suitable for restoration.
    /// Partial subscribed pushes are ignored; this always triggers an explicit
    /// `GeneralSettings{READ}`.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no complete push arrives, or a decode error.
    pub fn settings_complete(&self, timeout: Duration) -> crate::Result<GeneralSettingsMessage> {
        self.read_state(
            MessageType::GeneralSettings,
            timeout,
            |message: &GeneralSettingsMessage| {
                message.screen_brightness.is_some()
                    && message.led_brightness.is_some()
                    && message.dimmed_led_brightness.is_some()
                    && message.scene_block_bypass.is_some()
                    && message.hold_timing.is_some()
                    && message.stomp_mode_auto_assign.is_some()
                    && message.swap_tempo_tuner_access.is_some()
                    && message.enable_dynamic_delay_compensation.is_some()
                    && message.gig_view_stomp_access_enabled.is_some()
                    && message.midi_channel.is_some()
                    && message.midi_over_usb.is_some()
                    && message.midi_clock_in_enabled.is_some()
                    && message.ignore_duplicate_pc.is_some()
                    && message.disable_internet_connection_check.is_some()
                    && message.enable_preset_dimmed.is_some()
                    && message.enable_scene_dimmed.is_some()
                    && message.enable_stomp_dimmed.is_some()
                    && master_volume_assignment(message).is_some()
                    && global_bypass_state(message).is_some()
            },
        )
    }

    /// Read complete tuner settings (`input_port_id`, reference offset, and mute).
    ///
    /// # Errors
    ///
    /// Returns a timeout if no complete push arrives, or a decode error.
    pub fn tuner_complete(&self, timeout: Duration) -> crate::Result<TunerMessage> {
        self.read_state(MessageType::Tuner, timeout, |message: &TunerMessage| {
            message.input_port_id.is_some() && message.frequency.is_some() && message.mute.is_some()
        })
    }

    /// Read the complete 28-index Global EQ state plus its whole-EQ bypass flag.
    ///
    /// # Errors
    ///
    /// Returns a timeout if no index-complete push arrives, or a decode error.
    pub fn global_eq_complete(&self, timeout: Duration) -> crate::Result<GlobalEqMessage> {
        self.read_state(
            MessageType::GlobalEq,
            timeout,
            |message: &GlobalEqMessage| {
                if message.bypassed.is_none() || message.parameters.len() < 28 {
                    return false;
                }
                let mut seen = [false; 28];
                for parameter in &message.parameters {
                    let Ok(index) = usize::try_from(parameter.parameter_index) else {
                        return false;
                    };
                    if index >= seen.len() || seen[index] {
                        return false;
                    }
                    seen[index] = true;
                }
                seen.into_iter().all(std::convert::identity)
            },
        )
    }

    /// Read and merge explicit active-slot and cycle reads. Mode pushes are
    /// partial, so completeness must not depend on both fields sharing a push.
    ///
    /// # Errors
    ///
    /// Returns a timeout or decode error from either explicit read.
    pub fn mode_complete(&self, timeout: Duration) -> crate::Result<ModeMessage> {
        let mut current = self.mode(timeout)?;
        let cycle = self.mode_cycle(timeout)?;
        current.available_modes = Some(crate::proto::mode_message::AvailableModes::AvailableModes(
            AvailableModes { modes: cycle },
        ));
        Ok(current)
    }

    /// Send a safe typed sparse global-settings update.
    ///
    /// # Errors
    ///
    /// Refuses an empty or invalid patch before I/O, or returns a send error.
    pub fn update_settings(&self, patch: &GeneralSettingsPatch) -> crate::Result<()> {
        let message = build_settings_update(patch)?;
        self.session.send(
            MessageType::GeneralSettings,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Set footswitch HOLD timing in milliseconds.
    ///
    /// # Errors
    ///
    /// Refuses invalid timing before I/O, or returns a send error.
    pub fn set_hold_timing(&self, milliseconds: u32) -> crate::Result<()> {
        let message = build_hold_timing(milliseconds)?;
        self.session.send(
            MessageType::GeneralSettings,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Set how block bypass edits are retained across scenes.
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn set_scene_bypass_behavior(&self, behavior: SceneBypassBehavior) -> crate::Result<()> {
        let message = GeneralSettingsMessage {
            action: MessageAction::Update as i32,
            scene_block_bypass: Some(
                crate::proto::general_settings_message::SceneBlockBypass::SceneBlockBypass(
                    behavior as i32,
                ),
            ),
            ..Default::default()
        };
        self.session.send(
            MessageType::GeneralSettings,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Read, merge, and write a complete Master Volume output assignment.
    ///
    /// # Errors
    ///
    /// Refuses empty intent, or returns read, decode, or send errors.
    pub fn set_master_volume_assignment(
        &self,
        patch: MasterVolumeAssignmentPatch,
        timeout: Duration,
    ) -> crate::Result<()> {
        if patch == MasterVolumeAssignmentPatch::default() {
            return Err(crate::Error::InvalidParameter(
                "MasterVolumeAssignmentPatch contains no change".into(),
            ));
        }
        let current = self.settings_complete(timeout)?;
        let mut target = master_volume_assignment(&current).ok_or_else(|| {
            crate::Error::Decode("complete settings omitted master_volume_assignment".into())
        })?;
        target.out12 = patch.out12.unwrap_or(target.out12);
        target.out34 = patch.out34.unwrap_or(target.out34);
        target.send12 = patch.send12.unwrap_or(target.send12);
        target.headphones = patch.headphones.unwrap_or(target.headphones);
        self.restore_master_volume_assignment(target)
    }

    /// Restore a complete Master Volume assignment without depending on stale state.
    ///
    /// # Errors
    ///
    /// Returns a session error if the complete update cannot be sent.
    pub fn restore_master_volume_assignment(
        &self,
        assignment: MasterVolumeAssignment,
    ) -> crate::Result<()> {
        let message = build_master_volume_assignment(assignment);
        self.session.send(
            MessageType::GeneralSettings,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Read, merge, and write complete Cab and IR Loader global-bypass groups.
    ///
    /// # Errors
    ///
    /// Refuses empty intent, or returns read, decode, or send errors.
    pub fn set_global_bypass(
        &self,
        patch: GlobalBypassPatch,
        timeout: Duration,
    ) -> crate::Result<()> {
        if patch == GlobalBypassPatch::default() {
            return Err(crate::Error::InvalidParameter(
                "GlobalBypassPatch contains no change".into(),
            ));
        }
        let current = self.settings_complete(timeout)?;
        let mut target = global_bypass_state(&current).ok_or_else(|| {
            crate::Error::Decode("complete settings omitted global bypass groups".into())
        })?;
        target.cab = patch.cab.unwrap_or(target.cab);
        target.ir = patch.ir.unwrap_or(target.ir);
        self.restore_global_bypass(target)
    }

    /// Restore both complete global-bypass row groups without reading current state.
    ///
    /// # Errors
    ///
    /// Returns a session error if the complete update cannot be sent.
    pub fn restore_global_bypass(&self, state: GlobalBypassState) -> crate::Result<()> {
        let message = build_global_bypass(state);
        self.session.send(
            MessageType::GeneralSettings,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Bypass or enable the whole Global EQ (`true` means EQ off).
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn set_global_eq_bypassed(&self, bypassed: bool) -> crate::Result<()> {
        let message = GlobalEqMessage {
            action: MessageAction::Update as i32,
            bypassed: Some(crate::proto::global_eq_message::Bypassed::Bypassed(
                bypassed,
            )),
            ..Default::default()
        };
        self.session.send(
            MessageType::GlobalEq,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Set selected controls of one Global EQ band, one sparse index per write.
    ///
    /// # Errors
    ///
    /// Refuses invalid or empty intent before I/O, or returns a send error.
    pub fn set_global_eq(&self, band: u8, patch: GlobalEqBandPatch) -> crate::Result<()> {
        for message in build_global_eq_band(band, patch)? {
            self.session.send(
                MessageType::GlobalEq,
                &prost::Message::encode_to_vec(&message),
            )?;
        }
        Ok(())
    }

    /// Set selected Global EQ OUT-tab controls. Level is normalized, not dB.
    ///
    /// # Errors
    ///
    /// Refuses invalid or empty intent before I/O, or returns a send error.
    pub fn set_global_eq_output(&self, patch: GlobalEqOutputPatch) -> crate::Result<()> {
        for message in build_global_eq_output(patch)? {
            self.session.send(
                MessageType::GlobalEq,
                &prost::Message::encode_to_vec(&message),
            )?;
        }
        Ok(())
    }

    /// Replace the footswitch mode cycle after enforcing known device constraints.
    ///
    /// # Errors
    ///
    /// Refuses an invalid cycle before I/O, or returns a send error.
    pub fn set_mode_cycle(&self, slots: &[FootswitchModeSlot]) -> crate::Result<()> {
        let message = build_mode_cycle(slots)?;
        self.session
            .send(MessageType::Mode, &prost::Message::encode_to_vec(&message))
    }

    /// Switch to one valid base or hybrid footswitch-mode slot.
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn set_mode(&self, slot: FootswitchModeSlot) -> crate::Result<()> {
        let message = ModeMessage {
            action: MessageAction::Update as i32,
            mode: Some(crate::proto::mode_message::Mode::Mode(slot as u32)),
            ..Default::default()
        };
        self.session
            .send(MessageType::Mode, &prost::Message::encode_to_vec(&message))
    }

    /// Open or close Gig View on the unit.
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn set_gig_view(&self, shown: bool) -> crate::Result<()> {
        let message = ShowGigViewMessage {
            action: MessageAction::Update as i32,
            show: shown,
            ..Default::default()
        };
        self.session.send(
            MessageType::ShowGigView,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Open or close the tuner on the unit.
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn show_tuner(&self, shown: bool) -> crate::Result<()> {
        let message = ShowTunerMessage {
            action: MessageAction::Update as i32,
            show: shown,
            ..Default::default()
        };
        self.session.send(
            MessageType::ShowTuner,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Select one input known to be accepted by the tuner.
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn set_tuner_input(&self, input: TunerInput) -> crate::Result<()> {
        let message = TunerMessage {
            action: MessageAction::Update as i32,
            input_port_id: Some(crate::proto::tuner_message::InputPortId::InputPortId(
                input as i32,
            )),
            ..Default::default()
        };
        self.session
            .send(MessageType::Tuner, &prost::Message::encode_to_vec(&message))
    }

    /// Set whether opening the tuner mutes the outputs.
    ///
    /// # Errors
    ///
    /// Returns a session error if the update cannot be sent.
    pub fn set_tuner_mute(&self, muted: bool) -> crate::Result<()> {
        let message = TunerMessage {
            action: MessageAction::Update as i32,
            mute: Some(crate::proto::tuner_message::Mute::Mute(muted)),
            ..Default::default()
        };
        self.session
            .send(MessageType::Tuner, &prost::Message::encode_to_vec(&message))
    }

    /// Set tuner reference as a Hz offset from 440, within the unit's 425-455 Hz range.
    ///
    /// # Errors
    ///
    /// Refuses a non-finite or out-of-range offset before I/O, or returns a send error.
    pub fn set_tuner_reference(&self, offset_hz: f32) -> crate::Result<()> {
        if !offset_hz.is_finite() || !(-15.0..=15.0).contains(&offset_hz) {
            return Err(crate::Error::InvalidParameter(format!(
                "tuner reference offset must be finite and within -15..=15 Hz, got {offset_hz}"
            )));
        }
        let message = TunerMessage {
            action: MessageAction::Update as i32,
            frequency: Some(crate::proto::tuner_message::Frequency::Frequency(offset_hz)),
            ..Default::default()
        };
        self.session
            .send(MessageType::Tuner, &prost::Message::encode_to_vec(&message))
    }

    /// List the presets in a setlist, in slot order.
    ///
    /// Unlike [`QuadCortex::read_preset`], this does NOT change what is
    /// loaded on the grid. There is no host-initiated "list" request: a
    /// `File` READ makes the device push a folder listing per setlist, so
    /// this sends that READ and waits for the listing whose key matches
    /// `setlist`.
    ///
    /// The device always reports a setlist as its full complement of 256
    /// slots, most typically empty. By default only occupied slots are
    /// returned; pass `include_empty = true` for the complete slot map (e.g.
    /// to find a free slot to save into).
    ///
    /// `setlist` must be a 256-slot factory or user setlist key. Variable-length
    /// plugin and capture folders have no observed completion marker, so this
    /// method deliberately does not accept a partial update as their listing.
    ///
    /// Note the trailing-slash asymmetry this absorbs: recalls need the
    /// factory path WITH its trailing slash, but the device reports that same
    /// folder's listing key WITHOUT one. Keys are compared with trailing
    /// slashes normalised away.
    ///
    /// A listing that arrives is complete. A targeted listing still transfers
    /// all 256 slots and takes about five seconds on the observed unit, so use
    /// a realistic timeout; a timeout is not an answer about the setlist's
    /// contents.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no matching listing arrives
    /// within `timeout`, or [`crate::Error::Decode`] on a malformed reply.
    pub fn list_presets(
        &self,
        setlist: &str,
        timeout: Duration,
        include_empty: bool,
    ) -> crate::Result<Vec<PresetEntry>> {
        let wanted = setlist.trim_end_matches('/').to_string();
        // Name the folder we want.
        //
        // A bare `File` READ makes the device enumerate EVERYTHING - 399
        // folders and over 600 KB on the unit measured - and we then discard
        // all but one. Naming the folder narrows what it sends: measured at
        // 14.1 s bare versus 5.3 s targeted, returning the same listing.
        //
        // `list_folders` deliberately still sends the bare form, because
        // enumerating everything is exactly what it wants.
        let request = FileMessage {
            action: MessageAction::Read as i32,
            folder: Some(crate::proto::file_message::Folder::Folder(
                crate::proto::FolderInfo {
                    key: Some(crate::proto::folder_info::Key::Key(setlist.to_string())),
                    ..Default::default()
                },
            )),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);

        let match_key = wanted.clone();
        let reply = self.session.await_broadcast(
            MessageType::File,
            || self.session.send(MessageType::File, &payload),
            timeout,
            move |m| {
                prost::Message::decode(m.body.as_ref())
                    .is_ok_and(|message: FileMessage| matches_setlist_listing(&message, &match_key))
            },
        )?;

        let decoded: FileMessage = prost::Message::decode(reply.body.as_ref())
            .map_err(|e| crate::Error::Decode(format!("FileMessage: {e}")))?;
        let Some(crate::proto::file_message::Folder::Folder(folder)) = decoded.folder else {
            return Err(crate::Error::Decode("File reply carried no folder".into()));
        };

        // `from_proto` returns None for an empty slot. The device always
        // reports a setlist's full complement of 256 slots, most of which are
        // typically empty, so filtering here IS the `include_empty == false`
        // path; there is nothing to return for an empty slot but its index.
        preset_entries(&folder, include_empty)
    }

    /// Return a setlist already announced to the subscribed state cache.
    ///
    /// Performs no device I/O. `None` means no complete listing for this key
    /// is currently trustworthy; callers may fall back to [`Self::list_presets`].
    #[must_use]
    pub fn cached_presets(&self, setlist: &str, include_empty: bool) -> Option<Vec<PresetEntry>> {
        self.session
            .state_cache()
            .folder(setlist)
            .and_then(|folder| preset_entries(&folder.value, include_empty).ok())
    }

    /// Look a preset up by the name shown on the unit.
    ///
    /// Matching is exact but case-insensitive. Returns the listing entry
    /// whose `index` is the position [`QuadCortex::read_preset`] and
    /// [`QuadCortex::recall_preset`] take.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::NotFound`] if no preset of that name exists in
    /// the setlist, or propagates [`QuadCortex::list_presets`] errors.
    pub fn find_preset(
        &self,
        name: &str,
        setlist: &str,
        timeout: Duration,
    ) -> crate::Result<PresetEntry> {
        let wanted = name.trim().to_lowercase();
        let entries = self.list_presets(setlist, timeout, false)?;
        entries
            .into_iter()
            .find(|e| e.name.trim().to_lowercase() == wanted)
            .ok_or_else(|| crate::Error::NotFound(format!("no preset named {name:?} in {setlist}")))
    }

    /// Enumerate every folder the device knows about.
    ///
    /// A single `File` READ makes the device enumerate all its folders (399
    /// on the observed unit), arriving over ten to twenty seconds - so this
    /// uses a collector rather than a single-shot waiter, and always blocks
    /// for the full `window`.
    ///
    /// # Errors
    ///
    /// Propagates session errors. An empty result means nothing arrived in
    /// the window, which usually means `window` was too short.
    pub fn list_folders(&self, window: Duration) -> crate::Result<Vec<Folder>> {
        let request = FileMessage {
            action: MessageAction::Read as i32,
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);

        let messages = self.session.collect(
            MessageType::File,
            || self.session.send(MessageType::File, &payload),
            window,
            |message| {
                prost::Message::decode(message.body.as_ref())
                    .is_ok_and(|file: FileMessage| file.action == MessageAction::Update as i32)
            },
        )?;

        let mut folders: Vec<Folder> = Vec::new();
        for m in &messages {
            let Ok(decoded) = prost::Message::decode(m.body.as_ref()) as Result<FileMessage, _>
            else {
                continue;
            };
            if let Some(crate::proto::file_message::Folder::Folder(f)) = decoded.folder {
                let Some(folder) = Folder::from_proto(&f) else {
                    continue;
                };
                // The device re-announces folders; retain the fullest valid
                // listing rather than a shorter partial sighting.
                if let Some(existing) = folders.iter_mut().find(|item| item.key == folder.key) {
                    if folder.slots > existing.slots {
                        *existing = folder;
                    }
                } else {
                    folders.push(folder);
                }
            }
        }
        folders.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(folders)
    }

    /// Fetch the device's raw `ModelRepo` payload.
    ///
    /// This is the catalog: what turns an integer model id stored in a preset
    /// into a name, a category, and a parameter list. It comes FROM the
    /// device, so it covers installed block types, including purchased plugin
    /// models. Neural Capture entries describe capture block types; individual
    /// captures are inventoried separately through `File` listings.
    ///
    /// The payload is large (~47 KB, spanning several hundred HID reports),
    /// so allow a generous timeout. The transport already gunzips a
    /// frame-level gzip wrapper; whatever remains is returned raw here for
    /// the catalog parser to interpret.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no payload-bearing
    /// `ModelRepo` message arrives within `timeout`.
    pub fn fetch_model_repo(&self, timeout: Duration) -> crate::Result<Vec<u8>> {
        use crate::proto::{ModelRepoMessage, model_repo_message as mrm};

        // The paced handshake already asked for and waited for this payload,
        // so serve that copy rather than requesting and transferring the same
        // 46 KB again.
        if let Some(captured) = self.session.captured_model_repo() {
            return Ok(captured);
        }

        let request = ModelRepoMessage {
            action: MessageAction::Read as i32,
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);

        let reply = self.session.await_broadcast(
            MessageType::ModelRepo,
            || self.session.send(MessageType::ModelRepo, &payload),
            timeout,
            // The device emits ModelRepo messages without a payload too;
            // only a payload-bearing one is the catalog.
            |m| {
                prost::Message::decode(m.body.as_ref())
                    .ok()
                    .and_then(|r: ModelRepoMessage| r.model_repo_payload)
                    .is_some_and(|mrm::ModelRepoPayload::ModelRepoPayload(p)| !p.is_empty())
            },
        )?;

        let decoded: ModelRepoMessage = prost::Message::decode(reply.body.as_ref())
            .map_err(|e| crate::Error::Decode(format!("ModelRepoMessage: {e}")))?;
        let Some(mrm::ModelRepoPayload::ModelRepoPayload(bytes)) = decoded.model_repo_payload
        else {
            return Err(crate::Error::Decode(
                "ModelRepo reply carried no payload".into(),
            ));
        };
        Ok(bytes)
    }

    /// Save the working grid into a slot.
    ///
    /// **Destructive.** This overwrites whatever is in the slot, and the
    /// device offers no undo.
    ///
    /// Does NOT upload a preset: the message names a destination and the unit
    /// commits whatever is in the working grid. What gets saved is what
    /// `grid show` reports.
    ///
    /// `name` separates the three save-shaped operations Cortex Control
    /// offers, which are one message on the wire:
    ///
    /// - `None` - save in place, keeping the slot's existing name.
    /// - `Some(name)` - save-as into an empty slot, or rename an occupied
    ///   one. The device does not distinguish those two.
    ///
    /// **The device may not store the name you asked for.** On a collision
    /// within the setlist it de-duplicates: the base is truncated and a
    /// `_N` suffix appended, to 20 characters total. A unique name is stored
    /// verbatim and is not length-limited. If the stored name matters, read
    /// the slot back and use what the device reports.
    ///
    /// The acknowledgement proves that a save landed. Metadata read-back is
    /// polled until an expected name or instrument change appears, because
    /// complete listings are eventually consistent. An overwrite that keeps
    /// both name and instrument unchanged is wire-indistinguishable at the
    /// metadata level; in that case the acknowledgement plus the first fresh
    /// complete listing is the strongest available confirmation.
    ///
    /// Measured from a capture of Cortex Control on `CorOS` 4.0.1; see
    /// `docs/protocol.md`. The rename-on-collision behaviour is corroborated
    /// by `pyquadcortex` (MIT), which documents the same.
    ///
    /// # Errors
    ///
    /// [`crate::Error::InvalidSlot`] for a malformed slot,
    /// [`crate::Error::NotFound`] if `setlist` is the factory library, and
    /// [`crate::Error::ReadTimeout`] if the device does not acknowledge.
    pub fn save_current_preset(
        &self,
        setlist: &str,
        slot: &str,
        name: Option<&str>,
        instrument: Instrument,
        timeout: Duration,
    ) -> crate::Result<PresetEntry> {
        // Refused here rather than in each caller: the factory library is
        // read-only on the unit, every surface would otherwise have to
        // remember, and the cost of forgetting is someone's factory content.
        if is_factory_setlist(setlist) {
            return Err(crate::Error::NotFound(format!(
                "{setlist} is the factory library and is not writable"
            )));
        }

        let index = slot_to_position_checked(slot)
            .ok_or_else(|| crate::Error::InvalidSlot(slot.to_string()))?;
        let baseline = self
            .list_presets(setlist, timeout, false)?
            .into_iter()
            .find(|entry| entry.index == index);
        self.save_current_preset_with_baseline(
            setlist,
            slot,
            index,
            name,
            instrument,
            baseline,
            Instant::now() + timeout,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn save_current_preset_with_baseline(
        &self,
        setlist: &str,
        slot: &str,
        index: u32,
        name: Option<&str>,
        instrument: Instrument,
        baseline: Option<PresetEntry>,
        deadline: Instant,
    ) -> crate::Result<PresetEntry> {
        let entry = crate::proto::ProductData {
            index: Some(crate::proto::product_data::Index::Index(
                i32::try_from(index).unwrap_or(i32::MAX),
            )),
            name: name.map(|n| crate::proto::product_data::Name::Name(n.to_string())),
            instrument: Some(crate::proto::product_data::Instrument::Instrument(
                instrument as i32,
            )),
            ..Default::default()
        };
        let folder = crate::proto::FolderInfo {
            key: Some(crate::proto::folder_info::Key::Key(setlist.to_string())),
            is_factory: Some(crate::proto::folder_info::IsFactory::IsFactory(false)),
            files: vec![entry],
            ..Default::default()
        };
        let request = FileMessage {
            // CREATE, not UPDATE, even when overwriting. Being 0, it does not
            // appear on the wire at all.
            action: MessageAction::Create as i32,
            r#type: Some(crate::proto::file_message::Type::Type(0)),
            folder: Some(crate::proto::file_message::Folder::Folder(folder)),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);
        let wanted = setlist.trim_end_matches('/').to_string();

        // Wait for the acknowledging File reply rather than firing and
        // hoping. This is the one operation where "did it land" matters.
        self.session.await_broadcast(
            MessageType::File,
            || self.session.send(MessageType::File, &payload),
            deadline.saturating_duration_since(Instant::now()),
            move |message| {
                prost::Message::decode(message.body.as_ref())
                    .is_ok_and(|file: FileMessage| matches_save_ack(&file, &wanted, index))
            },
        )?;

        let metadata_change_expected = baseline.as_ref().is_none_or(|entry| {
            entry.instrument != Some(instrument)
                || name.is_some_and(|requested| requested != entry.name)
        });
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let listing_timeout = remaining.min(Duration::from_secs(10));
            match self.list_presets(setlist, listing_timeout, false) {
                Ok(entries) => {
                    let stored = entries.into_iter().find(|entry| {
                        entry.index == index
                            && entry.instrument == Some(instrument)
                            && name.is_none_or(|requested| {
                                stored_name_matches_request(requested, &entry.name)
                            })
                    });
                    if let Some(stored) = stored
                        && (!metadata_change_expected || baseline.as_ref() != Some(&stored))
                    {
                        return Ok(stored);
                    }
                }
                Err(crate::Error::ReadTimeout(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Err(crate::Error::SetlistUnconfirmed(format!(
            "save was acknowledged for {slot} in {setlist}, but fresh complete listings did not converge on the stored {instrument:?} metadata; do not retry blindly"
        )))
    }

    /// Copy a stored preset by preparing the destination, recalling the
    /// source, and saving that recalled grid.
    ///
    /// There is no host-drivable device copy operation. Preparing happens
    /// before the source recall, so an occupied destination is backed up before
    /// the working grid changes. The returned entry comes from a fresh complete
    /// destination listing and therefore reports the device's actual collision
    /// name and instrument tag.
    ///
    /// # Errors
    ///
    /// Propagates destination preparation, source recall, save, and fresh
    /// listing errors. A write whose final listing does not converge returns
    /// [`crate::Error::SetlistUnconfirmed`].
    /// `recall_consent` explicitly controls whether preparing the destination
    /// may discard the currently loaded working copy.
    #[allow(clippy::too_many_arguments)]
    pub fn copy_preset(
        &self,
        policy: &crate::safety::SavePolicy,
        from_setlist: &str,
        from_slot: &str,
        to_setlist: &str,
        to_slot: &str,
        name: Option<&str>,
        instrument: Instrument,
        recall_consent: crate::RecallConsent,
        timeout: Duration,
    ) -> crate::Result<CopyPresetReceipt> {
        if self.state_cache().status().phase == crate::CachePhase::Incomplete {
            self.read_current_preset(timeout)?;
        }
        let preparation = self.prepare_save_before_editing(
            policy,
            to_setlist,
            to_slot,
            crate::ScratchOverride::ScratchOnly,
            recall_consent,
            timeout,
        )?;
        let source = self.read_preset(from_setlist, from_slot, false, timeout)?;
        let source_name = source.name.as_ref().map(|value| {
            let crate::proto::binary_preset::Name::Name(value) = value;
            value.as_str()
        });
        let requested_name = name.or(source_name).unwrap_or("copy");
        let receipt = self.save_prepared(
            policy,
            preparation,
            crate::SaveConfirmation::explicit(true)?,
            Some(requested_name),
            instrument,
            timeout,
        )?;
        let stored = receipt.result_view().stored;
        Ok(CopyPresetReceipt {
            source_setlist: from_setlist.to_string(),
            source_slot: from_slot.to_ascii_uppercase(),
            stored,
        })
    }

    /// Create a direct child setlist under [`USER_SETLIST_ROOT`].
    ///
    /// A fresh preflight refuses an existing requested name. After sending,
    /// fresh directory listings must show exactly one new direct USER setlist;
    /// that delta proves which destination this operation created and captures
    /// any device-side rename.
    ///
    /// # Errors
    ///
    /// Returns a safety error for an invalid or existing name, propagates
    /// directory I/O errors, and returns [`crate::Error::SetlistUnconfirmed`]
    /// unless fresh listings prove the exact newly-created destination.
    pub fn create_setlist(&self, name: &str, timeout: Duration) -> crate::Result<Folder> {
        let request = build_create_setlist(name)?;
        let scan = (timeout / 3).clamp(Duration::from_millis(10), Duration::from_secs(20));
        let before = direct_user_setlists(self.list_folders(scan)?);
        let requested_key = user_setlist_key(name)?;
        if !before.iter().any(|folder| folder.key == USER_SETLIST) {
            return Err(crate::Error::SetlistUnconfirmed(
                "fresh directory preflight did not include My Presets, so it cannot prove the requested name is unused"
                    .into(),
            ));
        }
        if before.iter().any(|folder| folder.key == requested_key) {
            return Err(crate::Error::UnsafeSave(format!(
                "setlist {name:?} already exists; refusing to claim or overwrite it"
            )));
        }
        let known: std::collections::HashSet<String> =
            before.into_iter().map(|folder| folder.key).collect();
        self.session
            .send(MessageType::File, &prost::Message::encode_to_vec(&request))?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let folders = direct_user_setlists(self.list_folders(scan.min(remaining))?);
            if !known
                .iter()
                .all(|key| folders.iter().any(|folder| &folder.key == key))
            {
                continue;
            }
            let mut added = folders
                .into_iter()
                .filter(|folder| !known.contains(&folder.key))
                .collect::<Vec<_>>();
            if added.len() == 1 && added[0].key == requested_key {
                return Ok(added.remove(0));
            }
            if added.len() == 1 {
                return Err(crate::Error::SetlistUnconfirmed(format!(
                    "creating {name:?} was followed by unexpected USER setlist {}; it cannot safely be claimed as this operation's destination",
                    added[0].key
                )));
            }
            if added.len() > 1 {
                return Err(crate::Error::SetlistUnconfirmed(format!(
                    "multiple USER setlists appeared after creating {name:?}; none can safely be claimed as this operation's destination"
                )));
            }
        }
        Err(crate::Error::SetlistUnconfirmed(format!(
            "create was sent for {name:?}, but no fresh directory listing proved a newly-created destination; do not retry blindly"
        )))
    }

    /// Delete one named USER setlist and poll fresh directory listings until
    /// it is absent. Factory paths, the root, and `My Presets` are rejected by
    /// message construction before device I/O.
    ///
    /// # Errors
    ///
    /// Returns a safety error for protected or invalid names,
    /// [`crate::Error::NotFound`] for an absent target, and
    /// [`crate::Error::SetlistUnconfirmed`] unless complete-enough fresh
    /// listings repeatedly prove absence.
    pub fn delete_setlist(&self, name: &str, timeout: Duration) -> crate::Result<()> {
        let request = build_delete_setlist(name)?;
        let key = user_setlist_key(name)?;
        let scan = (timeout / 4).clamp(Duration::from_millis(10), Duration::from_secs(20));
        let before = direct_user_setlists(self.list_folders(scan)?);
        if !before.iter().any(|folder| folder.key == USER_SETLIST) {
            return Err(crate::Error::SetlistUnconfirmed(
                "fresh directory preflight did not include My Presets, so it cannot authorize setlist deletion"
                    .into(),
            ));
        }
        if !before.iter().any(|folder| folder.key == key) {
            return Err(crate::Error::NotFound(format!(
                "no USER setlist named {name:?}"
            )));
        }
        let retained: std::collections::HashSet<String> = before
            .into_iter()
            .filter(|folder| folder.key != key)
            .map(|folder| folder.key)
            .collect();
        self.session
            .send(MessageType::File, &prost::Message::encode_to_vec(&request))?;

        let deadline = Instant::now() + timeout;
        let mut absent_observations = 0;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let folders = direct_user_setlists(self.list_folders(scan.min(remaining))?);
            if !retained
                .iter()
                .all(|known| folders.iter().any(|folder| &folder.key == known))
                || folders.iter().any(|folder| folder.key == key)
            {
                absent_observations = 0;
            } else {
                absent_observations += 1;
                if absent_observations == 2 {
                    return Ok(());
                }
            }
        }
        Err(crate::Error::SetlistUnconfirmed(format!(
            "delete was sent for {name:?}, but fresh directory listings did not prove its absence; do not retry blindly"
        )))
    }

    /// Duplicate a setlist by composing create with one recall/save per
    /// occupied source slot. No `BulkOperation` is sent: that message only
    /// reports device progress and does not drive duplication.
    ///
    /// A failure after creation returns an honest partial receipt containing
    /// the destination and every copy already verified. The caller can inspect
    /// or delete that destination rather than assuming rollback occurred.
    /// `recall_consent` explicitly controls whether the first copied preset may
    /// replace a dirty or unknown working grid; it is applied to every copy.
    ///
    /// # Errors
    ///
    /// Returns an error when source/destination names are unsafe or destination
    /// creation cannot be proven. Failures after creation are represented in
    /// the returned partial receipt instead.
    pub fn duplicate_setlist(
        &self,
        source_name: &str,
        destination_name: &str,
        limit: Option<usize>,
        recall_consent: crate::RecallConsent,
        timeout: Duration,
    ) -> crate::Result<DuplicateSetlistReceipt> {
        let source_key = user_setlist_key(source_name)?;
        let destination = self.create_setlist(destination_name, timeout)?;
        let mut entries = match self.list_presets(&source_key, timeout, false) {
            Ok(entries) => entries,
            Err(error) => {
                return Ok(DuplicateSetlistReceipt {
                    destination: destination.clone(),
                    selected: 0,
                    copied: Vec::new(),
                    failure: Some(format!(
                        "destination {} was created, but reading source {source_key} failed before any copies: {error}. The empty destination remains",
                        destination.key
                    )),
                });
            }
        };
        if let Some(limit) = limit {
            entries.truncate(limit);
        }
        let mut receipt = DuplicateSetlistReceipt {
            destination: destination.clone(),
            selected: entries.len(),
            copied: Vec::new(),
            failure: None,
        };
        for entry in entries {
            let slot = position_to_slot(entry.index);
            let policy = crate::SavePolicy::new(
                destination.key.clone(),
                vec![crate::ScratchRange::new(&slot, &slot)?],
            )?;
            match self.copy_preset(
                &policy,
                &source_key,
                &slot,
                &destination.key,
                &slot,
                Some(&entry.name),
                entry.instrument.unwrap_or(Instrument::None),
                recall_consent,
                timeout,
            ) {
                Ok(copy) => receipt.copied.push(copy),
                Err(error) => {
                    receipt.failure = Some(format!(
                        "copying source {slot} failed after {}/{} verified copies: {error}. The partial destination remains at {}",
                        receipt.copied.len(),
                        receipt.selected,
                        destination.key
                    ));
                    break;
                }
            }
        }
        Ok(receipt)
    }

    /// Delete a preset from a setlist, by name.
    ///
    /// **Destructive and not undoable on the device.**
    ///
    /// Addressed by device FILE PATH, not by slot index - the opposite of
    /// [`Self::save_current_preset`], which addresses its target by slot. The
    /// path is the setlist key, the preset name, and a `.pb` extension. That
    /// asymmetry is the device's, not ours, and getting it the wrong way
    /// round silently does nothing.
    ///
    /// Because it is name-addressed, deleting depends on knowing the stored
    /// name - which a save may have altered on collision. List the setlist
    /// and use the name the device reports.
    ///
    /// Measured from a capture of Cortex Control on `CorOS` 4.0.1, and
    /// corroborated by `pyquadcortex` (MIT), which documents the same shape.
    ///
    /// # Errors
    ///
    /// [`crate::Error::NotFound`] if `setlist` is the factory library, and
    /// [`crate::Error::ReadTimeout`] if the device does not acknowledge.
    pub fn delete_preset(&self, setlist: &str, name: &str, timeout: Duration) -> crate::Result<()> {
        if is_factory_setlist(setlist) {
            return Err(crate::Error::NotFound(format!(
                "{setlist} is the factory library and is not writable"
            )));
        }

        let target_key = format!("{setlist}/{name}.pb");
        let entry = crate::proto::ProductData {
            key: Some(crate::proto::product_data::Key::Key(target_key.clone())),
            ..Default::default()
        };
        let folder = crate::proto::FolderInfo {
            key: Some(crate::proto::folder_info::Key::Key(setlist.to_string())),
            is_factory: Some(crate::proto::folder_info::IsFactory::IsFactory(false)),
            files: vec![entry],
            ..Default::default()
        };
        let request = FileMessage {
            action: MessageAction::Delete as i32,
            r#type: Some(crate::proto::file_message::Type::Type(0)),
            folder: Some(crate::proto::file_message::Folder::Folder(folder)),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&request);
        let wanted = setlist.trim_end_matches('/').to_string();

        self.session.await_broadcast(
            MessageType::File,
            || self.session.send(MessageType::File, &payload),
            timeout,
            move |message| {
                prost::Message::decode(message.body.as_ref())
                    .is_ok_and(|file: FileMessage| matches_delete_ack(&file, &wanted, &target_key))
            },
        )?;
        Ok(())
    }

    /// Move a preset to an empty slot in the same setlist.
    ///
    /// **Destructive.** The raw protocol can target an occupied slot, but that
    /// behaviour has not been established. This method first requests a fresh,
    /// complete 256-slot listing and refuses occupied destinations, empty
    /// sources, no-op moves, the factory library, and any source or destination
    /// outside the caller's safety policy before sending anything.
    ///
    /// The source is addressed by its device file path and the destination by
    /// linear slot index. Only same-setlist moves have been observed. File
    /// listings are eventually consistent, so the empty-slot observation alone
    /// is not a safety boundary; both slots must be declared disposable. After
    /// sending, this polls fresh complete listings until the source is absent
    /// and the source preset occupies the destination. A File acknowledgement
    /// is not required because the device may mutate storage without one.
    ///
    /// The wire shape is derived from `pyquadcortex` (MIT), where it is backed
    /// by a Cortex Control capture. Hardware-verified through the held daemon
    /// on `CorOS` 4.0.1: a prepared scratch preset moved `7A -> 7B -> 7A`, both
    /// listing-convergence checks passed, and cleanup restored both slots empty.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidSlot`] for a malformed source or destination,
    /// [`crate::Error::NotFound`] when the source is absent,
    /// [`crate::Error::UnsafeMove`] for the factory library, an occupied
    /// destination, a no-op move, or slots outside the safety policy,
    /// [`crate::Error::ReadTimeout`] if the preflight listing does not arrive,
    /// and [`crate::Error::MoveUnconfirmed`] when the write was sent but fresh
    /// listings did not prove convergence before the deadline.
    pub fn move_preset(
        &self,
        policy: &crate::safety::SavePolicy,
        setlist: &str,
        from_slot: &str,
        to_slot: &str,
        timeout: Duration,
    ) -> crate::Result<()> {
        let setlist = setlist.trim_end_matches('/');
        if is_factory_setlist(setlist) {
            return Err(crate::Error::UnsafeMove(format!(
                "{setlist} is the factory library and is not writable"
            )));
        }
        let source = slot_to_position_checked(from_slot)
            .ok_or_else(|| crate::Error::InvalidSlot(from_slot.to_string()))?;
        let destination = slot_to_position_checked(to_slot)
            .ok_or_else(|| crate::Error::InvalidSlot(to_slot.to_string()))?;
        let entries = self.list_presets(setlist, timeout, false)?;
        let (source_key, name) = move_source(policy, setlist, &entries, source, destination)?;
        let request = build_move_preset(setlist, &source_key, destination);
        let payload = prost::Message::encode_to_vec(&request);
        self.session.send(MessageType::File, &payload)?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let listing_timeout = remaining.min(Duration::from_secs(10));
            match self.list_presets(setlist, listing_timeout, false) {
                Ok(entries) if move_converged(&entries, &name, source, destination) => {
                    return Ok(());
                }
                Ok(_) | Err(crate::Error::ReadTimeout(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Err(crate::Error::MoveUnconfirmed(format!(
            "{name:?} was sent from {} to {}; do not retry blindly because the move may have landed - inspect a fresh preset listing",
            position_to_slot(source),
            position_to_slot(destination)
        )))
    }

    /// Tell the device this client is going away. Sends
    /// `Connection{connected: false}` (best effort).
    pub fn disconnect(&self) {
        self.session.disconnect();
    }

    /// Send `Connection{connected: false}` and stop the session.
    pub fn close(&mut self) {
        self.session.close();
    }
}

fn decode_version(reply: &InboundMessage) -> crate::Result<VersionMessage> {
    prost::Message::decode(reply.body.as_ref())
        .map_err(|e| crate::Error::Decode(format!("VersionMessage: {e}")))
}

// ---------------------------------------------------------------------------
// Module-level helper functions (testable without hardware)
// ---------------------------------------------------------------------------

/// Convert a slot name (e.g. `"28C"`) to a linear index (`(28-1)*8 + 2 == 218`).
///
/// A slot name is a bank number (1-32) followed by a letter A-H.
///
/// # Panics
///
/// Panics if the slot name is malformed. In a library context, use
/// [`slot_to_position_checked`] instead.
#[must_use]
pub fn slot_to_position(slot: &str) -> u32 {
    slot_to_position_checked(slot).expect("valid slot name")
}

/// Convert a slot name to a linear index, returning `None` on malformed input.
#[must_use]
pub fn slot_to_position_checked(slot: &str) -> Option<u32> {
    let slot = slot.trim();
    if slot.len() < 2 {
        return None;
    }
    let letter = slot.chars().last()?;
    let bank_str = &slot[..slot.len() - 1];
    let bank: u32 = bank_str.parse().ok()?;
    let letter_idx = match letter.to_ascii_lowercase() {
        'a' => 0,
        'b' => 1,
        'c' => 2,
        'd' => 3,
        'e' => 4,
        'f' => 5,
        'g' => 6,
        'h' => 7,
        _ => return None,
    };
    if bank == 0 || bank > BANKS {
        return None;
    }
    Some((bank - 1) * SLOTS_PER_BANK + letter_idx)
}

/// Convert a linear index back to a slot name (e.g. 218 -> `"28C"`).
#[must_use]
pub fn position_to_slot(index: u32) -> String {
    let bank = index / SLOTS_PER_BANK + 1;
    let letter = match index % SLOTS_PER_BANK {
        0 => 'A',
        1 => 'B',
        2 => 'C',
        3 => 'D',
        4 => 'E',
        5 => 'F',
        6 => 'G',
        _ => 'H',
    };
    format!("{bank}{letter}")
}

/// Convert an input port's wire `level` (0..1) to the dB the unit displays.
///
/// Input ports span -12 to +60 dB, so `dB = -12 + 72 * level`. Solved from
/// four owner-set trims read simultaneously on screen and on the wire.
#[must_use]
pub fn input_level_db(level: f64) -> f64 {
    -12.0 + 72.0 * level
}

/// Convert displayed input-gain dB to the wire `level` an input port takes.
///
/// # Errors
///
/// Returns [`crate::Error::Framing`] if `db` is outside -12..+60 dB (values
/// that do not exist on the unit).
pub fn db_to_input_level(db: f64) -> crate::Result<f64> {
    if !(-12.0..=60.0).contains(&db) {
        return Err(crate::Error::Framing(format!(
            "input gain runs -12..+60 dB; {db} dB does not exist"
        )));
    }
    Ok((db + 12.0) / 72.0)
}

// ---------------------------------------------------------------------------
// Grid editing
//
// These wrap the pure builders in `crate::grid` with sending and, where the
// device gives us something to check, verification. Nothing here re-derives a
// message shape; if a trap is encoded in a builder it stays encoded.
//
// Every edit below changes the WORKING COPY on the grid. Nothing is persisted
// until [`QuadCortex::save_current_preset`] commits the working grid.
// ---------------------------------------------------------------------------

impl QuadCortex {
    /// Re-point one grid row's input.
    ///
    /// # Errors
    ///
    /// The benign USB status-stage stall is swallowed. Returns
    /// [`crate::Error::GridWriteUnconfirmed`] when a complete live-grid read
    /// does not report the requested route, and propagates read/session errors.
    pub fn set_chain_input(&self, row: Row, port: crate::GridInputPort) -> crate::Result<()> {
        let expected = u32::from(port);
        self.send_grid_and_verify(
            &crate::grid::set_chain_input(row, port),
            |preset| {
                chain_at(preset, row).and_then(|chain| {
                    chain.in_portid.as_ref().map(|value| {
                        let crate::proto::chain::InPortid::InPortid(port) = value;
                        *port
                    })
                }) == Some(expected)
            },
            format!("wire row {} input to {port}", row.wire()),
        )
    }

    /// Re-point one grid row's output.
    ///
    /// The device stores meaningless wire ids rather than rejecting them, so
    /// this method accepts only the closed [`crate::GridOutputPort`] values.
    ///
    /// # Errors
    ///
    /// As [`QuadCortex::set_chain_input`].
    pub fn set_chain_output(&self, row: Row, port: crate::GridOutputPort) -> crate::Result<()> {
        let expected = u32::from(port);
        self.send_grid_and_verify(
            &crate::grid::set_chain_output(row, port),
            |preset| {
                chain_at(preset, row).and_then(|chain| {
                    chain.out_portid.as_ref().map(|value| {
                        let crate::proto::chain::OutPortid::OutPortid(port) = value;
                        *port
                    })
                }) == Some(expected)
            },
            format!("wire row {} output to {port}", row.wire()),
        )
    }

    /// Set one block parameter on the ACTIVE scene.
    ///
    /// To set a per-scene value use [`QuadCortex::set_param_in_scene`], which
    /// sequences the three messages the device requires.
    ///
    /// # Errors
    ///
    /// As [`QuadCortex::set_chain_input`].
    pub fn set_param(
        &self,
        row: Row,
        column: u32,
        param_index: u32,
        value: Value,
    ) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_param(row, column, param_index, value))
    }

    /// Select one dynamic list parameter by its device-returned option name.
    ///
    /// The option list comes from `source`, normally a fresh current-preset
    /// read, because some lists enumerate blocks and change with the preset.
    /// `target` may be the positional parameter index or its catalog name. The
    /// selected option is normalized centrally as `index / (count - 1)`.
    ///
    /// This changes only the active scene in the working copy and does not save.
    ///
    /// # Errors
    ///
    /// Returns an invalid-parameter or not-found error before the write when
    /// the cell, parameter, dynamic option list, or option name is absent.
    pub fn set_param_option(
        &self,
        row: Row,
        column: u32,
        target: ParameterTarget,
        option: &str,
        source: &BinaryPreset,
        timeout: Duration,
    ) -> crate::Result<ParameterWrite> {
        validate_grid_cell(row, column)?;
        let catalog = if matches!(target, ParameterTarget::Name(_)) {
            let payload = match self.session.captured_model_repo() {
                Some(payload) => payload,
                None => self.fetch_model_repo(timeout)?,
            };
            Some(crate::Catalog::parse(&payload)?)
        } else {
            None
        };
        let write = option_parameter_write(source, row, column, &target, option, catalog.as_ref())?;
        self.set_param(row, column, write.index, write.value.clone())?;
        Ok(write)
    }

    /// Select an existing device-returned Neural Capture in a block.
    ///
    /// Pass `Some(DEFAULT_CAPTURE_MODEL)` to place and verify the default block
    /// first, or `None` to require an existing capture block at the cell. The
    /// capture selector is always sent before `parameters`, in their caller
    /// order, because selecting a capture silently resets that block's other
    /// parameters to the capture defaults. Index 5 is reserved for the exact
    /// `<content hash><display name>` selector and cannot be supplied in
    /// `parameters`.
    ///
    /// This changes only the working copy and does not save the preset.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error before I/O for a malformed cell, capture
    /// entry, model, parameter value, or reserved index. Returns
    /// [`crate::Error::NotFound`] if `model` is `None` and the cell is not an
    /// existing capture block. A failed placement is returned immediately and
    /// no selector or follow-up parameter is sent.
    pub fn set_capture(
        &self,
        row: Row,
        column: u32,
        capture: &LibraryEntry,
        model: Option<u32>,
        parameters: &[ParameterWrite],
        timeout: Duration,
    ) -> crate::Result<Option<Placement>> {
        validate_grid_cell(row, column)?;
        capture.validate_capture()?;
        validate_parameter_writes(parameters)?;
        if parameters
            .iter()
            .any(|parameter| parameter.index == CAPTURE_FILE_NAME_PARAM)
        {
            return Err(crate::Error::InvalidParameter(format!(
                "capture follow-up parameters must not include reserved index {CAPTURE_FILE_NAME_PARAM}"
            )));
        }
        if let Some(model_id) = model {
            if model_id != DEFAULT_CAPTURE_MODEL {
                return Err(crate::Error::InvalidParameter(format!(
                    "new capture blocks use default model 14000, got {model_id}"
                )));
            }
        } else {
            let preset = self.read_current_preset(timeout)?;
            let existing = model_id_at(&preset, row, column);
            if !existing.is_some_and(is_capture_model) {
                return Err(crate::Error::NotFound(format!(
                    "screen row {} column {column} does not hold an existing capture block",
                    row.screen()
                )));
            }
        }

        let placement = model
            .map(|model_id| self.set_block(row, column, model_id, timeout))
            .transpose()?;
        self.set_param(
            row,
            column,
            CAPTURE_FILE_NAME_PARAM,
            Value::Text(format!("{}{}", capture.key, capture.name)),
        )?;
        for parameter in parameters {
            self.set_param(row, column, parameter.index, parameter.value.clone())?;
        }
        Ok(placement)
    }

    /// Select an existing device-returned impulse response in one IR Loader slot.
    ///
    /// `slot` is 0 or 1. The library key is written unchanged to parameter 2
    /// or 10, followed by the display name at parameter 22 or 23. The key is a
    /// library identity, not a filesystem path. Pass a loader model in
    /// 29001..=29008 to place and verify it first, or `None` to require a
    /// compatible loader already at the cell.
    ///
    /// A fresh grid read can prove only that the strings were stored. The
    /// device accepts nonsense references byte-for-byte; absence of the warning
    /// icon must still be checked on the unit before claiming the IR loaded.
    /// This method does not save the preset.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error before I/O for a malformed cell, slot,
    /// entry, or loader model. Returns [`crate::Error::NotFound`] if `model` is
    /// `None` and the cell is not an existing IR Loader. A failed placement
    /// stops the operation before either string parameter is sent.
    pub fn set_ir(
        &self,
        row: Row,
        column: u32,
        ir: &LibraryEntry,
        slot: u32,
        model: Option<u32>,
        timeout: Duration,
    ) -> crate::Result<Option<Placement>> {
        validate_grid_cell(row, column)?;
        ir.validate_ir()?;
        let slot = usize::try_from(slot)
            .map_err(|_| crate::Error::InvalidParameter("IR Loader slot must be 0 or 1".into()))?;
        if slot > 1 {
            return Err(crate::Error::InvalidParameter(format!(
                "IR Loader slot must be 0 or 1, got {slot}"
            )));
        }
        if let Some(model_id) = model {
            if !is_ir_loader_model(model_id) {
                return Err(crate::Error::InvalidParameter(format!(
                    "IR Loader model must be in 29001..=29008, got {model_id}"
                )));
            }
        } else {
            let preset = self.read_current_preset(timeout)?;
            let existing = model_id_at(&preset, row, column);
            if !existing.is_some_and(is_ir_loader_model) {
                return Err(crate::Error::NotFound(format!(
                    "screen row {} column {column} does not hold an existing IR Loader",
                    row.screen()
                )));
            }
        }

        let placement = model
            .map(|model_id| self.set_block(row, column, model_id, timeout))
            .transpose()?;
        self.set_param(
            row,
            column,
            IR_PATH_PARAMS[slot],
            Value::Text(ir.key.clone()),
        )?;
        self.set_param(
            row,
            column,
            IR_NAME_PARAMS[slot],
            Value::Text(ir.name.clone()),
        )?;
        Ok(placement)
    }

    /// Resolve and write a parameter using either its wire index or display name.
    ///
    /// This is the host-facing parameter API. Name lookup, read-only-meter
    /// refusal, real-unit conversion, and the scene-write sequence live here
    /// so the CLI, daemon, MCP server, and GUI cannot implement them
    /// differently.
    ///
    /// Addressing by name reads the live grid to identify the model in the
    /// cell, then resolves that model through the catalog captured by the
    /// handshake. A real-unit value therefore requires a named target; a raw
    /// index has no range metadata to convert against.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidParameter`] for malformed selectors,
    /// out-of-range normalised values, read-only meters, or real-unit writes
    /// without a named parameter. Returns [`crate::Error::NotFound`] when the
    /// cell is empty or the model/parameter is absent from the catalog.
    /// Returns [`crate::Error::GridWriteUnconfirmed`] when a complete live-grid
    /// read does not contain the requested value on the target scene.
    #[allow(clippy::too_many_arguments)]
    pub fn set_parameter(
        &self,
        row: Row,
        column: u32,
        target: ParameterTarget,
        input: ParameterInput,
        scene: Option<u32>,
        promote: bool,
        timeout: Duration,
    ) -> crate::Result<ParameterWrite> {
        if row.wire() > 3 {
            return Err(crate::Error::InvalidRow(format!(
                "wire rows are 0-3, got {}",
                row.wire()
            )));
        }
        if column > 7 {
            return Err(crate::Error::InvalidParameter(format!(
                "grid columns are 0-7, got {column}"
            )));
        }
        if let Some(scene) = scene {
            if scene > 7 {
                return Err(crate::Error::InvalidParameter(format!(
                    "scenes are 0-7, got {scene}"
                )));
            }
        }

        let ParameterWrite { index, value } =
            self.resolve_parameter(row, column, target, input, timeout)?;

        let verification_scene = match scene {
            Some(scene) => scene,
            None => self.active_scene(timeout.max(READ_BACK_TIMEOUT))?,
        };

        if let Some(scene) = scene {
            self.set_param_in_scene(row, column, index, value.clone(), scene, promote)?;
        } else {
            if promote {
                self.set_param_scene_mode(row, column, index, true)?;
            }
            self.set_param(row, column, index, value.clone())?;
        }

        let after = self.read_current_preset(timeout.max(READ_BACK_TIMEOUT))?;
        let Some(parameter) = parameter_at(&after, row, column, index) else {
            return Err(crate::Error::GridWriteUnconfirmed(format!(
                "parameter {index} is absent at wire row {} column {column}",
                row.wire()
            )));
        };
        let promoted = parameter.scene_mode.as_ref().is_some_and(|mode| {
            let crate::proto::param::SceneMode::SceneMode(enabled) = mode;
            *enabled
        });
        if (promote && !promoted) || !parameter_matches(parameter, &value, verification_scene)? {
            return Err(crate::Error::GridWriteUnconfirmed(format!(
                "parameter {index} at wire row {} column {column} did not read back as requested for scene {verification_scene}",
                row.wire()
            )));
        }

        Ok(ParameterWrite { index, value })
    }

    /// Resolve a host-facing selector and input to the wire write.
    fn resolve_parameter(
        &self,
        row: Row,
        column: u32,
        target: ParameterTarget,
        input: ParameterInput,
        timeout: Duration,
    ) -> crate::Result<ParameterWrite> {
        let (index, value) = match target {
            ParameterTarget::Index(index) => {
                let value = match input {
                    ParameterInput::Normalised(value) => normalised_value(value)?,
                    ParameterInput::Text(value) => Value::Text(value),
                    ParameterInput::Real(_) => {
                        return Err(crate::Error::InvalidParameter(
                            "a real-unit value requires a parameter name so its range is known"
                                .into(),
                        ));
                    }
                };
                (index, value)
            }
            ParameterTarget::Name(name) => {
                if name.trim().is_empty() {
                    return Err(crate::Error::InvalidParameter(
                        "parameter name cannot be empty".into(),
                    ));
                }
                let preset = self.read_current_preset(timeout)?;
                let model_id = model_id_at(&preset, row, column).ok_or_else(|| {
                    crate::Error::NotFound(format!(
                        "screen row {} column {column} is empty",
                        row.screen()
                    ))
                })?;
                let payload = match self.session.captured_model_repo() {
                    Some(payload) => payload,
                    None => self.fetch_model_repo(timeout)?,
                };
                let catalog = crate::Catalog::parse(&payload)?;
                let model = catalog.get(model_id).ok_or_else(|| {
                    crate::Error::NotFound(format!(
                        "model {model_id} is not in this unit's catalog"
                    ))
                })?;
                let parameter = model.parameter(&name).ok_or_else(|| {
                    let names: Vec<&str> =
                        model.parameters.iter().map(|p| p.name.as_str()).collect();
                    crate::Error::NotFound(format!(
                        "{} has no parameter {name:?}. It has: {}",
                        model.name,
                        names.join(", ")
                    ))
                })?;
                let value = parameter_value(parameter, input)?;
                let index = u32::try_from(parameter.index).map_err(|_| {
                    crate::Error::InvalidParameter(format!(
                        "parameter index {} does not fit on the wire",
                        parameter.index
                    ))
                })?;
                (index, value)
            }
        };
        Ok(ParameterWrite { index, value })
    }

    /// Make a block parameter follow scenes, or stop it following them.
    ///
    /// The flag travels alone; see [`crate::grid::set_param_scene_mode`].
    ///
    /// # Errors
    ///
    /// Propagates session send failures. This low-level primitive does not
    /// verify the resulting flag; host surfaces use [`QuadCortex::set_parameter`]
    /// for a complete scene-aware write and read-back.
    pub fn set_param_scene_mode(
        &self,
        row: Row,
        column: u32,
        param_index: u32,
        enabled: bool,
    ) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_param_scene_mode(
            row,
            column,
            param_index,
            enabled,
        ))
    }

    /// Set a block parameter on a NAMED scene.
    ///
    /// This is three messages, and it has to be. The device only keeps a
    /// per-scene value for a parameter whose `scene_mode` is set, it applies
    /// a written value to whichever scene is ACTIVE rather than to an index,
    /// and it accepts either the flag or a value in one message but never
    /// both. So: promote, switch, write. Ordering over the pipe is enough; no
    /// settle delay is needed.
    ///
    /// **Side effect:** this leaves the unit sitting on `scene`. That is
    /// visible on the hardware and changes what subsequent scene-relative
    /// writes target.
    ///
    /// Pass `promote = false` only if the parameter is known already to be
    /// scene-following; promoting an already-promoted parameter is harmless.
    ///
    /// # Errors
    ///
    /// As [`QuadCortex::set_chain_input`].
    pub fn set_param_in_scene(
        &self,
        row: Row,
        column: u32,
        param_index: u32,
        value: Value,
        scene: u32,
        promote: bool,
    ) -> crate::Result<()> {
        validate_scene(scene)?;
        let promotion =
            promote.then(|| crate::grid::set_param_scene_mode(row, column, param_index, true));
        let write = crate::grid::set_param(row, column, param_index, value);
        self.send_control_sequence(promotion.as_ref(), Some(scene), &write)
    }

    /// Set one preset-local tempo or metronome parameter.
    ///
    /// The supported [`TempoParameter`] values preserve the unit's positional
    /// indices, including the gap at index 1 and the screen names that disagree
    /// with the catalog. A normalised input is validated directly. A real-unit
    /// input is converted through model 25000 in the device catalog; parameters
    /// absent from that catalog cannot be guessed. Text is never valid here.
    ///
    /// The resulting `Grid{UPDATE}` has no row key: it carries one hash-25000
    /// model in `tempo_program_data`. This operation is derived from licensed
    /// upstream evidence and remains provisional pending this crate's hardware
    /// smoke. It does not claim support for Tempo MODE or internal MIDI clock
    /// writes, for which positive wire evidence is absent.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidParameter`] before I/O for a non-finite or
    /// out-of-range normalised value, text, or an unconvertible real-unit value.
    /// Catalog acquisition may also return its documented read/decode errors.
    pub fn set_tempo_param(
        &self,
        parameter: TempoParameter,
        input: ParameterInput,
        timeout: Duration,
    ) -> crate::Result<()> {
        let value = match input {
            ParameterInput::Normalised(value) => normalised_value(value)?,
            ParameterInput::Real(value) => {
                if !value.is_finite() {
                    return Err(crate::Error::InvalidParameter(format!(
                        "real-unit values must be finite, got {value}"
                    )));
                }
                let payload = match self.session.captured_model_repo() {
                    Some(payload) => payload,
                    None => self.fetch_model_repo(timeout)?,
                };
                let catalog = crate::Catalog::parse(&payload)?;
                let model = catalog.get(crate::grid::TEMPO_CONTROL).ok_or_else(|| {
                    crate::Error::NotFound("tempo model 25000 is absent from the catalog".into())
                })?;
                let specification = model
                    .parameters
                    .get(parameter.index() as usize)
                    .ok_or_else(|| {
                        crate::Error::InvalidParameter(format!(
                            "the catalog does not describe tempo parameter index {}",
                            parameter.index()
                        ))
                    })?;
                parameter_value(specification, ParameterInput::Real(value))?
            }
            ParameterInput::Text(_) => {
                return Err(crate::Error::InvalidParameter(
                    "tempo parameters are numeric; use a normalised or real-unit value".into(),
                ));
            }
        };
        let Value::Normalised(value) = value else {
            return Err(crate::Error::InvalidParameter(
                "tempo parameters require a numeric value".into(),
            ));
        };
        self.send_grid(&crate::grid::set_tempo_param(parameter.index(), value)?)
    }

    /// Set a list-valued tempo parameter by zero-based option number.
    ///
    /// The four established lists use exact `option / (count - 1)` conversion:
    /// time signature 21, subdivisions 4, sound 6, and routing 5. Other tempo
    /// parameters are rejected rather than assigned an invented cardinality.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidParameter`] before I/O for a non-list
    /// parameter or an option outside that list.
    pub fn set_tempo_option(&self, parameter: TempoParameter, option: u32) -> crate::Result<()> {
        let count = parameter.option_count().ok_or_else(|| {
            crate::Error::InvalidParameter(format!(
                "tempo parameter index {} is not an established option list",
                parameter.index()
            ))
        })?;
        if option >= count {
            return Err(crate::Error::InvalidParameter(format!(
                "tempo parameter index {} has options 0-{}, got {option}",
                parameter.index(),
                count - 1
            )));
        }
        #[allow(clippy::cast_precision_loss)]
        let value = option as f32 / (count - 1) as f32;
        self.send_grid(&crate::grid::set_tempo_param(parameter.index(), value)?)
    }

    /// Set the metronome rhythmic subdivision.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn set_tempo_subdivision(&self, subdivision: TempoSubdivision) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_tempo_param(
            TempoParameter::Subdivisions.index(),
            subdivision.normalised(),
        )?)
    }

    /// Set the metronome sound.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn set_metronome_sound(&self, sound: MetronomeSound) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_tempo_param(
            TempoParameter::Sound.index(),
            sound.normalised(),
        )?)
    }

    /// Set the metronome output routing.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn set_metronome_routing(&self, routing: MetronomeRouting) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_tempo_param(
            TempoParameter::Routing.index(),
            routing.normalised(),
        )?)
    }

    /// Set the metronome time signature.
    ///
    /// The device may also rewrite positional `STEPSTATE` parameters 10-22,
    /// which hold beat accents. Callers verifying this write should check the
    /// target rather than requiring unrelated tempo parameters to stay equal.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn set_time_signature(&self, signature: TimeSignature) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_tempo_param(
            TempoParameter::TimeSignature.index(),
            signature.normalised(),
        )?)
    }

    /// Turn this preset's tempo LED on or off.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn set_tempo_led(&self, on: bool) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_tempo_param(
            TempoParameter::LedLight.index(),
            if on { 1.0 } else { 0.0 },
        )?)
    }

    /// Set this preset's metronome volume, where zero is silent.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidParameter`] before I/O unless `value` is
    /// finite and in 0..=1.
    pub fn set_metronome_volume(&self, value: f32) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_tempo_param(
            TempoParameter::Volume.index(),
            value,
        )?)
    }

    /// Assign one block to a STOMP footswitch.
    ///
    /// This always sends the device's required two-message sequence: DELETE
    /// the cell's existing assignment, then UPDATE it with the new footswitch.
    /// An UPDATE alone leaves an old assignment in place.
    ///
    /// # Errors
    ///
    /// Returns an invalid row or parameter error before I/O for a cell outside
    /// the four-by-eight grid.
    pub fn set_stomp_assignment(
        &self,
        row: Row,
        column: u32,
        footswitch: Footswitch,
    ) -> crate::Result<()> {
        let delete = crate::grid::clear_stomp_assignment(row, column)?;
        let update = crate::grid::set_stomp_assignment(row, column, footswitch.wire())?;
        self.send_grid(&delete)?;
        self.send_grid(&update)
    }

    /// Remove any STOMP assignment from one block.
    ///
    /// # Errors
    ///
    /// Returns an invalid row or parameter error before I/O for a cell outside
    /// the four-by-eight grid.
    pub fn clear_stomp_assignment(&self, row: Row, column: u32) -> crate::Result<()> {
        self.send_grid(&crate::grid::clear_stomp_assignment(row, column)?)
    }

    /// Make one preset-local STOMP footswitch momentary or latching.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn set_stomp_momentary(
        &self,
        footswitch: Footswitch,
        momentary: bool,
    ) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_stomp_momentary(
            footswitch.wire(),
            momentary,
        )?)
    }

    /// Set one preset-local STOMP label.
    ///
    /// `single = false` writes `stomp_labels`; `single = true` writes
    /// `single_stomp_labels`, used when the switch drives one block.
    ///
    /// # Errors
    ///
    /// Returns a session error if the message cannot be sent.
    pub fn set_stomp_label(
        &self,
        footswitch: Footswitch,
        label: &str,
        single: bool,
    ) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_stomp_label(
            footswitch.wire(),
            label.to_string(),
            single,
        )?)
    }

    /// Assign an expression pedal and normalized sweep range to one parameter.
    ///
    /// Minimum may exceed maximum to reverse the pedal. Both endpoints must be
    /// finite and within 0..=1. An index target needs no catalog model; a name
    /// target is resolved case-insensitively through the supplied model.
    ///
    /// # Errors
    ///
    /// Returns an invalid row or parameter error before I/O for a bad cell or
    /// range.
    #[allow(clippy::too_many_arguments)]
    pub fn set_expression(
        &self,
        row: Row,
        column: u32,
        target: ParameterTarget,
        pedal: ExpressionPedal,
        minimum: f32,
        maximum: f32,
        model: Option<&crate::catalog::Model>,
    ) -> crate::Result<()> {
        let param_index = match target {
            ParameterTarget::Index(index) => index,
            ParameterTarget::Name(name) => {
                if name.trim().is_empty() {
                    return Err(crate::Error::InvalidParameter(
                        "parameter name cannot be empty".into(),
                    ));
                }
                let model = model.ok_or_else(|| {
                    crate::Error::InvalidParameter(
                        "a named expression parameter requires its catalog model".into(),
                    )
                })?;
                let parameter = model.parameter(&name).ok_or_else(|| {
                    crate::Error::NotFound(format!(
                        "{} has no expression parameter {name:?}",
                        model.name
                    ))
                })?;
                if parameter.kind.is_read_only()
                    || matches!(
                        parameter.kind,
                        ParameterKind::Str | ParameterKind::Empty | ParameterKind::Unknown
                    )
                {
                    return Err(crate::Error::InvalidParameter(format!(
                        "{} cannot be assigned to an expression pedal",
                        parameter.name
                    )));
                }
                u32::try_from(parameter.index).map_err(|_| {
                    crate::Error::InvalidParameter(format!(
                        "parameter index {} does not fit on the wire",
                        parameter.index
                    ))
                })?
            }
        };
        self.send_grid(&crate::grid::set_expression(
            row,
            column,
            param_index,
            pedal.wire(),
            minimum,
            maximum,
        )?)
    }

    /// Assign expression-pedal bypass behavior to one block.
    ///
    /// # Errors
    ///
    /// Returns an invalid row or parameter error before I/O for a bad cell or
    /// delay above 5000 ms.
    #[allow(clippy::too_many_arguments)]
    pub fn set_expression_bypass(
        &self,
        row: Row,
        column: u32,
        pedal: ExpressionPedal,
        mode: ExpressionBypassMode,
        invert: bool,
        delay_ms: u32,
        latch_emulation: bool,
    ) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_expression_bypass(
            row,
            column,
            pedal.wire(),
            mode as u32,
            invert,
            delay_ms,
            latch_emulation,
        )?)
    }

    /// Replace the MIDI output messages for one footswitch or expression source.
    ///
    /// This sends message type `MIDISettings`, never `Grid`: firmware accepts
    /// and ignores a Grid update carrying the preset's MIDI fields.
    ///
    /// # Errors
    ///
    /// Returns an invalid parameter error before I/O when more than 12 messages
    /// are supplied.
    pub fn set_midi_out(&self, source: MidiSource, messages: &[MidiOut]) -> crate::Result<()> {
        let message = build_midi_settings(source.wire(), messages, false)?;
        self.session.send(
            MessageType::MidiSettings,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Replace the up-to-12 MIDI messages sent when this preset loads.
    ///
    /// # Errors
    ///
    /// Returns an invalid parameter error before I/O when more than 12 messages
    /// are supplied.
    pub fn set_preset_load_midi_out(&self, messages: &[MidiOut]) -> crate::Result<()> {
        let message = build_midi_settings(0, messages, true)?;
        self.session.send(
            MessageType::MidiSettings,
            &prost::Message::encode_to_vec(&message),
        )
    }

    /// Set a combined-splitter parameter by raw wire index.
    ///
    /// Splitters exist only on wire rows 0 and 2. This writes
    /// `combined_splitter`, never the read-only `splitter` collection, and
    /// sends no invented model hash. `value` must be finite and normalised to
    /// 0..=1.
    ///
    /// When `scene` is present, promotion (if requested), scene switching, and
    /// the value are sent as three separate messages. The scene-mode flag must
    /// never be packed with the value.
    ///
    /// This operation is derived from licensed upstream evidence and remains
    /// provisional until this crate's hardware smoke passes.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidRow`], [`crate::Error::InvalidScene`], or
    /// [`crate::Error::InvalidParameter`] before any write for invalid input.
    pub fn set_splitter_param(
        &self,
        row: Row,
        param_index: u32,
        value: f32,
        scene: Option<u32>,
        promote: bool,
    ) -> crate::Result<()> {
        if let Some(scene) = scene {
            validate_scene(scene)?;
        }
        let write = crate::grid::set_splitter_param(row, param_index, value)?;
        let promotion = if scene.is_some() && promote {
            Some(crate::grid::set_splitter_param_scene_mode(
                row,
                param_index,
                true,
            )?)
        } else {
            None
        };
        self.send_control_sequence(promotion.as_ref(), scene, &write)
    }

    /// Set a mixer parameter by raw wire index.
    ///
    /// Mixers exist only on wire rows 0 and 2 and are addressed in
    /// `chain.mixer` with model id 11000. Values and optional scene promotion
    /// follow the same validation and separate-message sequence as
    /// [`Self::set_splitter_param`]. This operation remains provisionally
    /// verified offline.
    ///
    /// # Errors
    ///
    /// As [`Self::set_splitter_param`].
    pub fn set_mixer_param(
        &self,
        row: Row,
        param_index: u32,
        value: f32,
        scene: Option<u32>,
        promote: bool,
    ) -> crate::Result<()> {
        self.set_sub_control_param(
            row,
            crate::grid::SubControl::Mixer,
            param_index,
            value,
            scene,
            promote,
        )
    }

    /// Set one row's lane-output parameter by raw wire index.
    ///
    /// The write targets `chain.output_control` with model id 23000. Values
    /// must be finite and normalised to 0..=1. This operation remains
    /// provisionally verified offline.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidScene`] or
    /// [`crate::Error::InvalidParameter`] before any write for invalid input.
    pub fn set_lane_output(
        &self,
        row: Row,
        param_index: u32,
        value: f32,
        scene: Option<u32>,
        promote: bool,
    ) -> crate::Result<()> {
        self.set_sub_control_param(
            row,
            crate::grid::SubControl::LaneOutput,
            param_index,
            value,
            scene,
            promote,
        )
    }

    /// Set one row's input-gate parameter by raw wire index.
    ///
    /// The write targets `chain.input_control` with model id 28000. This
    /// deliberately exposes only the raw-index core: a future name-resolving
    /// wrapper must reject catalog parameters of kind `Meter` rather than
    /// presenting gate meters as controls. Values must be finite and
    /// normalised to 0..=1. This operation remains provisionally verified
    /// offline.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidScene`] or
    /// [`crate::Error::InvalidParameter`] before any write for invalid input.
    pub fn set_input_gate(
        &self,
        row: Row,
        param_index: u32,
        value: f32,
        scene: Option<u32>,
        promote: bool,
    ) -> crate::Result<()> {
        self.set_sub_control_param(
            row,
            crate::grid::SubControl::InputGate,
            param_index,
            value,
            scene,
            promote,
        )
    }

    fn set_sub_control_param(
        &self,
        row: Row,
        control: crate::grid::SubControl,
        param_index: u32,
        value: f32,
        scene: Option<u32>,
        promote: bool,
    ) -> crate::Result<()> {
        if let Some(scene) = scene {
            validate_scene(scene)?;
        }
        let write = crate::grid::set_sub_control_param(row, control, param_index, value)?;
        let promotion = if scene.is_some() && promote {
            Some(crate::grid::set_sub_control_param_scene_mode(
                row,
                control,
                param_index,
                true,
            )?)
        } else {
            None
        };
        self.send_control_sequence(promotion.as_ref(), scene, &write)
    }

    fn send_control_sequence(
        &self,
        promotion: Option<&crate::proto::GridMessage>,
        scene: Option<u32>,
        write: &crate::proto::GridMessage,
    ) -> crate::Result<()> {
        let promotion_payload = promotion.map(prost::Message::encode_to_vec);
        let scene_payload = scene.map(|scene| {
            prost::Message::encode_to_vec(&SceneMessage {
                action: MessageAction::Update as i32,
                selected_scene: Some(crate::proto::scene_message::SelectedScene::SelectedScene(
                    scene,
                )),
                ..Default::default()
            })
        });
        let write_payload = prost::Message::encode_to_vec(write);
        let mut messages = Vec::with_capacity(3);
        if let Some(payload) = promotion_payload.as_deref() {
            messages.push((MessageType::Grid, payload));
        }
        if let Some(payload) = scene_payload.as_deref() {
            messages.push((MessageType::Scene, payload));
        }
        messages.push((MessageType::Grid, write_payload.as_slice()));
        self.session.send_many(messages)
    }

    /// Mute or unmute an even row's split/mix path.
    ///
    /// This writes one `split_bypass` entry, which controls all eight scenes;
    /// it never writes `mix_bypass`, the read-back field. This operation
    /// remains provisionally verified offline.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidRow`] for an odd row before any write.
    pub fn set_split_mute(&self, row: Row, muted: bool) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_split_mute(row, muted)?)
    }

    /// Bypass or enable one block on the active scene.
    ///
    /// For a block that does not follow scenes this lands on all eight stored
    /// scene slots at once, because bypass is then one global state.
    ///
    /// # Errors
    ///
    /// As [`QuadCortex::set_chain_input`].
    pub fn set_bypass(&self, row: Row, column: u32, bypassed: bool) -> crate::Result<()> {
        let scene = self.active_scene(READ_BACK_TIMEOUT)?;
        self.send_grid_and_verify(
            &crate::grid::set_bypass(row, column, bypassed),
            |preset| {
                if model_at(preset, row, column).is_none() {
                    return false;
                }
                let Some((scenes, scene_mode)) = bypass_state_at(preset, row, column) else {
                    return false;
                };
                if scene_mode == Some(true) {
                    scenes.get(usize::try_from(scene).ok().unwrap_or(usize::MAX)) == Some(&bypassed)
                } else {
                    !scenes.is_empty() && scenes.iter().all(|value| *value == bypassed)
                }
            },
            format!(
                "wire row {} column {column} bypass to {bypassed}",
                row.wire()
            ),
        )
    }

    /// Remove the block at a cell.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::GridWriteUnconfirmed`] when the target cell is
    /// absent from the complete live-grid read or remains occupied. Read and
    /// session failures are propagated.
    pub fn remove_block(&self, row: Row, column: u32) -> crate::Result<()> {
        self.send_grid_and_verify(
            &crate::grid::remove_block(row, column),
            |preset| {
                crate::helpers::model_at(preset, row.wire(), column).is_some_and(|model| {
                    !matches!(model.hash, Some(crate::proto::model::Hash::Hash(id)) if id != 0)
                })
            },
            format!("wire row {} column {column} to be empty", row.wire()),
        )
    }

    /// Move one occupied grid cell to an empty destination and verify it by
    /// reading the complete live grid back.
    ///
    /// Cross-row moves let the device create or adjust a parallel path; its
    /// computed split and rejoin columns are visible in the returned live
    /// preset. The optional `GridMove.grid` snapshot is advisory and is not
    /// sent.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidBlockMove`] for an invalid column, a
    /// no-op, or an occupied destination; [`crate::Error::NotFound`] for an
    /// empty source; and [`crate::Error::BlockMoveUnconfirmed`] when read-back
    /// does not show the exact source model at the destination with the source
    /// cleared.
    pub fn move_block(
        &self,
        from_row: Row,
        from_column: u32,
        to_row: Row,
        to_column: u32,
        drop: bool,
        timeout: Duration,
    ) -> crate::Result<()> {
        if from_column > 7 || to_column > 7 {
            return Err(crate::Error::InvalidBlockMove(format!(
                "columns are numbered 0-7, got {from_column} -> {to_column}"
            )));
        }
        if (from_row, from_column) == (to_row, to_column) {
            return Err(crate::Error::InvalidBlockMove(format!(
                "source and destination are both wire row {} (screen row {}) column {from_column}",
                from_row.wire(),
                from_row.screen()
            )));
        }

        let before = self.read_current_preset(timeout)?;
        let source_model = model_state_at(&before, from_row, from_column).ok_or_else(|| {
            crate::Error::NotFound(format!(
                "no block at source wire row {} (screen row {}) column {from_column}",
                from_row.wire(),
                from_row.screen()
            ))
        })?;
        let model_id = model_id_at(&before, from_row, from_column)
            .expect("occupied source model has a nonzero id");
        let source_bypass = bypass_state_at(&before, from_row, from_column);
        if let Some(destination_model) = model_id_at(&before, to_row, to_column) {
            return Err(crate::Error::InvalidBlockMove(format!(
                "destination wire row {} (screen row {}) column {to_column} is occupied by model {destination_model}",
                to_row.wire(),
                to_row.screen()
            )));
        }

        let message = crate::grid::move_block(from_row, from_column, to_row, to_column, drop);
        self.session.send(
            MessageType::GridMove,
            &prost::Message::encode_to_vec(&message),
        )?;

        let after = self.read_current_preset(timeout)?;
        if model_id_at(&after, from_row, from_column).is_none()
            && model_state_at(&after, to_row, to_column) == Some(source_model)
            && bypass_state_at(&after, to_row, to_column) == source_bypass
        {
            return Ok(());
        }
        Err(crate::Error::BlockMoveUnconfirmed(format!(
            "expected model {model_id} and its parameter/bypass state to move from wire row {} column {from_column} to wire row {} column {to_column}, but live-grid read-back did not confirm the complete move",
            from_row.wire(),
            to_row.wire()
        )))
    }

    /// Set a row's split and mix points, activating a parallel branch.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidRow`] for an odd row, which has no
    /// splitter. Returns [`crate::Error::GridWriteUnconfirmed`] when the row is
    /// absent or its split/mix state does not match a complete live-grid read.
    pub fn set_split(&self, row: Row, split: i32, mix: i32) -> crate::Result<()> {
        self.send_grid_and_verify(
            &crate::grid::set_split(row, split, mix)?,
            |preset| {
                chain_at(preset, row).is_some_and(|chain| {
                    chain
                        .split_control_points
                        .first()
                        .map_or(split < 0 && mix < 0, |points| {
                            points.split == split && points.mix == mix
                        })
                })
            },
            format!("wire row {} split {split} mix {mix}", row.wire()),
        )
    }

    /// Place a model in a grid cell, verifying the device accepted it.
    ///
    /// **A placement can be refused for want of DSP capacity.** The preset has
    /// a processing budget; a block that does not fit is accepted on the wire
    /// like any other write and is simply absent afterwards. Nothing in the
    /// reply says so - every host write is stalled and there is no per-block
    /// error message.
    ///
    /// So this verifies, which is possible without saving: a matching `Grid`
    /// broadcast is the fast path. If no echo arrives within `timeout`, it
    /// reads the live grid back and returns [`crate::Error::BlockRefused`]
    /// only when the cell is confirmed not to hold the requested model.
    ///
    /// Use [`QuadCortex::set_block_unverified`] to send and return
    /// immediately, in which case a save and read-back is the only way to
    /// learn whether the block is there.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::BlockRefused`] if neither an echo nor the
    /// confirming read-back shows the model in the requested cell. A failed
    /// read-back returns that read error rather than guessing.
    pub fn set_block(
        &self,
        row: Row,
        column: u32,
        model_id: u32,
        timeout: Duration,
    ) -> crate::Result<Placement> {
        let payload = crate::grid::encode(&crate::grid::set_block(row, column, model_id));
        let wire_row = row.wire();

        let echoed = self.session.await_broadcast(
            MessageType::Grid,
            || self.session.send(MessageType::Grid, &payload),
            timeout,
            move |m| grid_echoes_cell(m, wire_row, column, model_id),
        );

        match echoed {
            Ok(_) => Ok(Placement::EchoConfirmed),
            Err(crate::Error::ReadTimeout(_)) => {
                // A missing echo is NOT proof of refusal.
                //
                // Measured on hardware 2026-08-02: placing blocks into a
                // freshly recalled preset, the first three produced no echo
                // within 5 s and the next two echoed immediately - yet a
                // read-back showed ALL FIVE present. The device's echo
                // latency varies with how busy it is, exactly as its
                // handshake latency does, so a fixed timeout produces false
                // refusals on a busy unit.
                //
                // Reporting a placement as refused when it worked is the
                // worse direction of error: the caller re-adds the block, or
                // gives up on an edit that actually landed. So treat the echo
                // as a FAST PATH and the grid as ground truth.
                match self.read_current_preset(timeout.max(READ_BACK_TIMEOUT)) {
                    Ok(preset) => {
                        if preset_has_block(&preset, wire_row, column, model_id) {
                            Ok(Placement::ReadBackConfirmed)
                        } else {
                            Err(crate::Error::BlockRefused(format!(
                                "the device did not place model {model_id} at wire row \
                                 {wire_row} (screen row {}) column {column}: no echo \
                                 within {timeout:?}, and a read-back confirms the cell \
                                 does not hold it. The usual cause is that the preset \
                                 has no DSP capacity left for this block - try a \
                                 cheaper one, or free a block",
                                row.screen()
                            )))
                        }
                    }
                    // The read-back itself failed, so we genuinely cannot
                    // tell. Preserve that error rather than calling an
                    // indeterminate placement a refusal.
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Place a model in a grid cell without waiting for the device's echo.
    ///
    /// Faster, but a placement refused for DSP capacity is indistinguishable
    /// from one that worked. Prefer [`QuadCortex::set_block`].
    ///
    /// # Errors
    ///
    /// As [`QuadCortex::set_chain_input`].
    pub fn set_block_unverified(&self, row: Row, column: u32, model_id: u32) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_block(row, column, model_id))
    }

    /// Encode and send a grid message.
    fn send_grid(&self, message: &crate::proto::GridMessage) -> crate::Result<()> {
        let payload = crate::grid::encode(message);
        self.session.send(MessageType::Grid, &payload)
    }

    fn send_grid_and_verify(
        &self,
        message: &crate::proto::GridMessage,
        matches: impl FnOnce(&BinaryPreset) -> bool,
        expected: String,
    ) -> crate::Result<()> {
        self.send_grid(message)?;
        let after = self.read_current_preset(READ_BACK_TIMEOUT)?;
        if matches(&after) {
            return Ok(());
        }
        Err(crate::Error::GridWriteUnconfirmed(format!(
            "expected {expected}, but a complete live-grid read did not confirm it"
        )))
    }
}

/// Whether a preset holds a given model at a given cell.
///
/// Used to settle a `set_block` whose echo did not arrive: the grid is
/// ground truth, the echo only a fast path.
fn preset_has_block(
    preset: &crate::proto::BinaryPreset,
    row: u32,
    column: u32,
    model_id: u32,
) -> bool {
    let Some(model) = crate::helpers::model_at(preset, row, column) else {
        return false;
    };
    // A zero hash means the cell is EMPTY, which is how remove_block encodes
    // a removal. So it can never confirm a placement, not even of "model 0".
    matches!(
        model.hash,
        Some(crate::proto::model::Hash::Hash(id)) if id != 0 && id == model_id
    )
}

/// Whether a `Grid` broadcast names the given cell holding the given model.
///
/// Both `row` and `column` may arrive WITHOUT presence, in which case the
/// element's position in its repeated field is the index. Treating an absent
/// field as "not a match" would reject valid echoes and report a working
/// placement as refused.
fn grid_echoes_cell(message: &InboundMessage, row: u32, column: u32, model_id: u32) -> bool {
    use crate::proto::{GridMessage, grid_message};

    let Ok(decoded) = prost::Message::decode(message.body.as_ref()) as Result<GridMessage, _>
    else {
        return false;
    };
    let Some(grid_message::Preset::Preset(preset)) = decoded.preset else {
        return false;
    };

    crate::helpers::blocks(&preset)
        .into_iter()
        .any(|block| block.row == row && block.column == column && block.model_id == model_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::{Frame, FrameReassembler, encode_message};
    use crate::link::FakeLink;
    use crate::message::Message;
    use std::sync::Arc;

    fn file_folder(key: &str, files: Vec<crate::proto::ProductData>) -> crate::proto::FolderInfo {
        crate::proto::FolderInfo {
            key: Some(crate::proto::folder_info::Key::Key(key.into())),
            files,
            ..Default::default()
        }
    }

    fn indexed_file(index: i32) -> crate::proto::ProductData {
        crate::proto::ProductData {
            index: Some(crate::proto::product_data::Index::Index(index)),
            ..Default::default()
        }
    }

    fn complete_listing_for(key: &str, occupied: &[(i32, &str, Instrument)]) -> FileMessage {
        let files = (0..i32::try_from(SETLIST_SLOTS).unwrap())
            .map(|index| {
                let mut file = indexed_file(index);
                if let Some((_, name, instrument)) =
                    occupied.iter().find(|(slot, _, _)| *slot == index)
                {
                    file.name = Some(crate::proto::product_data::Name::Name((*name).into()));
                    file.key = Some(crate::proto::product_data::Key::Key(format!(
                        "{key}/{name}.pb"
                    )));
                    file.instrument = Some(crate::proto::product_data::Instrument::Instrument(
                        *instrument as i32,
                    ));
                }
                file
            })
            .collect();
        FileMessage {
            action: MessageAction::Update as i32,
            folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                key, files,
            ))),
            ..Default::default()
        }
    }

    fn complete_listing(occupied: &[(i32, &str)]) -> FileMessage {
        let occupied = occupied
            .iter()
            .map(|(slot, name)| (*slot, *name, Instrument::Guitar))
            .collect::<Vec<_>>();
        complete_listing_for(USER_SETLIST, &occupied)
    }

    fn push_file(link: &FakeLink, file: &FileMessage) {
        push_proto(link, MessageType::File, file);
    }

    fn push_proto(link: &FakeLink, message_type: MessageType, message: &impl prost::Message) {
        for mut report in
            encode_message(message_type as u16, &prost::Message::encode_to_vec(message))
        {
            report[0] = crate::ReportId::Input as u8;
            link.push_inbound(report);
        }
    }

    fn wait_for_file_write(link: &FakeLink, start: usize) -> (usize, FileMessage) {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut next = start;
        let mut reassembler = FrameReassembler::new();
        while Instant::now() < deadline {
            let written = link.written();
            while let Some(report) = written.get(next) {
                next += 1;
                let Ok(frame) = Frame::parse(report) else {
                    continue;
                };
                let Ok(Some(body)) = reassembler.feed(&frame) else {
                    continue;
                };
                let Ok(message) = Message::parse(&body) else {
                    continue;
                };
                if message.message_type == MessageType::File as u16 {
                    return (next, prost::Message::decode(message.body).unwrap());
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("timed out waiting for a File write from report {start}");
    }

    fn wait_for_write(link: &FakeLink, start: usize, expected: MessageType) -> (usize, Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut next = start;
        let mut reassembler = FrameReassembler::new();
        while Instant::now() < deadline {
            let written = link.written();
            while let Some(report) = written.get(next) {
                next += 1;
                let Ok(frame) = Frame::parse(report) else {
                    continue;
                };
                let Ok(Some(body)) = reassembler.feed(&frame) else {
                    continue;
                };
                let Ok(message) = Message::parse(&body) else {
                    continue;
                };
                if message.message_type == expected as u16 {
                    return (next, message.body.to_vec());
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("timed out waiting for a {expected:?} write from report {start}");
    }

    fn only_grid_model(body: &[u8]) -> crate::proto::Model {
        let message: crate::proto::GridMessage = prost::Message::decode(body).unwrap();
        crate::grid::preset_of(&message).unwrap().chains[0].models[0].clone()
    }

    fn push_current_preset(link: &FakeLink, request: &[u8], preset: BinaryPreset) {
        let request: RecallPresetMessage = prost::Message::decode(request).unwrap();
        push_proto(
            link,
            MessageType::RecallPreset,
            &RecallPresetMessage {
                action: MessageAction::Update as i32,
                request_id: request.request_id,
                preset: Some(crate::proto::recall_preset_message::Preset::Preset(preset)),
                ..Default::default()
            },
        );
    }

    fn answer_active_scene(link: &FakeLink, request: &[u8], scene: u32) {
        let request: SceneMessage = prost::Message::decode(request).unwrap();
        push_proto(
            link,
            MessageType::Scene,
            &SceneMessage {
                action: MessageAction::Update as i32,
                request_id: request.request_id,
                selected_scene: Some(crate::proto::scene_message::SelectedScene::SelectedScene(
                    scene,
                )),
            },
        );
    }

    fn one_chain(chain: crate::proto::Chain) -> BinaryPreset {
        BinaryPreset {
            chains: vec![chain],
            ..Default::default()
        }
    }

    fn seed_live_cache(session: &crate::Session) {
        let state = session.state_cache();
        let generation = state.status().generation;
        state.begin_subscription(generation);
        let message = RecallPresetMessage {
            preset: Some(crate::proto::recall_preset_message::Preset::Preset(
                BinaryPreset {
                    chains: vec![crate::proto::Chain::default(); 4],
                    ..Default::default()
                },
            )),
            ..Default::default()
        };
        state.observe(
            generation,
            MessageType::RecallPreset,
            &prost::Message::encode_to_vec(&message),
        );
        state.finish_subscription(generation);
    }

    fn answer_stored_recall(link: &FakeLink, request: &[u8], preset: BinaryPreset) {
        let request: SetlistPositionMessage = prost::Message::decode(request).unwrap();
        push_proto(
            link,
            MessageType::RecallPreset,
            &RecallPresetMessage {
                request_id: request.request_id.map(|request_id| {
                    let crate::proto::setlist_position_message::RequestId::RequestId(value) =
                        request_id;
                    crate::proto::recall_preset_message::RequestId::RequestId(value)
                }),
                preset: Some(crate::proto::recall_preset_message::Preset::Preset(preset)),
                ..Default::default()
            },
        );
    }

    fn io_ports(message: &IoSettingsMessage) -> &PortSettings {
        let Some(crate::proto::io_settings_message::Settings::Settings(settings)) =
            message.settings.as_ref()
        else {
            panic!("IOSettings update omitted settings");
        };
        settings
    }

    #[test]
    fn io_port_ids_are_typed_and_preserve_the_interleaved_wire_values() {
        assert_eq!(InputPort::Input1 as u32, 1);
        assert_eq!(InputPort::Input2 as u32, 2);
        assert_eq!(InputPort::Input12 as u32, 3);
        assert_eq!(InputPort::Return1 as u32, 4);
        assert_eq!(InputPort::Return2 as u32, 5);
        assert_eq!(InputPort::Return12 as u32, 6);
        assert_eq!(InputPort::try_from(4).unwrap(), InputPort::Return1);
        assert!(InputPort::try_from(7).is_err());

        let outputs = [
            OutputPort::Xlr12,
            OutputPort::Out34,
            OutputPort::Send12,
            OutputPort::Xlr1,
            OutputPort::Xlr2,
            OutputPort::Out3,
            OutputPort::Out4,
            OutputPort::Send1,
            OutputPort::Send2,
        ];
        assert_eq!(outputs.map(|port| port as u32), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert!(OutputPort::try_from(10).is_err());
    }

    #[test]
    fn io_completion_matches_the_measured_coros_4_0_1_capability_matrix() {
        let input = |input_port_id| {
            let full = matches!(input_port_id, 1 | 2);
            InputPortSettings {
                input_port_id,
                level: Some(crate::proto::input_port_settings::Level::Level(0.5)),
                input_zmode: full.then_some(
                    crate::proto::input_port_settings::InputZmode::InputZmode(0.5),
                ),
                input_type: full
                    .then_some(crate::proto::input_port_settings::InputType::InputType(0.5)),
                ground_lift: Some(crate::proto::input_port_settings::GroundLift::GroundLift(
                    0.0,
                )),
                plugged: None,
            }
        };
        let output = |output_port_id| OutputPortSettings {
            output_port_id,
            level: Some(crate::proto::output_port_settings::Level::Level(0.5)),
            ground_lift: matches!(output_port_id, 1 | 4 | 5).then_some(
                crate::proto::output_port_settings::GroundLift::GroundLift(0.0),
            ),
            mute: matches!(output_port_id, 1 | 2 | 4 | 5 | 6 | 7)
                .then_some(crate::proto::output_port_settings::Mute::Mute(false)),
            plugged: None,
        };
        let complete = IoSettingsMessage {
            settings: Some(crate::proto::io_settings_message::Settings::Settings(
                PortSettings {
                    in_port: [1, 2, 4, 5].map(input).to_vec(),
                    out_port: [4, 5, 1, 6, 7, 2, 8, 9].map(output).to_vec(),
                    usb_port: Some(crate::proto::port_settings::UsbPort::UsbPort(
                        UsbPortSettings {
                            level: Some(crate::proto::usb_port_settings::Level::Level(0.5)),
                            hp_select: Some(crate::proto::usb_port_settings::HpSelect::HpSelect(
                                0.5,
                            )),
                            dry_wet: Some(crate::proto::usb_port_settings::DryWet::DryWet(1.0)),
                            plugged: None,
                        },
                    )),
                    midi_port: Some(crate::proto::port_settings::MidiPort::MidiPort(
                        MidiPortSettings {
                            midi_thru: Some(crate::proto::midi_port_settings::MidiThru::MidiThru(
                                0.0,
                            )),
                        },
                    )),
                    ..Default::default()
                },
            )),
            xlr1_2_linked: Some(crate::proto::io_settings_message::Xlr12Linked::Xlr12Linked(
                false,
            )),
            out3_4_linked: Some(crate::proto::io_settings_message::Out34Linked::Out34Linked(
                false,
            )),
            ..Default::default()
        };
        assert!(io_settings_are_complete(&complete));

        let mut missing_pairing = complete.clone();
        missing_pairing.out3_4_linked = None;
        assert!(!io_settings_are_complete(&missing_pairing));

        let mut missing_applicable_field = complete.clone();
        let Some(crate::proto::io_settings_message::Settings::Settings(settings)) =
            missing_applicable_field.settings.as_mut()
        else {
            unreachable!();
        };
        settings.out_port[0].ground_lift = None;
        assert!(!io_settings_are_complete(&missing_applicable_field));

        let mut unexpected_inapplicable_field = complete.clone();
        let Some(crate::proto::io_settings_message::Settings::Settings(settings)) =
            unexpected_inapplicable_field.settings.as_mut()
        else {
            unreachable!();
        };
        settings.in_port[2].input_zmode = Some(
            crate::proto::input_port_settings::InputZmode::InputZmode(0.5),
        );
        assert!(!io_settings_are_complete(&unexpected_inapplicable_field));

        let mut duplicate_input = complete;
        if let Some(settings) = duplicate_input.settings.as_mut() {
            let crate::proto::io_settings_message::Settings::Settings(settings) = settings;
            settings.in_port[3].input_port_id = 4;
        }
        assert!(!io_settings_are_complete(&duplicate_input));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn input_output_and_usb_builders_send_one_field_per_message_in_patch_order() {
        let inputs = build_input_port_updates(
            InputPort::Return1,
            InputPortPatch {
                level: Some(0.1),
                impedance: Some(0.2),
                input_type: Some(0.3),
                ground_lift: Some(0.4),
            },
        )
        .unwrap();
        assert_eq!(inputs.len(), 4);
        for message in &inputs {
            assert_eq!(message.action, MessageAction::Update as i32);
            let settings = io_ports(message);
            assert_eq!(settings.in_port.len(), 1);
            assert!(settings.out_port.is_empty());
            assert!(settings.usb_port.is_none());
            assert_eq!(settings.in_port[0].input_port_id, 4);
            let port = &settings.in_port[0];
            assert_eq!(
                [
                    port.level.is_some(),
                    port.input_zmode.is_some(),
                    port.input_type.is_some(),
                    port.ground_lift.is_some()
                ]
                .into_iter()
                .filter(|set| *set)
                .count(),
                1
            );
        }
        assert!(
            inputs[0]
                .settings
                .as_ref()
                .is_some_and(|_| io_ports(&inputs[0]).in_port[0].level.is_some())
        );
        assert!(io_ports(&inputs[1]).in_port[0].input_zmode.is_some());
        assert!(io_ports(&inputs[2]).in_port[0].input_type.is_some());
        assert!(io_ports(&inputs[3]).in_port[0].ground_lift.is_some());

        let outputs = build_output_port_updates(
            OutputPort::Xlr1,
            OutputPortPatch {
                level: Some(0.2),
                ground_lift: Some(0.5),
                mute: Some(true),
            },
        )
        .unwrap();
        assert_eq!(outputs.len(), 3);
        for message in &outputs {
            let settings = io_ports(message);
            assert_eq!(settings.out_port.len(), 1);
            assert_eq!(settings.out_port[0].output_port_id, 4);
            let port = &settings.out_port[0];
            assert_eq!(
                [
                    port.level.is_some(),
                    port.ground_lift.is_some(),
                    port.mute.is_some()
                ]
                .into_iter()
                .filter(|set| *set)
                .count(),
                1
            );
        }
        assert!(io_ports(&outputs[0]).out_port[0].level.is_some());
        assert!(io_ports(&outputs[1]).out_port[0].ground_lift.is_some());
        assert!(io_ports(&outputs[2]).out_port[0].mute.is_some());

        let usb = build_usb_port_updates(UsbPortPatch {
            level: Some(0.25),
            headphone_source: Some(0.5),
            dry_wet: Some(0.75),
        })
        .unwrap();
        assert_eq!(usb.len(), 3);
        let usb_fields = usb
            .iter()
            .map(|message| {
                let Some(crate::proto::port_settings::UsbPort::UsbPort(port)) =
                    io_ports(message).usb_port.as_ref()
                else {
                    panic!("USB update omitted USB port");
                };
                [
                    port.level.is_some(),
                    port.hp_select.is_some(),
                    port.dry_wet.is_some(),
                ]
            })
            .collect::<Vec<_>>();
        assert_eq!(
            usb_fields,
            vec![
                [true, false, false],
                [false, true, false],
                [false, false, true]
            ]
        );
    }

    #[test]
    fn midi_and_pairing_updates_each_have_one_top_level_control() {
        let midi = build_midi_thru_update(true);
        let settings = io_ports(&midi);
        assert!(settings.in_port.is_empty() && settings.out_port.is_empty());
        let Some(crate::proto::port_settings::MidiPort::MidiPort(port)) =
            settings.midi_port.as_ref()
        else {
            panic!("MIDI update omitted MIDI port");
        };
        assert!(matches!(
            port.midi_thru,
            Some(crate::proto::midi_port_settings::MidiThru::MidiThru(value))
                if (value - 1.0).abs() < f32::EPSILON
        ));

        let pairings = build_output_pairing_updates(OutputPairingPatch {
            xlr12: Some(true),
            out34: Some(false),
        })
        .unwrap();
        assert_eq!(pairings.len(), 2);
        assert!(pairings[0].xlr1_2_linked.is_some());
        assert!(pairings[0].out3_4_linked.is_none());
        assert!(pairings[1].xlr1_2_linked.is_none());
        assert!(pairings[1].out3_4_linked.is_some());
    }

    #[test]
    fn invalid_and_empty_io_patches_do_no_fake_link_io() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());

        assert!(
            qc.set_input_port(InputPort::Input1, InputPortPatch::default())
                .is_err()
        );
        assert!(
            qc.set_input_port(
                InputPort::Input1,
                InputPortPatch {
                    level: Some(0.5),
                    ground_lift: Some(f32::NAN),
                    ..Default::default()
                },
            )
            .is_err()
        );
        assert!(
            qc.set_output_port(
                OutputPort::Xlr1,
                OutputPortPatch {
                    level: Some(f32::INFINITY),
                    ..Default::default()
                },
            )
            .is_err()
        );
        assert!(
            qc.set_usb_port(UsbPortPatch {
                level: Some(0.5),
                dry_wet: Some(1.1),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            qc.set_output_pairing(OutputPairingPatch::default())
                .is_err()
        );
        assert_eq!(link.write_count(), 0);
        session.close();
    }

    #[test]
    fn scene_management_encodes_the_verified_wire_shapes() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let mut next = 0;

        qc.switch_scene(2).unwrap();
        let (after, body) = wait_for_write(&link, next, MessageType::Scene);
        next = after;
        let scene: SceneMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!(scene.action, MessageAction::Update as i32);
        assert!(matches!(
            scene.selected_scene,
            Some(crate::proto::scene_message::SelectedScene::SelectedScene(2))
        ));

        qc.set_scene_label(1, Some("Wide Lead")).unwrap();
        let (after, body) = wait_for_write(&link, next, MessageType::SceneLabel);
        next = after;
        let label: SceneLabelMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!((label.index, label.label.as_str()), (1, "Wide Lead"));

        qc.set_scene_label(3, None).unwrap();
        let (after, body) = wait_for_write(&link, next, MessageType::SceneLabel);
        next = after;
        let label: SceneLabelMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!((label.index, label.label.as_str()), (3, SCENE_UNLABELLED));

        qc.set_scene_color(4, 0xff12_34ab).unwrap();
        let (after, body) = wait_for_write(&link, next, MessageType::SceneColor);
        next = after;
        let color: SceneColorMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!((color.index, color.color), (4, 0xff12_34ab));

        qc.copy_scene(1, 5, false).unwrap();
        let (after, body) = wait_for_write(&link, next, MessageType::SceneCopy);
        next = after;
        let copy: SceneCopyMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!(
            (copy.from_index, copy.to_index, copy.is_swap),
            (1, 5, false)
        );
        assert_eq!(copy.action, MessageAction::Update as i32);

        qc.copy_scene(2, 6, true).unwrap();
        let (_, body) = wait_for_write(&link, next, MessageType::SceneCopy);
        let swap: SceneCopyMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!((swap.from_index, swap.to_index, swap.is_swap), (2, 6, true));

        for result in [
            qc.switch_scene(8),
            qc.set_scene_label(8, Some("invalid")),
            qc.set_scene_color(8, 0),
            qc.copy_scene(0, 8, false),
        ] {
            assert!(matches!(result, Err(crate::Error::InvalidScene(_))));
        }
        session.close();
    }

    fn complete_settings_fixture() -> GeneralSettingsMessage {
        let mut message = build_settings_update(&GeneralSettingsPatch {
            screen_brightness: Some(59),
            led_brightness: Some(47),
            dimmed_led_brightness: Some(23),
            stomp_mode_auto_assign: Some(true),
            swap_tempo_tuner_access: Some(false),
            enable_dynamic_delay_compensation: Some(true),
            gig_view_stomp_access_enabled: Some(false),
            midi_channel: Some(1),
            midi_over_usb: Some(true),
            midi_clock_in_enabled: Some(false),
            ignore_duplicate_pc: Some(true),
            disable_internet_connection_check: Some(false),
            enable_preset_dimmed: Some(true),
            enable_scene_dimmed: Some(true),
            enable_stomp_dimmed: Some(true),
        })
        .unwrap();
        message.action = 0;
        message.scene_block_bypass =
            Some(crate::proto::general_settings_message::SceneBlockBypass::SceneBlockBypass(0));
        message.hold_timing =
            Some(crate::proto::general_settings_message::HoldTiming::HoldTiming(3));
        message.master_volume_assignment = build_master_volume_assignment(MasterVolumeAssignment {
            out12: true,
            out34: true,
            send12: false,
            headphones: true,
        })
        .master_volume_assignment;
        let bypass = build_global_bypass(GlobalBypassState {
            cab: [false, true, false, true],
            ir: [true, false, true, false],
        });
        message.global_bypass_cab = bypass.global_bypass_cab;
        message.global_bypass_ir = bypass.global_bypass_ir;
        message
    }

    #[test]
    fn safe_settings_builders_encode_one_field_intent_and_checked_hold_steps() {
        let patch = GeneralSettingsPatch {
            swap_tempo_tuner_access: Some(true),
            ..Default::default()
        };
        let message = build_settings_update(&patch).unwrap();
        assert_eq!(message.action, MessageAction::Update as i32);
        assert_eq!(
            message.swap_tempo_tuner_access,
            Some(
                crate::proto::general_settings_message::SwapTempoTunerAccess::SwapTempoTunerAccess(
                    true
                )
            )
        );
        assert!(message.screen_brightness.is_none());
        assert!(message.hold_timing.is_none());
        assert!(message.master_volume_assignment.is_none());
        assert!(message.global_bypass_cab.is_none());
        assert!(message.global_bypass_ir.is_none());
        assert!(message.power_option.is_none());
        assert!(message.reset_wifi_networks.is_none());
        assert!(message.reset_settings.is_none());
        assert!(message.factory_reset.is_none());
        assert!(message.internal_midi_clock_enabled.is_none());

        for (milliseconds, index) in [(500, 0), (600, 1), (1000, 5)] {
            let message = build_hold_timing(milliseconds).unwrap();
            assert_eq!(
                message.hold_timing,
                Some(crate::proto::general_settings_message::HoldTiming::HoldTiming(index))
            );
        }
        for invalid in [0, 499, 501, 550, 1001, u32::MAX] {
            assert!(build_hold_timing(invalid).is_err());
        }
        assert!(build_settings_update(&GeneralSettingsPatch::default()).is_err());
        assert!(
            build_settings_update(&GeneralSettingsPatch {
                midi_channel: Some(0),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            build_settings_update(&GeneralSettingsPatch {
                screen_brightness: Some(101),
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn nested_restore_builders_always_carry_complete_groups() {
        let assignment = MasterVolumeAssignment {
            out12: true,
            out34: false,
            send12: true,
            headphones: false,
        };
        let message = build_master_volume_assignment(assignment);
        assert_eq!(master_volume_assignment(&message), Some(assignment));
        assert!(message.global_bypass_cab.is_none());

        let bypass = GlobalBypassState {
            cab: [true, false, true, false],
            ir: [false, true, false, true],
        };
        let message = build_global_bypass(bypass);
        assert_eq!(global_bypass_state(&message), Some(bypass));
        assert!(message.master_volume_assignment.is_none());
    }

    #[test]
    fn nested_partial_intent_reads_merges_and_writes_complete_state() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (next, body) = wait_for_write(&fake, 0, MessageType::GeneralSettings);
            let read: GeneralSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(read.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::GeneralSettings,
                &complete_settings_fixture(),
            );
            let (next, body) = wait_for_write(&fake, next, MessageType::GeneralSettings);
            let update: GeneralSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(
                master_volume_assignment(&update),
                Some(MasterVolumeAssignment {
                    out12: true,
                    out34: false,
                    send12: false,
                    headphones: true,
                })
            );

            let (next, body) = wait_for_write(&fake, next, MessageType::GeneralSettings);
            let read: GeneralSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(read.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::GeneralSettings,
                &complete_settings_fixture(),
            );
            let (_, body) = wait_for_write(&fake, next, MessageType::GeneralSettings);
            let update: GeneralSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(
                global_bypass_state(&update),
                Some(GlobalBypassState {
                    cab: [true; 4],
                    ir: [true, false, true, false],
                })
            );
        });

        qc.set_master_volume_assignment(
            MasterVolumeAssignmentPatch {
                out34: Some(false),
                ..Default::default()
            },
            Duration::from_secs(1),
        )
        .unwrap();
        qc.set_global_bypass(
            GlobalBypassPatch {
                cab: Some([true; 4]),
                ir: None,
            },
            Duration::from_secs(1),
        )
        .unwrap();
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn global_eq_builders_use_five_band_stride_and_output_indices() {
        let messages = build_global_eq_band(
            3,
            GlobalEqBandPatch {
                gain: Some(0.75),
                q: Some(0.2),
                filter_type: Some(GlobalEqFilter::HighShelf),
                enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        let pairs = messages
            .iter()
            .map(|message| {
                assert_eq!(message.parameters.len(), 1);
                let parameter = &message.parameters[0];
                (parameter.parameter_index, parameter.value)
            })
            .collect::<Vec<_>>();
        assert_eq!(pairs, vec![(10, 0.75), (12, 0.2), (13, 0.75), (14, 0.0)]);

        let output = build_global_eq_output(GlobalEqOutputPatch {
            level: Some(0.4),
            out12: Some(true),
            out34: Some(false),
        })
        .unwrap();
        assert_eq!(
            output
                .iter()
                .map(|message| {
                    let parameter = &message.parameters[0];
                    (parameter.parameter_index, parameter.value)
                })
                .collect::<Vec<_>>(),
            vec![(25, 0.4), (26, 1.0), (27, 0.0)]
        );
        for result in [
            build_global_eq_band(
                0,
                GlobalEqBandPatch {
                    gain: Some(0.5),
                    ..Default::default()
                },
            ),
            build_global_eq_band(
                6,
                GlobalEqBandPatch {
                    gain: Some(0.5),
                    ..Default::default()
                },
            ),
            build_global_eq_band(1, GlobalEqBandPatch::default()),
            build_global_eq_band(
                1,
                GlobalEqBandPatch {
                    q: Some(f32::NAN),
                    ..Default::default()
                },
            ),
        ] {
            assert!(result.is_err());
        }
        assert!(build_global_eq_output(GlobalEqOutputPatch::default()).is_err());
        assert!(
            build_global_eq_output(GlobalEqOutputPatch {
                level: Some(1.01),
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn modes_and_tuner_are_typed_and_invalid_shapes_do_no_io() {
        assert!(FootswitchModeSlot::try_from(8).is_ok());
        assert!(FootswitchModeSlot::try_from(9).is_err());
        assert!(FootswitchModeSlot::try_from(10).is_err());
        assert!(build_mode_cycle(&[]).is_err());
        assert!(build_mode_cycle(&[FootswitchModeSlot::PresetStomp]).is_err());
        assert!(
            build_mode_cycle(&[
                FootswitchModeSlot::PresetScene,
                FootswitchModeSlot::StompScene,
            ])
            .is_err()
        );
        let cycle = build_mode_cycle(&[FootswitchModeSlot::PresetStomp, FootswitchModeSlot::Scene])
            .unwrap();
        let Some(crate::proto::mode_message::AvailableModes::AvailableModes(available)) =
            cycle.available_modes
        else {
            panic!("mode cycle missing");
        };
        assert_eq!(available.modes, vec![4, 1]);

        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        assert!(qc.set_mode_cycle(&[]).is_err());
        assert!(qc.set_tuner_reference(f32::NAN).is_err());
        assert!(qc.set_tuner_reference(-15.01).is_err());
        assert!(qc.set_tuner_reference(15.01).is_err());
        assert!(qc
            .set_master_volume_assignment(
                MasterVolumeAssignmentPatch::default(),
                Duration::ZERO,
            )
            .is_err());
        assert!(
            qc.set_global_bypass(GlobalBypassPatch::default(), Duration::ZERO)
                .is_err()
        );
        assert!(link.written().is_empty());
        session.close();
    }

    #[test]
    fn global_mode_view_and_tuner_methods_send_exact_sparse_shapes() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());

        qc.set_scene_bypass_behavior(SceneBypassBehavior::NeverOverwrite)
            .unwrap();
        qc.set_global_eq_bypassed(true).unwrap();
        qc.set_global_eq(
            5,
            GlobalEqBandPatch {
                frequency: Some(0.4),
                ..Default::default()
            },
        )
        .unwrap();
        qc.set_mode(FootswitchModeSlot::StompScene).unwrap();
        qc.set_gig_view(true).unwrap();
        qc.show_tuner(false).unwrap();
        qc.set_tuner_input(TunerInput::Return2).unwrap();
        qc.set_tuner_mute(true).unwrap();
        qc.set_tuner_reference(2.0).unwrap();

        let (next, body) = wait_for_write(&link, 0, MessageType::GeneralSettings);
        let settings: GeneralSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!(settings.action, MessageAction::Update as i32);
        assert_eq!(
            settings.scene_block_bypass,
            Some(crate::proto::general_settings_message::SceneBlockBypass::SceneBlockBypass(2))
        );
        assert!(settings.master_volume_assignment.is_none());

        let (next, body) = wait_for_write(&link, next, MessageType::GlobalEq);
        let bypass: GlobalEqMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(matches!(
            bypass.bypassed,
            Some(crate::proto::global_eq_message::Bypassed::Bypassed(true))
        ));
        assert!(bypass.parameters.is_empty());

        let (next, body) = wait_for_write(&link, next, MessageType::GlobalEq);
        let band: GlobalEqMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!(
            band.parameters,
            vec![GlobalEqParameter {
                parameter_index: 21,
                value: 0.4
            }]
        );

        let (next, body) = wait_for_write(&link, next, MessageType::Mode);
        let mode: ModeMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(matches!(
            mode.mode,
            Some(crate::proto::mode_message::Mode::Mode(8))
        ));
        assert!(mode.available_modes.is_none());

        let (next, body) = wait_for_write(&link, next, MessageType::ShowGigView);
        let gig: ShowGigViewMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(gig.show);
        let (next, body) = wait_for_write(&link, next, MessageType::ShowTuner);
        let show: ShowTunerMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(!show.show);

        let (next, body) = wait_for_write(&link, next, MessageType::Tuner);
        let input: TunerMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(matches!(
            input.input_port_id,
            Some(crate::proto::tuner_message::InputPortId::InputPortId(5))
        ));
        assert!(input.mute.is_none() && input.frequency.is_none());
        let (next, body) = wait_for_write(&link, next, MessageType::Tuner);
        let mute: TunerMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(matches!(
            mute.mute,
            Some(crate::proto::tuner_message::Mute::Mute(true))
        ));
        assert!(mute.input_port_id.is_none() && mute.frequency.is_none());
        let (_, body) = wait_for_write(&link, next, MessageType::Tuner);
        let reference: TunerMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(matches!(
            reference.frequency,
            Some(crate::proto::tuner_message::Frequency::Frequency(2.0))
        ));
        assert!(reference.input_port_id.is_none() && reference.mute.is_none());
        session.close();
    }

    #[test]
    fn capture_selection_places_then_writes_exact_reference_then_follow_ups_in_order() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (_, body) = wait_for_write(&fake, 0, MessageType::Grid);
            let placement: crate::proto::GridMessage =
                prost::Message::decode(body.as_slice()).unwrap();
            push_proto(&fake, MessageType::Grid, &placement);
        });
        let capture = LibraryEntry {
            key: "a".repeat(64),
            name: "Fictional Capture".into(),
        };
        let follow_ups = [
            ParameterWrite {
                index: 4,
                value: Value::Normalised(0.56),
            },
            ParameterWrite {
                index: 7,
                value: Value::Text("after selection".into()),
            },
        ];

        assert_eq!(
            qc.set_capture(
                Row::from_wire(1),
                3,
                &capture,
                Some(DEFAULT_CAPTURE_MODEL),
                &follow_ups,
                Duration::from_secs(1),
            )
            .unwrap(),
            Some(Placement::EchoConfirmed)
        );
        responder.join().unwrap();

        let (next, placement) = wait_for_write(&link, 0, MessageType::Grid);
        let placement = only_grid_model(&placement);
        assert_eq!(
            placement.hash,
            Some(crate::proto::model::Hash::Hash(14_000))
        );
        assert_eq!(
            placement.column,
            Some(crate::proto::model::Column::Column(3))
        );

        let (next, selector) = wait_for_write(&link, next, MessageType::Grid);
        let selector = only_grid_model(&selector);
        assert_eq!(
            selector.params[0].index,
            Some(crate::proto::param::Index::Index(5))
        );
        assert_eq!(
            selector.params[0].param_values[0].value,
            Some(crate::proto::param_value::Value::StringValue(format!(
                "{}{}",
                capture.key, capture.name
            )))
        );

        let (next, first_follow_up) = wait_for_write(&link, next, MessageType::Grid);
        let first_follow_up = only_grid_model(&first_follow_up);
        assert_eq!(
            first_follow_up.params[0].index,
            Some(crate::proto::param::Index::Index(4))
        );
        let (_, second_follow_up) = wait_for_write(&link, next, MessageType::Grid);
        let second_follow_up = only_grid_model(&second_follow_up);
        assert_eq!(
            second_follow_up.params[0].index,
            Some(crate::proto::param::Index::Index(7))
        );
        session.close();
    }

    #[test]
    fn capture_selection_reuses_only_an_existing_capture_block() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (next, request) = wait_for_write(&fake, 0, MessageType::RecallPreset);
            let mut preset = BinaryPreset {
                chains: vec![crate::proto::Chain::default(); 4],
                ..Default::default()
            };
            preset.chains[0].models = vec![crate::proto::Model::default(); 8];
            preset.chains[0].models[2].hash = Some(crate::proto::model::Hash::Hash(14_001));
            push_current_preset(&fake, &request, preset);
            wait_for_write(&fake, next, MessageType::Grid);
        });
        let capture = LibraryEntry {
            key: "b".repeat(64),
            name: "Existing Fictional Capture".into(),
        };

        assert_eq!(
            qc.set_capture(
                Row::from_wire(0),
                2,
                &capture,
                None,
                &[],
                Duration::from_secs(1),
            )
            .unwrap(),
            None
        );
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn ir_selection_uses_exact_slot_one_key_then_name() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (_, body) = wait_for_write(&fake, 0, MessageType::Grid);
            let placement: crate::proto::GridMessage =
                prost::Message::decode(body.as_slice()).unwrap();
            push_proto(&fake, MessageType::Grid, &placement);
        });
        let ir = LibraryEntry {
            key: "CIR_fictional_content_key".into(),
            name: "Fictional IR".into(),
        };

        qc.set_ir(
            Row::from_wire(2),
            6,
            &ir,
            1,
            Some(LAST_IR_LOADER_MODEL),
            Duration::from_secs(1),
        )
        .unwrap();
        responder.join().unwrap();

        let (next, placement) = wait_for_write(&link, 0, MessageType::Grid);
        assert_eq!(
            only_grid_model(&placement).hash,
            Some(crate::proto::model::Hash::Hash(29_008))
        );
        let (next, path) = wait_for_write(&link, next, MessageType::Grid);
        let path = only_grid_model(&path);
        assert_eq!(
            path.params[0].index,
            Some(crate::proto::param::Index::Index(10))
        );
        assert_eq!(
            path.params[0].param_values[0].value,
            Some(crate::proto::param_value::Value::StringValue(
                ir.key.clone()
            ))
        );
        let (_, name) = wait_for_write(&link, next, MessageType::Grid);
        let name = only_grid_model(&name);
        assert_eq!(
            name.params[0].index,
            Some(crate::proto::param::Index::Index(23))
        );
        assert_eq!(
            name.params[0].param_values[0].value,
            Some(crate::proto::param_value::Value::StringValue(
                ir.name.clone()
            ))
        );
        session.close();
    }

    #[test]
    fn capture_and_ir_inputs_are_refused_before_fake_link_io() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let valid_capture = LibraryEntry {
            key: "c".repeat(64),
            name: "Fictional Capture".into(),
        };
        let valid_ir = LibraryEntry {
            key: "CIR_fictional".into(),
            name: "Fictional IR".into(),
        };
        let malformed_capture = LibraryEntry {
            key: "not-a-content-hash".into(),
            name: "Fictional Capture".into(),
        };
        let path_ir = LibraryEntry {
            key: "/tmp/fictional.wav".into(),
            name: "Fictional IR".into(),
        };
        let reserved = [ParameterWrite {
            index: CAPTURE_FILE_NAME_PARAM,
            value: Value::Text("caller override".into()),
        }];

        for result in [
            qc.set_capture(
                Row::from_wire(0),
                0,
                &malformed_capture,
                Some(DEFAULT_CAPTURE_MODEL),
                &[],
                Duration::ZERO,
            ),
            qc.set_capture(
                Row::from_wire(0),
                0,
                &valid_capture,
                Some(DEFAULT_CAPTURE_MODEL),
                &reserved,
                Duration::ZERO,
            ),
            qc.set_capture(
                Row::from_wire(4),
                0,
                &valid_capture,
                Some(DEFAULT_CAPTURE_MODEL),
                &[],
                Duration::ZERO,
            ),
            qc.set_capture(
                Row::from_wire(0),
                0,
                &valid_capture,
                Some(13_999),
                &[],
                Duration::ZERO,
            ),
            qc.set_ir(
                Row::from_wire(0),
                0,
                &valid_ir,
                2,
                Some(FIRST_IR_LOADER_MODEL),
                Duration::ZERO,
            ),
            qc.set_ir(
                Row::from_wire(0),
                0,
                &path_ir,
                0,
                Some(FIRST_IR_LOADER_MODEL),
                Duration::ZERO,
            ),
            qc.set_ir(
                Row::from_wire(0),
                0,
                &valid_ir,
                0,
                Some(FIRST_IR_LOADER_MODEL - 1),
                Duration::ZERO,
            ),
        ] {
            assert!(matches!(
                result,
                Err(crate::Error::InvalidParameter(_) | crate::Error::InvalidRow(_))
            ));
        }
        assert!(link.written().is_empty());
        session.close();
    }

    #[test]
    fn failed_capture_placement_stops_before_selector_and_follow_up_writes() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (next, _) = wait_for_write(&fake, 0, MessageType::Grid);
            let (_, request) = wait_for_write(&fake, next, MessageType::RecallPreset);
            push_current_preset(&fake, &request, BinaryPreset::default());
        });
        let capture = LibraryEntry {
            key: "d".repeat(64),
            name: "Fictional Capture".into(),
        };
        let follow_up = [ParameterWrite {
            index: 4,
            value: Value::Normalised(0.56),
        }];

        assert!(matches!(
            qc.set_capture(
                Row::from_wire(0),
                0,
                &capture,
                Some(DEFAULT_CAPTURE_MODEL),
                &follow_up,
                Duration::from_millis(10),
            ),
            Err(crate::Error::BlockRefused(_))
        ));
        responder.join().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            link.write_count(),
            2,
            "selector or follow-up write was sent after failed placement"
        );
        let (_, first) = wait_for_write(&link, 0, MessageType::Grid);
        let first = only_grid_model(&first);
        assert!(
            first.params.is_empty(),
            "selector was sent after failed placement"
        );
        session.close();
    }

    #[test]
    fn stomp_assignment_sends_delete_then_update() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());

        qc.set_stomp_assignment(Row::from_wire(0), 0, Footswitch::A)
            .unwrap();
        let (next, body) = wait_for_write(&link, 0, MessageType::Grid);
        let delete: crate::proto::GridMessage = prost::Message::decode(body.as_slice()).unwrap();
        let (_, body) = wait_for_write(&link, next, MessageType::Grid);
        let update: crate::proto::GridMessage = prost::Message::decode(body.as_slice()).unwrap();

        assert_eq!(delete.action, MessageAction::Delete as i32);
        assert_eq!(update.action, MessageAction::Update as i32);
        let deleted = &crate::grid::preset_of(&delete)
            .unwrap()
            .stomp_mode_assignments[0];
        let assigned = &crate::grid::preset_of(&update)
            .unwrap()
            .stomp_mode_assignments[0];
        assert_eq!((deleted.row, deleted.column), (0, 0));
        assert_eq!(
            (assigned.row, assigned.column, assigned.stomp_index),
            (0, 0, 0)
        );

        session.close();
    }

    #[test]
    fn midi_output_uses_type_eight_and_exact_nested_fields() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let cc = MidiOut::cc(3, 10, 64).unwrap();

        qc.set_midi_out(MidiSource::FootswitchA, &[cc]).unwrap();
        let (next, body) = wait_for_write(&link, 0, MessageType::MidiSettings);
        let general: MidiSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!(general.action, MessageAction::Update as i32);
        assert!(general.preset_load_messages.is_none());
        let Some(crate::proto::midi_settings_message::GeneralMidiMessages::GeneralMidiMessages(
            group,
        )) = general.general_midi_messages
        else {
            panic!("general MIDI group missing");
        };
        assert_eq!(group.messages.len(), 1);
        assert_eq!(
            group.messages[0].source,
            Some(crate::proto::general_midi_message::Source::Source(0))
        );
        assert_eq!(
            group.messages[0].msg,
            vec![MidiMessageInfo {
                r#type: 1,
                channel: 3,
                param1: 10,
                param2: 64,
                param3: 0,
            }]
        );

        let pc = MidiOut::pc(5, 7, 1, 2).unwrap();
        qc.set_preset_load_midi_out(&[pc]).unwrap();
        let (_, body) = wait_for_write(&link, next, MessageType::MidiSettings);
        let load: MidiSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(load.general_midi_messages.is_none());
        let Some(crate::proto::midi_settings_message::PresetLoadMessages::PresetLoadMessages(
            group,
        )) = load.preset_load_messages
        else {
            panic!("preset-load MIDI group missing");
        };
        assert_eq!(
            group.messages[0].source,
            Some(crate::proto::general_midi_message::Source::Source(0))
        );
        assert_eq!(
            group.messages[0].msg[0],
            MidiMessageInfo {
                r#type: 3,
                channel: 5,
                param1: 1,
                param2: 2,
                param3: 7,
            }
        );

        session.close();
    }

    #[test]
    fn checked_control_types_and_midi_limits_fail_before_io() {
        assert!(Footswitch::try_from(8).is_err());
        assert!(ExpressionPedal::try_from(0).is_err());
        assert!(ExpressionPedal::try_from(3).is_err());
        assert!(MidiSource::try_from(10).is_err());
        assert!(MidiOut::cc(0, 0, 0).is_err());
        assert!(MidiOut::cc(17, 0, 0).is_err());
        assert!(MidiOut::cc(1, 128, 0).is_err());
        assert!(MidiOut::cc_toggle(1, 0, 0, 128).is_err());
        assert!(MidiOut::pc(1, 128, 0, 0).is_err());

        assert_eq!(
            MidiOut::cc(3, 10, 64).unwrap(),
            MidiOut {
                message_type: MidiOutType::ControlChange,
                channel: 3,
                param1: 10,
                param2: 64,
                param3: 0,
            }
        );
        assert_eq!(
            MidiOut::cc_toggle(4, 30, 5, 120).unwrap(),
            MidiOut {
                message_type: MidiOutType::ControlChangeToggle,
                channel: 4,
                param1: 30,
                param2: 5,
                param3: 120,
            }
        );
        assert_eq!(
            MidiOut::expression_cc(6, 40, 12, 13).unwrap(),
            MidiOut {
                message_type: MidiOutType::ControlChange,
                channel: 6,
                param1: 40,
                param2: 12,
                param3: 13,
            }
        );
        assert_eq!(
            MidiOut::pc(5, 7, 1, 2).unwrap(),
            MidiOut {
                message_type: MidiOutType::ProgramChange,
                channel: 5,
                param1: 1,
                param2: 2,
                param3: 7,
            }
        );

        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let allowed = vec![MidiOut::cc(1, 0, 0).unwrap(); 12];
        let messages = vec![MidiOut::cc(1, 0, 0).unwrap(); 13];
        assert!(build_midi_settings(0, &allowed, false).is_ok());
        assert!(qc.set_midi_out(MidiSource::Expression2, &messages).is_err());
        assert!(qc.set_preset_load_midi_out(&messages).is_err());
        assert!(link.written().is_empty());
        session.close();
    }

    #[test]
    fn row_level_scene_writes_promote_then_switch_then_write() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let mut next = 0;

        for operation in [
            qc.set_splitter_param(Row::from_wire(0), 3, 0.25, Some(1), true),
            qc.set_mixer_param(Row::from_wire(2), 0, 0.5, Some(2), true),
            qc.set_lane_output(Row::from_wire(1), 1, 0.75, Some(3), true),
            qc.set_input_gate(Row::from_wire(3), 1, 1.0, Some(4), true),
        ] {
            operation.unwrap();
            let (after, body) = wait_for_write(&link, next, MessageType::Grid);
            next = after;
            let promotion: crate::proto::GridMessage =
                prost::Message::decode(body.as_slice()).unwrap();
            let chain = &crate::grid::preset_of(&promotion).unwrap().chains[0];
            let parameter = chain
                .combined_splitter
                .first()
                .or_else(|| chain.mixer.first())
                .or_else(|| chain.output_control.first())
                .or_else(|| chain.input_control.first())
                .unwrap()
                .params
                .first()
                .unwrap();
            assert_eq!(
                parameter.scene_mode,
                Some(crate::proto::param::SceneMode::SceneMode(true))
            );
            assert!(parameter.param_values.is_empty());

            let (after, body) = wait_for_write(&link, next, MessageType::Scene);
            next = after;
            let switch: SceneMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert!(switch.selected_scene.is_some());

            let (after, body) = wait_for_write(&link, next, MessageType::Grid);
            next = after;
            let write: crate::proto::GridMessage = prost::Message::decode(body.as_slice()).unwrap();
            let chain = &crate::grid::preset_of(&write).unwrap().chains[0];
            let parameter = chain
                .combined_splitter
                .first()
                .or_else(|| chain.mixer.first())
                .or_else(|| chain.output_control.first())
                .or_else(|| chain.input_control.first())
                .unwrap()
                .params
                .first()
                .unwrap();
            assert!(parameter.scene_mode.is_none());
            assert_eq!(parameter.param_values.len(), 1);
        }

        qc.set_mixer_param(Row::from_wire(0), 0, 0.5, Some(5), false)
            .unwrap();
        let (after, _) = wait_for_write(&link, next, MessageType::Scene);
        let (_, body) = wait_for_write(&link, after, MessageType::Grid);
        let write: crate::proto::GridMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert!(
            crate::grid::preset_of(&write).unwrap().chains[0].mixer[0].params[0]
                .scene_mode
                .is_none()
        );

        session.close();
    }

    #[test]
    fn concurrent_scene_targeted_writes_remain_atomic_and_ordered() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = Arc::new(QuadCortex::new(session.clone()));
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for (scene, parameter) in [(1, 101), (2, 202)] {
            let qc = qc.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..100 {
                    qc.set_param_in_scene(
                        Row::from_wire(0),
                        0,
                        parameter,
                        Value::Normalised(0.5),
                        scene,
                        true,
                    )
                    .unwrap();
                }
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }

        let messages = link
            .written()
            .into_iter()
            .map(|report| {
                let frame = Frame::parse(&report).unwrap();
                assert!(frame.flags.is_first() && frame.flags.is_last());
                Message::parse(&frame.data).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(messages.len(), 600);
        for sequence in messages.chunks_exact(3) {
            assert_eq!(sequence[0].message_type, MessageType::Grid as u16);
            assert_eq!(sequence[1].message_type, MessageType::Scene as u16);
            assert_eq!(sequence[2].message_type, MessageType::Grid as u16);

            let promotion: crate::proto::GridMessage =
                prost::Message::decode(sequence[0].body.clone()).unwrap();
            let switch: SceneMessage = prost::Message::decode(sequence[1].body.clone()).unwrap();
            let write: crate::proto::GridMessage =
                prost::Message::decode(sequence[2].body.clone()).unwrap();
            let promoted_index =
                crate::grid::preset_of(&promotion).unwrap().chains[0].models[0].params[0].index;
            let written_index =
                crate::grid::preset_of(&write).unwrap().chains[0].models[0].params[0].index;
            assert_eq!(promoted_index, written_index);
            let expected_scene =
                if matches!(written_index, Some(crate::proto::param::Index::Index(101))) {
                    1
                } else {
                    assert!(matches!(
                        written_index,
                        Some(crate::proto::param::Index::Index(202))
                    ));
                    2
                };
            assert_eq!(
                switch.selected_scene,
                Some(crate::proto::scene_message::SelectedScene::SelectedScene(
                    expected_scene
                ))
            );
        }
        session.close();
    }

    #[test]
    fn invalid_row_level_writes_are_refused_before_fake_link_io() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());

        for result in [
            qc.set_splitter_param(Row::from_wire(1), 0, 0.5, None, false),
            qc.set_mixer_param(Row::from_wire(3), 0, 0.5, None, false),
            qc.set_split_mute(Row::from_wire(1), true),
            qc.set_lane_output(Row::from_wire(4), 0, 0.5, None, false),
            qc.set_input_gate(Row::from_wire(4), 0, 0.5, None, false),
        ] {
            assert!(matches!(result, Err(crate::Error::InvalidRow(_))));
        }
        for result in [
            qc.set_splitter_param(Row::from_wire(0), 0, f32::NAN, None, false),
            qc.set_mixer_param(Row::from_wire(0), 0, -0.1, None, false),
            qc.set_lane_output(Row::from_wire(0), 0, 1.1, None, false),
            qc.set_input_gate(Row::from_wire(0), 0, f32::INFINITY, None, false),
        ] {
            assert!(matches!(result, Err(crate::Error::InvalidParameter(_))));
        }
        assert!(matches!(
            qc.set_input_gate(Row::from_wire(0), 0, 0.5, Some(8), true),
            Err(crate::Error::InvalidScene(_))
        ));
        assert!(link.written().is_empty());

        session.close();
    }

    #[test]
    fn list_option_write_uses_the_current_presets_dynamic_cardinality() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let source = BinaryPreset {
            chains: vec![crate::proto::Chain {
                row: Some(crate::proto::chain::Row::Row(0)),
                models: vec![crate::proto::Model {
                    hash: Some(crate::proto::model::Hash::Hash(4242)),
                    column: Some(crate::proto::model::Column::Column(0)),
                    params: vec![crate::proto::Param {
                        index: Some(crate::proto::param::Index::Index(6)),
                        dynamic_steps: vec![
                            "Off".into(),
                            "Follow Input".into(),
                            "Fictional Block".into(),
                            "Input 2".into(),
                        ],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let written = qc
            .set_param_option(
                Row::from_wire(0),
                0,
                ParameterTarget::Index(6),
                "Input 2",
                &source,
                Duration::ZERO,
            )
            .unwrap();
        assert_eq!(written.index, 6);
        assert_eq!(written.value, Value::Normalised(1.0));
        let (_, body) = wait_for_write(&link, 0, MessageType::Grid);
        let message: crate::proto::GridMessage = prost::Message::decode(body.as_slice()).unwrap();
        let parameter = &crate::grid::preset_of(&message).unwrap().chains[0].models[0].params[0];
        assert_eq!(parameter.index, Some(crate::proto::param::Index::Index(6)));
        assert!(matches!(
            parameter.param_values[0].value,
            Some(crate::proto::param_value::Value::FloatValue(1.0))
        ));
        session.close();
    }

    #[test]
    fn list_option_parameter_name_uses_the_source_blocks_model() {
        let catalog = crate::Catalog::from_xml(
            r#"<Models><Category id="1" name="Fictional"><Model id="4242" name="Fictional Utility"><Parameter defaultValue="0" max="1" min="0" name="SOURCE" type="switch" units=""/></Model></Category></Models>"#,
        )
        .unwrap();
        let source = BinaryPreset {
            chains: vec![crate::proto::Chain {
                models: vec![crate::proto::Model {
                    hash: Some(crate::proto::model::Hash::Hash(4242)),
                    params: vec![crate::proto::Param {
                        dynamic_steps: vec!["Off".into(), "On".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let write = option_parameter_write(
            &source,
            Row::from_wire(0),
            0,
            &ParameterTarget::Name("source".into()),
            "On",
            Some(&catalog),
        )
        .unwrap();
        assert_eq!(
            write,
            ParameterWrite {
                index: 0,
                value: Value::Normalised(1.0)
            }
        );
    }

    #[test]
    fn tempo_option_enums_are_checked_and_use_exact_denominators() {
        assert_eq!(
            TempoSubdivision::try_from(0).unwrap(),
            TempoSubdivision::Quarter
        );
        assert_eq!(
            TempoSubdivision::try_from(3).unwrap(),
            TempoSubdivision::Sixteenth
        );
        assert!(TempoSubdivision::try_from(4).is_err());
        assert_eq!(
            MetronomeSound::try_from(5).unwrap(),
            MetronomeSound::SoftKit
        );
        assert!(MetronomeSound::try_from(6).is_err());
        assert_eq!(
            MetronomeRouting::try_from(4).unwrap(),
            MetronomeRouting::Send12
        );
        assert!(MetronomeRouting::try_from(5).is_err());
        assert_eq!(
            TimeSignature::try_from(20).unwrap(),
            TimeSignature::SevenEight223
        );
        assert!(TimeSignature::try_from(21).is_err());

        assert!((TempoSubdivision::Eighth.normalised() - 1.0 / 3.0).abs() < f32::EPSILON);
        assert!((MetronomeSound::Block.normalised() - 1.0 / 5.0).abs() < f32::EPSILON);
        assert!((MetronomeRouting::Out34.normalised() - 3.0 / 4.0).abs() < f32::EPSILON);
        assert!((TimeSignature::ThreeFour.normalised() - 1.0 / 20.0).abs() < f32::EPSILON);
        for last in [
            TempoSubdivision::Sixteenth.normalised(),
            MetronomeSound::SoftKit.normalised(),
            MetronomeRouting::Send12.normalised(),
            TimeSignature::SevenEight223.normalised(),
        ] {
            assert!((last - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn tempo_client_methods_send_expected_positional_writes() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());

        qc.set_tempo_param(
            TempoParameter::Pan,
            ParameterInput::Normalised(0.37),
            Duration::ZERO,
        )
        .unwrap();
        qc.set_tempo_option(TempoParameter::Routing, 3).unwrap();
        qc.set_tempo_subdivision(TempoSubdivision::Eighth).unwrap();
        qc.set_metronome_sound(MetronomeSound::Block).unwrap();
        qc.set_metronome_routing(MetronomeRouting::Out34).unwrap();
        qc.set_time_signature(TimeSignature::ThreeFour).unwrap();
        qc.set_tempo_led(true).unwrap();
        qc.set_metronome_volume(0.0).unwrap();

        let expected = [
            (TempoParameter::Pan.index(), 0.37),
            (TempoParameter::Routing.index(), 3.0 / 4.0),
            (TempoParameter::Subdivisions.index(), 1.0 / 3.0),
            (TempoParameter::Sound.index(), 1.0 / 5.0),
            (TempoParameter::Routing.index(), 3.0 / 4.0),
            (TempoParameter::TimeSignature.index(), 1.0 / 20.0),
            (TempoParameter::LedLight.index(), 1.0),
            (TempoParameter::Volume.index(), 0.0),
        ];
        let mut next = 0;
        for (index, value) in expected {
            let (after, body) = wait_for_write(&link, next, MessageType::Grid);
            next = after;
            let message: crate::proto::GridMessage =
                prost::Message::decode(body.as_slice()).unwrap();
            let preset = crate::grid::preset_of(&message).unwrap();
            assert!(preset.chains.is_empty());
            assert_eq!(preset.tempo_program_data.len(), 1);
            let tempo = &preset.tempo_program_data[0];
            assert_eq!(
                tempo.hash,
                Some(crate::proto::model::Hash::Hash(crate::grid::TEMPO_CONTROL))
            );
            assert!(tempo.column.is_none());
            assert_eq!(
                tempo.params[0].index,
                Some(crate::proto::param::Index::Index(index))
            );
            assert!(matches!(
                tempo.params[0].param_values[0].value,
                Some(crate::proto::param_value::Value::FloatValue(actual))
                    if (actual - value).abs() < f32::EPSILON
            ));
        }

        session.close();
    }

    #[test]
    fn invalid_tempo_inputs_are_refused_before_fake_link_io() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());

        for result in [
            qc.set_tempo_param(
                TempoParameter::Tempo,
                ParameterInput::Normalised(f32::NAN),
                Duration::ZERO,
            ),
            qc.set_tempo_param(
                TempoParameter::Tempo,
                ParameterInput::Normalised(-0.01),
                Duration::ZERO,
            ),
            qc.set_tempo_param(
                TempoParameter::Tempo,
                ParameterInput::Real(f64::INFINITY),
                Duration::ZERO,
            ),
            qc.set_tempo_param(
                TempoParameter::Tempo,
                ParameterInput::Text("invalid".into()),
                Duration::ZERO,
            ),
            qc.set_tempo_option(TempoParameter::Tempo, 0),
            qc.set_tempo_option(TempoParameter::Subdivisions, 4),
            qc.set_metronome_volume(1.01),
        ] {
            assert!(matches!(result, Err(crate::Error::InvalidParameter(_))));
        }
        assert!(link.written().is_empty());

        session.close();
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn state_reads_ignore_partial_pushes_and_encode_plain_reads() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let mut next = 0;

            let (after, body) = wait_for_write(&fake, next, MessageType::MasterVolume);
            next = after;
            let request: MasterVolumeMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::MasterVolume,
                &MasterVolumeMessage {
                    engaged: Some(crate::proto::master_volume_message::Engaged::Engaged(true)),
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::MasterVolume,
                &MasterVolumeMessage {
                    volume: Some(crate::proto::master_volume_message::Volume::Volume(0.5)),
                    ..Default::default()
                },
            );

            let (after, body) = wait_for_write(&fake, next, MessageType::Looper);
            next = after;
            let request: LooperMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::Looper,
                &LooperMessage {
                    state: Some(crate::proto::looper_message::State::State(2)),
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::Looper,
                &LooperMessage {
                    status: Some(crate::proto::looper_message::Status::Status(
                        crate::proto::LooperStatus::default(),
                    )),
                    ..Default::default()
                },
            );

            let (after, body) = wait_for_write(&fake, next, MessageType::Tuner);
            next = after;
            let request: TunerMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::Tuner,
                &TunerMessage {
                    frequency: Some(crate::proto::tuner_message::Frequency::Frequency(2.0)),
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::Tuner,
                &TunerMessage {
                    input_port_id: Some(crate::proto::tuner_message::InputPortId::InputPortId(1)),
                    frequency: Some(crate::proto::tuner_message::Frequency::Frequency(2.0)),
                    ..Default::default()
                },
            );

            let (after, body) = wait_for_write(&fake, next, MessageType::IoSettings);
            next = after;
            let request: IoSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::IoSettings,
                &IoSettingsMessage {
                    settings: Some(crate::proto::io_settings_message::Settings::Settings(
                        crate::proto::PortSettings::default(),
                    )),
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::IoSettings,
                &IoSettingsMessage {
                    settings: Some(crate::proto::io_settings_message::Settings::Settings(
                        crate::proto::PortSettings {
                            in_port: vec![crate::proto::InputPortSettings::default()],
                            ..Default::default()
                        },
                    )),
                    ..Default::default()
                },
            );

            let (after, body) = wait_for_write(&fake, next, MessageType::GeneralSettings);
            next = after;
            let request: GeneralSettingsMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::GeneralSettings,
                &GeneralSettingsMessage {
                    screen_brightness: Some(
                        crate::proto::general_settings_message::ScreenBrightness::ScreenBrightness(
                            50,
                        ),
                    ),
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::GeneralSettings,
                &GeneralSettingsMessage {
                    scene_block_bypass: Some(
                        crate::proto::general_settings_message::SceneBlockBypass::SceneBlockBypass(
                            0,
                        ),
                    ),
                    ..Default::default()
                },
            );

            let (_, body) = wait_for_write(&fake, next, MessageType::GlobalEq);
            let request: GlobalEqMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(&fake, MessageType::GlobalEq, &GlobalEqMessage::default());
            push_proto(
                &fake,
                MessageType::GlobalEq,
                &GlobalEqMessage {
                    bypassed: Some(crate::proto::global_eq_message::Bypassed::Bypassed(false)),
                    ..Default::default()
                },
            );
        });

        assert!(
            qc.master_volume(Duration::from_secs(1))
                .unwrap()
                .volume
                .is_some()
        );
        assert!(qc.looper(Duration::from_secs(1)).unwrap().status.is_some());
        let tuner = qc.tuner(Duration::from_secs(1)).unwrap();
        assert!(tuner.input_port_id.is_some());
        assert!(matches!(
            tuner.frequency,
            Some(crate::proto::tuner_message::Frequency::Frequency(value))
                if (value - 2.0).abs() < f32::EPSILON
        ));
        let io = qc.io_settings(Duration::from_secs(1)).unwrap();
        assert!(io.settings.is_some());
        assert!(
            qc.settings(Duration::from_secs(1))
                .unwrap()
                .scene_block_bypass
                .is_some()
        );
        assert!(
            qc.global_eq(Duration::from_secs(1))
                .unwrap()
                .bypassed
                .is_some()
        );

        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn mode_and_mode_cycle_require_distinct_field_presence() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (after_mode, body) = wait_for_write(&fake, 0, MessageType::Mode);
            let request: ModeMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::Mode,
                &ModeMessage {
                    available_modes: Some(
                        crate::proto::mode_message::AvailableModes::AvailableModes(
                            crate::proto::AvailableModes {
                                modes: vec![0, 1, 2],
                            },
                        ),
                    ),
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::Mode,
                &ModeMessage {
                    mode: Some(crate::proto::mode_message::Mode::Mode(1)),
                    ..Default::default()
                },
            );

            let (_, body) = wait_for_write(&fake, after_mode, MessageType::Mode);
            let request: ModeMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::Mode,
                &ModeMessage {
                    mode: Some(crate::proto::mode_message::Mode::Mode(2)),
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::Mode,
                &ModeMessage {
                    available_modes: Some(
                        crate::proto::mode_message::AvailableModes::AvailableModes(
                            crate::proto::AvailableModes { modes: vec![2, 0] },
                        ),
                    ),
                    ..Default::default()
                },
            );
        });

        assert!(matches!(
            qc.mode(Duration::from_secs(1)).unwrap().mode,
            Some(crate::proto::mode_message::Mode::Mode(1))
        ));
        assert_eq!(qc.mode_cycle(Duration::from_secs(1)).unwrap(), vec![2, 0]);

        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn recents_and_pinned_models_handle_their_distinct_empty_rules() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (after, body) = wait_for_write(&fake, 0, MessageType::RecentsFavorites);
            let request: RecentsFavoritesMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            assert!(!request.is_favorites);
            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage::default(),
            );
            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage {
                    items: vec![RecentsFavoritesItem {
                        name: "Fictional Recent".into(),
                        folder_key: USER_SETLIST.into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            );

            let (_, body) = wait_for_write(&fake, after, MessageType::PinnedModels);
            let request: PinnedModelsMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(request.action, MessageAction::Read as i32);
            push_proto(
                &fake,
                MessageType::PinnedModels,
                &PinnedModelsMessage {
                    request_id: request.request_id,
                    ..Default::default()
                },
            );
        });

        assert_eq!(qc.recents(Duration::from_secs(1)).unwrap().items.len(), 1);
        assert!(qc.pinned_models(Duration::from_secs(1)).unwrap().is_empty());

        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn favorites_retries_and_correlates_an_empty_reply_by_request_id() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (after_first, body) = wait_for_write(&fake, 0, MessageType::RecentsFavorites);
            let first: RecentsFavoritesMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(first.action, MessageAction::Read as i32);
            assert!(first.is_favorites);
            let Some(crate::proto::recents_favorites_message::RequestId::RequestId(first_id)) =
                first.request_id
            else {
                panic!("Favorites request needs an id")
            };

            let (_, body) = wait_for_write(&fake, after_first, MessageType::RecentsFavorites);
            let second: RecentsFavoritesMessage = prost::Message::decode(body.as_slice()).unwrap();
            let Some(crate::proto::recents_favorites_message::RequestId::RequestId(second_id)) =
                second.request_id
            else {
                panic!("Favorites retry needs an id")
            };
            assert_ne!(first_id, second_id);
            assert!(second.is_favorites);

            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage {
                    request_id: Some(
                        crate::proto::recents_favorites_message::RequestId::RequestId(first_id),
                    ),
                    items: vec![RecentsFavoritesItem {
                        name: "Wrong delayed list".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage {
                    request_id: Some(
                        crate::proto::recents_favorites_message::RequestId::RequestId(second_id),
                    ),
                    // The reply deliberately omits is_favorites and has no items.
                    ..Default::default()
                },
            );
        });

        assert!(
            qc.favorites(Duration::from_millis(400), 2)
                .unwrap()
                .is_empty()
        );
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn pin_mutations_send_one_id_with_operation_specific_actions() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());

        qc.pin_model(4006).unwrap();
        let (after_pin, body) = wait_for_write(&link, 0, MessageType::PinnedModels);
        assert_eq!(body, vec![0x1a, 0x02, 0xa6, 0x1f]);
        let pin: PinnedModelsMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!(pin.action, MessageAction::Create as i32);
        assert_eq!(pin.models, vec![4006]);
        assert!(pin.captures.is_empty());
        assert!(pin.request_id.is_none());

        qc.unpin_model(4006).unwrap();
        let (_, body) = wait_for_write(&link, after_pin, MessageType::PinnedModels);
        assert_eq!(body, vec![0x08, 0x02, 0x1a, 0x02, 0xa6, 0x1f]);
        let unpin: PinnedModelsMessage = prost::Message::decode(body.as_slice()).unwrap();
        assert_eq!(unpin.action, MessageAction::Delete as i32);
        assert_eq!(unpin.models, vec![4006]);
        assert!(unpin.captures.is_empty());
        assert!(unpin.request_id.is_none());

        session.close();
    }

    #[test]
    fn favorite_mutations_require_an_exact_same_operation_echo() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let target = RecentsFavoritesItem {
            name: "Fictional Favorite".into(),
            folder_key: USER_SETLIST.into(),
            folder_name: "My Presets".into(),
            is_factory: false,
            is_plugin: false,
        };
        let fake = link.clone();
        let expected = target.clone();
        let responder = std::thread::spawn(move || {
            let (after_add, body) = wait_for_write(&fake, 0, MessageType::RecentsFavorites);
            let add: RecentsFavoritesMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(add.action, MessageAction::Create as i32);
            assert!(add.is_favorites);
            assert_eq!(add.items.as_slice(), std::slice::from_ref(&expected));
            assert!(add.request_id.is_none());

            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage {
                    action: MessageAction::Delete as i32,
                    is_favorites: true,
                    items: vec![expected.clone()],
                    ..Default::default()
                },
            );
            let mut wrong_target = expected.clone();
            wrong_target.folder_key = "/fictional/wrong-folder".into();
            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage {
                    action: MessageAction::Create as i32,
                    is_favorites: true,
                    items: vec![wrong_target],
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage {
                    action: MessageAction::Create as i32,
                    is_favorites: true,
                    items: vec![expected.clone()],
                    ..Default::default()
                },
            );

            let (_, body) = wait_for_write(&fake, after_add, MessageType::RecentsFavorites);
            let remove: RecentsFavoritesMessage = prost::Message::decode(body.as_slice()).unwrap();
            assert_eq!(remove.action, MessageAction::Delete as i32);
            assert!(remove.is_favorites);
            assert_eq!(remove.items.as_slice(), std::slice::from_ref(&expected));
            assert!(remove.request_id.is_none());

            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage {
                    action: MessageAction::Delete as i32,
                    is_favorites: false,
                    items: vec![expected.clone()],
                    ..Default::default()
                },
            );
            push_proto(
                &fake,
                MessageType::RecentsFavorites,
                &RecentsFavoritesMessage {
                    action: MessageAction::Delete as i32,
                    is_favorites: true,
                    items: vec![expected],
                    ..Default::default()
                },
            );
        });

        qc.add_favorite(&target, Duration::from_secs(1)).unwrap();
        qc.remove_favorite(&target, Duration::from_secs(1)).unwrap();

        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn favorite_mutation_rejects_incomplete_metadata_without_io() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());

        let error = qc
            .add_favorite(&RecentsFavoritesItem::default(), Duration::from_millis(1))
            .unwrap_err();
        assert!(matches!(error, crate::Error::InvalidParameter(_)));
        assert!(link.written().is_empty());

        session.close();
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn variable_library_listings_require_request_id_and_folder_identity() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (after_ir, ir_request) = wait_for_file_write(&fake, 0);
            assert_eq!(ir_request.action, MessageAction::Read as i32);
            assert!(matches!(
                ir_request.r#type,
                Some(crate::proto::file_message::Type::Type(1))
            ));
            assert!(ir_request.folder.is_none());
            let Some(crate::proto::file_message::RequestId::RequestId(ir_id)) =
                ir_request.request_id
            else {
                panic!("IR request needs an id")
            };
            push_file(
                &fake,
                &FileMessage {
                    action: MessageAction::Update as i32,
                    request_id: Some(crate::proto::file_message::RequestId::RequestId(ir_id + 1)),
                    folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                        IR_LIBRARY,
                        vec![crate::proto::ProductData {
                            key: Some(crate::proto::product_data::Key::Key("wrong-id".into())),
                            name: Some(crate::proto::product_data::Name::Name("Wrong".into())),
                            ..Default::default()
                        }],
                    ))),
                    ..Default::default()
                },
            );
            push_file(
                &fake,
                &FileMessage {
                    action: MessageAction::Update as i32,
                    request_id: Some(crate::proto::file_message::RequestId::RequestId(ir_id)),
                    folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                        "another_ir_folder",
                        Vec::new(),
                    ))),
                    ..Default::default()
                },
            );
            push_file(
                &fake,
                &FileMessage {
                    action: MessageAction::Update as i32,
                    request_id: Some(crate::proto::file_message::RequestId::RequestId(ir_id)),
                    folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                        &format!("{IR_LIBRARY}/"),
                        Vec::new(),
                    ))),
                    ..Default::default()
                },
            );

            let (_, capture_request) = wait_for_file_write(&fake, after_ir);
            assert_eq!(capture_request.action, MessageAction::Read as i32);
            assert!(matches!(
                capture_request.r#type,
                Some(crate::proto::file_message::Type::Type(2))
            ));
            let Some(crate::proto::file_message::RequestId::RequestId(capture_id)) =
                capture_request.request_id
            else {
                panic!("capture request needs an id")
            };
            push_file(
                &fake,
                &FileMessage {
                    action: MessageAction::Update as i32,
                    request_id: Some(crate::proto::file_message::RequestId::RequestId(capture_id)),
                    folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                        CAPTURES_LIBRARY,
                        vec![
                            crate::proto::ProductData {
                                key: Some(crate::proto::product_data::Key::Key(
                                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                                        .into(),
                                )),
                                name: Some(crate::proto::product_data::Name::Name(
                                    "Fictional Capture".into(),
                                )),
                                ..Default::default()
                            },
                            // A keyless product cannot be loaded and is excluded.
                            crate::proto::ProductData {
                                name: Some(crate::proto::product_data::Name::Name(
                                    "Unusable".into(),
                                )),
                                ..Default::default()
                            },
                        ],
                    ))),
                    ..Default::default()
                },
            );
        });

        assert!(
            qc.list_irs(None, Duration::from_secs(1))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            qc.captures(Duration::from_secs(1)).unwrap(),
            vec![LibraryEntry {
                key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                name: "Fictional Capture".into(),
            }]
        );

        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn invalid_block_moves_are_refused_before_device_io() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let timeout = Duration::from_millis(10);

        for result in [
            qc.move_block(Row::from_wire(0), 8, Row::from_wire(0), 1, true, timeout),
            qc.move_block(Row::from_wire(0), 1, Row::from_wire(0), 1, true, timeout),
        ] {
            assert!(matches!(result, Err(crate::Error::InvalidBlockMove(_))));
        }
        assert_eq!(link.write_count(), 0);
        session.close();
    }

    #[test]
    fn model_state_excludes_the_cell_address() {
        let mut models = vec![crate::proto::Model::default(); 3];
        models[2] = crate::proto::Model {
            hash: Some(crate::proto::model::Hash::Hash(12_044)),
            column: Some(crate::proto::model::Column::Column(2)),
            ..Default::default()
        };
        let preset = BinaryPreset {
            chains: vec![crate::proto::Chain {
                models,
                ..Default::default()
            }],
            ..Default::default()
        };

        let state = model_state_at(&preset, Row::from_wire(0), 2).unwrap();
        assert_eq!(state.hash, Some(crate::proto::model::Hash::Hash(12_044)));
        assert_eq!(state.column, None);
    }

    #[test]
    fn setlist_listing_rejects_every_malformed_complete_shape() {
        let key = USER_SETLIST;
        let files = (0..i32::try_from(SETLIST_SLOTS).unwrap())
            .map(indexed_file)
            .collect::<Vec<_>>();
        let message = FileMessage {
            action: MessageAction::Update as i32,
            folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                key,
                files.clone(),
            ))),
            ..Default::default()
        };
        assert!(matches_setlist_listing(&message, key));

        let mutate_file = |index: usize, file: crate::proto::ProductData| {
            let mut malformed = message.clone();
            let Some(crate::proto::file_message::Folder::Folder(folder)) =
                malformed.folder.as_mut()
            else {
                unreachable!()
            };
            folder.files[index] = file;
            malformed
        };
        let mutate_folder = |mutator: fn(&mut crate::proto::FolderInfo)| {
            let mut malformed = message.clone();
            let Some(crate::proto::file_message::Folder::Folder(folder)) =
                malformed.folder.as_mut()
            else {
                unreachable!()
            };
            mutator(folder);
            malformed
        };

        let mut wrong_action = message.clone();
        wrong_action.action = MessageAction::Create as i32;
        let mut wrong_type = message.clone();
        wrong_type.r#type = Some(crate::proto::file_message::Type::Type(1));
        let mut no_folder = message.clone();
        no_folder.folder = None;
        let mut move_shaped = message.clone();
        move_shaped.to_folder = Some(crate::proto::file_message::ToFolder::ToFolder(file_folder(
            key,
            vec![indexed_file(1)],
        )));
        let missing_index = mutate_file(17, crate::proto::ProductData::default());
        let negative_index = mutate_file(17, indexed_file(-1));
        let index_256 = mutate_file(17, indexed_file(256));
        let duplicate = mutate_file(255, indexed_file(254));
        let duplicate_and_missing = mutate_file(255, crate::proto::ProductData::default());
        let wrong_key = mutate_folder(|folder| {
            folder.key = Some(crate::proto::folder_info::Key::Key(
                "/media/p4/Presets/Fictional Other".into(),
            ));
        });
        let short = mutate_folder(|folder| {
            folder.files.pop();
        });

        for (case, malformed) in [
            ("wrong action", wrong_action),
            ("wrong type", wrong_type),
            ("missing folder", no_folder),
            ("wrong folder", wrong_key),
            ("move-shaped folder pair", move_shaped),
            ("255 entries", short),
            ("entry with no index", missing_index),
            ("negative index", negative_index),
            ("index 256", index_256),
            ("duplicate index", duplicate),
            ("duplicate/malformed combination", duplicate_and_missing),
        ] {
            assert!(!matches_setlist_listing(&malformed, key), "accepted {case}");
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn concurrent_public_file_flows_ignore_wrong_targets_and_finish_in_reply_order() {
        const LIST: &str = "/media/p4/Presets/Fictional List";
        const SAVE: &str = "/media/p4/Presets/Fictional Save";
        const DELETE: &str = "/media/p4/Presets/Fictional Delete";
        const DELETE_NAME: &str = "Fictional Gone";

        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let barrier = Arc::new(std::sync::Barrier::new(4));

        let list_session = session.clone();
        let list_tx = done_tx.clone();
        let list_barrier = barrier.clone();
        let list = std::thread::spawn(move || {
            list_barrier.wait();
            QuadCortex::new(list_session)
                .list_presets(LIST, Duration::from_secs(2), false)
                .unwrap();
            list_tx.send("list").unwrap();
        });
        let save_session = session.clone();
        let save_tx = done_tx.clone();
        let save_barrier = barrier.clone();
        let save = std::thread::spawn(move || {
            save_barrier.wait();
            QuadCortex::new(save_session)
                .save_current_preset(
                    SAVE,
                    "1B",
                    Some("Fictional Stored"),
                    Instrument::Bass,
                    Duration::from_secs(2),
                )
                .unwrap();
            save_tx.send("save").unwrap();
        });
        let delete_session = session.clone();
        let delete_tx = done_tx;
        let delete_barrier = barrier.clone();
        let delete = std::thread::spawn(move || {
            delete_barrier.wait();
            QuadCortex::new(delete_session)
                .delete_preset(DELETE, DELETE_NAME, Duration::from_secs(2))
                .unwrap();
            delete_tx.send("delete").unwrap();
        });
        barrier.wait();

        let mut next = 0;
        let mut requests = Vec::new();
        for _ in 0..3 {
            let (after, request) = wait_for_file_write(&link, next);
            next = after;
            requests.push(request);
        }
        assert!(requests.iter().any(|request| {
            request.action == MessageAction::Read as i32
                && request.folder.as_ref().is_some_and(|folder| match folder {
                    crate::proto::file_message::Folder::Folder(folder) => {
                        folder_key(folder) == Some(LIST)
                    }
                })
        }));
        assert!(requests.iter().any(|request| {
            request.action == MessageAction::Read as i32
                && request.folder.as_ref().is_some_and(|folder| match folder {
                    crate::proto::file_message::Folder::Folder(folder) => {
                        folder_key(folder) == Some(SAVE)
                    }
                })
        }));
        assert!(requests.iter().any(|request| {
            request.action == MessageAction::Delete as i32
                && request.folder.as_ref().is_some_and(|folder| match folder {
                    crate::proto::file_message::Folder::Folder(folder) => {
                        folder_key(folder) == Some(DELETE)
                    }
                })
        }));

        push_file(&link, &complete_listing_for(SAVE, &[]));
        let (after_save_request, save_request) = wait_for_file_write(&link, next);
        next = after_save_request;
        assert_eq!(save_request.action, MessageAction::Create as i32);
        assert!(
            save_request
                .folder
                .as_ref()
                .is_some_and(|folder| match folder {
                    crate::proto::file_message::Folder::Folder(folder) =>
                        folder_key(folder) == Some(SAVE),
                })
        );

        push_file(
            &link,
            &complete_listing_for("/media/p4/Presets/Fictional Wrong", &[]),
        );
        push_file(
            &link,
            &FileMessage {
                action: MessageAction::Create as i32,
                folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                    SAVE,
                    vec![indexed_file(2)],
                ))),
                ..Default::default()
            },
        );
        push_file(
            &link,
            &FileMessage {
                action: MessageAction::Delete as i32,
                folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                    DELETE,
                    vec![crate::proto::ProductData {
                        key: Some(crate::proto::product_data::Key::Key(format!(
                            "{DELETE}/Fictional Wrong.pb"
                        ))),
                        ..Default::default()
                    }],
                ))),
                ..Default::default()
            },
        );
        assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());

        let delete_key = format!("{DELETE}/{DELETE_NAME}.pb");
        push_file(
            &link,
            &FileMessage {
                action: MessageAction::Delete as i32,
                folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                    DELETE,
                    vec![crate::proto::ProductData {
                        key: Some(crate::proto::product_data::Key::Key(delete_key)),
                        ..Default::default()
                    }],
                ))),
                ..Default::default()
            },
        );
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "delete"
        );

        push_file(&link, &complete_listing_for(LIST, &[]));
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "list"
        );

        push_file(
            &link,
            &FileMessage {
                action: MessageAction::Create as i32,
                folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                    SAVE,
                    vec![indexed_file(1)],
                ))),
                ..Default::default()
            },
        );
        let (_, final_listing_request) = wait_for_file_write(&link, next);
        assert_eq!(final_listing_request.action, MessageAction::Read as i32);
        push_file(
            &link,
            &complete_listing_for(SAVE, &[(1, "Fictional Stored", Instrument::Bass)]),
        );
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "save"
        );

        list.join().unwrap();
        save.join().unwrap();
        delete.join().unwrap();
        session.close();
    }

    #[test]
    fn save_polling_ignores_stale_metadata_until_the_listing_converges() {
        const SETLIST: &str = "/media/p4/Presets/Fictional Save Poll";
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (after_baseline, _) = wait_for_file_write(&fake, 0);
            push_file(
                &fake,
                &complete_listing_for(SETLIST, &[(0, "Fictional Old", Instrument::Bass)]),
            );
            let (after_save, save) = wait_for_file_write(&fake, after_baseline);
            assert_eq!(save.action, MessageAction::Create as i32);
            push_file(
                &fake,
                &FileMessage {
                    action: MessageAction::Create as i32,
                    folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                        SETLIST,
                        vec![indexed_file(0)],
                    ))),
                    ..Default::default()
                },
            );
            let (after_stale_request, _) = wait_for_file_write(&fake, after_save);
            push_file(
                &fake,
                &complete_listing_for(SETLIST, &[(0, "Fictional Old", Instrument::Bass)]),
            );
            let (_, _) = wait_for_file_write(&fake, after_stale_request);
            push_file(
                &fake,
                &complete_listing_for(SETLIST, &[(0, "Fictional New", Instrument::Bass)]),
            );
        });

        let stored = qc
            .save_current_preset(
                SETLIST,
                "1A",
                Some("Fictional New"),
                Instrument::Bass,
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(stored.name, "Fictional New");
        assert_eq!(
            link.write_count(),
            4,
            "baseline, save, stale poll, converged poll"
        );
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn identical_file_acknowledgements_have_no_correlation_evidence() {
        let save = FileMessage {
            action: MessageAction::Create as i32,
            folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                USER_SETLIST,
                vec![indexed_file(8)],
            ))),
            ..Default::default()
        };
        let target = format!("{USER_SETLIST}/Fictional Preset.pb");
        let delete = FileMessage {
            action: MessageAction::Delete as i32,
            folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                USER_SETLIST,
                vec![crate::proto::ProductData {
                    key: Some(crate::proto::product_data::Key::Key(target.clone())),
                    ..Default::default()
                }],
            ))),
            ..Default::default()
        };

        for acknowledgement in [&save, &save.clone()] {
            assert!(matches_save_ack(acknowledgement, USER_SETLIST, 8));
            assert!(acknowledgement.request_id.is_none());
        }
        for acknowledgement in [&delete, &delete.clone()] {
            assert!(matches_delete_ack(acknowledgement, USER_SETLIST, &target));
            assert!(acknowledgement.request_id.is_none());
        }
    }

    #[test]
    fn move_wire_shape_carries_no_listing_epoch_or_compare_and_swap_guard() {
        let source = format!("{USER_SETLIST}/Fictional Source.pb");
        let message = build_move_preset(USER_SETLIST, &source, 9);

        assert!(message.request_id.is_none());
        assert!(message.total_bulk_create_count.is_none());
        assert!(message.preset_payload.is_none());
        assert_eq!(message.action, MessageAction::Move as i32);
    }

    #[test]
    fn file_mutation_acknowledgements_require_the_exact_operation_and_target() {
        let key = USER_SETLIST;
        let save = FileMessage {
            action: MessageAction::Create as i32,
            folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                key,
                vec![indexed_file(8)],
            ))),
            ..Default::default()
        };
        assert!(matches_save_ack(&save, key, 8));
        assert!(!matches_save_ack(&save, key, 9));
        assert!(!matches_delete_ack(&save, key, "anything"));

        let target = format!("{key}/Fictional Preset.pb");
        let delete = FileMessage {
            action: MessageAction::Delete as i32,
            folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                key,
                vec![crate::proto::ProductData {
                    key: Some(crate::proto::product_data::Key::Key(target.clone())),
                    ..Default::default()
                }],
            ))),
            ..Default::default()
        };
        assert!(matches_delete_ack(&delete, key, &target));
        assert!(!matches_delete_ack(&delete, key, "wrong.pb"));
        assert!(!matches_save_ack(&delete, key, 8));

        let move_message = build_move_preset(key, &target, 9);
        assert_eq!(move_message.action, MessageAction::Move as i32);
        let Some(crate::proto::file_message::Folder::Folder(source)) = move_message.folder.as_ref()
        else {
            panic!("move needs a source folder")
        };
        let Some(crate::proto::file_message::ToFolder::ToFolder(destination)) =
            move_message.to_folder.as_ref()
        else {
            panic!("move needs a destination folder")
        };
        assert_eq!(folder_key(source), Some(key));
        assert_eq!(folder_key(destination), Some(key));
        assert!(matches!(
            source.files[0].key.as_ref(),
            Some(crate::proto::product_data::Key::Key(value)) if value == &target
        ));
        assert!(matches!(
            destination.files[0].index,
            Some(crate::proto::product_data::Index::Index(9))
        ));
        assert!(!matches_delete_ack(&move_message, key, &target));
    }

    #[test]
    fn instruments_are_the_exact_closed_wire_enumeration() {
        let values = [
            Instrument::None,
            Instrument::Guitar,
            Instrument::Bass,
            Instrument::Synth,
            Instrument::Vocal,
            Instrument::Other,
        ];
        assert_eq!(values.map(|value| value as i32), [0, 1, 2, 3, 4, 5]);
        for (wire, value) in values.into_iter().enumerate() {
            assert_eq!(
                Instrument::try_from(i32::try_from(wire).unwrap()).unwrap(),
                value
            );
        }
        assert!(Instrument::try_from(6).is_err());
        assert_eq!(
            serde_json::to_string(&Instrument::Synth).unwrap(),
            "\"synth\""
        );
    }

    #[test]
    fn stored_save_names_accept_only_the_request_or_its_collision_suffix() {
        assert!(stored_name_matches_request("Fictional", "Fictional"));
        assert!(stored_name_matches_request("Fictional", "Fictional_1"));
        assert!(stored_name_matches_request(
            "Fictional Very Long Name",
            "Fictional Very Lo_12"
        ));
        assert!(!stored_name_matches_request("Fictional", "Other_1"));
        assert!(!stored_name_matches_request("Fictional", "Fiction_1"));
        assert!(!stored_name_matches_request(
            "Fictional",
            "Fictional_suffix"
        ));
        assert!(!stored_name_matches_request("Fictional", "Fictional_0"));
    }

    #[test]
    fn setlist_messages_stay_under_the_user_root_and_protect_special_folders() {
        let create = build_create_setlist("Fictional Temp").unwrap();
        let Some(crate::proto::file_message::Folder::Folder(folder)) = create.folder else {
            panic!("create needs a folder")
        };
        assert_eq!(
            folder_key(&folder),
            Some("/media/p4/Presets/Fictional Temp")
        );
        assert!(matches!(
            folder.name,
            Some(crate::proto::folder_info::Name::Name(ref name)) if name == "Fictional Temp"
        ));
        assert_eq!(create.action, MessageAction::Create as i32);

        let delete = build_delete_setlist("Fictional Temp").unwrap();
        assert_eq!(delete.action, MessageAction::Delete as i32);
        for unsafe_name in [
            "",
            ".",
            "..",
            "Nested/Name",
            "Nested\\Name",
            "/opt/neuraldsp/Factory Library",
        ] {
            assert!(
                build_create_setlist(unsafe_name).is_err(),
                "accepted {unsafe_name:?}"
            );
            assert!(
                build_delete_setlist(unsafe_name).is_err(),
                "accepted {unsafe_name:?}"
            );
        }
        assert!(build_delete_setlist("My Presets").is_err());
    }

    #[test]
    fn protected_setlist_deletions_are_refused_before_fake_link_io() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        for name in ["My Presets", "/", "/opt/neuraldsp/Factory Library"] {
            assert!(qc.delete_setlist(name, Duration::from_millis(20)).is_err());
        }
        assert_eq!(link.write_count(), 0);
        session.close();
    }

    #[test]
    fn duplicate_receipt_never_calls_partial_progress_complete() {
        let receipt = DuplicateSetlistReceipt {
            destination: Folder {
                key: "/media/p4/Presets/Fictional Partial".into(),
                name: "Fictional Partial".into(),
                slots: 256,
                occupied: 1,
                is_factory: false,
            },
            selected: 2,
            copied: Vec::new(),
            failure: Some("fictional failure after creation".into()),
        };
        assert!(!receipt.complete());
        let json = serde_json::to_value(&receipt).unwrap();
        assert_eq!(json["destination"]["key"], receipt.destination.key);
        assert!(json["failure"].as_str().unwrap().contains("after creation"));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn copy_prepares_destination_before_source_recall_and_reads_actual_metadata() {
        const DESTINATION: &str = "/media/p4/Presets/Fictional Destination";
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        seed_live_cache(&session);
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (after_target_list, target_list) = wait_for_file_write(&fake, 0);
            assert_eq!(
                folder_key(match target_list.folder.as_ref().unwrap() {
                    crate::proto::file_message::Folder::Folder(folder) => folder,
                }),
                Some(DESTINATION)
            );
            push_file(&fake, &complete_listing_for(DESTINATION, &[]));

            let (after_target_recall, target_recall) =
                wait_for_write(&fake, after_target_list, MessageType::SetlistPosition);
            let target: SetlistPositionMessage =
                prost::Message::decode(target_recall.as_slice()).unwrap();
            assert!(matches!(
                target.folder_key,
                Some(crate::proto::setlist_position_message::FolderKey::FolderKey(ref key)) if key == DESTINATION
            ));
            answer_stored_recall(
                &fake,
                &target_recall,
                BinaryPreset {
                    chains: vec![crate::proto::Chain::default(); 4],
                    ..Default::default()
                },
            );

            let (after_source_recall, source_recall) =
                wait_for_write(&fake, after_target_recall, MessageType::SetlistPosition);
            let source: SetlistPositionMessage =
                prost::Message::decode(source_recall.as_slice()).unwrap();
            assert!(matches!(
                source.folder_key,
                Some(crate::proto::setlist_position_message::FolderKey::FolderKey(ref key)) if key == USER_SETLIST
            ));
            answer_stored_recall(
                &fake,
                &source_recall,
                BinaryPreset {
                    name: Some(crate::proto::binary_preset::Name::Name(
                        "Fictional Source".into(),
                    )),
                    chains: vec![crate::proto::Chain::default(); 4],
                    ..Default::default()
                },
            );

            let (after_revalidation, _) = wait_for_file_write(&fake, after_source_recall);
            push_file(&fake, &complete_listing_for(DESTINATION, &[]));
            let (after_save, save) = wait_for_file_write(&fake, after_revalidation);
            let Some(crate::proto::file_message::Folder::Folder(folder)) = save.folder.as_ref()
            else {
                panic!("save needs folder")
            };
            assert_eq!(save.action, MessageAction::Create as i32);
            assert!(matches!(
                folder.files[0].instrument,
                Some(crate::proto::product_data::Instrument::Instrument(2))
            ));
            push_file(
                &fake,
                &FileMessage {
                    action: MessageAction::Create as i32,
                    folder: Some(crate::proto::file_message::Folder::Folder(file_folder(
                        DESTINATION,
                        vec![indexed_file(1)],
                    ))),
                    ..Default::default()
                },
            );
            let (_, _) = wait_for_file_write(&fake, after_save);
            push_file(
                &fake,
                &complete_listing_for(DESTINATION, &[(1, "Fictional Source_1", Instrument::Bass)]),
            );
        });
        let policy = crate::SavePolicy::new(
            DESTINATION,
            vec![crate::ScratchRange::new("1B", "1B").unwrap()],
        )
        .unwrap();
        let receipt = qc
            .copy_preset(
                &policy,
                USER_SETLIST,
                "6A",
                DESTINATION,
                "1B",
                None,
                Instrument::Bass,
                crate::RecallConsent::DiscardWorkingCopy,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(receipt.stored.name, "Fictional Source_1");
        assert_eq!(receipt.stored.instrument, Some(Instrument::Bass));
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn copy_requires_caller_consent_before_recalling_a_dirty_working_copy() {
        const DESTINATION: &str = "/media/p4/Presets/Fictional Destination";
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        seed_live_cache(&session);
        let state = session.state_cache();
        let generation = state.status().generation;
        state.observe(
            generation,
            MessageType::PresetDirty,
            &prost::Message::encode_to_vec(&crate::proto::PresetDirtyMessage {
                action: MessageAction::Update as i32,
                is_dirty: true,
                ..Default::default()
            }),
        );
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (_, request) = wait_for_file_write(&fake, 0);
            assert_eq!(request.action, MessageAction::Read as i32);
            push_file(&fake, &complete_listing_for(DESTINATION, &[]));
        });
        let policy = crate::SavePolicy::new(
            DESTINATION,
            vec![crate::ScratchRange::new("1A", "1A").unwrap()],
        )
        .unwrap();

        let error = qc
            .copy_preset(
                &policy,
                USER_SETLIST,
                "6A",
                DESTINATION,
                "1A",
                None,
                Instrument::Guitar,
                crate::RecallConsent::RequireClean,
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(matches!(error, crate::Error::UnsafeSave(_)));
        responder.join().unwrap();
        assert_eq!(link.write_count(), 1, "copy must stop before either recall");
        session.close();
    }

    #[test]
    fn move_requires_an_occupied_source_and_an_empty_destination() {
        let policy = crate::SavePolicy::new(
            USER_SETLIST,
            vec![crate::ScratchRange::new("2A", "2C").unwrap()],
        )
        .unwrap();
        let source_key = format!("{USER_SETLIST}/Fictional Source.pb");
        let entries = vec![
            PresetEntry {
                index: 8,
                name: "Fictional Source".into(),
                key: Some(source_key.clone()),
                instrument: None,
            },
            PresetEntry {
                index: 10,
                name: "Occupied Target".into(),
                key: Some(format!("{USER_SETLIST}/Occupied Target.pb")),
                instrument: None,
            },
        ];

        assert_eq!(
            move_source(&policy, USER_SETLIST, &entries, 8, 9).unwrap(),
            (source_key, "Fictional Source".into())
        );
        assert!(matches!(
            move_source(&policy, USER_SETLIST, &entries, 7, 9),
            Err(crate::Error::NotFound(_))
        ));
        assert!(matches!(
            move_source(&policy, USER_SETLIST, &entries, 8, 8),
            Err(crate::Error::UnsafeMove(_))
        ));
        assert!(matches!(
            move_source(&policy, USER_SETLIST, &entries, 8, 10),
            Err(crate::Error::UnsafeMove(_))
        ));
        assert!(matches!(
            move_source(&policy, USER_SETLIST, &entries, 8, 11),
            Err(crate::Error::UnsafeMove(_))
        ));

        let moved = vec![PresetEntry {
            index: 9,
            name: "Fictional Source".into(),
            key: None,
            instrument: None,
        }];
        assert!(move_converged(&moved, "fictional source", 8, 9));
        assert!(!move_converged(&entries, "Fictional Source", 8, 9));
    }

    #[test]
    fn move_public_flow_lists_sends_then_waits_for_convergence() {
        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (after_listing, listing) = wait_for_file_write(&fake, 0);
            assert_eq!(listing.action, MessageAction::Read as i32);
            push_file(&fake, &complete_listing(&[(8, "Fictional Source")]));
            let (after_move, sent) = wait_for_file_write(&fake, after_listing);
            assert_eq!(sent.action, MessageAction::Move as i32);
            let (_, convergence_read) = wait_for_file_write(&fake, after_move);
            assert_eq!(convergence_read.action, MessageAction::Read as i32);
            push_file(&fake, &complete_listing(&[(9, "Fictional Source")]));
        });
        let policy = crate::SavePolicy::new(
            USER_SETLIST,
            vec![crate::ScratchRange::new("2A", "2B").unwrap()],
        )
        .unwrap();

        qc.move_preset(&policy, USER_SETLIST, "2A", "2B", Duration::from_secs(1))
            .unwrap();
        assert_eq!(qc.state_cache().status().storage_revision, 1);
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn slot_to_position_round_trips() {
        assert_eq!(slot_to_position("1A"), 0);
        assert_eq!(slot_to_position("1H"), 7);
        assert_eq!(slot_to_position("28C"), 218);
        assert_eq!(slot_to_position("32H"), 255);
    }

    #[test]
    fn position_to_slot_round_trips() {
        assert_eq!(position_to_slot(0), "1A");
        assert_eq!(position_to_slot(7), "1H");
        assert_eq!(position_to_slot(218), "28C");
        assert_eq!(position_to_slot(255), "32H");
    }

    #[test]
    fn slot_to_position_rejects_invalid() {
        assert_eq!(slot_to_position_checked(""), None);
        assert_eq!(slot_to_position_checked("X"), None);
        assert_eq!(slot_to_position_checked("0A"), None);
        assert_eq!(slot_to_position_checked("33A"), None);
        assert_eq!(slot_to_position_checked("1I"), None);
        assert_eq!(slot_to_position_checked("1Z"), None);
    }

    #[test]
    fn input_level_db_converts_correctly() {
        // 0 dB is exactly 1/6.
        assert!((input_level_db(1.0 / 6.0) - 0.0).abs() < 0.01);
        // Minimum: -12 dB at level 0.
        assert!((input_level_db(0.0) - (-12.0)).abs() < 0.01);
        // Maximum: +60 dB at level 1.
        assert!((input_level_db(1.0) - 60.0).abs() < 0.01);
    }

    #[test]
    fn db_to_input_level_inverts() {
        for db in [-12.0, 0.0, 17.2, 60.0] {
            let level = db_to_input_level(db).unwrap();
            assert!((input_level_db(level) - db).abs() < 0.01, "{db} dB");
        }
    }

    #[test]
    fn db_to_input_level_rejects_out_of_range() {
        assert!(db_to_input_level(-13.0).is_err());
        assert!(db_to_input_level(61.0).is_err());
    }

    #[test]
    fn normalised_parameter_values_are_checked_before_the_wire() {
        assert_eq!(normalised_value(0.0).unwrap(), Value::Normalised(0.0));
        assert_eq!(normalised_value(1.0).unwrap(), Value::Normalised(1.0));
        for invalid in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
            assert!(
                matches!(
                    normalised_value(invalid),
                    Err(crate::Error::InvalidParameter(_))
                ),
                "{invalid:?} should be refused"
            );
        }
    }

    fn parameter(kind: ParameterKind) -> Parameter {
        Parameter {
            index: 0,
            name: "TEST".into(),
            kind,
            min: 0.0,
            max: 10.0,
            default: 5.0,
            units: String::new(),
            step_names: Vec::new(),
        }
    }

    #[test]
    fn named_parameter_values_match_the_catalog_type_before_the_wire() {
        for kind in [
            ParameterKind::Float,
            ParameterKind::Int,
            ParameterKind::Switch,
            ParameterKind::Fader,
        ] {
            assert_eq!(
                parameter_value(&parameter(kind), ParameterInput::Normalised(0.5)).unwrap(),
                Value::Normalised(0.5)
            );
        }

        let numeric = parameter(ParameterKind::Float);
        assert_eq!(
            parameter_value(&numeric, ParameterInput::Real(5.0)).unwrap(),
            Value::Normalised(0.5)
        );
        assert!(matches!(
            parameter_value(&numeric, ParameterInput::Text("wrong".into())),
            Err(crate::Error::InvalidParameter(_))
        ));

        let text = parameter(ParameterKind::Str);
        assert_eq!(
            parameter_value(&text, ParameterInput::Text("right".into())).unwrap(),
            Value::Text("right".into())
        );
        assert!(matches!(
            parameter_value(&text, ParameterInput::Normalised(0.5)),
            Err(crate::Error::InvalidParameter(_))
        ));

        for kind in [
            ParameterKind::Meter,
            ParameterKind::Empty,
            ParameterKind::Unknown,
        ] {
            assert!(matches!(
                parameter_value(&parameter(kind), ParameterInput::Normalised(0.5)),
                Err(crate::Error::InvalidParameter(_))
            ));
        }
    }

    // -- listing decode ----------------------------------------------------

    use crate::proto::{FolderInfo, ProductData, folder_info, product_data};

    fn product(index: i32, name: &str) -> ProductData {
        ProductData {
            index: Some(product_data::Index::Index(index)),
            name: Some(product_data::Name::Name(name.into())),
            ..Default::default()
        }
    }

    #[test]
    fn preset_entry_reads_an_occupied_slot() {
        let entry = PresetEntry::from_proto(&product(218, "Plexi Sunrise"))
            .unwrap()
            .unwrap();
        assert_eq!(entry.index, 218);
        assert_eq!(entry.name, "Plexi Sunrise");
        assert_eq!(position_to_slot(entry.index), "28C");
    }

    #[test]
    fn preset_entry_rejects_empty_slots() {
        // The device always reports a setlist's full 256 slots; an empty one
        // is signalled by an absent or blank name, NOT by omitting the entry.
        assert!(PresetEntry::from_proto(&product(5, "")).unwrap().is_none());
        let nameless = ProductData {
            index: Some(product_data::Index::Index(5)),
            ..Default::default()
        };
        assert!(PresetEntry::from_proto(&nameless).unwrap().is_none());
    }

    #[test]
    fn preset_entry_rejects_entry_without_index() {
        let no_index = ProductData {
            name: Some(product_data::Name::Name("Orphan".into())),
            ..Default::default()
        };
        assert!(PresetEntry::from_proto(&no_index).unwrap().is_none());
    }

    #[test]
    fn preset_entry_rejects_an_unknown_instrument_value() {
        let mut entry = product(5, "Fictional Future Instrument");
        entry.instrument = Some(product_data::Instrument::Instrument(99));
        assert!(matches!(
            PresetEntry::from_proto(&entry),
            Err(crate::Error::Decode(_))
        ));
    }

    #[test]
    fn folder_key_normalises_trailing_slash() {
        // The trailing-slash asymmetry: recalls need the factory path WITH a
        // slash, but the device reports that folder's listing key WITHOUT one.
        let with_slash = FolderInfo {
            key: Some(folder_info::Key::Key("/media/p4/Factory/".into())),
            ..Default::default()
        };
        let without = FolderInfo {
            key: Some(folder_info::Key::Key("/media/p4/Factory".into())),
            ..Default::default()
        };
        assert_eq!(folder_key(&with_slash), folder_key(&without));
        assert_eq!(folder_key(&with_slash), Some("/media/p4/Factory"));
    }

    #[test]
    fn folder_without_key_is_unusable() {
        assert!(Folder::from_proto(&FolderInfo::default()).is_none());
    }

    #[test]
    fn folder_counts_only_occupied_slots() {
        let info = FolderInfo {
            key: Some(folder_info::Key::Key(USER_SETLIST.into())),
            name: Some(folder_info::Name::Name("My Presets".into())),
            files: vec![
                product(0, "One"),
                product(1, ""),
                product(2, "Three"),
                product(3, ""),
            ],
            ..Default::default()
        };
        let folder = Folder::from_proto(&info).unwrap();
        assert_eq!(folder.slots, 4);
        assert_eq!(folder.occupied, 2);
        assert!(!folder.is_factory);
    }

    // -- recall payload ----------------------------------------------------

    #[test]
    fn build_recall_rejects_a_malformed_slot() {
        // A bad slot name from a CLI argument must be an error, not a panic.
        assert!(matches!(
            build_recall(USER_SETLIST, "99Z", false, None),
            Err(crate::Error::InvalidSlot(_))
        ));
    }

    #[test]
    fn build_recall_encodes_the_linear_position() {
        use crate::proto::setlist_position_message as spm;
        let payload = build_recall(USER_SETLIST, "28C", false, Some(7)).unwrap();
        let decoded: SetlistPositionMessage = prost::Message::decode(payload.as_slice()).unwrap();
        assert_eq!(decoded.action, MessageAction::Update as i32);
        assert_eq!(decoded.position, Some(spm::Position::Position(218)));
        assert_eq!(decoded.request_id, Some(spm::RequestId::RequestId(7)));
        assert_eq!(decoded.is_factory, Some(spm::IsFactory::IsFactory(false)));
    }

    #[test]
    fn build_recall_omits_request_id_when_untagged() {
        let payload = build_recall(USER_SETLIST, "1A", false, None).unwrap();
        let decoded: SetlistPositionMessage = prost::Message::decode(payload.as_slice()).unwrap();
        assert_eq!(decoded.request_id, None);
    }

    // -- grid echo matching -------------------------------------------------

    use crate::proto::{Chain, GridMessage, Model, chain, grid_message, model};

    /// Build a `Grid` broadcast as the device would send it. `keyed` controls
    /// whether row and column carry explicit values or rely on position.
    fn grid_echo(row: u32, column: u32, model_id: u32, keyed: bool) -> InboundMessage {
        let mut m = Model {
            hash: Some(model::Hash::Hash(model_id)),
            ..Default::default()
        };
        let mut c = Chain::default();
        if keyed {
            m.column = Some(model::Column::Column(column));
            c.row = Some(chain::Row::Row(row));
            c.models = vec![m];
        } else {
            // Positional: pad so the element sits at its index.
            c.models = (0..=column)
                .map(|i| {
                    if i == column {
                        m.clone()
                    } else {
                        Model::default()
                    }
                })
                .collect();
        }
        let chains = if keyed {
            vec![c]
        } else {
            (0..=row)
                .map(|i| {
                    if i == row {
                        c.clone()
                    } else {
                        Chain::default()
                    }
                })
                .collect()
        };
        let message = GridMessage {
            action: MessageAction::Update as i32,
            request_id: None,
            preset: Some(grid_message::Preset::Preset(crate::proto::BinaryPreset {
                chains,
                ..Default::default()
            })),
            update_type: None,
        };
        InboundMessage {
            message_type: MessageType::Grid,
            body: bytes::Bytes::from(prost::Message::encode_to_vec(&message)),
            request_id: None,
        }
    }

    #[test]
    fn an_echo_naming_the_cell_confirms_the_placement() {
        assert!(grid_echoes_cell(&grid_echo(2, 3, 1001, true), 2, 3, 1001));
    }

    #[test]
    fn an_echo_without_presence_is_matched_positionally() {
        // Row and column may arrive WITHOUT presence, in which case position
        // in the repeated field is the index. Treating an absent field as
        // "no match" would report a working placement as BlockRefused.
        assert!(grid_echoes_cell(&grid_echo(2, 3, 1001, false), 2, 3, 1001));
    }

    #[test]
    fn an_echo_for_another_cell_does_not_confirm() {
        // A false positive here reports a DSP-refused block as placed.
        let echo = grid_echo(2, 3, 1001, true);
        assert!(!grid_echoes_cell(&echo, 1, 3, 1001), "wrong row matched");
        assert!(!grid_echoes_cell(&echo, 2, 4, 1001), "wrong column matched");
        assert!(!grid_echoes_cell(&echo, 2, 3, 9999), "wrong model matched");
    }

    #[test]
    fn a_message_that_is_not_a_grid_echo_does_not_confirm() {
        let not_a_grid = InboundMessage {
            message_type: MessageType::Grid,
            body: bytes::Bytes::from_static(b"\xff\xff\xff\xff"),
            request_id: None,
        };
        assert!(!grid_echoes_cell(&not_a_grid, 0, 0, 1));

        let empty = GridMessage {
            action: MessageAction::Update as i32,
            request_id: None,
            preset: None,
            update_type: None,
        };
        let no_preset = InboundMessage {
            message_type: MessageType::Grid,
            body: bytes::Bytes::from(prost::Message::encode_to_vec(&empty)),
            request_id: None,
        };
        assert!(!grid_echoes_cell(&no_preset, 0, 0, 1));
    }

    #[test]
    fn preset_has_block_finds_an_occupied_cell() {
        use crate::proto::{BinaryPreset, Chain, Model, model};
        let preset = BinaryPreset {
            chains: vec![
                Chain::default(),
                Chain {
                    models: vec![
                        Model::default(),
                        Model {
                            hash: Some(model::Hash::Hash(6025)),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(preset_has_block(&preset, 1, 1, 6025));
        // A different model in the cell is not a match.
        assert!(!preset_has_block(&preset, 1, 1, 1001));
        // An empty cell, a row that is out of range, a column that is.
        assert!(!preset_has_block(&preset, 1, 0, 6025));
        assert!(!preset_has_block(&preset, 9, 1, 6025));
        assert!(!preset_has_block(&preset, 1, 9, 6025));
    }

    #[test]
    fn preset_has_block_treats_a_zero_hash_as_empty() {
        use crate::proto::{BinaryPreset, Chain, Model, model};
        let preset = BinaryPreset {
            chains: vec![Chain {
                models: vec![Model {
                    hash: Some(model::Hash::Hash(0)),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!preset_has_block(&preset, 0, 0, 0));
    }

    #[test]
    fn routing_and_split_writes_require_matching_complete_readback() {
        use crate::proto::{Chain, SplitControlPoints, chain};

        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (next, _) = wait_for_write(&fake, 0, MessageType::Grid);
            let (next, request) = wait_for_write(&fake, next, MessageType::RecallPreset);
            push_current_preset(
                &fake,
                &request,
                one_chain(Chain {
                    in_portid: Some(chain::InPortid::InPortid(1)),
                    ..Default::default()
                }),
            );
            let (next, _) = wait_for_write(&fake, next, MessageType::Grid);
            let (next, request) = wait_for_write(&fake, next, MessageType::RecallPreset);
            push_current_preset(
                &fake,
                &request,
                one_chain(Chain {
                    out_portid: Some(chain::OutPortid::OutPortid(19)),
                    ..Default::default()
                }),
            );
            let (next, _) = wait_for_write(&fake, next, MessageType::Grid);
            let (_, request) = wait_for_write(&fake, next, MessageType::RecallPreset);
            push_current_preset(
                &fake,
                &request,
                one_chain(Chain {
                    split_control_points: vec![SplitControlPoints { split: 3, mix: 6 }],
                    ..Default::default()
                }),
            );
        });

        qc.set_chain_input(Row::from_wire(0), crate::GridInputPort::Input1)
            .unwrap();
        qc.set_chain_output(Row::from_wire(0), crate::GridOutputPort::Multiple)
            .unwrap();
        qc.set_split(Row::from_wire(0), 3, 6).unwrap();
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn block_removal_requires_a_returned_empty_cell() {
        use crate::proto::{Chain, Model};

        for (read_back, expected) in [
            (
                one_chain(Chain {
                    models: vec![Model::default()],
                    ..Default::default()
                }),
                true,
            ),
            (one_chain(Chain::default()), false),
        ] {
            let link = FakeLink::new();
            let session = Arc::new(crate::Session::over(link.clone()).unwrap());
            let qc = QuadCortex::new(session.clone());
            let fake = link.clone();
            let responder = std::thread::spawn(move || {
                let (next, _) = wait_for_write(&fake, 0, MessageType::Grid);
                let (_, request) = wait_for_write(&fake, next, MessageType::RecallPreset);
                push_current_preset(&fake, &request, read_back);
            });
            assert_eq!(qc.remove_block(Row::from_wire(0), 0).is_ok(), expected);
            responder.join().unwrap();
            session.close();
        }
    }

    #[test]
    fn bypass_readback_checks_the_active_scene_and_occupied_cell() {
        use crate::proto::{
            Bypass, Chain, ColBypass, Model, SceneBypass, bypass, col_bypass, model,
        };

        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (next, request) = wait_for_write(&fake, 0, MessageType::Scene);
            answer_active_scene(&fake, &request, 2);
            let (next, _) = wait_for_write(&fake, next, MessageType::Grid);
            let (_, request) = wait_for_write(&fake, next, MessageType::RecallPreset);
            push_current_preset(
                &fake,
                &request,
                BinaryPreset {
                    chains: vec![Chain {
                        models: vec![Model {
                            hash: Some(model::Hash::Hash(42)),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    bypass: vec![Bypass {
                        row: Some(bypass::Row::Row(0)),
                        col_bypass: vec![ColBypass {
                            column: Some(col_bypass::Column::Column(0)),
                            scene_bypass: (0..8)
                                .map(|scene| SceneBypass { bypass: scene == 2 })
                                .collect(),
                            scene_mode: Some(col_bypass::SceneMode::SceneMode(true)),
                        }],
                    }],
                    ..Default::default()
                },
            );
        });

        qc.set_bypass(Row::from_wire(0), 0, true).unwrap();
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn parameter_readback_uses_explicit_keys_and_target_scene() {
        use crate::proto::{Chain, Model, Param, ParamValue, chain, model, param, param_value};

        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (next, _) = wait_for_write(&fake, 0, MessageType::Grid);
            let (next, _) = wait_for_write(&fake, next, MessageType::Scene);
            let (next, _) = wait_for_write(&fake, next, MessageType::Grid);
            let (_, request) = wait_for_write(&fake, next, MessageType::RecallPreset);
            let mut values = vec![ParamValue::default(); 8];
            values[3].value = Some(param_value::Value::FloatValue(0.75));
            push_current_preset(
                &fake,
                &request,
                one_chain(Chain {
                    row: Some(chain::Row::Row(2)),
                    models: vec![Model {
                        column: Some(model::Column::Column(3)),
                        hash: Some(model::Hash::Hash(42)),
                        params: vec![Param {
                            index: Some(param::Index::Index(4)),
                            param_values: values,
                            scene_mode: Some(param::SceneMode::SceneMode(true)),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            );
        });

        let applied = qc
            .set_parameter(
                Row::from_wire(2),
                3,
                ParameterTarget::Index(4),
                ParameterInput::Normalised(0.75),
                Some(3),
                true,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(applied.value, Value::Normalised(0.75));
        responder.join().unwrap();
        session.close();
    }

    #[test]
    fn mismatched_grid_readback_is_not_reported_as_success() {
        use crate::proto::{Chain, chain};

        let link = FakeLink::new();
        let session = Arc::new(crate::Session::over(link.clone()).unwrap());
        let qc = QuadCortex::new(session.clone());
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            let (next, _) = wait_for_write(&fake, 0, MessageType::Grid);
            let (_, request) = wait_for_write(&fake, next, MessageType::RecallPreset);
            push_current_preset(
                &fake,
                &request,
                one_chain(Chain {
                    in_portid: Some(chain::InPortid::InPortid(2)),
                    ..Default::default()
                }),
            );
        });

        assert!(matches!(
            qc.set_chain_input(Row::from_wire(0), crate::GridInputPort::Input1),
            Err(crate::Error::GridWriteUnconfirmed(_))
        ));
        responder.join().unwrap();
        session.close();
    }
}

#[cfg(test)]
mod save_tests {
    //! The guards on the one destructive operation.
    //!
    //! Both refusals happen before anything reaches the wire, so they can be
    //! tested over a fake link - and they are worth testing precisely because
    //! the failure mode is someone else's work being overwritten.

    use super::*;
    use crate::link::FakeLink;
    use std::sync::Arc;

    fn client() -> QuadCortex {
        QuadCortex::new(Arc::new(
            crate::Session::over(FakeLink::new()).expect("session over a fake link"),
        ))
    }

    #[test]
    fn the_factory_library_is_recognised_by_path() {
        assert!(is_factory_setlist("/opt/neuraldsp/Factory Library"));
        assert!(!is_factory_setlist(USER_SETLIST));
    }

    #[test]
    fn saving_to_the_factory_library_is_refused_before_it_reaches_the_wire() {
        let qc = client();
        let err = qc
            .save_current_preset(
                "/opt/neuraldsp/Factory Library",
                "1A",
                None,
                Instrument::Guitar,
                Duration::from_millis(50),
            )
            .expect_err("the factory library must never be written to");
        assert!(
            matches!(err, crate::Error::NotFound(_)),
            "expected a refusal naming the reason, got {err:?}"
        );
        qc.session.stop();
    }

    #[test]
    fn a_malformed_slot_is_refused_rather_than_guessed_at() {
        let qc = client();
        // A wrong slot that was silently coerced would overwrite the wrong
        // preset, which is the worst outcome this API has.
        let err = qc
            .save_current_preset(
                USER_SETLIST,
                "99Z",
                None,
                Instrument::Guitar,
                Duration::from_millis(50),
            )
            .expect_err("99Z is not a slot");
        assert!(
            matches!(err, crate::Error::InvalidSlot(_)),
            "expected InvalidSlot, got {err:?}"
        );
        qc.session.stop();
    }
}

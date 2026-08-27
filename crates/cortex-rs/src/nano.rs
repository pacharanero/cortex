// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-FileCopyrightText: 2026 Desktop Nano Cortex Contributors
// SPDX-FileCopyrightText: 2026 Nano Cortex Web Editor Contributors
// SPDX-License-Identifier: AGPL-3.0-or-later AND Apache-2.0 AND MIT

//! Nano Cortex application envelope, current-state model, and bounded
//! working-state operations.
//!
//! The Nano shares the Quad's HID framing, but not its application envelope or
//! domain model. Its current-state response is a protobuf-style body followed
//! by a four-byte footer. The field map is adapted from the Apache-2.0-licensed
//! `rixrix/deskop-nano-cortex` decoder, which credits the MIT-licensed
//! `choldy/nano-cortex-web-editor`. The Gate-reduction write layout follows
//! the same licensed sources; the Nano-specific FX model-name and parameter-label
//! tables are adapted from `deskop-nano-cortex`. See `THIRD-PARTY-NOTICES.md`.
//!
//! @see spec/roadmap.md [NANO-001.3]
//! @see spec/110-framing/design.md
//! @see spec/130-domain-model/spec.md

use crate::{Error, Result};

#[cfg(any(feature = "hid", test))]
use crate::link::HidLink;

#[cfg(feature = "hid")]
use crate::Transport;
#[cfg(any(feature = "hid", test))]
use crate::link::read_message;

/// Hardware-verified read-only request for the complete Nano current state.
///
/// The last four bytes are the request footer. Pass this application body to
/// [`crate::framing::encode_reports`] or directly to the HID transport's raw
/// write path; never append the Quad's eight-byte trailer.
pub const CURRENT_STATE_REQUEST: [u8; 12] = [
    0x08, 0x03, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x01, 0x00, 0x00, 0x00,
];

/// Footer observed on a Nano current-state response.
pub const CURRENT_STATE_RESPONSE_FOOTER: NanoFooter = NanoFooter([0x02, 0x00, 0x00, 0x00]);
const CONFIRMATION_FOOTER: NanoFooter = NanoFooter([0x85, 0x00, 0x00, 0x00]);

/// Four-byte command-specific footer on a Nano application message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NanoFooter(pub [u8; 4]);

/// Fixed roles in the Nano Cortex signal chain, in signal-flow order.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NanoSlotRole {
    /// Input gate.
    Gate,
    /// First effect before the Capture block.
    PreFx1,
    /// Second effect before the Capture block.
    PreFx2,
    /// Neural Capture block.
    Capture,
    /// Cab/IR block.
    IrCab,
    /// First effect after the Cab/IR block.
    PostFx1,
    /// Second effect after the Cab/IR block.
    PostFx2,
    /// Third effect after the Cab/IR block.
    PostFx3,
}

/// One role in the fixed Nano signal chain.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NanoSlotState {
    /// Position and function of this slot.
    pub role: NanoSlotRole,
    /// Assigned Capture or IR name when the state message supplies one.
    pub loaded_name: Option<String>,
    /// Numeric model identifier for one of the five variable FX slots.
    #[cfg_attr(feature = "typescript", ts(type = "number | null"))]
    pub model_id: Option<u64>,
    /// Nano-specific display name resolved from the numeric effect model id.
    pub model_name: Option<String>,
    /// Bypass state when present in the current-state message.
    pub bypassed: Option<bool>,
}

/// Resolve a Nano effect model id to its product-facing display name.
///
/// This is deliberately separate from the Quad Cortex runtime catalog: the
/// products mostly share an id namespace, but not every id has the same name.
/// Unknown ids remain unresolved so newer firmware is never mislabelled.
#[must_use]
pub const fn fx_model_name(model_id: u64) -> Option<&'static str> {
    match fx_model_profile(model_id) {
        Some(profile) => Some(profile.name),
        None => None,
    }
}

#[derive(Clone, Copy)]
struct NanoFxProfile {
    name: &'static str,
    parameter_names: &'static [&'static str],
}

#[allow(clippy::too_many_lines)] // One explicit table keeps model metadata auditable.
const fn fx_model_profile(model_id: u64) -> Option<NanoFxProfile> {
    let profile = match model_id {
        2 => NanoFxProfile {
            name: "Obsessive Drive",
            parameter_names: &["Drive", "Peak", "Tone", "Volume"],
        },
        3 => NanoFxProfile {
            name: "OD250",
            parameter_names: &["Gain", "Volume"],
        },
        4 => NanoFxProfile {
            name: "Rodent Drive",
            parameter_names: &["Distortion", "Filter", "Volume"],
        },
        6 => NanoFxProfile {
            name: "Exotic",
            parameter_names: &["Gain", "Bass", "Treble", "Volume"],
        },
        13 => NanoFxProfile {
            name: "Chief OD1",
            parameter_names: &["Gain", "Level"],
        },
        18 => NanoFxProfile {
            name: "Chief BD2",
            parameter_names: &["Gain", "Tone", "Volume"],
        },
        22 => NanoFxProfile {
            name: "Facial Fuzz",
            parameter_names: &["Fuzz", "Volume", "Pickup", "Pickup Level"],
        },
        23 => NanoFxProfile {
            name: "Exotic Z Boost",
            parameter_names: &["Gain", "Bass", "Treble", "Volume"],
        },
        27 => NanoFxProfile {
            name: "Green 808",
            parameter_names: &["Overdrive", "Tone", "Level"],
        },
        3000 => NanoFxProfile {
            name: "Microtubes B3K",
            parameter_names: &["Drive", "Growl", "Midboost", "Tone", "Level", "Blend"],
        },
        3007 => NanoFxProfile {
            name: "Exotic Bass Z Boost",
            parameter_names: &["Gain", "Bass", "Treble", "Volume"],
        },
        4001 => NanoFxProfile {
            name: "Parametric 3",
            parameter_names: &[
                "1 Gain", "1 Freq", "1 Q", "1 Type", "1 Active", "2 Gain", "2 Freq", "2 Q",
                "2 Type", "2 Active", "3 Gain", "3 Freq", "3 Q", "3 Type", "3 Active", "Output",
            ],
        },
        4003 => NanoFxProfile {
            name: "Low-High Cut",
            parameter_names: &["HPF Slope", "HPF Freq", "LPF Slope", "LPF Freq", "Output"],
        },
        4005 => NanoFxProfile {
            name: "Graphic 9",
            parameter_names: &[
                "65Hz", "125Hz", "250Hz", "500hz", "1kHz", "2kHz", "4kHz", "8kHz", "16kHz", "HPF",
                "LPF", "Output",
            ],
        },
        5001 => NanoFxProfile {
            name: "Legendary 87 (M)",
            parameter_names: &["Input", "Ratio", "Attack", "Release", "Makeup", "Mix"],
        },
        5004 => NanoFxProfile {
            name: "Solid State Comp (M)",
            parameter_names: &["Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix"],
        },
        5005 => NanoFxProfile {
            name: "VCA Comp (M)",
            parameter_names: &["Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix"],
        },
        5007 => NanoFxProfile {
            name: "Opto Comp (M)",
            parameter_names: &["Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix"],
        },
        5012 => NanoFxProfile {
            name: "Legendary 87 (ST)",
            parameter_names: &["Input", "Ratio", "Attack", "Release", "Makeup", "Mix"],
        },
        5013 => NanoFxProfile {
            name: "Solid State Comp (ST)",
            parameter_names: &["Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix"],
        },
        5014 => NanoFxProfile {
            name: "VCA Comp (ST)",
            parameter_names: &["Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix"],
        },
        5015 => NanoFxProfile {
            name: "Opto Comp (ST)",
            parameter_names: &["Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix"],
        },
        6004 => NanoFxProfile {
            name: "Tape Delay",
            parameter_names: &[
                "Mix",
                "Feedback",
                "High Pass",
                "Low Pass",
                "Drive",
                "Delay Time",
                "Wow",
                "Flutter",
                "Ping Pong",
                "Sync",
                "Sync Note",
            ],
        },
        6010 => NanoFxProfile {
            name: "Analog Delay",
            parameter_names: &[
                "Mix",
                "Feedback",
                "High Pass",
                "Low Pass",
                "Ping Pong",
                "Delay Time",
                "Mod Rate",
                "Mod Depth",
                "Width",
                "Drive",
                "Sync",
                "Sync Note",
            ],
        },
        6011 => NanoFxProfile {
            name: "Digital Delay (ST)",
            parameter_names: &[
                "Mix",
                "Feedback",
                "High Pass",
                "Low Pass",
                "Ping Pong",
                "Delay Time",
                "Mod Rate",
                "Mod Depth",
                "Width",
                "Dyn Depth",
                "Dyn Mode",
                "Threshold",
                "Attack",
                "Release",
                "Knee",
                "Feedback Depth",
                "Sync",
                "Sync Note",
            ],
        },
        6012 => NanoFxProfile {
            name: "Dual Delay",
            parameter_names: &[
                "Mix",
                "Delay Time L",
                "Feedback L",
                "X-Feedback",
                "Delay Time R",
                "Feedback R",
                "High Pass",
                "Low Pass",
                "Mod Rate",
                "Mod Depth",
                "Link FBack",
                "Dyn Depth",
                "Dyn Mode",
                "Threshold",
                "Attack",
                "Release",
                "Knee",
                "Feedback Depth",
                "Sync L",
                "Sync Note L",
                "Sync R",
                "Sync Note R",
            ],
        },
        6014 => NanoFxProfile {
            name: "Dual Reverse Delay",
            parameter_names: &[
                "Mix",
                "Delay Time L",
                "Feedback L",
                "X-Feedback",
                "Feedback Mode",
                "Delay Time R",
                "Feedback R",
                "Overlap",
                "Trig Threshold",
                "Dyn Depth",
                "Dyn Mode",
                "Threshold",
                "Attack",
                "Release",
                "Knee",
                "Feedback Depth",
                "High Pass",
                "Low Pass",
                "Link FBack",
                "Sync",
                "Sync Note L",
                "Sync Note R",
            ],
        },
        6015 => NanoFxProfile {
            name: "Circular Delay",
            parameter_names: &[
                "Mix",
                "Tap Preset",
                "Delay Time",
                "Feedback",
                "Diffusion",
                "High Pass",
                "Low Pass",
                "Mod Rate",
                "Mod Depth",
                "Vintage Mode",
                "Sync",
                "Sync Note",
            ],
        },
        7004 => NanoFxProfile {
            name: "Tremolo",
            parameter_names: &[
                "Rate",
                "Depth",
                "Waveform",
                "Duty Cycle",
                "Width",
                "Smoothing",
                "LFO Active",
                "Fade In",
                "Fade Out",
                "Boost",
                "Sync",
                "Sync Note",
            ],
        },
        7021 => NanoFxProfile {
            name: "MX Flanger",
            parameter_names: &[
                "Mix",
                "Manual",
                "Width",
                "Speed",
                "Regen",
                "Output",
                "Sync",
                "Sync Note",
            ],
        },
        7022 => NanoFxProfile {
            name: "Dream Chorus",
            parameter_names: &[
                "Mix",
                "Speed",
                "Depth",
                "Mode",
                "Output",
                "Sync",
                "Sync Note",
            ],
        },
        7023 => NanoFxProfile {
            name: "Chorus 229T",
            parameter_names: &[
                "Mix",
                "Rate",
                "Depth",
                "Width",
                "Output",
                "Sync",
                "Sync Note",
            ],
        },
        7024 => NanoFxProfile {
            name: "Chief CE2W (ST)",
            parameter_names: &[
                "Mix",
                "Rate",
                "Depth",
                "Type",
                "Width",
                "Output",
                "Sync",
                "Sync Note",
            ],
        },
        7027 => NanoFxProfile {
            name: "Chief DC2W (ST)",
            parameter_names: &["Mix", "Mode", "Mode Type", "Drive", "Output"],
        },
        7028 => NanoFxProfile {
            name: "MX Phase 95",
            parameter_names: &[
                "Mix",
                "Speed",
                "Type",
                "Mode",
                "Output",
                "Sync",
                "Sync Note",
            ],
        },
        7029 => NanoFxProfile {
            name: "MX Vibe",
            parameter_names: &[
                "Mix",
                "Vibe",
                "Speed",
                "Level",
                "Depth",
                "Output",
                "Sync",
                "Sync Note",
            ],
        },
        8000 => NanoFxProfile {
            name: "Room",
            parameter_names: &["Mix", "Decay", "Pre Delay", "High Pass", "Low Pass"],
        },
        8003 => NanoFxProfile {
            name: "Hall",
            parameter_names: &["Mix", "Decay", "Pre Delay", "High Pass", "Low Pass"],
        },
        8007 => NanoFxProfile {
            name: "Modulated",
            parameter_names: &[
                "Mix",
                "Decay",
                "Pre Delay",
                "Mod Speed",
                "Mod Depth",
                "High Pass",
                "Low Pass",
            ],
        },
        8008 => NanoFxProfile {
            name: "Ambience",
            parameter_names: &["Mix", "Size", "Pre Delay", "High Pass", "Low Pass"],
        },
        8009 => NanoFxProfile {
            name: "Cave",
            parameter_names: &[
                "Mix",
                "Decay",
                "Pre Delay",
                "Damping",
                "High Pass",
                "Low Pass",
            ],
        },
        8011 => NanoFxProfile {
            name: "Mind Hall",
            parameter_names: &[
                "Mix",
                "Decay",
                "Pre Delay",
                "High Pass",
                "Low Pass",
                "Damping",
            ],
        },
        9010 => NanoFxProfile {
            name: "Bubba Wah",
            parameter_names: &["Wah"],
        },
        9012 => NanoFxProfile {
            name: "Bass Wah",
            parameter_names: &["Wah"],
        },
        9013 => NanoFxProfile {
            name: "Crying Wah",
            parameter_names: &["Wah"],
        },
        9014 => NanoFxProfile {
            name: "Crying Clyde Wah",
            parameter_names: &["Wah"],
        },
        16001 => NanoFxProfile {
            name: "Adaptive Gate",
            parameter_names: &["Noise Reduction"],
        },
        16002 => NanoFxProfile {
            name: "Utility Gate",
            parameter_names: &["Threshold", "Attack", "Hold", "Release", "Range"],
        },
        16006 => NanoFxProfile {
            name: "Volume",
            parameter_names: &["Level", "Curve"],
        },
        16011 => NanoFxProfile {
            name: "Doubler",
            parameter_names: &["Spread", "Dry Level", "FX Level"],
        },
        18001 => NanoFxProfile {
            name: "Transpose",
            parameter_names: &["Mix", "Semitones", "Pitch Fine", "High Pass", "Low Pass"],
        },
        24001 => NanoFxProfile {
            name: "Love Meat",
            parameter_names: &[
                "Sensitivity",
                "Attack",
                "Decay",
                "Color",
                "Intensity",
                "Blend",
                "Trig Detection",
                "Trigger Mode",
                "Filter Cutoff",
                "Filter Type",
                "Level",
            ],
        },
        24006 => NanoFxProfile {
            name: "Envelope Filter",
            parameter_names: &[
                "Sens",
                "Attack",
                "Decay",
                "LP/BP/HP",
                "Level",
                "Freq",
                "Freq Depth",
                "Reso",
                "Mix",
            ],
        },
        _ => return None,
    };
    Some(profile)
}

/// One host-facing Nano FX parameter enriched with licensed semantic metadata.
///
/// The wire response supplies only positional normalized values. `index` is
/// therefore the authoritative zero-based wire index, while `name` remains
/// absent for unknown models and parameters added by newer firmware.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NanoFxParameter {
    /// Zero-based index used by the Nano FX parameter write command.
    pub index: u8,
    /// Semantic label from the licensed model profile, when known.
    pub name: Option<String>,
    /// Raw normalized value supplied by the device, from 0.0 through 1.0.
    pub normalized: f32,
}

/// Attach licensed semantic labels to raw Nano FX parameter values.
///
/// Every raw value is retained. Unknown models and values beyond a known
/// profile receive no name rather than a guessed label.
///
/// # Panics
///
/// Panics if `values` contains more than 256 entries. The Nano wire format
/// bounds parameter counts to one byte, so such input cannot come from a
/// decoded parameter refresh.
#[must_use]
pub fn describe_fx_params(model_id: Option<u64>, values: &[f32]) -> Vec<NanoFxParameter> {
    let names = model_id
        .and_then(fx_model_profile)
        .map(|profile| profile.parameter_names)
        .unwrap_or_default();
    values
        .iter()
        .copied()
        .enumerate()
        .map(|(index, normalized)| NanoFxParameter {
            index: u8::try_from(index).expect("Nano refresh length limits parameter indices to u8"),
            name: names.get(index).map(|name| (*name).to_owned()),
            normalized,
        })
        .collect()
}

/// Raw 0-255 values for the Nano's five amplifier controls.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NanoAmpState {
    /// Gain control.
    pub gain: Option<u8>,
    /// Output level control.
    pub level: Option<u8>,
    /// Bass control.
    pub bass: Option<u8>,
    /// Mid control.
    pub mid: Option<u8>,
    /// Treble control.
    pub treble: Option<u8>,
}

impl NanoAmpState {
    /// Value of one typed amp control, preserving absence.
    #[must_use]
    pub const fn value(&self, control: NanoAmpControl) -> Option<u8> {
        match control {
            NanoAmpControl::Gain => self.gain,
            NanoAmpControl::Level => self.level,
            NanoAmpControl::Bass => self.bass,
            NanoAmpControl::Mid => self.mid,
            NanoAmpControl::Treble => self.treble,
        }
    }
}

/// One of the Nano's five amplifier controls.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NanoAmpControl {
    /// Gain control.
    Gain,
    /// Output level control.
    Level,
    /// Bass control.
    Bass,
    /// Mid control.
    Mid,
    /// Treble control.
    Treble,
}

/// Nano roles addressable by the measured bypass command.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NanoBypassTarget {
    /// Input gate.
    Gate,
    /// First pre effect.
    PreFx1,
    /// Second pre effect.
    PreFx2,
    /// First post effect.
    PostFx1,
    /// Second post effect.
    PostFx2,
    /// Third post effect.
    PostFx3,
}

impl NanoBypassTarget {
    const fn wire_id(self) -> u8 {
        match self {
            Self::PreFx1 => 4,
            Self::PreFx2 => 5,
            Self::PostFx1 => 6,
            Self::PostFx2 => 7,
            Self::PostFx3 => 8,
            Self::Gate => 9,
        }
    }

    /// Corresponding fixed-chain role.
    #[must_use]
    pub const fn role(self) -> NanoSlotRole {
        match self {
            Self::Gate => NanoSlotRole::Gate,
            Self::PreFx1 => NanoSlotRole::PreFx1,
            Self::PreFx2 => NanoSlotRole::PreFx2,
            Self::PostFx1 => NanoSlotRole::PostFx1,
            Self::PostFx2 => NanoSlotRole::PostFx2,
            Self::PostFx3 => NanoSlotRole::PostFx3,
        }
    }
}

impl NanoAmpControl {
    const fn wire_id(self) -> u8 {
        match self {
            Self::Gain => 0,
            Self::Level => 1,
            Self::Bass => 2,
            Self::Mid => 3,
            Self::Treble => 4,
        }
    }
}

/// Build the Nano application body that sets one amp control to a raw 0-255
/// value. HID framing is added separately by the shared framing layer.
#[must_use]
pub fn build_set_amp(control: NanoAmpControl, value: u8) -> Vec<u8> {
    let mut body = vec![0x18, control.wire_id(), 0x20];
    push_varint(&mut body, u64::from(value));
    body.extend([0x28, 0x00, 0x1a, 0x00, 0x00, 0x00]);
    body
}

/// Send one Nano amp-control write. The device sends no acknowledgement;
/// callers must wait for the state-read pacing interval and verify with a
/// separate [`read_current_state`] request before reporting success.
///
/// # Errors
///
/// Returns a transport error or rejects a transport opened for another device
/// before any write.
#[cfg(feature = "hid")]
pub fn write_amp(transport: &Transport, control: NanoAmpControl, value: u8) -> Result<()> {
    if transport.device_kind() != crate::DeviceKind::NanoCortex {
        return Err(Error::UnsupportedDeviceOperation {
            device: transport.device_kind(),
            operation: "Nano amp-control write",
        });
    }
    transport.write(&build_set_amp(control, value))
}

/// Build the Nano application body that sets Gate reduction as a percentage.
/// HID framing is added separately by the shared framing layer.
///
/// # Errors
///
/// Returns [`Error::InvalidParameter`] when `percent` is greater than 100.
pub fn build_set_gate_reduction(percent: u8) -> Result<Vec<u8>> {
    if percent > 100 {
        return Err(Error::InvalidParameter(format!(
            "Nano Gate reduction must be 0-100%, got {percent}"
        )));
    }
    let mut body = vec![0x18, 0x0b, 0x20];
    push_varint(&mut body, u64::from(percent) + 108);
    body.extend([0x28, 0x00, 0x1a, 0x00, 0x00, 0x00]);
    Ok(body)
}

/// Send one Nano Gate-reduction write. Callers must not treat the successful
/// transport write as confirmation: wait for the state-read pacing interval
/// and verify with a separate [`read_current_state`] request.
///
/// # Errors
///
/// Returns [`Error::InvalidParameter`] for a percentage greater than 100, a
/// transport error, or rejects a transport opened for another device before
/// any write.
#[cfg(feature = "hid")]
pub fn write_gate_reduction(transport: &Transport, percent: u8) -> Result<()> {
    let body = build_set_gate_reduction(percent)?;
    if transport.device_kind() != crate::DeviceKind::NanoCortex {
        return Err(Error::UnsupportedDeviceOperation {
            device: transport.device_kind(),
            operation: "Nano Gate-reduction write",
        });
    }
    transport.write(&body)
}

/// Build the Nano application body that bypasses or enables Gate/FX.
#[must_use]
pub fn build_set_bypass(target: NanoBypassTarget, bypassed: bool) -> [u8; 10] {
    [
        0x08,
        0x01,
        0x18,
        target.wire_id(),
        0x20,
        u8::from(bypassed),
        0x1f,
        0x00,
        0x00,
        0x00,
    ]
}

/// Send one Nano Gate/FX bypass write. Verify through a separately paced
/// [`read_current_state`] request before reporting success.
///
/// # Errors
///
/// Returns [`Error::InvalidParameter`] for a non-finite or out-of-range value,
/// a transport error, or rejects a transport opened for another device before
/// any write.
#[cfg(feature = "hid")]
pub fn write_bypass(transport: &Transport, target: NanoBypassTarget, bypassed: bool) -> Result<()> {
    if transport.device_kind() != crate::DeviceKind::NanoCortex {
        return Err(Error::UnsupportedDeviceOperation {
            device: transport.device_kind(),
            operation: "Nano bypass write",
        });
    }
    transport.write(&build_set_bypass(target, bypassed))
}

/// One of the Nano's five editable FX slots, addressable by the parameter
/// refresh and write commands. The wire index maps to the fixed chain order:
/// 0 = Pre FX 1, 1 = Pre FX 2, 2 = Post FX 1, 3 = Post FX 2, 4 = Post FX 3.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NanoFxSlot {
    /// First pre effect.
    PreFx1,
    /// Second pre effect.
    PreFx2,
    /// First post effect.
    PostFx1,
    /// Second post effect.
    PostFx2,
    /// Third post effect.
    PostFx3,
}

impl NanoFxSlot {
    const fn wire_id(self) -> u8 {
        match self {
            Self::PreFx1 => 0,
            Self::PreFx2 => 1,
            Self::PostFx1 => 2,
            Self::PostFx2 => 3,
            Self::PostFx3 => 4,
        }
    }

    /// Corresponding fixed-chain role.
    #[must_use]
    pub const fn role(self) -> NanoSlotRole {
        match self {
            Self::PreFx1 => NanoSlotRole::PreFx1,
            Self::PreFx2 => NanoSlotRole::PreFx2,
            Self::PostFx1 => NanoSlotRole::PostFx1,
            Self::PostFx2 => NanoSlotRole::PostFx2,
            Self::PostFx3 => NanoSlotRole::PostFx3,
        }
    }
}

/// Footer on a Nano FX parameter refresh response.
#[allow(dead_code)]
const FX_PARAM_REFRESH_FOOTER: NanoFooter = NanoFooter([0x8a, 0x00, 0x00, 0x00]);

/// Build the Nano application body that requests an FX parameter refresh for
/// one editable slot. The device returns normalized f32 parameter values.
#[must_use]
pub fn build_fx_param_refresh(slot: NanoFxSlot) -> [u8; 8] {
    [0x08, 0x03, 0x18, slot.wire_id(), 0x89, 0x00, 0x00, 0x00]
}

/// Build the Nano application body that sets one FX parameter to a normalized
/// 0.0-1.0 value.
///
/// # Errors
///
/// Returns [`Error::InvalidParameter`] for a non-finite or out-of-range value.
pub fn build_fx_param_write(slot: NanoFxSlot, param_index: u8, normalized: f32) -> Result<Vec<u8>> {
    if !normalized.is_finite() || !(0.0..=1.0).contains(&normalized) {
        return Err(Error::InvalidParameter(format!(
            "Nano FX parameter value must be finite and normalized to 0-1, got {normalized}"
        )));
    }
    let mut body = vec![0x08, 0x01, 0x18, slot.wire_id(), 0x20, param_index, 0x2d];
    body.extend(normalized.to_le_bytes());
    body.extend([0x63, 0x00, 0x00, 0x00]);
    Ok(body)
}

/// Read and decode FX parameter values for one editable slot from an open
/// Nano transport. Returns normalized 0.0-1.0 f32 values, one per parameter
/// the loaded model exposes. The number of values varies by model.
///
/// # Errors
///
/// Returns a transport, framing, timeout, or decode error. A transport opened
/// for another device is rejected before any write.
#[cfg(feature = "hid")]
pub fn read_fx_params(
    transport: &Transport,
    slot: NanoFxSlot,
    timeout: std::time::Duration,
) -> Result<Vec<f32>> {
    if transport.device_kind() != crate::DeviceKind::NanoCortex {
        return Err(Error::UnsupportedDeviceOperation {
            device: transport.device_kind(),
            operation: "Nano FX parameter refresh",
        });
    }
    read_fx_params_from_link(transport.raw_device(), slot, timeout)
}

/// Read Nano FX parameters through a raw HID link.
///
/// This is deliberately link-based so the Nano-specific envelope can be
/// regression-tested without a physical device. Callers that own a
/// [`Transport`] must use [`read_fx_params`], which first verifies the device
/// family.
#[cfg(any(feature = "hid", test))]
fn read_fx_params_from_link(
    link: &impl HidLink,
    slot: NanoFxSlot,
    timeout: std::time::Duration,
) -> Result<Vec<f32>> {
    write_nano_message(link, &build_fx_param_refresh(slot))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(Error::ReadTimeout(timeout));
        }
        let message = read_message(
            link,
            crate::framing::HidReportGeometry::NANO_CORTEX,
            remaining,
        )?;
        if let Some(values) = decode_fx_param_refresh_response(&message)? {
            return Ok(values);
        }
        // Skip unrelated messages (ack footers, etc.)
    }
}

#[cfg(any(feature = "hid", test))]
fn write_nano_message(link: &impl HidLink, message: &[u8]) -> Result<()> {
    for report in
        crate::framing::encode_reports(crate::framing::HidReportGeometry::NANO_CORTEX, message)
    {
        let written = link.write(&report)?;
        if written != report.len() {
            return Err(Error::Framing(format!(
                "short HID write: wrote {written} of {} bytes",
                report.len()
            )));
        }
    }
    Ok(())
}

/// Decode one FX parameter refresh response, ignoring unrelated Nano messages.
///
/// The measured response has no slot or request identifier. The held daemon
/// serializes Nano operations, but an older queued response remains
/// wire-indistinguishable from the requested slot's reply.
#[cfg(any(feature = "hid", test))]
fn decode_fx_param_refresh_response(message: &[u8]) -> Result<Option<Vec<f32>>> {
    if message.len() < 4 {
        return Ok(None);
    }
    let (body, footer) = split_envelope(message)?;
    if footer == CONFIRMATION_FOOTER {
        return Err(decode_confirmation(body)?);
    }
    let is_fx_refresh = body.len() >= 3 && body[..3] == [0x08, 0x06, 0x22];
    if footer != FX_PARAM_REFRESH_FOOTER {
        return if is_fx_refresh {
            Err(Error::Decode(
                "Nano FX parameter refresh has an unexpected footer".into(),
            ))
        } else {
            Ok(None)
        };
    }
    // The refresh reply shape: 08 06 22 <length> <f32 values...> 8a 00 00 00.
    if !is_fx_refresh {
        return Err(Error::Decode(
            "Nano FX parameter refresh has an unexpected body".into(),
        ));
    }
    if body.len() < 4 {
        return Err(Error::Decode(
            "Nano FX parameter refresh is missing its length".into(),
        ));
    }
    let value_len = body[3] as usize;
    if value_len % 4 != 0 {
        return Err(Error::Decode(
            "Nano FX parameter refresh has a non-float value length".into(),
        ));
    }
    let expected_len = 4 + value_len;
    if body.len() != expected_len {
        return Err(Error::Decode(
            "Nano FX parameter refresh has an unexpected message length".into(),
        ));
    }
    let values = body[4..]
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>();
    if values
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(Error::Decode(
            "Nano FX parameter refresh contains a non-normalized value".into(),
        ));
    }
    Ok(Some(values))
}

/// Send one Nano FX parameter write. The device does not acknowledge; callers
/// must verify with a separate [`read_fx_params`] request before reporting
/// success.
///
/// # Errors
///
/// Returns a transport error, or rejects a transport opened for another
/// device before any write.
#[cfg(feature = "hid")]
pub fn write_fx_param(
    transport: &Transport,
    slot: NanoFxSlot,
    param_index: u8,
    normalized: f32,
) -> Result<()> {
    if transport.device_kind() != crate::DeviceKind::NanoCortex {
        return Err(Error::UnsupportedDeviceOperation {
            device: transport.device_kind(),
            operation: "Nano FX parameter write",
        });
    }
    transport.write(&build_fx_param_write(slot, param_index, normalized)?)
}

fn push_varint(output: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            return;
        }
    }
}

/// Assignments of the four Nano footswitch actions.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NanoFootswitchAssignments {
    /// Switch I, action A.
    pub ia: u8,
    /// Switch I, action B.
    pub ib: u8,
    /// Switch II, action A.
    pub iia: u8,
    /// Switch II, action B.
    pub iib: u8,
}

/// Typed read-only snapshot decoded from the Nano current-state response.
///
/// Optional fields preserve protobuf presence. An absent control is unknown,
/// not zero, false, or an empty string. `slots` always contains the eight fixed
/// roles in signal-flow order so hosts can render one chain without pretending
/// it is a Quad grid.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NanoCurrentState {
    /// Firmware string supplied by the device, if present.
    pub firmware: Option<String>,
    /// Amplifier controls in their raw 0-255 device representation.
    pub amp: NanoAmpState,
    /// Selected Capture slot, if present.
    pub capture_slot: Option<u8>,
    /// Capture volume in its raw 0-255 representation, if present.
    pub capture_volume: Option<u8>,
    /// Gate reduction percentage, if the normalized field decodes to 0-100.
    pub gate_reduction: Option<u8>,
    /// Four footswitch assignments, present only when all four fields exist.
    pub footswitch_assignments: Option<NanoFootswitchAssignments>,
    /// Eight fixed roles in signal-flow order.
    pub slots: Vec<NanoSlotState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProtoValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
    Fixed64(u64),
}

/// Decode one reassembled Nano current-state response.
///
/// The input starts at the protobuf body and includes the four-byte footer; HID
/// report IDs, frame lengths, flags, and padding must already have been removed
/// by [`crate::FrameReassembler`].
///
/// # Errors
///
/// Returns [`Error::Decode`] when the message is too short, has the wrong
/// footer, contains malformed protobuf fields, or carries no recognised
/// current-state fields.
pub fn decode_current_state(message: &[u8]) -> Result<NanoCurrentState> {
    let (body, footer) = split_envelope(message)?;
    if footer == CONFIRMATION_FOOTER {
        return Err(decode_confirmation(body)?);
    }
    if footer != CURRENT_STATE_RESPONSE_FOOTER {
        return Err(Error::Decode(format!(
            "unexpected Nano current-state footer: {:02x?}",
            footer.0
        )));
    }

    let fields = parse_proto_fields(body)?;
    let amp = NanoAmpState {
        gain: varint_u8(&fields, 3)?,
        level: varint_u8(&fields, 4)?,
        bass: varint_u8(&fields, 5)?,
        mid: varint_u8(&fields, 6)?,
        treble: varint_u8(&fields, 7)?,
    };
    let capture_slot = varint_u8(&fields, 11)?;
    let capture_volume = varint_u8(&fields, 44)?;
    let gate_on = optional_inverted_flag(&fields, 54)?;
    let cab_ir_on = optional_flag(&fields, 12)?;
    let gate_reduction = fixed32_f32(&fields, 53).and_then(gate_reduction_from_dump_value);
    let firmware = bytes(&fields, 24).map(ascii).transpose()?;
    let capture_name = submessage_name(&fields, 32)?;
    let ir_name = submessage_name(&fields, 33)?;
    let bypass = bytes(&fields, 31);
    let model_ids = [48, 49, 50, 51, 52].map(|field| model_id(&fields, field));
    let footswitch_assignments = match (
        varint_u8(&fields, 14)?,
        varint_u8(&fields, 15)?,
        varint_u8(&fields, 38)?,
        varint_u8(&fields, 39)?,
    ) {
        (Some(ia), Some(ib), Some(iia), Some(iib)) => {
            Some(NanoFootswitchAssignments { ia, ib, iia, iib })
        }
        _ => None,
    };

    if amp == NanoAmpState::default()
        && firmware.is_none()
        && capture_name.is_none()
        && ir_name.is_none()
        && model_ids.iter().all(Option::is_none)
    {
        return Err(Error::Decode(
            "Nano response contains no recognised current-state fields".into(),
        ));
    }

    let effect = |index, role| {
        let model_id = model_ids[index];
        NanoSlotState {
            role,
            loaded_name: None,
            model_id,
            model_name: model_id.and_then(fx_model_name).map(str::to_owned),
            bypassed: bypass
                .and_then(|values| values.get(index))
                .map(|value| *value != 0),
        }
    };

    let slots = vec![
        NanoSlotState {
            role: NanoSlotRole::Gate,
            loaded_name: None,
            model_id: None,
            model_name: None,
            bypassed: gate_on.map(|on| !on),
        },
        effect(0, NanoSlotRole::PreFx1),
        effect(1, NanoSlotRole::PreFx2),
        NanoSlotState {
            role: NanoSlotRole::Capture,
            loaded_name: capture_name,
            model_id: None,
            model_name: None,
            bypassed: None,
        },
        NanoSlotState {
            role: NanoSlotRole::IrCab,
            loaded_name: ir_name,
            model_id: None,
            model_name: None,
            bypassed: cab_ir_on.map(|on| !on),
        },
        effect(2, NanoSlotRole::PostFx1),
        effect(3, NanoSlotRole::PostFx2),
        effect(4, NanoSlotRole::PostFx3),
    ];

    Ok(NanoCurrentState {
        firmware,
        amp,
        capture_slot,
        capture_volume,
        gate_reduction,
        footswitch_assignments,
        slots,
    })
}

fn decode_confirmation(body: &[u8]) -> Result<Error> {
    let fields = parse_proto_fields(body)?;
    let kind = match first(&fields, 1) {
        Some(ProtoValue::Varint(value)) => *value,
        _ => 0,
    };
    let text = bytes(&fields, 4).map(ascii).transpose()?;
    if kind == 3 && text.as_deref() == Some("Device is busy!") {
        return Ok(Error::DeviceBusy {
            device: crate::DeviceKind::NanoCortex,
        });
    }
    Err(Error::Decode(
        "unrecognised Nano confirmation response".into(),
    ))
}

/// Read and decode one complete current-state response from an open Nano
/// transport.
///
/// The timeout is one total deadline for the whole multi-report response, not
/// a fresh allowance for every report. The transport must have been opened for
/// [`crate::DeviceKind::NanoCortex`].
///
/// # Errors
///
/// Returns a transport, framing, timeout, or decode error. A transport opened
/// for another device is rejected before any write.
#[cfg(feature = "hid")]
pub fn read_current_state(
    transport: &Transport,
    timeout: std::time::Duration,
) -> Result<NanoCurrentState> {
    if transport.device_kind() != crate::DeviceKind::NanoCortex {
        return Err(Error::UnsupportedDeviceOperation {
            device: transport.device_kind(),
            operation: "Nano current-state read",
        });
    }

    write_nano_message(transport.raw_device(), &CURRENT_STATE_REQUEST)?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(Error::ReadTimeout(timeout));
        }
        let message = read_message(
            transport.raw_device(),
            crate::framing::HidReportGeometry::NANO_CORTEX,
            remaining,
        )?;
        let (_, footer) = split_envelope(&message)?;
        if footer == CURRENT_STATE_RESPONSE_FOOTER || footer == CONFIRMATION_FOOTER {
            return decode_current_state(&message);
        }
        // A write response can remain queued ahead of the requested state
        // response. It has its own command footer and is not a decode failure
        // for this request.
    }
}

fn split_envelope(message: &[u8]) -> Result<(&[u8], NanoFooter)> {
    let footer_start = message
        .len()
        .checked_sub(4)
        .ok_or_else(|| Error::Decode("Nano message is shorter than its four-byte footer".into()))?;
    let (body, footer) = message.split_at(footer_start);
    Ok((
        body,
        NanoFooter(footer.try_into().expect("four-byte split")),
    ))
}

fn parse_proto_fields(bytes: &[u8]) -> Result<Vec<(u64, ProtoValue<'_>)>> {
    let mut fields = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let (tag, next) = read_varint(bytes, cursor)?;
        cursor = next;
        let field = tag >> 3;
        if field == 0 {
            return Err(Error::Decode("protobuf field zero is invalid".into()));
        }
        let value = match tag & 7 {
            0 => {
                let (value, next) = read_varint(bytes, cursor)?;
                cursor = next;
                ProtoValue::Varint(value)
            }
            1 => {
                let end = cursor
                    .checked_add(8)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| Error::Decode("truncated fixed64 field".into()))?;
                let raw: [u8; 8] = bytes[cursor..end].try_into().expect("eight-byte range");
                cursor = end;
                ProtoValue::Fixed64(u64::from_le_bytes(raw))
            }
            2 => {
                let (length, start) = read_varint(bytes, cursor)?;
                let length = usize::try_from(length)
                    .map_err(|_| Error::Decode("protobuf byte field is too large".into()))?;
                let end = start
                    .checked_add(length)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| Error::Decode("truncated length-delimited field".into()))?;
                cursor = end;
                ProtoValue::Bytes(&bytes[start..end])
            }
            5 => {
                let end = cursor
                    .checked_add(4)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| Error::Decode("truncated fixed32 field".into()))?;
                let raw: [u8; 4] = bytes[cursor..end].try_into().expect("four-byte range");
                cursor = end;
                ProtoValue::Fixed32(u32::from_le_bytes(raw))
            }
            wire => {
                return Err(Error::Decode(format!(
                    "unsupported protobuf wire type {wire}"
                )));
            }
        };
        fields.push((field, value));
    }
    Ok(fields)
}

fn read_varint(bytes: &[u8], start: usize) -> Result<(u64, usize)> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    for (offset, byte) in bytes.iter().copied().skip(start).take(10).enumerate() {
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, start + offset + 1));
        }
        shift += 7;
    }
    Err(Error::Decode(
        "truncated or overflowing protobuf varint".into(),
    ))
}

fn first<'a>(fields: &'a [(u64, ProtoValue<'a>)], field: u64) -> Option<&'a ProtoValue<'a>> {
    fields
        .iter()
        .find_map(|(number, value)| (*number == field).then_some(value))
}

fn varint_u8(fields: &[(u64, ProtoValue<'_>)], field: u64) -> Result<Option<u8>> {
    match first(fields, field) {
        None => Ok(None),
        Some(ProtoValue::Varint(value)) => u8::try_from(*value)
            .map(Some)
            .map_err(|_| Error::Decode(format!("Nano field {field} exceeds u8"))),
        Some(_) => Err(Error::Decode(format!(
            "Nano field {field} has the wrong wire type"
        ))),
    }
}

fn bytes<'a>(fields: &'a [(u64, ProtoValue<'a>)], field: u64) -> Option<&'a [u8]> {
    match first(fields, field) {
        Some(ProtoValue::Bytes(value)) => Some(value),
        _ => None,
    }
}

fn optional_flag(fields: &[(u64, ProtoValue<'_>)], field: u64) -> Result<Option<bool>> {
    Ok(varint_u8(fields, field)?.map(|value| value != 0))
}

fn optional_inverted_flag(fields: &[(u64, ProtoValue<'_>)], field: u64) -> Result<Option<bool>> {
    Ok(varint_u8(fields, field)?.map(|value| value == 0))
}

fn fixed32_f32(fields: &[(u64, ProtoValue<'_>)], field: u64) -> Option<f32> {
    match first(fields, field) {
        Some(ProtoValue::Fixed32(value)) => Some(f32::from_bits(*value)),
        _ => None,
    }
}

fn model_id(fields: &[(u64, ProtoValue<'_>)], field: u64) -> Option<u64> {
    match first(fields, field) {
        Some(ProtoValue::Varint(value)) => Some(*value),
        Some(ProtoValue::Bytes(value)) => read_varint(value, 0)
            .ok()
            .filter(|(_, used)| *used == value.len())
            .map(|(value, _)| value),
        _ => None,
    }
}

fn submessage_name(fields: &[(u64, ProtoValue<'_>)], field: u64) -> Result<Option<String>> {
    let Some(raw) = bytes(fields, field) else {
        return Ok(None);
    };
    let nested = parse_proto_fields(raw)?;
    bytes(&nested, 2).map(ascii).transpose()
}

fn ascii(bytes: &[u8]) -> Result<String> {
    if bytes.is_empty() || !bytes.iter().all(|byte| (0x20..0x7f).contains(byte)) {
        return Err(Error::Decode(
            "Nano text field is not printable ASCII".into(),
        ));
    }
    String::from_utf8(bytes.to_vec()).map_err(|error| Error::Decode(error.to_string()))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn gate_reduction_from_dump_value(value: f32) -> Option<u8> {
    if !value.is_finite() {
        return None;
    }
    let percent = value.mul_add(255.0, -108.0).round();
    (0.0..=100.0).contains(&percent).then_some(percent as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Frame, FrameReassembler, HidReportGeometry, framing::encode_reports, link::FakeLink,
    };

    fn varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                return bytes;
            }
        }
    }

    fn field_varint(output: &mut Vec<u8>, field: u64, value: u64) {
        output.extend(varint(field << 3));
        output.extend(varint(value));
    }

    fn field_bytes(output: &mut Vec<u8>, field: u64, value: &[u8]) {
        output.extend(varint((field << 3) | 2));
        output.extend(varint(value.len() as u64));
        output.extend(value);
    }

    fn field_fixed32(output: &mut Vec<u8>, field: u64, value: f32) {
        output.extend(varint((field << 3) | 5));
        output.extend(value.to_le_bytes());
    }

    fn fictional_state() -> Vec<u8> {
        let mut body = Vec::new();
        field_varint(&mut body, 3, 101);
        field_varint(&mut body, 4, 102);
        field_varint(&mut body, 5, 103);
        // Field 6 deliberately absent: absence must remain visible.
        field_varint(&mut body, 7, 105);
        field_varint(&mut body, 11, 7);
        field_varint(&mut body, 12, 1);
        field_varint(&mut body, 14, 1);
        field_varint(&mut body, 15, 2);
        field_bytes(&mut body, 24, b"NC-FICTION-1.2.3");
        field_bytes(&mut body, 31, &[0, 1, 0, 1, 0]);
        let mut capture = Vec::new();
        field_bytes(&mut capture, 2, b"Fictional Capture");
        field_bytes(&mut body, 32, &capture);
        let mut ir = Vec::new();
        field_bytes(&mut ir, 2, b"Fictional Cabinet");
        field_bytes(&mut body, 33, &ir);
        field_varint(&mut body, 38, 3);
        field_varint(&mut body, 39, 4);
        field_varint(&mut body, 44, 106);
        for (field, model) in (48..=52).zip([4, 7024, 4001, 6010, 8000]) {
            field_varint(&mut body, field, model);
        }
        field_fixed32(&mut body, 53, f32::from(158_u8) / 255.0);
        field_varint(&mut body, 54, 0);
        body.extend(CURRENT_STATE_RESPONSE_FOOTER.0);
        body
    }

    #[test]
    fn current_state_request_matches_the_hardware_verified_application_body() {
        assert_eq!(
            CURRENT_STATE_REQUEST,
            [
                0x08, 0x03, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x01, 0, 0, 0
            ]
        );
        let reports = encode_reports(HidReportGeometry::NANO_CORTEX, &CURRENT_STATE_REQUEST);
        assert_eq!(reports.len(), 1);
        assert_eq!(
            &reports[0][..15],
            &[
                0x02, 0x0c, 0xc0, 0x08, 0x03, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x01, 0, 0, 0
            ]
        );
    }

    #[test]
    fn amp_write_builder_uses_typed_control_ids_and_complete_varints() {
        assert_eq!(
            build_set_amp(NanoAmpControl::Gain, 0),
            [0x18, 0, 0x20, 0, 0x28, 0, 0x1a, 0, 0, 0]
        );
        assert_eq!(
            build_set_amp(NanoAmpControl::Treble, 255),
            [0x18, 4, 0x20, 0xff, 0x01, 0x28, 0, 0x1a, 0, 0, 0]
        );
    }

    #[test]
    fn gate_reduction_builder_encodes_the_offset_and_varint_boundary() {
        assert_eq!(
            build_set_gate_reduction(0).unwrap(),
            [0x18, 0x0b, 0x20, 0x6c, 0x28, 0, 0x1a, 0, 0, 0]
        );
        assert_eq!(
            build_set_gate_reduction(19).unwrap(),
            [0x18, 0x0b, 0x20, 0x7f, 0x28, 0, 0x1a, 0, 0, 0]
        );
        assert_eq!(
            build_set_gate_reduction(20).unwrap(),
            [0x18, 0x0b, 0x20, 0x80, 0x01, 0x28, 0, 0x1a, 0, 0, 0]
        );
        assert_eq!(
            build_set_gate_reduction(100).unwrap(),
            [0x18, 0x0b, 0x20, 0xd0, 0x01, 0x28, 0, 0x1a, 0, 0, 0]
        );
    }

    #[test]
    fn gate_reduction_builder_rejects_values_above_one_hundred() {
        assert!(matches!(
            build_set_gate_reduction(101),
            Err(Error::InvalidParameter(_))
        ));
    }

    #[test]
    fn bypass_builder_covers_only_gate_and_the_five_variable_fx_roles() {
        assert_eq!(
            build_set_bypass(NanoBypassTarget::PreFx1, false),
            [0x08, 1, 0x18, 4, 0x20, 0, 0x1f, 0, 0, 0]
        );
        assert_eq!(
            build_set_bypass(NanoBypassTarget::Gate, true),
            [0x08, 1, 0x18, 9, 0x20, 1, 0x1f, 0, 0, 0]
        );
    }

    #[test]
    fn fx_param_refresh_builder_uses_typed_slot_ids() {
        assert_eq!(
            build_fx_param_refresh(NanoFxSlot::PreFx1),
            [0x08, 0x03, 0x18, 0, 0x89, 0, 0, 0]
        );
        assert_eq!(
            build_fx_param_refresh(NanoFxSlot::PostFx3),
            [0x08, 0x03, 0x18, 4, 0x89, 0, 0, 0]
        );
    }

    #[test]
    fn fx_param_refresh_reply_requires_its_measured_footer() {
        let mut response = vec![0x08, 0x06, 0x22, 8];
        response.extend(0.25_f32.to_le_bytes());
        response.extend(0.75_f32.to_le_bytes());
        response.extend(FX_PARAM_REFRESH_FOOTER.0);
        assert_eq!(
            decode_fx_param_refresh_response(&response).unwrap(),
            Some(vec![0.25, 0.75])
        );

        *response.last_mut().unwrap() = 1;
        assert!(decode_fx_param_refresh_response(&response).is_err());
        assert!(matches!(
            decode_fx_param_refresh_response(&[0x08, 0x01]),
            Ok(None)
        ));
    }

    #[test]
    fn fx_param_refresh_preserves_the_measured_busy_confirmation() {
        let mut response = Vec::new();
        field_varint(&mut response, 1, 3);
        field_bytes(&mut response, 4, b"Device is busy!");
        response.extend(CONFIRMATION_FOOTER.0);

        assert!(matches!(
            decode_fx_param_refresh_response(&response),
            Err(Error::DeviceBusy {
                device: crate::DeviceKind::NanoCortex
            })
        ));
    }

    #[test]
    fn fx_param_refresh_rejects_non_normalized_values() {
        for value in [1.5, f32::NAN] {
            let mut response = vec![0x08, 0x06, 0x22, 4];
            response.extend(value.to_le_bytes());
            response.extend(FX_PARAM_REFRESH_FOOTER.0);
            assert!(matches!(
                decode_fx_param_refresh_response(&response),
                Err(Error::Decode(_))
            ));
        }
    }

    #[test]
    fn fx_param_refresh_uses_the_hid_link_and_skips_unrelated_messages() {
        let link = FakeLink::new();
        let unrelated = encode_reports(HidReportGeometry::NANO_CORTEX, &[0x08, 0x01, 0, 0, 0, 0]);
        let mut response = vec![0x08, 0x06, 0x22, 4];
        response.extend(0.5_f32.to_le_bytes());
        response.extend(FX_PARAM_REFRESH_FOOTER.0);
        let response = encode_reports(HidReportGeometry::NANO_CORTEX, &response);
        for report in unrelated.into_iter().chain(response) {
            link.push_inbound(report);
        }

        assert_eq!(
            read_fx_params_from_link(
                &link,
                NanoFxSlot::PostFx1,
                std::time::Duration::from_millis(20)
            )
            .unwrap(),
            vec![0.5]
        );
        assert_eq!(
            link.written(),
            encode_reports(
                HidReportGeometry::NANO_CORTEX,
                &build_fx_param_refresh(NanoFxSlot::PostFx1)
            )
        );
    }

    #[test]
    fn fx_param_write_builder_encodes_little_endian_float_with_footer_63() {
        let body = build_fx_param_write(NanoFxSlot::PostFx1, 0, 0.5).unwrap();
        assert_eq!(
            body,
            vec![
                0x08, 0x01, 0x18, 2, 0x20, 0, 0x2d, 0x00, 0x00, 0x00, 0x3f, 0x63, 0, 0, 0
            ]
        );
    }

    #[test]
    fn fx_param_write_rejects_invalid_values_instead_of_mutating_the_device() {
        for value in [-1.0, 2.0, f32::NAN, f32::INFINITY] {
            assert!(matches!(
                build_fx_param_write(NanoFxSlot::PreFx1, 1, value),
                Err(Error::InvalidParameter(_))
            ));
        }
    }

    #[test]
    fn fictional_current_state_decodes_to_one_ordered_eight_role_chain() {
        let state = decode_current_state(&fictional_state()).unwrap();
        assert_eq!(state.firmware.as_deref(), Some("NC-FICTION-1.2.3"));
        assert_eq!(state.amp.gain, Some(101));
        assert_eq!(state.amp.mid, None);
        assert_eq!(state.gate_reduction, Some(50));
        assert_eq!(state.slots.len(), 8);
        assert_eq!(
            state.slots.iter().map(|slot| slot.role).collect::<Vec<_>>(),
            vec![
                NanoSlotRole::Gate,
                NanoSlotRole::PreFx1,
                NanoSlotRole::PreFx2,
                NanoSlotRole::Capture,
                NanoSlotRole::IrCab,
                NanoSlotRole::PostFx1,
                NanoSlotRole::PostFx2,
                NanoSlotRole::PostFx3
            ]
        );
        assert_eq!(state.slots[1].model_id, Some(4));
        assert_eq!(state.slots[1].model_name.as_deref(), Some("Rodent Drive"));
        assert_eq!(
            state.slots[2].model_name.as_deref(),
            Some("Chief CE2W (ST)")
        );
        assert_eq!(state.slots[1].bypassed, Some(false));
        assert_eq!(state.slots[2].bypassed, Some(true));
        assert_eq!(
            state.slots[3].loaded_name.as_deref(),
            Some("Fictional Capture")
        );
        assert_eq!(
            state.slots[4].loaded_name.as_deref(),
            Some("Fictional Cabinet")
        );
    }

    #[test]
    fn nano_model_names_are_product_specific_and_unknown_ids_remain_unknown() {
        assert_eq!(fx_model_name(18_001), Some("Transpose"));
        assert_eq!(fx_model_name(6_010), Some("Analog Delay"));
        assert_eq!(fx_model_name(99_999), None);
    }

    #[test]
    fn every_known_nano_fx_model_has_a_nonempty_parameter_profile() {
        let known_ids = [
            2, 3, 4, 6, 13, 18, 22, 23, 27, 3_000, 3_007, 4_001, 4_003, 4_005, 5_001, 5_004, 5_005,
            5_007, 5_012, 5_013, 5_014, 5_015, 6_004, 6_010, 6_011, 6_012, 6_014, 6_015, 7_004,
            7_021, 7_022, 7_023, 7_024, 7_027, 7_028, 7_029, 8_000, 8_003, 8_007, 8_008, 8_009,
            8_011, 9_010, 9_012, 9_013, 9_014, 16_001, 16_002, 16_006, 16_011, 18_001, 24_001,
            24_006,
        ];
        assert_eq!(known_ids.len(), 53);
        for model_id in known_ids {
            let profile = fx_model_profile(model_id).expect("known Nano model needs a profile");
            assert!(!profile.name.is_empty());
            assert!(!profile.parameter_names.is_empty(), "model {model_id}");
            assert!(profile.parameter_names.iter().all(|name| !name.is_empty()));
        }
    }

    #[test]
    fn nano_fx_parameters_keep_wire_indices_and_known_names() {
        assert_eq!(
            describe_fx_params(Some(27), &[0.25, 0.5, 0.75]),
            vec![
                NanoFxParameter {
                    index: 0,
                    name: Some("Overdrive".into()),
                    normalized: 0.25,
                },
                NanoFxParameter {
                    index: 1,
                    name: Some("Tone".into()),
                    normalized: 0.5,
                },
                NanoFxParameter {
                    index: 2,
                    name: Some("Level".into()),
                    normalized: 0.75,
                },
            ]
        );
    }

    #[test]
    fn analog_delay_parameter_profile_retains_wire_index_order() {
        assert_eq!(
            fx_model_profile(6_010).unwrap().parameter_names,
            [
                "Mix",
                "Feedback",
                "High Pass",
                "Low Pass",
                "Ping Pong",
                "Delay Time",
                "Mod Rate",
                "Mod Depth",
                "Width",
                "Drive",
                "Sync",
                "Sync Note",
            ]
        );
    }

    #[test]
    fn nano_fx_parameters_preserve_unknown_models_and_profile_extensions() {
        let unknown = describe_fx_params(Some(99_999), &[0.25]);
        assert_eq!(unknown[0].index, 0);
        assert_eq!(unknown[0].name, None);
        assert!((unknown[0].normalized - 0.25).abs() < f32::EPSILON);

        let extended = describe_fx_params(Some(3), &[0.1, 0.2, 0.3]);
        assert_eq!(extended[0].name.as_deref(), Some("Gain"));
        assert_eq!(extended[1].name.as_deref(), Some("Volume"));
        assert_eq!(extended[2].index, 2);
        assert_eq!(extended[2].name, None);
        assert!((extended[2].normalized - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn shared_nano_framing_reassembles_before_typed_decode() {
        let message = fictional_state();
        let reports = encode_reports(HidReportGeometry::NANO_CORTEX, &message);
        assert!(reports.len() > 1);
        let mut reassembler = FrameReassembler::new();
        let mut decoded = None;
        for report in reports {
            let frame = Frame::parse(&report).unwrap();
            if let Some(message) = reassembler.feed(&frame).unwrap() {
                decoded = Some(decode_current_state(&message).unwrap());
            }
        }
        assert_eq!(decoded.unwrap().capture_slot, Some(7));
    }

    #[test]
    fn malformed_envelopes_and_fields_fail_closed() {
        assert!(decode_current_state(&[1, 2, 3]).is_err());
        let mut wrong_footer = fictional_state();
        *wrong_footer.last_mut().unwrap() = 9;
        assert!(decode_current_state(&wrong_footer).is_err());
        let malformed = [0x1a, 0x80, 0x02, 0x02, 0, 0, 0];
        assert!(decode_current_state(&malformed).is_err());
    }

    #[test]
    fn exact_bluetooth_ownership_confirmation_is_a_typed_busy_error() {
        let mut message = Vec::new();
        field_varint(&mut message, 1, 3);
        field_bytes(&mut message, 4, b"Device is busy!");
        message.extend(CONFIRMATION_FOOTER.0);
        assert!(matches!(
            decode_current_state(&message),
            Err(Error::DeviceBusy {
                device: crate::DeviceKind::NanoCortex
            })
        ));

        message[4] = b'd';
        assert!(matches!(
            decode_current_state(&message),
            Err(Error::Decode(_))
        ));
    }

    #[test]
    fn typed_state_round_trips_through_the_future_host_contract() {
        let state = decode_current_state(&fictional_state()).unwrap();
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(
            serde_json::from_str::<NanoCurrentState>(&json).unwrap(),
            state
        );
    }
}

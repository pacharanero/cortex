// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Grid-edit message construction.
//!
//! Every function here builds a grid-edit protobuf payload and nothing else -
//! no I/O, no session, no device. That is deliberate: the grid is where the
//! protocol's silent-no-op traps live, and a wrong message is accepted on the
//! wire and simply does nothing. Keeping construction pure means each trap
//! can be pinned by a test rather than discovered on hardware.
//!
//! ## The shape that works
//!
//! A grid edit is a SPARSE, KEYED update: a `BinaryPreset` carrying only the
//! elements being changed, each addressed by explicit `row` (and `column` for
//! a model). The device locates each element by that key.
//!
//! A preset freshly read from a recall carries NO explicit `row`, so writing
//! one back wholesale does nothing at all - confirmed on hardware upstream: a
//! full-preset write that re-pointed `in_portid` read back unchanged. Use the
//! keyed builders here, never a round-tripped preset.
//!
//! ## Rows are zero-based; the unit shows 1-4
//!
//! `row = 0` is the top row on screen and `row = 2` is the one labelled 3.
//! This fails QUIETLY: an edit lands on a real row, just not the intended
//! one, and reads back perfectly. [`Row`] exists so a caller has to say which
//! convention it is using.
//!
//! @see spec/150-client/spec.md [FR-31] [FR-38]
//! @see spec/130-domain-model/spec.md

use crate::proto::{
    BinaryPreset, Bypass, Chain, ColBypass, Expression, ExpressionBypassInfo, GridMessage,
    GridMoveElement, GridMoveMessage, Model, Param, ParamValue, SceneBypass, StompModeAssignment,
    binary_preset, bypass, chain, col_bypass, grid_message, message_action::Enum as MessageAction,
    model, param, param_value,
};

/// A grid row, which exists to stop the zero-based/one-based confusion being
/// silent.
///
/// The wire is zero-based; the unit's screen labels rows 1-4. An edit sent to
/// the wrong row succeeds, reads back correctly, and changes the wrong thing,
/// so the two conventions must never be implicitly interchangeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Row(u32);

impl Row {
    /// From a zero-based wire index, as stored in a preset.
    #[must_use]
    pub const fn from_wire(row: u32) -> Self {
        Self(row)
    }

    /// From an untrusted zero-based wire index.
    ///
    /// Use this for CLI, socket, or other external input. [`Row::from_wire`]
    /// remains the infallible constructor for indices already obtained from a
    /// decoded four-row preset.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidRow`] unless `row` is 0-3.
    pub fn try_from_wire(row: u32) -> crate::Result<Self> {
        if row > 3 {
            return Err(crate::Error::InvalidRow(format!(
                "wire rows are numbered 0-3, got {row}"
            )));
        }
        Ok(Self(row))
    }

    /// From the 1-4 label shown on the unit.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidRow`] unless `row` is 1-4. Passing 0
    /// here almost certainly means a wire index was passed to the wrong
    /// constructor.
    pub fn from_screen(row: u32) -> crate::Result<Self> {
        if !(1..=4).contains(&row) {
            return Err(crate::Error::InvalidRow(format!(
                "screen rows are numbered 1-4, got {row}"
            )));
        }
        Ok(Self(row - 1))
    }

    /// The zero-based wire index.
    #[must_use]
    pub const fn wire(self) -> u32 {
        self.0
    }

    /// The 1-based number shown on the unit.
    #[must_use]
    pub const fn screen(self) -> u32 {
        self.0 + 1
    }

    /// Whether a splitter or mixer can exist on this row.
    ///
    /// A branch can only originate on row 0 or row 2, whose parallel lane is
    /// the row below it. Rows 1 and 3 report an empty splitter/mixer
    /// collection and a write addressed there does nothing.
    #[must_use]
    pub const fn can_branch(self) -> bool {
        self.0 % 2 == 0
    }
}

/// A complete known input destination for one grid row.
///
/// This is deliberately distinct from [`crate::InputPort`], which models the
/// narrower set of physical inputs accepted by I/O Settings. Grid routing also
/// accepts USB, previous-row, empty, and sidechain sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum GridInputPort {
    /// No input.
    Empty = 0,
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
    /// Output of the preceding grid row.
    PreviousRow = 7,
    /// USB input 5.
    Usb5 = 8,
    /// USB input 6.
    Usb6 = 9,
    /// USB input 7.
    Usb7 = 10,
    /// USB input 8.
    Usb8 = 11,
    /// Combined USB input 5/6.
    Usb56 = 12,
    /// Combined USB input 7/8.
    Usb78 = 13,
    /// Internal sidechain buffer.
    SidechainBuffer = 14,
}

/// A complete known output destination for one grid row.
///
/// This is deliberately distinct from [`crate::OutputPort`], which models the
/// narrower I/O Settings controls. Grid routing also accepts USB and internal
/// row-to-row destinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum GridOutputPort {
    /// No output.
    Empty = 0,
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
    /// USB output 5.
    Usb5 = 10,
    /// USB output 6.
    Usb6 = 11,
    /// USB output 7.
    Usb7 = 12,
    /// USB output 8.
    Usb8 = 13,
    /// Combined USB output 5/6.
    Usb56 = 14,
    /// Combined USB output 7/8.
    Usb78 = 15,
    /// Internal destination row 3.
    NextRow3 = 16,
    /// Internal destination row 4.
    NextRow4 = 17,
    /// Combined internal destination rows 3/4.
    NextRow34 = 18,
    /// Multiple device-selected outputs.
    Multiple = 19,
    /// USB output 3.
    Usb3 = 20,
    /// USB output 4.
    Usb4 = 21,
    /// Combined USB output 3/4.
    Usb34 = 22,
}

macro_rules! routing_port_impl {
    ($type:ty, $label:literal, [$($variant:ident = $id:literal => $name:literal),+ $(,)?]) => {
        impl $type {
            /// Every valid value, in wire order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Stable name used by CLI, daemon, and MCP surfaces.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }
        }

        impl From<$type> for u32 {
            fn from(value: $type) -> Self {
                value as Self
            }
        }

        impl TryFrom<u32> for $type {
            type Error = crate::Error;

            fn try_from(value: u32) -> crate::Result<Self> {
                match value {
                    $($id => Ok(Self::$variant),)+
                    _ => Err(crate::Error::InvalidParameter(format!(
                        "unknown {} grid-routing port id {value}", $label
                    ))),
                }
            }
        }

        impl std::str::FromStr for $type {
            type Err = crate::Error;

            fn from_str(value: &str) -> crate::Result<Self> {
                match value {
                    $($name => Ok(Self::$variant),)+
                    _ => Err(crate::Error::InvalidParameter(format!(
                        "unknown {} grid-routing port {value:?}; expected one of: {}",
                        $label,
                        Self::ALL.iter().map(|port| port.as_str()).collect::<Vec<_>>().join(", ")
                    ))),
                }
            }
        }

        impl std::fmt::Display for $type {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

routing_port_impl!(
    GridInputPort,
    "input",
    [
        Empty = 0 => "empty",
        Input1 = 1 => "input1",
        Input2 = 2 => "input2",
        Input12 = 3 => "input12",
        Return1 = 4 => "return1",
        Return2 = 5 => "return2",
        Return12 = 6 => "return12",
        PreviousRow = 7 => "previous_row",
        Usb5 = 8 => "usb5",
        Usb6 = 9 => "usb6",
        Usb7 = 10 => "usb7",
        Usb8 = 11 => "usb8",
        Usb56 = 12 => "usb56",
        Usb78 = 13 => "usb78",
        SidechainBuffer = 14 => "sidechain_buffer",
    ]
);

routing_port_impl!(
    GridOutputPort,
    "output",
    [
        Empty = 0 => "empty",
        Xlr12 = 1 => "xlr12",
        Out34 = 2 => "out34",
        Send12 = 3 => "send12",
        Xlr1 = 4 => "xlr1",
        Xlr2 = 5 => "xlr2",
        Out3 = 6 => "out3",
        Out4 = 7 => "out4",
        Send1 = 8 => "send1",
        Send2 = 9 => "send2",
        Usb5 = 10 => "usb5",
        Usb6 = 11 => "usb6",
        Usb7 = 12 => "usb7",
        Usb8 = 13 => "usb8",
        Usb56 = 14 => "usb56",
        Usb78 = 15 => "usb78",
        NextRow3 = 16 => "next_row3",
        NextRow4 = 17 => "next_row4",
        NextRow34 = 18 => "next_row34",
        Multiple = 19 => "multiple",
        Usb3 = 20 => "usb3",
        Usb4 = 21 => "usb4",
        Usb34 = 22 => "usb34",
    ]
);

/// A value to write into a parameter.
///
/// Not every parameter is a number: a cabinet's microphone selection is a
/// string, and a capture's identity is a string. The device stores whichever
/// it is given, so the distinction has to survive to the wire.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Value {
    /// A normalised 0..1 float, which is what the wire carries for a knob.
    Normalised(f32),
    /// A string, e.g. a microphone selection or a capture key.
    Text(String),
}

/// A row-level control stored outside `chain.models` but addressed by a fixed
/// model id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubControl {
    /// The mixer where an even row's two lanes recombine (model 11000).
    Mixer,
    /// One row's output volume, pan, mute, and solo controls (model 23000).
    LaneOutput,
    /// One row's input noise gate (model 28000).
    InputGate,
}

/// Model id of the preset-local tempo and metronome control.
pub const TEMPO_CONTROL: u32 = 25_000;

impl SubControl {
    const fn model_id(self) -> u32 {
        match self {
            Self::Mixer => 11_000,
            Self::LaneOutput => 23_000,
            Self::InputGate => 28_000,
        }
    }
}

impl Value {
    fn into_param_value(self) -> ParamValue {
        ParamValue {
            value: Some(match self {
                Self::Normalised(v) => param_value::Value::FloatValue(v),
                Self::Text(v) => param_value::Value::StringValue(v),
            }),
        }
    }
}

/// Wrap a sparse preset in a `GridMessage` with the given action.
fn grid(action: MessageAction, preset: BinaryPreset) -> GridMessage {
    GridMessage {
        action: action as i32,
        request_id: None,
        preset: Some(grid_message::Preset::Preset(preset)),
        update_type: None,
    }
}

/// A preset holding one keyed chain.
fn preset_with_chain(chain: Chain) -> BinaryPreset {
    BinaryPreset {
        chains: vec![chain],
        ..Default::default()
    }
}

/// A chain keyed by row and nothing else.
fn keyed_chain(row: Row) -> Chain {
    Chain {
        row: Some(chain::Row::Row(row.wire())),
        ..Default::default()
    }
}

/// A model keyed by column and nothing else.
fn keyed_model(column: u32) -> Model {
    Model {
        column: Some(model::Column::Column(column)),
        ..Default::default()
    }
}

fn normalised_param(param_index: u32, value: f32) -> crate::Result<Param> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(crate::Error::InvalidParameter(format!(
            "normalised values are 0.0-1.0, got {value}"
        )));
    }
    Ok(Param {
        index: Some(param::Index::Index(param_index)),
        param_values: vec![Value::Normalised(value).into_param_value()],
        ..Default::default()
    })
}

fn scene_mode_param(param_index: u32, enabled: bool) -> Param {
    Param {
        index: Some(param::Index::Index(param_index)),
        scene_mode: Some(param::SceneMode::SceneMode(enabled)),
        ..Default::default()
    }
}

fn require_branch_row(row: Row, control: &str) -> crate::Result<()> {
    if !matches!(row.wire(), 0 | 2) {
        return Err(crate::Error::InvalidRow(format!(
            "row {} (screen row {}) has no {control}: only wire rows 0 and 2 can branch",
            row.wire(),
            row.screen()
        )));
    }
    Ok(())
}

fn require_grid_row(row: Row) -> crate::Result<()> {
    if row.wire() > 3 {
        return Err(crate::Error::InvalidRow(format!(
            "wire rows are numbered 0-3, got {}",
            row.wire()
        )));
    }
    Ok(())
}

fn require_grid_cell(row: Row, column: u32) -> crate::Result<()> {
    require_grid_row(row)?;
    if column > 7 {
        return Err(crate::Error::InvalidParameter(format!(
            "grid columns are numbered 0-7, got {column}"
        )));
    }
    Ok(())
}

/// Remove any STOMP assignment for one grid cell.
///
/// This is the first half of assignment: the device requires DELETE followed
/// by UPDATE, and an UPDATE alone leaves the previous assignment in place.
///
/// # Errors
///
/// Returns an invalid row or parameter error unless the cell is within the
/// four-by-eight grid.
pub fn clear_stomp_assignment(row: Row, column: u32) -> crate::Result<GridMessage> {
    require_grid_cell(row, column)?;
    Ok(grid(
        MessageAction::Delete,
        BinaryPreset {
            stomp_mode_assignments: vec![StompModeAssignment {
                row: row.wire(),
                column,
                ..Default::default()
            }],
            ..Default::default()
        },
    ))
}

/// Build the UPDATE half of assigning one cell to a STOMP footswitch.
///
/// # Errors
///
/// Returns an invalid row or parameter error unless the cell is within the
/// four-by-eight grid and the footswitch is 0-7.
pub fn set_stomp_assignment(row: Row, column: u32, footswitch: u32) -> crate::Result<GridMessage> {
    require_grid_cell(row, column)?;
    if footswitch > 7 {
        return Err(crate::Error::InvalidParameter(format!(
            "footswitches are numbered 0-7 (A-H), got {footswitch}"
        )));
    }
    Ok(grid(
        MessageAction::Update,
        BinaryPreset {
            stomp_mode_assignments: vec![StompModeAssignment {
                row: row.wire(),
                column,
                stomp_index: footswitch,
            }],
            ..Default::default()
        },
    ))
}

/// Set whether one preset-local STOMP footswitch is momentary.
///
/// # Errors
///
/// Returns an invalid parameter error unless the footswitch is 0-7.
pub fn set_stomp_momentary(footswitch: u32, momentary: bool) -> crate::Result<GridMessage> {
    if footswitch > 7 {
        return Err(crate::Error::InvalidParameter(format!(
            "footswitches are numbered 0-7 (A-H), got {footswitch}"
        )));
    }
    Ok(grid(
        MessageAction::Update,
        BinaryPreset {
            stomp_is_momentary: [(footswitch, momentary)].into_iter().collect(),
            ..Default::default()
        },
    ))
}

/// Set one entry in either preset-local STOMP label map.
///
/// # Errors
///
/// Returns an invalid parameter error unless the footswitch is 0-7.
pub fn set_stomp_label(footswitch: u32, label: String, single: bool) -> crate::Result<GridMessage> {
    if footswitch > 7 {
        return Err(crate::Error::InvalidParameter(format!(
            "footswitches are numbered 0-7 (A-H), got {footswitch}"
        )));
    }
    let mut preset = BinaryPreset::default();
    if single {
        preset.single_stomp_labels.insert(footswitch, label);
    } else {
        preset.stomp_labels.insert(footswitch, label);
    }
    Ok(grid(MessageAction::Update, preset))
}

/// Assign one expression pedal and normalized sweep range to a block parameter.
///
/// # Errors
///
/// Returns an invalid row or parameter error for a bad cell, a pedal other than
/// 1 or 2, or a non-finite or out-of-range endpoint.
pub fn set_expression(
    row: Row,
    column: u32,
    param_index: u32,
    pedal: i32,
    minimum: f32,
    maximum: f32,
) -> crate::Result<GridMessage> {
    require_grid_cell(row, column)?;
    if !matches!(pedal, 1 | 2) {
        return Err(crate::Error::InvalidParameter(format!(
            "expression pedals are numbered 1 or 2, got {pedal}"
        )));
    }
    normalised_param(param_index, minimum)?;
    normalised_param(param_index, maximum)?;
    let mut chain = keyed_chain(row);
    let mut model = keyed_model(column);
    model.params.push(Param {
        index: Some(param::Index::Index(param_index)),
        expression: Some(param::Expression::Expression(pedal)),
        expression_min: Some(param::ExpressionMin::ExpressionMin(minimum)),
        expression_max: Some(param::ExpressionMax::ExpressionMax(maximum)),
        ..Default::default()
    });
    chain.models.push(model);
    Ok(grid(MessageAction::Update, preset_with_chain(chain)))
}

/// Assign one expression pedal to block bypass, including its behavior.
///
/// # Errors
///
/// Returns an invalid row or parameter error for a bad cell, pedal, mode, or
/// delay above 5000 ms.
pub fn set_expression_bypass(
    row: Row,
    column: u32,
    pedal: i32,
    mode: u32,
    invert: bool,
    delay_ms: u32,
    latch_emulation: bool,
) -> crate::Result<GridMessage> {
    require_grid_cell(row, column)?;
    if !matches!(pedal, 1 | 2) {
        return Err(crate::Error::InvalidParameter(format!(
            "expression pedals are numbered 1 or 2, got {pedal}"
        )));
    }
    if mode > 2 {
        return Err(crate::Error::InvalidParameter(format!(
            "expression bypass modes are numbered 0-2, got {mode}"
        )));
    }
    if delay_ms > 5_000 {
        return Err(crate::Error::InvalidParameter(format!(
            "expression bypass delay is 0-5000 ms, got {delay_ms}"
        )));
    }
    let mut chain = keyed_chain(row);
    let mut model = keyed_model(column);
    model.bypass_expression.push(Expression {
        expression: pedal,
        expression_min: 0.0,
        expression_max: 1.0,
    });
    model.expression_bypass_info.push(ExpressionBypassInfo {
        r#type: mode,
        invert,
        delay_ms,
        latch_emulation,
    });
    chain.models.push(model);
    Ok(grid(MessageAction::Update, preset_with_chain(chain)))
}

/// Re-point one grid row's INPUT.
///
/// This row-keyed shape is the only one that actually moves an input; a
/// full-preset write whose chains lack an explicit `row` is accepted and
/// ignored.
#[must_use]
pub fn set_chain_input(row: Row, port: GridInputPort) -> GridMessage {
    let mut chain = keyed_chain(row);
    chain.in_portid = Some(chain::InPortid::InPortid(port.into()));
    grid(MessageAction::Update, preset_with_chain(chain))
}

/// Re-point one grid row's OUTPUT.
///
/// The underlying wire field accepts meaningless ids and stores them rather
/// than rejecting them. The closed [`GridOutputPort`] prevents that mistake at
/// this public boundary. Values 16 to 18 are named internal row-to-row routes;
/// 19 is the real `multiple` output.
#[must_use]
pub fn set_chain_output(row: Row, port: GridOutputPort) -> GridMessage {
    let mut chain = keyed_chain(row);
    chain.out_portid = Some(chain::OutPortid::OutPortid(port.into()));
    grid(MessageAction::Update, preset_with_chain(chain))
}

/// Set one block parameter.
///
/// The value lands on whichever scene is ACTIVE. `param_values[0]` is not a
/// scene selector - the device applies the first entry to the current scene
/// and ignores any beyond it - so nothing is ever padded here.
///
/// For a per-scene value the caller must first promote the parameter with
/// [`set_param_scene_mode`], then switch scene, then write. Those cannot be
/// combined; see that function.
#[must_use]
pub fn set_param(row: Row, column: u32, param_index: u32, value: Value) -> GridMessage {
    let mut chain = keyed_chain(row);
    let mut model = keyed_model(column);
    model.params.push(Param {
        index: Some(param::Index::Index(param_index)),
        param_values: vec![value.into_param_value()],
        ..Default::default()
    });
    chain.models.push(model);
    grid(MessageAction::Update, preset_with_chain(chain))
}

/// Make a block parameter follow scenes, or stop it following them.
///
/// **This flag must travel ALONE.** A `Grid` update carrying both
/// `scene_mode` and a `param_values` entry is treated as a plain value write
/// and the flag is silently dropped. That is why this is a separate builder
/// rather than an argument to [`set_param`]: the type system should not let a
/// caller express the combination that does not work.
///
/// A parameter only keeps per-scene values while this is set; until then it
/// has a single global value.
#[must_use]
pub fn set_param_scene_mode(row: Row, column: u32, param_index: u32, enabled: bool) -> GridMessage {
    let mut chain = keyed_chain(row);
    let mut model = keyed_model(column);
    model.params.push(Param {
        index: Some(param::Index::Index(param_index)),
        scene_mode: Some(param::SceneMode::SceneMode(enabled)),
        // Deliberately no param_values: see above.
        ..Default::default()
    });
    chain.models.push(model);
    grid(MessageAction::Update, preset_with_chain(chain))
}

/// Set one preset-local tempo or metronome parameter.
///
/// Tempo is the exception to ordinary grid keying: `tempo_program_data` sits
/// outside `chains` and is applied without a row or column key. The model hash
/// and parameter index remain explicit.
///
/// # Errors
///
/// Returns [`crate::Error::InvalidParameter`] unless `value` is finite and in
/// 0..=1.
pub fn set_tempo_param(param_index: u32, value: f32) -> crate::Result<GridMessage> {
    let preset = BinaryPreset {
        tempo_program_data: vec![Model {
            hash: Some(model::Hash::Hash(TEMPO_CONTROL)),
            params: vec![normalised_param(param_index, value)?],
            ..Default::default()
        }],
        ..Default::default()
    };
    Ok(grid(MessageAction::Update, preset))
}

/// Set one combined-splitter parameter on an even row.
///
/// This writes `combined_splitter`, never the read-only `splitter` view, and
/// deliberately supplies no model hash because the device's own writable
/// shape has none.
///
/// # Errors
///
/// Returns [`crate::Error::InvalidRow`] for an odd row or
/// [`crate::Error::InvalidParameter`] unless `value` is finite and in 0..=1.
pub fn set_splitter_param(row: Row, param_index: u32, value: f32) -> crate::Result<GridMessage> {
    require_branch_row(row, "splitter")?;
    let mut chain = keyed_chain(row);
    chain.combined_splitter.push(Model {
        params: vec![normalised_param(param_index, value)?],
        ..Default::default()
    });
    Ok(grid(MessageAction::Update, preset_with_chain(chain)))
}

/// Set a combined-splitter parameter's scene-following flag, without a value.
///
/// # Errors
///
/// Returns [`crate::Error::InvalidRow`] for an odd row.
pub fn set_splitter_param_scene_mode(
    row: Row,
    param_index: u32,
    enabled: bool,
) -> crate::Result<GridMessage> {
    require_branch_row(row, "splitter")?;
    let mut chain = keyed_chain(row);
    chain.combined_splitter.push(Model {
        params: vec![scene_mode_param(param_index, enabled)],
        ..Default::default()
    });
    Ok(grid(MessageAction::Update, preset_with_chain(chain)))
}

/// Set one mixer, lane-output, or input-gate parameter.
///
/// The collection and its required model id are selected together by
/// [`SubControl`], preventing a valid id from being paired with the wrong
/// repeated field.
///
/// # Errors
///
/// Returns [`crate::Error::InvalidRow`] when a mixer is addressed on an odd
/// row or [`crate::Error::InvalidParameter`] unless `value` is finite and in
/// 0..=1.
pub fn set_sub_control_param(
    row: Row,
    control: SubControl,
    param_index: u32,
    value: f32,
) -> crate::Result<GridMessage> {
    if control == SubControl::Mixer {
        require_branch_row(row, "mixer")?;
    } else {
        require_grid_row(row)?;
    }
    let mut chain = keyed_chain(row);
    let model = Model {
        hash: Some(model::Hash::Hash(control.model_id())),
        params: vec![normalised_param(param_index, value)?],
        ..Default::default()
    };
    match control {
        SubControl::Mixer => chain.mixer.push(model),
        SubControl::LaneOutput => chain.output_control.push(model),
        SubControl::InputGate => chain.input_control.push(model),
    }
    Ok(grid(MessageAction::Update, preset_with_chain(chain)))
}

/// Set a mixer, lane-output, or input-gate parameter's scene-following flag.
///
/// The flag travels without a value because packing both silently drops the
/// flag.
///
/// # Errors
///
/// Returns [`crate::Error::InvalidRow`] when a mixer is addressed on an odd
/// row.
pub fn set_sub_control_param_scene_mode(
    row: Row,
    control: SubControl,
    param_index: u32,
    enabled: bool,
) -> crate::Result<GridMessage> {
    if control == SubControl::Mixer {
        require_branch_row(row, "mixer")?;
    } else {
        require_grid_row(row)?;
    }
    let mut chain = keyed_chain(row);
    let model = Model {
        hash: Some(model::Hash::Hash(control.model_id())),
        params: vec![scene_mode_param(param_index, enabled)],
        ..Default::default()
    };
    match control {
        SubControl::Mixer => chain.mixer.push(model),
        SubControl::LaneOutput => chain.output_control.push(model),
        SubControl::InputGate => chain.input_control.push(model),
    }
    Ok(grid(MessageAction::Update, preset_with_chain(chain)))
}

/// Mute or unmute the split/mix path on an even row.
///
/// The writable field is `split_bypass`; `mix_bypass` is the read-back field
/// and writes to it are ignored. One entry changes all eight scenes.
///
/// # Errors
///
/// Returns [`crate::Error::InvalidRow`] for an odd row.
pub fn set_split_mute(row: Row, muted: bool) -> crate::Result<GridMessage> {
    require_branch_row(row, "splitter or mixer")?;
    let mut chain = keyed_chain(row);
    chain.split_bypass.push(SceneBypass { bypass: muted });
    Ok(grid(MessageAction::Update, preset_with_chain(chain)))
}

/// Bypass or enable one block.
///
/// Like a parameter value, `sceneBypass[0]` applies to whichever scene is
/// ACTIVE; entries beyond the first are ignored. For a block that does not
/// follow scenes, bypass is one global state and this write lands on all
/// eight stored scene slots at once.
#[must_use]
pub fn set_bypass(row: Row, column: u32, bypassed: bool) -> GridMessage {
    let preset = BinaryPreset {
        bypass: vec![Bypass {
            row: Some(bypass::Row::Row(row.wire())),
            col_bypass: vec![ColBypass {
                column: Some(col_bypass::Column::Column(column)),
                scene_bypass: vec![SceneBypass { bypass: bypassed }],
                scene_mode: None,
            }],
        }],
        ..Default::default()
    };
    grid(MessageAction::Update, preset)
}

/// Place a model in a grid cell, creating a block or replacing what is there.
///
/// The device makes no distinction between creating and replacing.
///
/// **A placement can be refused for want of DSP capacity.** The preset has a
/// processing budget, and a block that does not fit is accepted on the wire
/// like any other write and is simply absent afterwards. Nothing in the reply
/// says so - every host write is stalled, and there is no per-block error. The
/// only way to know is to watch for the device's `Grid` echo naming the cell.
#[must_use]
pub fn set_block(row: Row, column: u32, model_id: u32) -> GridMessage {
    let mut chain = keyed_chain(row);
    let mut model = keyed_model(column);
    model.hash = Some(model::Hash::Hash(model_id));
    chain.models.push(model);
    grid(MessageAction::Update, preset_with_chain(chain))
}

/// Remove the block at a cell, leaving it empty.
///
/// **The ACTION is what marks the removal.** An `UPDATE` carrying `hash: 0`
/// is transmitted and ignored by the firmware; only `DELETE` removes. The
/// `hash: 0` is still sent because the device's own broadcast carries it when
/// a block is deleted on the unit.
#[must_use]
pub fn remove_block(row: Row, column: u32) -> GridMessage {
    let mut chain = keyed_chain(row);
    let mut model = keyed_model(column);
    model.hash = Some(model::Hash::Hash(0));
    chain.models.push(model);
    grid(MessageAction::Delete, preset_with_chain(chain))
}

/// Move one occupied grid cell to an empty destination.
///
/// A cross-row move asks the device to create or adjust the parallel path; the
/// device computes its own split and rejoin columns. The optional advisory
/// grid snapshot is deliberately absent because it does not drive edits.
#[must_use]
pub fn move_block(
    from_row: Row,
    from_column: u32,
    to_row: Row,
    to_column: u32,
    drop: bool,
) -> GridMoveMessage {
    GridMoveMessage {
        r#move: vec![GridMoveElement {
            from_row: from_row.wire(),
            from_col: from_column,
            to_row: to_row.wire(),
            to_col: to_column,
            is_drop: drop,
        }],
        ..Default::default()
    }
}

/// Set a row's split and mix points, activating a parallel branch.
///
/// `split` is the column at which the row branches and `mix` where it
/// rejoins; `-1` for either means "none", so `mix = -1` is a branch that
/// never rejoins.
///
/// # Errors
///
/// Returns [`crate::Error::InvalidRow`] for an odd row. A branch can only
/// originate on row 0 or row 2, whose parallel lane is the row below it;
/// rows 1 and 3 have no splitter and a write addressed there does nothing.
/// Refusing beats sending a message that is silently discarded.
pub fn set_split(row: Row, split: i32, mix: i32) -> crate::Result<GridMessage> {
    require_branch_row(row, "splitter")?;
    let mut chain = keyed_chain(row);
    chain
        .split_control_points
        .push(crate::proto::SplitControlPoints { split, mix });
    Ok(grid(MessageAction::Update, preset_with_chain(chain)))
}

/// Encode a grid message for the wire.
#[must_use]
pub fn encode(message: &GridMessage) -> Vec<u8> {
    prost::Message::encode_to_vec(message)
}

/// The sparse preset carried by a grid message, for inspection in tests and
/// by callers that want to check what they are about to send.
#[must_use]
pub fn preset_of(message: &GridMessage) -> Option<&BinaryPreset> {
    message.preset.as_ref().map(|p| {
        let grid_message::Preset::Preset(preset) = p;
        preset
    })
}

/// The row a keyed chain addresses, if it carries one.
#[must_use]
fn chain_row(chain: &Chain) -> Option<u32> {
    chain.row.as_ref().map(|r| {
        let chain::Row::Row(v) = r;
        *v
    })
}

/// Whether a preset names an explicit row on every chain it carries.
///
/// A sparse update MUST; a preset round-tripped from a recall does NOT, which
/// is why writing one back does nothing. Callers building their own grid
/// messages can use this as a guard.
#[must_use]
pub fn is_row_keyed(preset: &BinaryPreset) -> bool {
    !preset.chains.is_empty() && preset.chains.iter().all(|c| chain_row(c).is_some())
}

#[allow(unused_imports)]
use binary_preset as _;

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(message: &GridMessage) -> GridMessage {
        let bytes = encode(message);
        prost::Message::decode(bytes.as_slice()).expect("round-trips")
    }

    fn only_chain(message: &GridMessage) -> Chain {
        preset_of(message).expect("has a preset").chains[0].clone()
    }

    // -- row conventions ---------------------------------------------------

    #[test]
    fn screen_rows_convert_to_wire_rows() {
        // The unit labels rows 1-4; the wire is 0-3. Conflating them lands an
        // edit on a real but unintended row, which reads back perfectly.
        assert_eq!(Row::from_screen(1).unwrap().wire(), 0);
        assert_eq!(Row::from_screen(4).unwrap().wire(), 3);
        assert_eq!(Row::from_wire(0).screen(), 1);
        assert_eq!(Row::from_wire(3).screen(), 4);
    }

    #[test]
    fn out_of_range_rows_are_refused() {
        // Screen rows start at 1, so 0 almost certainly means someone passed
        // a wire index to the wrong constructor. Values above the physical
        // four rows must not become valid merely because Row stores a u32.
        assert!(matches!(
            Row::from_screen(0),
            Err(crate::Error::InvalidRow(_))
        ));
        assert!(matches!(
            Row::from_screen(5),
            Err(crate::Error::InvalidRow(_))
        ));
        assert!(matches!(
            Row::try_from_wire(4),
            Err(crate::Error::InvalidRow(_))
        ));
    }

    #[test]
    fn only_even_wire_rows_can_branch() {
        assert!(Row::from_wire(0).can_branch());
        assert!(!Row::from_wire(1).can_branch());
        assert!(Row::from_wire(2).can_branch());
        assert!(!Row::from_wire(3).can_branch());
    }

    #[test]
    fn typed_grid_input_ports_cover_every_known_wire_value() {
        for (id, port) in GridInputPort::ALL.iter().copied().enumerate() {
            let id = u32::try_from(id).unwrap();
            assert_eq!(u32::from(port), id);
            assert_eq!(GridInputPort::try_from(id).unwrap(), port);
            assert_eq!(port.as_str().parse::<GridInputPort>().unwrap(), port);
            assert_eq!(serde_json::to_value(port).unwrap(), port.as_str());
        }
        assert!(
            GridInputPort::try_from(15).is_err(),
            "MAX_PORTS is not a route"
        );
        assert!("15".parse::<GridInputPort>().is_err());
    }

    #[test]
    fn typed_grid_output_ports_cover_every_known_wire_value() {
        for (id, port) in GridOutputPort::ALL.iter().copied().enumerate() {
            let id = u32::try_from(id).unwrap();
            assert_eq!(u32::from(port), id);
            assert_eq!(GridOutputPort::try_from(id).unwrap(), port);
            assert_eq!(port.as_str().parse::<GridOutputPort>().unwrap(), port);
            assert_eq!(serde_json::to_value(port).unwrap(), port.as_str());
        }
        assert!(
            GridOutputPort::try_from(23).is_err(),
            "MAX_PORTS is not a route"
        );
        assert!("23".parse::<GridOutputPort>().is_err());
    }

    // -- keying ------------------------------------------------------------

    #[test]
    fn every_builder_keys_its_chain_by_row() {
        // The single most important property here. An unkeyed chain is
        // accepted by the device and does nothing.
        for message in [
            set_chain_input(Row::from_wire(2), GridInputPort::Input1),
            set_chain_output(Row::from_wire(2), GridOutputPort::Multiple),
            set_param(Row::from_wire(2), 3, 0, Value::Normalised(0.5)),
            set_param_scene_mode(Row::from_wire(2), 3, 0, true),
            set_expression(Row::from_wire(2), 3, 0, 1, 0.0, 1.0).unwrap(),
            set_expression_bypass(Row::from_wire(2), 3, 1, 0, false, 0, false).unwrap(),
            set_splitter_param(Row::from_wire(2), 3, 0.5).unwrap(),
            set_sub_control_param(Row::from_wire(2), SubControl::Mixer, 0, 0.5).unwrap(),
            set_sub_control_param(Row::from_wire(2), SubControl::LaneOutput, 1, 0.5).unwrap(),
            set_sub_control_param(Row::from_wire(2), SubControl::InputGate, 1, 0.5).unwrap(),
            set_split_mute(Row::from_wire(2), true).unwrap(),
            set_block(Row::from_wire(2), 3, 1001),
            remove_block(Row::from_wire(2), 3),
        ] {
            let preset = preset_of(&decode(&message)).expect("has a preset").clone();
            assert!(
                is_row_keyed(&preset),
                "a builder produced a chain with no explicit row"
            );
            assert_eq!(chain_row(&preset.chains[0]), Some(2));
        }
    }

    #[test]
    fn a_recalled_preset_is_not_row_keyed() {
        // What a preset read back from the device looks like: chains present,
        // no explicit row. This is why writing one back wholesale does
        // nothing, and the guard callers can use to notice.
        let recalled = BinaryPreset {
            chains: vec![Chain::default(), Chain::default()],
            ..Default::default()
        };
        assert!(!is_row_keyed(&recalled));
    }

    #[test]
    fn model_edits_are_keyed_by_column() {
        let chain = only_chain(&decode(&set_param(
            Row::from_wire(0),
            5,
            2,
            Value::Normalised(0.25),
        )));
        let column = chain.models[0].column.as_ref().map(|c| {
            let model::Column::Column(v) = c;
            *v
        });
        assert_eq!(column, Some(5));
    }

    // -- parameters --------------------------------------------------------

    #[test]
    fn a_float_parameter_carries_one_value() {
        // param_values[0] applies to the ACTIVE scene; the index is not a
        // scene selector, so nothing is padded.
        let chain = only_chain(&decode(&set_param(
            Row::from_wire(0),
            1,
            7,
            Value::Normalised(0.75),
        )));
        let param = &chain.models[0].params[0];
        assert_eq!(param.param_values.len(), 1);
        assert_eq!(
            param.param_values[0].value,
            Some(param_value::Value::FloatValue(0.75))
        );
        let index = param.index.as_ref().map(|i| {
            let param::Index::Index(v) = i;
            *v
        });
        assert_eq!(index, Some(7));
    }

    #[test]
    fn a_string_parameter_survives_as_a_string() {
        // A cab's microphone selection and a capture's identity are strings,
        // not numbers. Coercing them to a float would store nonsense.
        let chain = only_chain(&decode(&set_param(
            Row::from_wire(0),
            1,
            22,
            Value::Text("NG_212 DG Neo_Condenser U47".into()),
        )));
        assert_eq!(
            chain.models[0].params[0].param_values[0].value,
            Some(param_value::Value::StringValue(
                "NG_212 DG Neo_Condenser U47".into()
            ))
        );
    }

    #[test]
    fn the_scene_mode_flag_travels_without_a_value() {
        // THE trap. A Grid update carrying both scene_mode and a
        // param_values entry is treated as a plain value write and the flag
        // is silently dropped. The two must be separate messages.
        let chain = only_chain(&decode(&set_param_scene_mode(
            Row::from_wire(1),
            4,
            3,
            true,
        )));
        let param = &chain.models[0].params[0];
        assert!(
            param.param_values.is_empty(),
            "scene_mode must travel alone; a value beside it silently drops the flag"
        );
        assert_eq!(param.scene_mode, Some(param::SceneMode::SceneMode(true)));
    }

    #[test]
    fn tempo_write_uses_hash_25000_without_a_row_key() {
        let message = decode(&set_tempo_param(7, 1.0 / 3.0).unwrap());
        assert_eq!(message.action, MessageAction::Update as i32);
        let preset = preset_of(&message).unwrap();
        assert!(preset.chains.is_empty());
        assert_eq!(preset.tempo_program_data.len(), 1);
        let tempo = &preset.tempo_program_data[0];
        assert_eq!(tempo.hash, Some(model::Hash::Hash(TEMPO_CONTROL)));
        assert!(tempo.column.is_none());
        assert_eq!(tempo.params.len(), 1);
        assert_eq!(tempo.params[0].index, Some(param::Index::Index(7)));
        assert_eq!(tempo.params[0].scene_mode, None);
        assert_eq!(
            tempo.params[0].param_values[0].value,
            Some(param_value::Value::FloatValue(1.0 / 3.0))
        );
    }

    #[test]
    fn tempo_write_accepts_boundaries_and_rejects_non_finite_or_out_of_range_values() {
        for boundary in [0.0, 1.0] {
            assert!(set_tempo_param(0, boundary).is_ok());
        }
        for invalid in [-f32::INFINITY, -0.001, 1.001, f32::INFINITY, f32::NAN] {
            assert!(matches!(
                set_tempo_param(0, invalid),
                Err(crate::Error::InvalidParameter(_))
            ));
        }
    }

    #[test]
    fn a_value_write_carries_no_scene_mode_flag() {
        // The converse: a plain value write must not set the flag, or it
        // would promote a parameter the caller never asked to promote.
        let chain = only_chain(&decode(&set_param(
            Row::from_wire(1),
            4,
            3,
            Value::Normalised(0.1),
        )));
        assert_eq!(chain.models[0].params[0].scene_mode, None);
    }

    #[test]
    fn stomp_assignment_preserves_zero_valued_keys_and_actions() {
        let delete = decode(&clear_stomp_assignment(Row::from_wire(0), 0).unwrap());
        assert_eq!(delete.action, MessageAction::Delete as i32);
        let assignment = &preset_of(&delete).unwrap().stomp_mode_assignments[0];
        assert_eq!((assignment.row, assignment.column), (0, 0));

        let update = decode(&set_stomp_assignment(Row::from_wire(0), 0, 0).unwrap());
        assert_eq!(update.action, MessageAction::Update as i32);
        let assignment = &preset_of(&update).unwrap().stomp_mode_assignments[0];
        assert_eq!(
            (assignment.row, assignment.column, assignment.stomp_index),
            (0, 0, 0)
        );
    }

    #[test]
    fn stomp_map_builders_write_only_the_selected_zero_key() {
        let momentary = decode(&set_stomp_momentary(0, true).unwrap());
        let preset = preset_of(&momentary).unwrap();
        assert_eq!(preset.stomp_is_momentary.get(&0), Some(&true));
        assert!(preset.stomp_labels.is_empty());
        assert!(preset.single_stomp_labels.is_empty());

        let general = decode(&set_stomp_label(0, "Group".into(), false).unwrap());
        let preset = preset_of(&general).unwrap();
        assert_eq!(
            preset.stomp_labels.get(&0).map(String::as_str),
            Some("Group")
        );
        assert!(preset.single_stomp_labels.is_empty());

        let single = decode(&set_stomp_label(0, "Single".into(), true).unwrap());
        let preset = preset_of(&single).unwrap();
        assert_eq!(
            preset.single_stomp_labels.get(&0).map(String::as_str),
            Some("Single")
        );
        assert!(preset.stomp_labels.is_empty());
    }

    #[test]
    fn expression_assignment_is_keyed_and_preserves_reversed_range() {
        let chain = only_chain(&decode(
            &set_expression(Row::from_wire(0), 0, 0, 2, 0.9, 0.1).unwrap(),
        ));
        assert_eq!(chain_row(&chain), Some(0));
        assert_eq!(chain.models[0].column, Some(model::Column::Column(0)));
        let parameter = &chain.models[0].params[0];
        assert_eq!(parameter.index, Some(param::Index::Index(0)));
        assert_eq!(parameter.expression, Some(param::Expression::Expression(2)));
        assert_eq!(
            parameter.expression_min,
            Some(param::ExpressionMin::ExpressionMin(0.9))
        );
        assert_eq!(
            parameter.expression_max,
            Some(param::ExpressionMax::ExpressionMax(0.1))
        );
        assert!(parameter.param_values.is_empty());
    }

    #[test]
    fn expression_bypass_writes_both_halves_at_zero_cell() {
        let chain = only_chain(&decode(
            &set_expression_bypass(Row::from_wire(0), 0, 1, 2, true, 250, true).unwrap(),
        ));
        assert_eq!(chain.models[0].column, Some(model::Column::Column(0)));
        assert_eq!(
            chain.models[0].bypass_expression,
            vec![Expression {
                expression: 1,
                expression_min: 0.0,
                expression_max: 1.0,
            }]
        );
        assert_eq!(
            chain.models[0].expression_bypass_info,
            vec![ExpressionBypassInfo {
                r#type: 2,
                invert: true,
                delay_ms: 250,
                latch_emulation: true,
            }]
        );
    }

    #[test]
    fn stomp_and_expression_builders_reject_invalid_addresses_and_values() {
        assert!(set_stomp_assignment(Row::from_wire(4), 0, 0).is_err());
        assert!(clear_stomp_assignment(Row::from_wire(0), 8).is_err());
        assert!(set_stomp_momentary(8, true).is_err());
        assert!(set_expression(Row::from_wire(0), 0, 0, 0, 0.0, 1.0).is_err());
        assert!(set_expression(Row::from_wire(0), 0, 0, 1, -0.1, 1.0).is_err());
        assert!(set_expression(Row::from_wire(0), 0, 0, 2, 0.0, f32::NAN).is_err());
        assert!(set_expression_bypass(Row::from_wire(0), 8, 1, 0, false, 0, false).is_err());
        assert!(set_expression_bypass(Row::from_wire(0), 0, 1, 3, false, 0, false).is_err());
        assert!(set_expression_bypass(Row::from_wire(0), 0, 1, 0, false, 5_001, false).is_err());
    }

    // -- bypass ------------------------------------------------------------

    #[test]
    fn bypass_is_keyed_by_row_and_column() {
        let message = decode(&set_bypass(Row::from_wire(2), 6, true));
        let preset = preset_of(&message).expect("has a preset");
        let entry = &preset.bypass[0];
        let row = entry.row.as_ref().map(|r| {
            let bypass::Row::Row(v) = r;
            *v
        });
        assert_eq!(row, Some(2));
        let column = entry.col_bypass[0].column.as_ref().map(|c| {
            let col_bypass::Column::Column(v) = c;
            *v
        });
        assert_eq!(column, Some(6));
        assert_eq!(entry.col_bypass[0].scene_bypass.len(), 1);
        assert!(entry.col_bypass[0].scene_bypass[0].bypass);
    }

    #[test]
    fn bypass_does_not_try_to_set_scene_mode() {
        // ColBypass.sceneMode is NOT host-writable - sent alone or beside a
        // bypass entry it is ignored - so we never pretend to set it.
        let message = decode(&set_bypass(Row::from_wire(0), 1, false));
        let preset = preset_of(&message).expect("has a preset");
        assert_eq!(preset.bypass[0].col_bypass[0].scene_mode, None);
    }

    // -- row-level controls ------------------------------------------------

    #[test]
    fn splitter_value_uses_combined_splitter_without_an_invented_hash() {
        let chain = only_chain(&decode(
            &set_splitter_param(Row::from_wire(0), 3, 0.25).unwrap(),
        ));
        assert!(chain.splitter.is_empty());
        assert!(chain.models.is_empty());
        assert!(chain.mixer.is_empty());
        assert_eq!(chain.combined_splitter.len(), 1);
        let splitter = &chain.combined_splitter[0];
        assert!(splitter.hash.is_none());
        let parameter = &splitter.params[0];
        assert_eq!(parameter.index, Some(param::Index::Index(3)));
        assert_eq!(parameter.scene_mode, None);
        assert_eq!(
            parameter.param_values[0].value,
            Some(param_value::Value::FloatValue(0.25))
        );
    }

    #[test]
    fn hash_addressed_sub_controls_use_their_exact_collections_and_ids() {
        for (control, expected_hash) in [
            (SubControl::Mixer, 11_000),
            (SubControl::LaneOutput, 23_000),
            (SubControl::InputGate, 28_000),
        ] {
            let chain = only_chain(&decode(
                &set_sub_control_param(Row::from_wire(0), control, 2, 0.75).unwrap(),
            ));
            let selected = match control {
                SubControl::Mixer => &chain.mixer,
                SubControl::LaneOutput => &chain.output_control,
                SubControl::InputGate => &chain.input_control,
            };
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].hash, Some(model::Hash::Hash(expected_hash)));
            assert_eq!(selected[0].params[0].index, Some(param::Index::Index(2)));
            assert_eq!(
                selected[0].params[0].param_values[0].value,
                Some(param_value::Value::FloatValue(0.75))
            );
            assert!(chain.models.is_empty());
            assert!(chain.splitter.is_empty());
            assert!(chain.combined_splitter.is_empty());
            assert_eq!(chain.mixer.is_empty(), control != SubControl::Mixer);
            assert_eq!(
                chain.output_control.is_empty(),
                control != SubControl::LaneOutput
            );
            assert_eq!(
                chain.input_control.is_empty(),
                control != SubControl::InputGate
            );
        }
    }

    #[test]
    fn row_level_scene_mode_is_always_separate_from_the_value() {
        let splitter = only_chain(&decode(
            &set_splitter_param_scene_mode(Row::from_wire(0), 3, true).unwrap(),
        ));
        let splitter_param = &splitter.combined_splitter[0].params[0];
        assert_eq!(
            splitter_param.scene_mode,
            Some(param::SceneMode::SceneMode(true))
        );
        assert!(splitter_param.param_values.is_empty());

        for control in [
            SubControl::Mixer,
            SubControl::LaneOutput,
            SubControl::InputGate,
        ] {
            let chain = only_chain(&decode(
                &set_sub_control_param_scene_mode(Row::from_wire(0), control, 1, true).unwrap(),
            ));
            let parameter = match control {
                SubControl::Mixer => &chain.mixer[0].params[0],
                SubControl::LaneOutput => &chain.output_control[0].params[0],
                SubControl::InputGate => &chain.input_control[0].params[0],
            };
            assert_eq!(
                parameter.scene_mode,
                Some(param::SceneMode::SceneMode(true))
            );
            assert!(parameter.param_values.is_empty());
        }
    }

    #[test]
    fn split_mute_writes_split_bypass_and_never_mix_bypass() {
        for muted in [false, true] {
            let chain = only_chain(&decode(&set_split_mute(Row::from_wire(2), muted).unwrap()));
            assert_eq!(chain.split_bypass, vec![SceneBypass { bypass: muted }]);
            assert!(chain.mix_bypass.is_empty());
        }
    }

    #[test]
    fn branch_controls_refuse_odd_rows_and_bad_values() {
        for row in [1, 3] {
            let row = Row::from_wire(row);
            assert!(matches!(
                set_splitter_param(row, 0, 0.5),
                Err(crate::Error::InvalidRow(_))
            ));
            assert!(matches!(
                set_sub_control_param(row, SubControl::Mixer, 0, 0.5),
                Err(crate::Error::InvalidRow(_))
            ));
            assert!(matches!(
                set_split_mute(row, true),
                Err(crate::Error::InvalidRow(_))
            ));
        }
        for invalid in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
            assert!(matches!(
                set_splitter_param(Row::from_wire(0), 0, invalid),
                Err(crate::Error::InvalidParameter(_))
            ));
            assert!(matches!(
                set_sub_control_param(Row::from_wire(0), SubControl::InputGate, 0, invalid),
                Err(crate::Error::InvalidParameter(_))
            ));
        }
    }

    // -- blocks ------------------------------------------------------------

    #[test]
    fn placing_a_block_is_an_update_carrying_the_model_id() {
        let message = decode(&set_block(Row::from_wire(0), 2, 1001));
        assert_eq!(message.action, MessageAction::Update as i32);
        let hash = only_chain(&message).models[0].hash.as_ref().map(|h| {
            let model::Hash::Hash(v) = h;
            *v
        });
        assert_eq!(hash, Some(1001));
    }

    #[test]
    fn removing_a_block_uses_the_delete_action() {
        // THE trap. An UPDATE carrying hash: 0 is transmitted and ignored by
        // the firmware; the ACTION is what marks the removal.
        let message = decode(&remove_block(Row::from_wire(0), 2));
        assert_eq!(
            message.action,
            MessageAction::Delete as i32,
            "remove_block must use DELETE; an UPDATE with hash 0 is ignored"
        );
        let hash = only_chain(&message).models[0].hash.as_ref().map(|h| {
            let model::Hash::Hash(v) = h;
            *v
        });
        assert_eq!(hash, Some(0));
    }

    #[test]
    fn block_move_is_addressed_without_an_advisory_grid_snapshot() {
        let message = move_block(Row::from_wire(2), 1, Row::from_wire(3), 7, true);
        assert_eq!(message.r#move.len(), 1);
        let movement = &message.r#move[0];
        assert_eq!(
            (
                movement.from_row,
                movement.from_col,
                movement.to_row,
                movement.to_col,
                movement.is_drop,
            ),
            (2, 1, 3, 7, true)
        );
        assert!(message.grid.is_none());
    }

    // -- splits ------------------------------------------------------------

    #[test]
    fn a_split_on_an_even_row_is_built() {
        let message = set_split(Row::from_wire(2), 3, 6).expect("row 2 can branch");
        let chain = only_chain(&decode(&message));
        assert_eq!(chain.split_control_points[0].split, 3);
        assert_eq!(chain.split_control_points[0].mix, 6);
    }

    #[test]
    fn a_non_rejoining_branch_uses_minus_one() {
        let message = set_split(Row::from_wire(0), 2, -1).expect("row 0 can branch");
        let chain = only_chain(&decode(&message));
        assert_eq!(chain.split_control_points[0].mix, -1);
    }

    #[test]
    fn a_split_on_an_odd_row_is_refused() {
        // Rows 1 and 3 have no splitter; a write there is discarded silently.
        // Refusing beats sending a message that does nothing.
        for row in [1, 3] {
            assert!(
                matches!(
                    set_split(Row::from_wire(row), 1, 2),
                    Err(crate::Error::InvalidRow(_))
                ),
                "wire row {row} has no splitter and should be refused"
            );
        }
    }

    // -- action defaults ---------------------------------------------------

    #[test]
    fn edits_default_to_the_update_action() {
        // proto3 makes CREATE the zero value, so an omitted action means
        // CREATE rather than UPDATE. Every edit must set it explicitly.
        for message in [
            set_chain_input(Row::from_wire(0), GridInputPort::Input1),
            set_param(Row::from_wire(0), 0, 0, Value::Normalised(0.0)),
            set_tempo_param(0, 0.0).unwrap(),
            set_splitter_param(Row::from_wire(0), 0, 0.0).unwrap(),
            set_sub_control_param(Row::from_wire(0), SubControl::Mixer, 0, 0.0).unwrap(),
            set_split_mute(Row::from_wire(0), true).unwrap(),
            set_bypass(Row::from_wire(0), 0, true),
            set_block(Row::from_wire(0), 0, 1),
        ] {
            assert_eq!(
                decode(&message).action,
                MessageAction::Update as i32,
                "an edit defaulted to CREATE, which is the proto3 zero value"
            );
        }
    }
}

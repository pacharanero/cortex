// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Grid-edit message construction.
//!
//! Every function here builds a `GridMessage` payload and nothing else - no
//! I/O, no session, no device. That is deliberate: the grid is where the
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
    BinaryPreset, Bypass, Chain, ColBypass, GridMessage, Model, Param, ParamValue, SceneBypass,
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

    /// From the 1-4 label shown on the unit.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidRow`] for 0, since screen rows start at
    /// 1 - passing 0 here almost certainly means a wire index was passed to
    /// the wrong constructor.
    pub fn from_screen(row: u32) -> crate::Result<Self> {
        if row == 0 {
            return Err(crate::Error::InvalidRow(
                "screen rows are numbered from 1; row 0 suggests a wire index".into(),
            ));
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

/// Re-point one grid row's INPUT.
///
/// This row-keyed shape is the only one that actually moves an input; a
/// full-preset write whose chains lack an explicit `row` is accepted and
/// ignored.
#[must_use]
pub fn set_chain_input(row: Row, in_portid: u32) -> GridMessage {
    let mut chain = keyed_chain(row);
    chain.in_portid = Some(chain::InPortid::InPortid(in_portid));
    grid(MessageAction::Update, preset_with_chain(chain))
}

/// Re-point one grid row's OUTPUT.
///
/// Note the device does NOT validate this field: an id that means nothing is
/// stored rather than rejected, so a typo reads back cleanly. Note also that
/// not every value is a physical destination - 16 to 18 are internal
/// row-to-row routing, while 19 (MULTIPLE) is a real output.
#[must_use]
pub fn set_chain_output(row: Row, out_portid: u32) -> GridMessage {
    let mut chain = keyed_chain(row);
    chain.out_portid = Some(chain::OutPortid::OutPortid(out_portid));
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
    if !row.can_branch() {
        return Err(crate::Error::InvalidRow(format!(
            "row {} (screen row {}) has no splitter: a branch can only originate on \
             wire row 0 or 2, whose parallel lane is the row below it",
            row.wire(),
            row.screen()
        )));
    }
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
    fn screen_row_zero_is_refused() {
        // Screen rows start at 1, so 0 almost certainly means someone passed
        // a wire index to the wrong constructor. Better to reject than to
        // silently edit the row above the one intended.
        assert!(matches!(
            Row::from_screen(0),
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

    // -- keying ------------------------------------------------------------

    #[test]
    fn every_builder_keys_its_chain_by_row() {
        // The single most important property here. An unkeyed chain is
        // accepted by the device and does nothing.
        for message in [
            set_chain_input(Row::from_wire(2), 1),
            set_chain_output(Row::from_wire(2), 19),
            set_param(Row::from_wire(2), 3, 0, Value::Normalised(0.5)),
            set_param_scene_mode(Row::from_wire(2), 3, 0, true),
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
            set_chain_input(Row::from_wire(0), 1),
            set_param(Row::from_wire(0), 0, 0, Value::Normalised(0.0)),
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

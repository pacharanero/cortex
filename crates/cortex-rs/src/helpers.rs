// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Pure helpers for reading preset topology and parameter values.
//!
//! Stored presets are mostly positional, while sparse updates carry explicit
//! row, column, and parameter keys. These helpers apply one rule consistently:
//! an explicit key wins, otherwise position in the repeated field is the key.
//!
//! @see spec/130-domain-model/spec.md
//! @see spec/130-domain-model/design.md [DES-HELPERS]

use std::collections::BTreeMap;

use crate::client::{MidiOut, MidiSource};
use crate::proto::{BinaryPreset, Model, Param};

/// The input-gate parameter that is a sampled gain-reduction meter, not a setting.
pub const GAIN_REDUCTION_PARAM: u32 = 2;

/// One occupied grid cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// Zero-based wire row. The unit labels rows 1-4.
    pub row: u32,
    /// Zero-based grid column.
    pub column: u32,
    /// Nonzero model id stored in the cell.
    pub model_id: u32,
}

/// One branch into a parallel lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Split {
    /// Zero-based wire row from which the branch originates.
    pub row: u32,
    /// Column where the branch leaves the main row.
    pub split_column: i32,
    /// Column where it rejoins, or `-1` when it never rejoins.
    pub mix_column: i32,
}

impl Split {
    /// Whether the parallel lane rejoins the originating row.
    #[must_use]
    pub const fn rejoins(self) -> bool {
        self.mix_column >= 0
    }

    /// Zero-based wire row occupied by the parallel lane.
    #[must_use]
    pub const fn lane_row(self) -> u32 {
        self.row + 1
    }
}

/// One block-to-footswitch STOMP assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StompAssignment {
    /// Zero-based wire row. Zero is a valid value despite lacking presence.
    pub row: u32,
    /// Zero-based grid column. Zero is a valid value despite lacking presence.
    pub column: u32,
    /// Zero-based footswitch index A-H. Zero is a valid value.
    pub footswitch: u32,
}

/// Availability of one zero-based grid row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowAvailability {
    /// The row contains at least one block.
    Occupied,
    /// The row is empty and available for an independent chain.
    Free,
    /// The row is empty but is the parallel lane of the branch above it.
    Reserved,
}

/// One row's occupancy and branch-lane relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RowStatus {
    /// Zero-based wire row. The unit labels rows 1-4.
    pub row: u32,
    /// Whether the row is occupied, free, or reserved as a branch lane.
    pub status: RowAvailability,
    /// Number of occupied cells on this row.
    pub block_count: usize,
    /// Originating zero-based row when a branch reserves this row.
    pub reserved_by: Option<u32>,
}

/// Cardinality information for comparing list-valued parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionCount {
    /// Both values were stored against the same option count.
    Same(usize),
    /// The dynamic list changed cardinality between the two presets.
    Changed {
        /// Option count used to store the first value.
        before: usize,
        /// Option count used to store the second value.
        after: usize,
    },
}

/// A list-option selector accepted by [`option_value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionSelector<'a> {
    /// Select by exact device-returned display name.
    Name(&'a str),
    /// Select by zero-based list position.
    Index(usize),
}

impl<'a> From<&'a str> for OptionSelector<'a> {
    fn from(value: &'a str) -> Self {
        Self::Name(value)
    }
}

impl From<usize> for OptionSelector<'_> {
    fn from(value: usize) -> Self {
        Self::Index(value)
    }
}

fn chain_row(position: usize, chain: &crate::proto::Chain) -> Option<u32> {
    chain.row.as_ref().map_or_else(
        || u32::try_from(position).ok(),
        |key| {
            let crate::proto::chain::Row::Row(row) = key;
            Some(*row)
        },
    )
}

fn model_column(position: usize, model: &Model) -> Option<u32> {
    model.column.as_ref().map_or_else(
        || u32::try_from(position).ok(),
        |key| {
            let crate::proto::model::Column::Column(column) = key;
            Some(*column)
        },
    )
}

fn param_index(position: usize, parameter: &Param) -> Option<u32> {
    parameter.index.as_ref().map_or_else(
        || u32::try_from(position).ok(),
        |key| {
            let crate::proto::param::Index::Index(index) = key;
            Some(*index)
        },
    )
}

pub(crate) fn model_at(preset: &BinaryPreset, row: u32, column: u32) -> Option<&Model> {
    preset
        .chains
        .iter()
        .enumerate()
        .filter(|(position, chain)| chain_row(*position, chain) == Some(row))
        .find_map(|(_, chain)| {
            chain
                .models
                .iter()
                .enumerate()
                .find(|(position, model)| model_column(*position, model) == Some(column))
                .map(|(_, model)| model)
        })
}

/// Return every occupied grid cell in preset order.
///
/// A model with an absent or zero hash is an empty cell. Explicit row and
/// column keys override repeated-field position; absent keys fall back to it.
#[must_use]
pub fn blocks(preset: &BinaryPreset) -> Vec<Block> {
    let mut found = Vec::new();
    for (chain_position, chain) in preset.chains.iter().enumerate() {
        let Some(row) = chain_row(chain_position, chain) else {
            continue;
        };
        for (model_position, model) in chain.models.iter().enumerate() {
            let Some(crate::proto::model::Hash::Hash(model_id)) = model.hash else {
                continue;
            };
            if model_id == 0 {
                continue;
            }
            let Some(column) = model_column(model_position, model) else {
                continue;
            };
            found.push(Block {
                row,
                column,
                model_id,
            });
        }
    }
    found
}

/// Return every active branch and its rejoin point in preset order.
///
/// A negative split means no branch. A mix of `-1` is retained because it is
/// an active branch that never rejoins. Only rows 0 and 2 are valid branch
/// origins on the hardware; malformed odd-row data is reported rather than
/// silently rewritten.
#[must_use]
pub fn splits(preset: &BinaryPreset) -> Vec<Split> {
    let mut found = Vec::new();
    for (position, chain) in preset.chains.iter().enumerate() {
        let Some(row) = chain_row(position, chain) else {
            continue;
        };
        found.extend(
            chain
                .split_control_points
                .iter()
                .filter(|points| points.split >= 0)
                .map(|points| Split {
                    row,
                    split_column: points.split,
                    mix_column: points.mix,
                }),
        );
    }
    found
}

/// Return zero-based rows whose input is explicitly connected to `from_port`.
///
/// Input port zero is valid and is matched when present. An absent `in_portid`
/// never matches, because its positional fallback applies only to the row key.
#[must_use]
pub fn input_chain_rows(preset: &BinaryPreset, from_port: u32) -> Vec<u32> {
    preset
        .chains
        .iter()
        .enumerate()
        .filter_map(|(position, chain)| {
            let crate::proto::chain::InPortid::InPortid(port) = chain.in_portid.as_ref()?;
            (*port == from_port)
                .then(|| chain_row(position, chain))
                .flatten()
        })
        .collect()
}

/// Return every block-to-footswitch STOMP assignment.
///
/// These protobuf scalar fields have no presence, so zero row, column, and
/// footswitch values are preserved rather than treated as an empty entry.
#[must_use]
pub fn stomp_assignments(preset: &BinaryPreset) -> Vec<StompAssignment> {
    preset
        .stomp_mode_assignments
        .iter()
        .map(|assignment| StompAssignment {
            row: assignment.row,
            column: assignment.column,
            footswitch: assignment.stomp_index,
        })
        .collect()
}

/// Decode the preset's 10-by-12 MIDI output storage by source.
///
/// Empty slots are omitted and sources with no messages are absent. The
/// repeated field is positional: slots 0-11 belong to source 0, through slots
/// 108-119 for source 9.
///
/// # Errors
///
/// Returns a decode error for a non-empty slot carrying an unknown source or
/// MIDI message type, or if data extends beyond the 120-slot storage area.
pub fn midi_out(preset: &BinaryPreset) -> crate::Result<BTreeMap<MidiSource, Vec<MidiOut>>> {
    let mut output = BTreeMap::new();
    for (position, message) in preset.midi_messages_general_v2.iter().enumerate() {
        let Some(message) = MidiOut::from_proto(message)? else {
            continue;
        };
        if position >= 120 {
            return Err(crate::Error::Decode(format!(
                "preset MIDI output has data beyond its 10x12 storage at slot {position}"
            )));
        }
        let source = MidiSource::try_from(u32::try_from(position / 12).map_err(|_| {
            crate::Error::Decode("preset MIDI source index does not fit on the wire".into())
        })?)?;
        output.entry(source).or_insert_with(Vec::new).push(message);
    }
    Ok(output)
}

/// Decode non-empty MIDI messages sent when the preset is loaded.
///
/// # Errors
///
/// Returns a decode error if a non-empty entry carries an unknown MIDI type.
pub fn preset_load_midi_out(preset: &BinaryPreset) -> crate::Result<Vec<MidiOut>> {
    preset
        .midi_messages
        .iter()
        .filter_map(|message| MidiOut::from_proto(message).transpose())
        .collect()
}

/// Return active-scene tempo values keyed by positional parameter index.
///
/// Stored tempo parameters are positional even if a malformed or sparse value
/// happens to carry an explicit index. Only the first float value is used.
#[must_use]
pub fn tempo_params(preset: &BinaryPreset) -> BTreeMap<u32, f32> {
    let Some(tempo) = preset.tempo_program_data.first() else {
        return BTreeMap::new();
    };
    tempo
        .params
        .iter()
        .enumerate()
        .filter_map(|(position, parameter)| {
            let value = parameter
                .param_values
                .iter()
                .find_map(|value| match value.value {
                    Some(crate::proto::param_value::Value::FloatValue(value)) => Some(value),
                    _ => None,
                })?;
            Some((u32::try_from(position).ok()?, value))
        })
        .collect()
}

/// Return the current preset's dynamic option list for one block parameter.
///
/// Dynamic lists are authoritative because some include one entry per block
/// and therefore change with the preset. Explicit row, column, and parameter
/// keys override position; absent keys use positional fallback.
#[must_use]
pub fn param_options(
    preset: &BinaryPreset,
    row: u32,
    column: u32,
    wanted_index: u32,
) -> Vec<String> {
    model_at(preset, row, column)
        .and_then(|model| {
            model
                .params
                .iter()
                .enumerate()
                .find(|(position, parameter)| {
                    param_index(*position, parameter) == Some(wanted_index)
                })
                .map(|(_, parameter)| parameter.dynamic_steps.clone())
        })
        .unwrap_or_default()
}

/// Convert an option name or index into its normalized wire value.
///
/// List parameters store `index / (count - 1)`. A single-option list has the
/// only representable value, `0.0`.
///
/// # Errors
///
/// Returns an invalid-parameter error for an empty list, unknown name, or
/// out-of-range index.
pub fn option_value<'a>(
    options: &'a [String],
    option: impl Into<OptionSelector<'a>>,
) -> crate::Result<f32> {
    if options.is_empty() {
        return Err(crate::Error::InvalidParameter(
            "no options; read them from the current preset with param_options() first".into(),
        ));
    }
    let index = match option.into() {
        OptionSelector::Name(name) => options
            .iter()
            .position(|candidate| candidate == name)
            .ok_or_else(|| {
                crate::Error::InvalidParameter(format!("unknown parameter option {name:?}"))
            })?,
        OptionSelector::Index(index) => index,
    };
    if index >= options.len() {
        return Err(crate::Error::InvalidParameter(format!(
            "option index {index} is outside 0..{}",
            options.len() - 1
        )));
    }
    if options.len() == 1 {
        return Ok(0.0);
    }
    #[allow(clippy::cast_precision_loss)]
    Ok(index as f32 / (options.len() - 1) as f32)
}

/// Return the option selected by a normalized wire value.
///
/// Values outside 0-1, non-finite values, and empty lists return `None`.
#[must_use]
pub fn option_at(options: &[String], value: f32) -> Option<&str> {
    if options.is_empty() || !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    let index = (value * (options.len() - 1) as f32).round() as usize;
    options.get(index).map(String::as_str)
}

/// Return zero-based rows available for a new independent chain.
///
/// An empty row immediately below a branch is reserved as that branch's lane
/// and is therefore not free.
#[must_use]
pub fn free_rows(preset: &BinaryPreset) -> Vec<u32> {
    row_status(preset)
        .into_iter()
        .filter(|row| row.status == RowAvailability::Free)
        .map(|row| row.row)
        .collect()
}

/// Describe occupancy and branch-lane reservation for every stored row.
///
/// An occupied branch lane remains `Occupied`, while `reserved_by` still names
/// the zero-based originating row so the topology is not lost.
#[must_use]
pub fn row_status(preset: &BinaryPreset) -> Vec<RowStatus> {
    let mut counts = BTreeMap::<u32, usize>::new();
    for block in blocks(preset) {
        *counts.entry(block.row).or_default() += 1;
    }
    let lanes: BTreeMap<u32, u32> = splits(preset)
        .into_iter()
        .map(|split| (split.lane_row(), split.row))
        .collect();
    let mut rows = preset
        .chains
        .iter()
        .enumerate()
        .filter_map(|(position, chain)| chain_row(position, chain))
        .collect::<Vec<_>>();
    rows.extend(counts.keys().copied());
    rows.extend(lanes.keys().copied());
    rows.sort_unstable();
    rows.dedup();
    rows.into_iter()
        .map(|row| {
            let block_count = counts.get(&row).copied().unwrap_or_default();
            let status = if block_count > 0 {
                RowAvailability::Occupied
            } else if lanes.contains_key(&row) {
                RowAvailability::Reserved
            } else {
                RowAvailability::Free
            };
            RowStatus {
                row,
                status,
                block_count,
                reserved_by: lanes.get(&row).copied(),
            }
        })
        .collect()
}

/// Whether two normalized parameter values have the same meaning.
///
/// Plain floats use a tolerance of `1e-4`; NaN equals only NaN because factory
/// content stores NaN in unused slots. List values compare the selected option,
/// including when the dynamic list changed cardinality.
///
/// # Errors
///
/// Returns an invalid-parameter error when a list has fewer than two options.
pub fn params_equal(a: f32, b: f32, option_count: Option<OptionCount>) -> crate::Result<bool> {
    params_equal_with_tolerance(a, b, option_count, 1.0e-4)
}

/// As [`params_equal`], with a caller-selected non-negative finite tolerance.
///
/// # Errors
///
/// Returns an invalid-parameter error for a degenerate option count or invalid
/// tolerance.
pub fn params_equal_with_tolerance(
    a: f32,
    b: f32,
    option_count: Option<OptionCount>,
    tolerance: f32,
) -> crate::Result<bool> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(crate::Error::InvalidParameter(format!(
            "parameter comparison tolerance must be finite and non-negative, got {tolerance}"
        )));
    }
    if let Some(counts) = option_count {
        let (before, after) = match counts {
            OptionCount::Same(count) => (count, count),
            OptionCount::Changed { before, after } => (before, after),
        };
        if before < 2 || after < 2 {
            return Err(crate::Error::InvalidParameter(format!(
                "a list parameter has at least 2 options; got {before} and {after}"
            )));
        }
        if !a.is_finite() || !b.is_finite() {
            return Ok(false);
        }
        if !(0.0..=1.0).contains(&a) || !(0.0..=1.0).contains(&b) {
            return Ok(false);
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            clippy::cast_sign_loss
        )]
        return Ok(
            (a * (before - 1) as f32).round() as usize == (b * (after - 1) as f32).round() as usize
        );
    }
    if a.is_nan() || b.is_nan() {
        return Ok(a.is_nan() && b.is_nan());
    }
    Ok((a - b).abs() <= tolerance)
}

/// Compare input-gate parameters while excluding the sampled gain-reduction meter.
///
/// Index 2 is live measurement data and is intentionally treated as equal so a
/// save-to-save preset comparison does not report it as a settings change.
///
/// # Errors
///
/// As [`params_equal`].
pub fn input_control_params_equal(
    param_index: u32,
    a: f32,
    b: f32,
    option_count: Option<OptionCount>,
) -> crate::Result<bool> {
    if param_index == GAIN_REDUCTION_PARAM {
        Ok(true)
    } else {
        params_equal(a, b, option_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{
        Chain, MidiMessageInfo, ParamValue, SplitControlPoints, StompModeAssignment, chain, model,
        param, param_value,
    };

    fn keyed_model(hash: u32, column: Option<u32>) -> Model {
        Model {
            hash: Some(model::Hash::Hash(hash)),
            column: column.map(model::Column::Column),
            ..Default::default()
        }
    }

    fn fictional_preset() -> BinaryPreset {
        BinaryPreset {
            chains: vec![
                Chain {
                    row: Some(chain::Row::Row(2)),
                    in_portid: Some(chain::InPortid::InPortid(0)),
                    models: vec![keyed_model(0, Some(7)), keyed_model(2002, Some(0))],
                    split_control_points: vec![SplitControlPoints { split: 3, mix: -1 }],
                    ..Default::default()
                },
                Chain {
                    row: Some(chain::Row::Row(0)),
                    in_portid: Some(chain::InPortid::InPortid(1)),
                    models: vec![keyed_model(1001, None), Model::default()],
                    split_control_points: vec![SplitControlPoints { split: -1, mix: -1 }],
                    ..Default::default()
                },
                Chain::default(),
                Chain::default(),
            ],
            stomp_mode_assignments: vec![StompModeAssignment::default()],
            ..Default::default()
        }
    }

    #[test]
    fn blocks_use_explicit_keys_before_position_and_zero_hash_is_empty() {
        assert_eq!(
            blocks(&fictional_preset()),
            vec![
                Block {
                    row: 2,
                    column: 0,
                    model_id: 2002,
                },
                Block {
                    row: 0,
                    column: 0,
                    model_id: 1001,
                },
            ]
        );
    }

    #[test]
    fn absent_row_and_column_keys_fall_back_to_position() {
        let preset = BinaryPreset {
            chains: vec![
                Chain::default(),
                Chain {
                    models: vec![Model::default(), keyed_model(42, None)],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            blocks(&preset)[0],
            Block {
                row: 1,
                column: 1,
                model_id: 42
            }
        );
    }

    #[test]
    fn splits_keep_non_rejoining_branches_and_omit_negative_splits() {
        let found = splits(&fictional_preset());
        assert_eq!(
            found,
            vec![Split {
                row: 2,
                split_column: 3,
                mix_column: -1
            }]
        );
        assert!(!found[0].rejoins());
        assert_eq!(found[0].lane_row(), 3);
    }

    #[test]
    fn input_rows_require_port_presence_and_preserve_explicit_zero() {
        let preset = fictional_preset();
        assert_eq!(input_chain_rows(&preset, 0), vec![2]);
        assert_eq!(input_chain_rows(&preset, 1), vec![0]);
        assert!(input_chain_rows(&preset, 9).is_empty());
    }

    #[test]
    fn zero_stomp_fields_are_a_real_assignment() {
        assert_eq!(
            stomp_assignments(&fictional_preset()),
            vec![StompAssignment {
                row: 0,
                column: 0,
                footswitch: 0
            }]
        );
    }

    #[test]
    fn midi_storage_is_ten_sources_by_twelve_slots() {
        let mut preset = BinaryPreset {
            midi_messages_general_v2: vec![MidiMessageInfo::default(); 120],
            ..Default::default()
        };
        preset.midi_messages_general_v2[0] = MidiMessageInfo {
            r#type: 1,
            channel: 1,
            param1: 7,
            param2: 8,
            param3: 0,
        };
        preset.midi_messages_general_v2[11] = MidiMessageInfo {
            r#type: 3,
            channel: 16,
            param1: 1,
            param2: 2,
            param3: 127,
        };
        preset.midi_messages_general_v2[12] = MidiMessageInfo {
            r#type: 2,
            channel: 2,
            param1: 9,
            param2: 10,
            param3: 11,
        };
        preset.midi_messages_general_v2[119] = MidiMessageInfo {
            r#type: 1,
            channel: 3,
            param1: 12,
            param2: 13,
            param3: 14,
        };
        let output = midi_out(&preset).unwrap();
        assert_eq!(output[&MidiSource::FootswitchA].len(), 2);
        assert_eq!(output[&MidiSource::FootswitchB].len(), 1);
        assert_eq!(output[&MidiSource::Expression2][0].param3, 14);
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn midi_storage_rejects_non_empty_data_beyond_slot_119() {
        let mut preset = BinaryPreset {
            midi_messages_general_v2: vec![MidiMessageInfo::default(); 121],
            ..Default::default()
        };
        preset.midi_messages_general_v2[120].r#type = 1;
        assert!(matches!(midi_out(&preset), Err(crate::Error::Decode(_))));
    }

    #[test]
    fn preset_load_midi_drops_empty_slots() {
        let preset = BinaryPreset {
            midi_messages: vec![
                MidiMessageInfo::default(),
                MidiMessageInfo {
                    r#type: 3,
                    channel: 4,
                    param1: 5,
                    param2: 6,
                    param3: 7,
                },
            ],
            ..Default::default()
        };
        let output = preset_load_midi_out(&preset).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].param3, 7);
    }

    #[test]
    fn tempo_parameters_are_positional_and_take_the_first_float() {
        let preset = BinaryPreset {
            tempo_program_data: vec![Model {
                params: vec![
                    Param {
                        index: Some(param::Index::Index(99)),
                        param_values: vec![
                            ParamValue {
                                value: Some(param_value::Value::IntValue(4)),
                            },
                            ParamValue {
                                value: Some(param_value::Value::FloatValue(0.25)),
                            },
                        ],
                        ..Default::default()
                    },
                    Param {
                        param_values: vec![
                            ParamValue {
                                value: Some(param_value::Value::FloatValue(0.75)),
                            },
                            ParamValue {
                                value: Some(param_value::Value::FloatValue(1.0)),
                            },
                        ],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            tempo_params(&preset),
            BTreeMap::from([(0, 0.25), (1, 0.75)])
        );
    }

    #[test]
    fn dynamic_options_honor_explicit_parameter_indices() {
        let mut preset = fictional_preset();
        preset.chains[0].models[1].params = vec![
            Param {
                index: Some(param::Index::Index(6)),
                dynamic_steps: vec!["Off".into(), "Fictional Input".into()],
                ..Default::default()
            },
            Param {
                dynamic_steps: vec!["Positional one".into()],
                ..Default::default()
            },
        ];
        assert_eq!(
            param_options(&preset, 2, 0, 6),
            vec!["Off", "Fictional Input"]
        );
        assert_eq!(param_options(&preset, 2, 0, 1), vec!["Positional one"]);
        assert!(param_options(&preset, 2, 7, 6).is_empty());
    }

    #[test]
    fn option_conversion_covers_names_indices_singletons_and_boundaries() {
        let options: Vec<String> = ["Off", "Follow", "Input 1", "Input 2"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert!(option_value(&options, "Off").unwrap().abs() < f32::EPSILON);
        assert!((option_value(&options, "Input 1").unwrap() - 2.0 / 3.0).abs() < 1.0e-6);
        assert!((option_value(&options, 1_usize).unwrap() - 1.0 / 3.0).abs() < 1.0e-6);
        assert!((option_value(&options, "Input 2").unwrap() - 1.0).abs() < f32::EPSILON);
        for (index, option) in options.iter().enumerate() {
            assert_eq!(
                option_at(&options, option_value(&options, index).unwrap()),
                Some(option.as_str())
            );
        }
        assert!(option_value(&["Only".into()], "Only").unwrap().abs() < f32::EPSILON);
        assert_eq!(option_at(&["Only".into()], 1.0), Some("Only"));
        assert!(option_value(&options, "Missing").is_err());
        assert!(option_value(&options, 4_usize).is_err());
        assert!(option_value(&[], 0_usize).is_err());
        assert_eq!(option_at(&options, f32::NAN), None);
        assert_eq!(option_at(&options, -0.01), None);
        assert_eq!(option_at(&options, 1.01), None);
    }

    #[test]
    fn row_status_distinguishes_free_reserved_and_occupied_lanes() {
        let mut preset = BinaryPreset {
            chains: vec![
                Chain {
                    models: vec![keyed_model(1, None)],
                    split_control_points: vec![SplitControlPoints { split: 2, mix: -1 }],
                    ..Default::default()
                },
                Chain::default(),
                Chain::default(),
                Chain::default(),
            ],
            ..Default::default()
        };
        assert_eq!(free_rows(&preset), vec![2, 3]);
        assert_eq!(
            row_status(&preset)[1],
            RowStatus {
                row: 1,
                status: RowAvailability::Reserved,
                block_count: 0,
                reserved_by: Some(0)
            }
        );
        preset.chains[1].models.push(keyed_model(2, None));
        assert_eq!(
            row_status(&preset)[1],
            RowStatus {
                row: 1,
                status: RowAvailability::Occupied,
                block_count: 1,
                reserved_by: Some(0)
            }
        );
    }

    #[test]
    fn row_status_and_free_rows_use_explicit_sparse_row_keys() {
        let preset = BinaryPreset {
            chains: vec![
                Chain {
                    row: Some(chain::Row::Row(2)),
                    models: vec![keyed_model(1, Some(0))],
                    split_control_points: vec![SplitControlPoints { split: 2, mix: -1 }],
                    ..Default::default()
                },
                Chain {
                    row: Some(chain::Row::Row(3)),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            row_status(&preset),
            vec![
                RowStatus {
                    row: 2,
                    status: RowAvailability::Occupied,
                    block_count: 1,
                    reserved_by: None,
                },
                RowStatus {
                    row: 3,
                    status: RowAvailability::Reserved,
                    block_count: 0,
                    reserved_by: Some(2),
                },
            ]
        );
        assert!(free_rows(&preset).is_empty());
    }

    #[test]
    fn parameter_comparison_handles_tolerance_lists_nan_and_meter_exclusion() {
        assert!(params_equal(0.5, 0.500_000_06, None).unwrap());
        assert!(!params_equal(0.5, 0.51, None).unwrap());
        assert!(
            params_equal(
                2.0 / 6.0,
                2.0 / 7.0,
                Some(OptionCount::Changed {
                    before: 7,
                    after: 8
                })
            )
            .unwrap()
        );
        assert!(
            !params_equal(
                2.0 / 6.0,
                3.0 / 7.0,
                Some(OptionCount::Changed {
                    before: 7,
                    after: 8
                })
            )
            .unwrap()
        );
        assert!(params_equal(f32::NAN, f32::NAN, None).unwrap());
        assert!(!params_equal(f32::NAN, 0.5, None).unwrap());
        assert!(params_equal(1.0 / 3.0, 1.0 / 3.0, Some(OptionCount::Same(4))).unwrap());
        assert!(params_equal(0.0, 0.0, Some(OptionCount::Same(1))).is_err());
        assert!(params_equal_with_tolerance(0.0, 0.0, None, f32::NAN).is_err());
        assert!(input_control_params_equal(GAIN_REDUCTION_PARAM, 0.1, 0.9, None).unwrap());
        assert!(!input_control_params_equal(1, 0.1, 0.9, None).unwrap());
    }
}

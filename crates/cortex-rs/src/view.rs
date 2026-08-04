// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Typed views of a preset, for anything that has to render one.
//!
//! `BinaryPreset` is the wire shape: wide, repeated-field, and awkward to
//! read - occupancy cannot be taken from `models.len()`, rows are zero-based
//! while the unit prints 1-4, and a scene-following parameter stores one
//! value per scene. These types answer those questions once, here, so the
//! CLI, the MCP server and the Tauri backend do not each answer them
//! differently - or differently by accident.
//!
//! They belong in the crate for exactly that reason. They were the CLI's
//! private output types, which would have left the GUI reimplementing the
//! same decisions against the same protobuf.
//!
//! Serialisable both ways: these cross the daemon socket as well as being
//! rendered.
//!
//! @see spec/roadmap.md PROT-004

/// Per-row routing and branch points.
fn preset_rows(preset: &crate::proto::BinaryPreset) -> Vec<Row> {
    use crate::proto::chain;
    preset
        .chains
        .iter()
        .enumerate()
        .map(|(row, c)| {
            // split_control_points has no protobuf presence, so read it
            // directly. A split of -1 means "no branch"; a mix of -1 means a
            // branch that never rejoins.
            let split = c.split_control_points.first();
            Row {
                row,
                screen_row: row + 1,
                in_port: c.in_portid.as_ref().map(|p| {
                    let chain::InPortid::InPortid(v) = p;
                    *v
                }),
                out_port: c.out_portid.as_ref().map(|p| {
                    let chain::OutPortid::OutPortid(v) = p;
                    *v
                }),
                split_at: split.map(|s| s.split).filter(|v| *v >= 0),
                mix_at: split.map(|s| s.mix).filter(|v| *v >= 0),
            }
        })
        .collect()
}

/// The bypass entry for a cell, if the preset carries one.
fn cell_bypass(preset: &crate::proto::BinaryPreset, row: usize, column: usize) -> Option<Bypass> {
    use crate::proto::{bypass, col_bypass};
    for (bypass_index, entry) in preset.bypass.iter().enumerate() {
        // Like models, row and column may arrive without presence, in which
        // case position is the index.
        let entry_row = entry.row.as_ref().map_or(bypass_index, |r| {
            let bypass::Row::Row(v) = r;
            *v as usize
        });
        if entry_row != row {
            continue;
        }
        for (col_index, cb) in entry.col_bypass.iter().enumerate() {
            let cb_col = cb.column.as_ref().map_or(col_index, |c| {
                let col_bypass::Column::Column(v) = c;
                *v as usize
            });
            if cb_col != column {
                continue;
            }
            return Some(Bypass {
                row,
                column,
                scenes: cb.scene_bypass.iter().map(|sb| sb.bypass).collect(),
                scene_mode: cb.scene_mode.as_ref().map(|m| {
                    let col_bypass::SceneMode::SceneMode(v) = m;
                    *v
                }),
            });
        }
    }
    None
}

/// Read the stored parameter values off a block, naming them where the
/// catalog can.
///
/// A stored preset omits the `index` on a parameter, in which case position
/// in the repeated field IS the index - the same positional rule as the
/// catalog. Only the first `param_values` entry is read: the device applies
/// that one to the active scene and ignores any beyond it.
fn block_params(
    model: &crate::proto::Model,
    catalog_entry: Option<&crate::Model>,
) -> Vec<ParamValue> {
    use crate::proto::{param, param_value};
    let mut out = Vec::new();
    for (position, p) in model.params.iter().enumerate() {
        let index = p.index.as_ref().map_or_else(
            || u32::try_from(position).unwrap_or_default(),
            |i| {
                let param::Index::Index(v) = i;
                *v
            },
        );
        let read = |pv: &crate::proto::ParamValue| match &pv.value {
            Some(param_value::Value::FloatValue(v)) => Some(ParamValueKind::Number(f64::from(*v))),
            Some(param_value::Value::StringValue(v)) => Some(ParamValueKind::Text(v.clone())),
            Some(param_value::Value::IntValue(v)) => Some(ParamValueKind::Number(f64::from(*v))),
            None => None,
        };
        let Some(value) = p.param_values.first().and_then(&read) else {
            continue;
        };
        // A scene-following parameter stores one value per scene. Showing
        // only the first hides exactly the difference a per-scene edit makes.
        let per_scene: Vec<ParamValueKind> = if p.param_values.len() > 1 {
            p.param_values.iter().filter_map(&read).collect()
        } else {
            Vec::new()
        };
        let name = catalog_entry
            .and_then(|m| m.parameters.get(index as usize))
            .map(|p| p.name.clone());
        out.push(ParamValue {
            index,
            name,
            value,
            per_scene,
        });
    }
    out
}

/// One block on the grid, resolved through the catalog where possible.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// Zero-based wire row. The unit labels rows 1-4.
    pub row: usize,
    /// The row as the unit displays it.
    pub screen_row: usize,
    /// Zero-based column.
    pub column: usize,
    /// The model's catalog id, which is what `set_block` takes.
    pub model_id: u32,
    /// `None` when the catalog could not be fetched.
    pub name: Option<String>,
    /// The catalog category, e.g. `Guitar Amplifier`. `None` when the
    /// catalog could not be fetched.
    pub category: Option<String>,
    /// Neural DSP's own attribution, verbatim. Never paraphrase it.
    pub based_on: Option<String>,
    /// Parameter values as stored, when asked for. Empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamValue>,
    /// Bypass state, when the preset carried one for this cell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypass: Option<Bypass>,
}

/// One stored parameter value on a block.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamValue {
    /// The wire index, which is what `set-param --index` takes.
    pub index: u32,
    /// The parameter name, when the catalog could resolve it.
    pub name: Option<String>,
    /// The stored value. A parameter can hold a string rather than a number.
    ///
    /// For a scene-following parameter the device stores one value PER
    /// SCENE, so see `per_scene` for the rest.
    pub value: ParamValueKind,
    /// Every stored value, when the parameter carries more than one - which
    /// is how a scene-following parameter is represented. Empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub per_scene: Vec<ParamValueKind>,
}

/// What a stored parameter value actually is.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ParamValueKind {
    /// A number, as stored.
    ///
    /// Widened to `f64` so both representations survive intact: the wire
    /// carries a parameter value as either a float or an int, and narrowing
    /// the int to `f32` would lose precision for no reason.
    Number(f64),
    /// A string, e.g. a microphone selection.
    Text(String),
}

/// A preset and the blocks it holds.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    /// The slot this preset came from, e.g. `1B`, or a marker for the
    /// live grid when it was read from working memory.
    pub slot: String,
    /// Absolute device path of the setlist holding it.
    pub setlist: String,
    /// The preset's name as the unit stores it.
    pub name: String,
    /// How many rows the preset carries. The unit has four.
    pub chains: usize,
    /// Per-row input/output routing and branch points.
    pub rows: Vec<Row>,
    /// Every occupied cell, in row-then-column order. Empty cells are
    /// absent rather than represented, so this is not a dense grid.
    pub blocks: Vec<Block>,
}

/// One grid row's routing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Row {
    /// Zero-based wire row.
    pub row: usize,
    /// The row as the unit displays it.
    pub screen_row: usize,
    /// Input port id feeding this row, if set.
    pub in_port: Option<u32>,
    /// Output port id this row feeds, if set. Values 16-18 are internal
    /// row-to-row routing rather than jacks; 19 (MULTIPLE) is a real output.
    pub out_port: Option<u32>,
    /// Column at which the row branches, or None. Only rows 0 and 2 can.
    pub split_at: Option<i32>,
    /// Column at which a branch rejoins. None means it never does.
    pub mix_at: Option<i32>,
}

/// A block's bypass state, per scene.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Bypass {
    /// Zero-based wire row. The unit labels rows 1-4.
    pub row: usize,
    /// Zero-based column, 0-7 left to right.
    pub column: usize,
    /// Bypass state for each stored scene slot, in order. A block that does
    /// not follow scenes carries the same value in all eight.
    pub scenes: Vec<bool>,
    /// Whether the block follows scenes. Not host-writable.
    pub scene_mode: Option<bool>,
}

impl Preset {
    /// Convert a preset into the output shape, naming blocks through the catalog.
    ///
    /// Shared by `preset` (a STORED slot, which the device can only read by
    /// recalling it) and `grid` (the LIVE working copy, read with no side
    /// effects).
    #[must_use]
    pub fn from_binary(
        preset: &crate::proto::BinaryPreset,
        catalog: Option<&crate::Catalog>,
        slot: &str,
        setlist: &str,
        with_params: bool,
    ) -> Preset {
        let name = preset
            .name
            .as_ref()
            .map_or("<unnamed>", |n| {
                let crate::proto::binary_preset::Name::Name(v) = n;
                v.as_str()
            })
            .to_string();

        let mut blocks = Vec::new();
        for (row, chain) in preset.chains.iter().enumerate() {
            for (column, model) in chain.models.iter().enumerate() {
                // Every row reports 8 column slots; an empty one has no hash or a
                // zero hash. Occupancy cannot be taken from models.len().
                let Some(crate::proto::model::Hash::Hash(id)) = model.hash else {
                    continue;
                };
                if id == 0 {
                    continue;
                }
                let entry = catalog.and_then(|c| c.get(id));
                blocks.push(Block {
                    row,
                    // Rows are 0-based on the wire and 1-4 on screen; carrying
                    // both means a reader never has to remember which this is.
                    screen_row: row + 1,
                    column,
                    model_id: id,
                    name: entry.map(|m| m.name.clone()),
                    category: entry.map(|m| m.category.clone()),
                    based_on: entry.and_then(|m| m.based_on.clone()),
                    params: if with_params {
                        block_params(model, entry)
                    } else {
                        Vec::new()
                    },
                    bypass: cell_bypass(preset, row, column),
                });
            }
        }

        Preset {
            slot: slot.to_string(),
            setlist: setlist.to_string(),
            name,
            chains: preset.chains.len(),
            rows: preset_rows(preset),
            blocks,
        }
    }
}

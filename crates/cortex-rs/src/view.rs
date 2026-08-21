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
//! Serialisable both ways: these cross local daemon IPC as well as being
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
            let row = c.row.as_ref().map_or(row, |key| {
                let chain::Row::Row(row) = key;
                usize::try_from(*row).unwrap_or(usize::MAX)
            });
            Row {
                row,
                screen_row: row.saturating_add(1),
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamValue>,
    /// Bypass state, when the preset carried one for this cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypass: Option<Bypass>,
}

/// One stored parameter value on a block.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub per_scene: Vec<ParamValueKind>,
}

/// What a stored parameter value actually is.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
    /// Scene labels and colours, always indexed A-H as 0-7.
    #[serde(default)]
    pub scenes: Vec<Scene>,
    /// Every occupied cell, in row-then-column order. Empty cells are
    /// absent rather than represented, so this is not a dense grid.
    pub blocks: Vec<Block>,
}

/// One scene's editable metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Scene {
    /// Zero-based scene index. The unit displays A-H.
    pub index: u32,
    /// Scene label, or `None` when the unit stores its one-space blank value.
    pub label: Option<String>,
    /// ARGB colour, when the preset carries one for this scene.
    pub color: Option<u32>,
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

/// Device firmware and identity, reshaped from the vendor's wire message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeviceVersion {
    /// `QC` or `ATMA` (the Nano Cortex codename).
    pub device_type: Option<String>,
    /// The name shown on the unit.
    pub custom_name: Option<String>,
    /// Device serial number.
    pub serial_number: Option<String>,
    /// The `CorOS` version. The vendor schema calls this field `zenos_git_hash`.
    pub coros_version: Option<String>,
    /// Main application firmware version.
    pub app_firmware: Option<String>,
    /// Main bootloader firmware version.
    pub bootloader_firmware: Option<String>,
    /// Footswitch/encoder controller application firmware.
    pub zencoder_app: Option<String>,
    /// Footswitch/encoder controller bootloader firmware.
    pub zencoder_bootloader: Option<String>,
    /// Wireless firmware build checksum. The vendor schema calls it a version.
    pub wireless_firmware_checksum: Option<String>,
    /// Linux kernel build string.
    pub linux_kernel: Option<String>,
    /// U-Boot version string.
    pub uboot: Option<String>,
    /// Device network MAC address.
    pub mac_address: Option<String>,
    /// Whether this unit uses the ESS codec variant.
    pub is_ess: Option<bool>,
}

impl From<&crate::proto::VersionMessage> for DeviceVersion {
    fn from(value: &crate::proto::VersionMessage) -> Self {
        use crate::proto::version_message::{
            AppFwVersion, BootloaderFwVersion, CustomName, DeviceSerialNumber, DeviceTypeOneOf,
            IsEss, LinuxKernelVersion, MacAddress, UbootVersion, ZencoderFwApp,
            ZencoderFwBootloader, ZenosGitHash, ZenwirelessFwVersion,
        };

        macro_rules! string_field {
            ($field:expr, $variant:path) => {
                $field.as_ref().map(|entry| {
                    let $variant(inner) = entry;
                    inner.clone()
                })
            };
        }

        Self {
            device_type: value.device_type.as_ref().and_then(|entry| {
                let DeviceTypeOneOf::DeviceType(id) = entry;
                crate::proto::version_message::DeviceType::try_from(*id)
                    .ok()
                    .map(|kind| kind.as_str_name().to_string())
            }),
            custom_name: string_field!(value.custom_name, CustomName::CustomName),
            serial_number: string_field!(
                value.device_serial_number,
                DeviceSerialNumber::DeviceSerialNumber
            ),
            coros_version: string_field!(value.zenos_git_hash, ZenosGitHash::ZenosGitHash),
            app_firmware: string_field!(value.app_fw_version, AppFwVersion::AppFwVersion),
            bootloader_firmware: string_field!(
                value.bootloader_fw_version,
                BootloaderFwVersion::BootloaderFwVersion
            ),
            zencoder_app: string_field!(value.zencoder_fw_app, ZencoderFwApp::ZencoderFwApp),
            zencoder_bootloader: string_field!(
                value.zencoder_fw_bootloader,
                ZencoderFwBootloader::ZencoderFwBootloader
            ),
            wireless_firmware_checksum: string_field!(
                value.zenwireless_fw_version,
                ZenwirelessFwVersion::ZenwirelessFwVersion
            )
            .map(|text| text.trim().to_string()),
            linux_kernel: string_field!(
                value.linux_kernel_version,
                LinuxKernelVersion::LinuxKernelVersion
            ),
            uboot: string_field!(value.uboot_version, UbootVersion::UbootVersion)
                .map(|text| text.trim().to_string()),
            mac_address: string_field!(value.mac_address, MacAddress::MacAddress),
            is_ess: value.is_ess.as_ref().map(|entry| {
                let IsEss::IsEss(enabled) = entry;
                *enabled
            }),
        }
    }
}

/// One grid column's share of the DSP load.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CpuColumn {
    /// Load for this column, as the device reports it.
    pub load: f32,
    /// Whether this column runs on the second DSP core.
    pub on_core2: bool,
}

/// DSP load, total and broken down by chain and column.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CpuLoad {
    /// Total load across both cores, if the device reported one.
    pub total: Option<f32>,
    /// Per chain, then per column within that chain.
    pub chains: Vec<Vec<CpuColumn>>,
}

impl From<&crate::proto::CpuLoadMessage> for CpuLoad {
    fn from(value: &crate::proto::CpuLoadMessage) -> Self {
        Self {
            total: value.cpu_total_load.as_ref().map(|total| {
                let crate::proto::cpu_load_message::CpuTotalLoad::CpuTotalLoad(load) = total;
                *load
            }),
            chains: value
                .chains
                .iter()
                .map(|chain| {
                    chain
                        .columns
                        .iter()
                        .map(|column| CpuColumn {
                            load: column.cpu_load,
                            on_core2: column.is_on_core2,
                        })
                        .collect()
                })
                .collect(),
        }
    }
}

/// A stable setlist-slot view for CLI, daemon, MCP, and GUI consumers.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PresetSlot {
    /// Linear slot index, 0-255.
    pub index: u32,
    /// Slot as displayed by the unit, e.g. `28C`.
    pub slot: String,
    /// Preset name, or an empty string for an unoccupied slot.
    pub name: String,
}

impl From<&crate::client::PresetEntry> for PresetSlot {
    fn from(value: &crate::client::PresetEntry) -> Self {
        Self {
            index: value.index,
            slot: crate::client::position_to_slot(value.index),
            name: value.name.clone(),
        }
    }
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

        let blocks = crate::helpers::blocks(preset)
            .into_iter()
            .filter_map(|block| {
                let row = usize::try_from(block.row).ok()?;
                let column = usize::try_from(block.column).ok()?;
                let model = crate::helpers::model_at(preset, block.row, block.column)?;
                let entry = catalog.and_then(|catalog| catalog.get(block.model_id));
                Some(Block {
                    row,
                    // Rows are 0-based on the wire and 1-4 on screen; carrying
                    // both means a reader never has to remember which this is.
                    screen_row: row + 1,
                    column,
                    model_id: block.model_id,
                    name: entry.map(|model| model.name.clone()),
                    category: entry.map(|model| model.category.clone()),
                    based_on: entry.and_then(|model| model.based_on.clone()),
                    params: if with_params {
                        block_params(model, entry)
                    } else {
                        Vec::new()
                    },
                    bypass: cell_bypass(preset, row, column),
                })
            })
            .collect();

        Preset {
            slot: slot.to_string(),
            setlist: setlist.to_string(),
            name,
            chains: preset.chains.len(),
            rows: preset_rows(preset),
            scenes: (0..8)
                .map(|index| Scene {
                    index,
                    label: preset
                        .scene_labels
                        .get(index as usize)
                        .filter(|label| !label.trim().is_empty())
                        .cloned(),
                    color: preset.scene_colors.get(index as usize).copied(),
                })
                .collect(),
            blocks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_view_keeps_row_conventions_routing_and_occupancy() {
        use crate::proto::{BinaryPreset, Chain, Model, SplitControlPoints};
        let preset = BinaryPreset {
            name: Some(crate::proto::binary_preset::Name::Name(
                "Fictional Rig".into(),
            )),
            scene_labels: vec!["Lead".into(), crate::client::SCENE_UNLABELLED.into()],
            scene_colors: vec![0xff11_2233, 0xff44_5566],
            chains: vec![Chain {
                in_portid: Some(crate::proto::chain::InPortid::InPortid(1)),
                out_portid: Some(crate::proto::chain::OutPortid::OutPortid(19)),
                models: vec![
                    Model::default(),
                    Model {
                        hash: Some(crate::proto::model::Hash::Hash(42)),
                        ..Default::default()
                    },
                ],
                split_control_points: vec![SplitControlPoints { split: 2, mix: 6 }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let view = Preset::from_binary(&preset, None, "3C", "/fictional/setlist", false);
        assert_eq!(view.name, "Fictional Rig");
        assert_eq!(view.rows[0].row, 0);
        assert_eq!(view.rows[0].screen_row, 1);
        assert_eq!(view.rows[0].in_port, Some(1));
        assert_eq!(view.rows[0].out_port, Some(19));
        assert_eq!(view.rows[0].split_at, Some(2));
        assert_eq!(view.rows[0].mix_at, Some(6));
        assert_eq!(view.scenes.len(), 8);
        assert_eq!(view.scenes[0].label.as_deref(), Some("Lead"));
        assert_eq!(view.scenes[0].color, Some(0xff11_2233));
        assert_eq!(view.scenes[1].label, None);
        assert_eq!(view.scenes[1].color, Some(0xff44_5566));
        assert_eq!(view.scenes[7].color, None);
        assert_eq!(view.blocks.len(), 1, "the empty cell must stay absent");
        assert_eq!(view.blocks[0].column, 1);
        assert_eq!(view.blocks[0].model_id, 42);
    }

    #[test]
    fn preset_view_uses_explicit_sparse_row_and_column_keys() {
        let preset = crate::proto::BinaryPreset {
            chains: vec![crate::proto::Chain {
                row: Some(crate::proto::chain::Row::Row(2)),
                models: vec![crate::proto::Model {
                    hash: Some(crate::proto::model::Hash::Hash(42)),
                    column: Some(crate::proto::model::Column::Column(0)),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let view = Preset::from_binary(&preset, None, "live", "/fictional/setlist", false);
        assert_eq!((view.rows[0].row, view.rows[0].screen_row), (2, 3));
        assert_eq!((view.blocks[0].row, view.blocks[0].column), (2, 0));
    }

    #[test]
    fn cpu_load_view_preserves_core_assignment() {
        let message = crate::proto::CpuLoadMessage {
            cpu_total_load: Some(crate::proto::cpu_load_message::CpuTotalLoad::CpuTotalLoad(
                37.5,
            )),
            chains: vec![crate::proto::CpuChainLoad {
                columns: vec![crate::proto::CpuColumnLoad {
                    cpu_load: 12.5,
                    is_on_core2: true,
                }],
            }],
            ..Default::default()
        };
        let view = CpuLoad::from(&message);
        assert!(view.total.is_some_and(|load| (load - 37.5).abs() < 0.001));
        assert!((view.chains[0][0].load - 12.5).abs() < 0.001);
        assert!(view.chains[0][0].on_core2);
    }

    #[test]
    fn preset_slot_adds_the_units_display_name() {
        let entry = crate::client::PresetEntry {
            index: 18,
            name: "Fictional Rig".into(),
            key: None,
            instrument: None,
        };
        let slot = PresetSlot::from(&entry);
        assert_eq!(slot.slot, "3C");
        assert_eq!(slot.name, "Fictional Rig");
    }

    #[test]
    fn compact_block_views_round_trip_without_optional_collections() {
        let block = Block {
            row: 0,
            screen_row: 1,
            column: 2,
            model_id: 1001,
            name: Some("Fictional Amp".into()),
            category: Some("Guitar Amplifier".into()),
            based_on: None,
            params: Vec::new(),
            bypass: None,
        };
        let value = serde_json::to_value(&block).unwrap();
        assert!(value.get("params").is_none());
        assert!(value.get("bypass").is_none());
        let decoded: Block = serde_json::from_value(value).unwrap();
        assert!(decoded.params.is_empty());
        assert!(decoded.bypass.is_none());
    }
}

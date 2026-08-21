// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::{Arc, Mutex};

use cortex_host::{DaemonClient, DaemonSupervisor, DeviceHealth, DevicePolicy, Request, Status};
use cortex_rs::client::{USER_SETLIST, is_factory_setlist};
use cortex_rs::view::{CpuLoad, ParamValue, Preset, PresetSlot};

pub mod capability;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardSnapshot {
    pub source: &'static str,
    pub status: Status,
    pub live: Option<LiveSnapshot>,
    pub directory: Vec<SetlistSnapshot>,
    /// Nano fixed-chain state; mutually exclusive with the Quad `live` grid.
    pub nano: Option<cortex_rs::nano::NanoCurrentState>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiveSnapshot {
    pub generation: u64,
    pub revision: u64,
    pub storage_revision: u64,
    pub preset_name: String,
    pub active_scene: u32,
    pub active_scene_label: String,
    pub preset_dirty: Option<bool>,
    pub cpu_load: Option<CpuLoad>,
    pub blocks: Vec<LiveBlock>,
    pub scenes: Vec<SceneSnapshot>,
}

/// One scene as the GUI presents it.
///
/// `index` is the zero-based value the protocol uses; `letter` is the A-H the
/// unit puts on screen. Both are carried deliberately: a control that sends
/// the displayed letter, or displays the wire index, is the row-numbering trap
/// in a different costume. The letter is rendered here rather than in the
/// webview so there is one implementation of that mapping.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SceneSnapshot {
    pub index: u32,
    pub letter: String,
    pub label: Option<String>,
    pub color: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiveBlock {
    pub row: usize,
    pub screen_row: usize,
    pub column: usize,
    pub model_id: u32,
    pub name: String,
    pub category: String,
    pub based_on: Option<String>,
    pub bypassed: bool,
    pub params: Vec<ParamValue>,
    /// Visual family this block belongs to, for colouring the grid the way the
    /// unit and Cortex Control do. See [`block_family`].
    pub family: String,
}

/// Classify a catalog category into the visual family the Quad Cortex colours
/// it as on screen.
///
/// **This mapping is not in the protocol.** The device exposes scene colours
/// but nothing that says what colour a *block* is; the grid palette is a UI
/// convention. The families here were read off Cortex Control's own block
/// picker, so they follow the vendor's grouping rather than an invented one -
/// which is why `Wah` is `utility` and not `filter`, and why `Loopers` is
/// `amp`-red rather than a utility grey. Reasoning about it would have got
/// both wrong.
///
/// Categories are matched exactly against the vocabulary a real unit reports.
/// Anything unrecognised - a category added by a later CorOS - falls back to
/// `other` and is drawn neutrally, rather than being guessed at or dropped.
fn block_family(category: &str) -> &'static str {
    match category {
        "Guitar Amplifier" | "Bass Amplifier" | "Synth" | "Loopers" => "amp",
        "Cabsim Guitar (M)" | "Cabsim Guitar (ST)" | "Cabsim Bass (M)" | "Cabsim Bass (ST)"
        | "IRLoaders" => "cab",
        "Guitar Overdrive" | "Bass Overdrive" => "drive",
        "Compressor" => "dynamics",
        "Equalizer" => "eq",
        "Filter" => "filter",
        "Modulation" => "modulation",
        "Delay" => "delay",
        "Reverb" => "reverb",
        "Pitch" => "pitch",
        "Neural Capture" | "Neural Capture Internal" => "capture",
        // Everything the picker draws in white or grey: morph, wah, the loop
        // and routing plumbing, and the input gate.
        "Morph"
        | "Wah"
        | "FX Loop"
        | "Utility"
        | "Utility_Deprecated"
        | "Splitter"
        | "Mixer"
        | "Internal Routing"
        | "Lane output control"
        | "Tempo control"
        | "InputGateControl" => "utility",
        _ => "other",
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SetlistSnapshot {
    pub key: String,
    pub name: String,
    pub is_factory: bool,
    pub slots: Vec<PresetSlot>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl CommandError {
    fn daemon(message: impl Into<String>) -> Self {
        Self {
            code: "daemon_request_failed",
            message: message.into(),
        }
    }

    fn invalid_cell(row: u32, column: u32) -> Self {
        Self {
            code: "invalid_cell",
            message: format!(
                "row {row}, column {column} is outside the grid; rows are zero-based 0-3 \
                 and display as 1-4, columns are 0-7"
            ),
        }
    }

    fn invalid_scene(scene: u32) -> Self {
        Self {
            code: "invalid_scene",
            message: format!(
                "scene {scene} is out of range; scenes are zero-based 0-7 and display as A-H"
            ),
        }
    }
}

/// Scenes the unit supports, as zero-based indices displayed A-H.
const SCENE_COUNT: u32 = 8;

/// Grid dimensions. Rows are zero-based on the wire and shown as 1-4.
const GRID_ROWS: u32 = 4;
const GRID_COLUMNS: u32 = 8;

trait DashboardSource: Send + Sync {
    fn dashboard(&self) -> Result<DashboardSnapshot, CommandError>;
    fn reconnect_now(&self) -> Result<(), CommandError>;
    fn switch_scene(&self, scene: u32) -> Result<(), CommandError>;
    fn recall_preset(&self, setlist: &str, slot: &str) -> Result<(), CommandError>;
    fn set_scene_label(&self, scene: u32, label: Option<String>) -> Result<(), CommandError>;
    fn set_bypass(&self, row: u32, column: u32, bypass: bool) -> Result<(), CommandError>;
    fn set_scene_color(&self, scene: u32, color: u32) -> Result<(), CommandError>;
    fn block_parameters(&self, row: u32, column: u32) -> Result<Vec<ParameterView>, CommandError>;
    fn set_parameter(
        &self,
        row: u32,
        column: u32,
        index: u32,
        input: cortex_rs::client::ParameterInput,
    ) -> Result<(), CommandError>;
    fn set_nano_amp(
        &self,
        _control: cortex_rs::nano::NanoAmpControl,
        _value: u8,
    ) -> Result<(), CommandError> {
        Err(CommandError::daemon(
            "this dashboard source does not support Nano amp writes",
        ))
    }
    fn set_nano_bypass(
        &self,
        _target: cortex_rs::nano::NanoBypassTarget,
        _bypassed: bool,
    ) -> Result<(), CommandError> {
        Err(CommandError::daemon(
            "this dashboard source does not support Nano bypass writes",
        ))
    }
    fn set_device(&self, _device: Option<cortex_rs::DeviceKind>) -> Result<(), CommandError> {
        Err(CommandError::daemon(
            "this dashboard source does not support device switching",
        ))
    }
    fn read_nano_fx_params(
        &self,
        _slot: cortex_rs::nano::NanoFxSlot,
    ) -> Result<Vec<f32>, CommandError> {
        Err(CommandError::daemon(
            "this dashboard source does not support Nano FX parameter reads",
        ))
    }
    fn set_nano_fx_param(
        &self,
        _slot: cortex_rs::nano::NanoFxSlot,
        _param_index: u8,
        _value: f32,
    ) -> Result<(), CommandError> {
        Err(CommandError::daemon(
            "this dashboard source does not support Nano FX parameter writes",
        ))
    }
}

struct GuiDaemonSupervisor {
    daemon: DaemonSupervisor,
    /// User's device preference. `None` means "try Quad then Nano" (the
    /// original auto-detect behaviour). `Some(Quad)` or `Some(Nano)` means
    /// the user explicitly chose a device; if the running session owns the
    /// wrong one, stop it and start the preferred one.
    preferred_device: Mutex<Option<cortex_rs::DeviceKind>>,
}

impl Default for GuiDaemonSupervisor {
    fn default() -> Self {
        Self {
            daemon: DaemonSupervisor::default(),
            preferred_device: Mutex::new(None),
        }
    }
}

impl GuiDaemonSupervisor {
    fn ensure(&self) -> Result<(), CommandError> {
        let preference = self
            .preferred_device
            .lock()
            .map_err(|_| CommandError::daemon("device preference lock is unavailable"))?;
        let policy = (*preference).map_or(DevicePolicy::Detect, DevicePolicy::Replace);
        self.daemon
            .ensure(policy)
            .map_err(|error| CommandError::daemon(error.to_string()))
    }

    fn set_device(&self, device: Option<cortex_rs::DeviceKind>) -> Result<(), CommandError> {
        let mut preference = self
            .preferred_device
            .lock()
            .map_err(|_| CommandError::daemon("device preference lock is unavailable"))?;
        *preference = device;
        let policy = device.map_or(DevicePolicy::Detect, DevicePolicy::Replace);
        self.daemon
            .ensure(policy)
            .map_err(|error| CommandError::daemon(error.to_string()))
    }
}

struct DaemonDashboardSource {
    client: DaemonClient,
    supervisor: GuiDaemonSupervisor,
    directory_cache: std::sync::Mutex<Option<(u64, u64, Vec<SetlistSnapshot>)>>,
    /// The parsed model catalog. Fetched once and kept: it is a 46 KB transfer
    /// describing 533 models, and the models a unit knows do not change while
    /// it is running.
    catalog_cache: std::sync::Mutex<Option<Arc<cortex_rs::Catalog>>>,
}

impl Default for DaemonDashboardSource {
    fn default() -> Self {
        Self {
            client: DaemonClient::default(),
            supervisor: GuiDaemonSupervisor::default(),
            directory_cache: std::sync::Mutex::new(None),
            catalog_cache: std::sync::Mutex::new(None),
        }
    }
}

impl DashboardSource for DaemonDashboardSource {
    fn dashboard(&self) -> Result<DashboardSnapshot, CommandError> {
        self.supervisor.ensure()?;
        let before = self
            .client
            .require_compatible()
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if before.device_kind == cortex_rs::DeviceKind::NanoCortex {
            let nano = self
                .client
                .request(&Request::NanoState)
                .map_err(|error| CommandError::daemon(error.to_string()))?;
            return Ok(DashboardSnapshot {
                source: "daemon",
                status: before,
                live: None,
                directory: Vec::new(),
                nano: Some(nano),
            });
        }
        if !status_is_live(&before) {
            return Ok(DashboardSnapshot {
                source: "daemon",
                status: before,
                live: None,
                directory: Vec::new(),
                nano: None,
            });
        }

        let preset: Preset = self
            .client
            .request(&Request::CurrentPreset {
                with_params: true,
                timeout_seconds: 15,
            })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        let active_scene: u32 = self
            .client
            .request(&Request::ActiveScene)
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        let cpu_load = self.client.request::<CpuLoad>(&Request::CpuLoad).ok();
        let directory = self.directory(&before);

        let after = self
            .client
            .require_compatible()
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if !status_is_live(&after) || after.cache.generation != before.cache.generation {
            return Ok(DashboardSnapshot {
                source: "daemon",
                status: after,
                live: None,
                directory: Vec::new(),
                nano: None,
            });
        }

        let active_scene_label = preset
            .scenes
            .get(active_scene as usize)
            .and_then(|scene| scene.label.clone())
            .unwrap_or_else(|| scene_letter(active_scene));
        let scenes = scene_snapshots(&preset.scenes);
        let blocks = preset
            .blocks
            .into_iter()
            .map(|block| {
                let bypassed = block
                    .bypass
                    .as_ref()
                    .and_then(|bypass| bypass.scenes.get(active_scene as usize))
                    .copied()
                    .unwrap_or(false);
                let category = block.category.unwrap_or_else(|| "Unknown".into());
                LiveBlock {
                    row: block.row,
                    screen_row: block.screen_row,
                    column: block.column,
                    model_id: block.model_id,
                    name: block
                        .name
                        .unwrap_or_else(|| format!("Model {}", block.model_id)),
                    family: block_family(&category).to_string(),
                    category,
                    based_on: block.based_on,
                    bypassed,
                    params: block.params,
                }
            })
            .collect();
        Ok(DashboardSnapshot {
            source: "daemon",
            live: Some(LiveSnapshot {
                generation: after.cache.generation,
                revision: after.cache.revision,
                storage_revision: after.cache.storage_revision,
                preset_name: preset.name,
                active_scene,
                active_scene_label,
                preset_dirty: None,
                cpu_load,
                blocks,
                scenes,
            }),
            status: after,
            directory,
            nano: None,
        })
    }

    fn set_nano_amp(
        &self,
        control: cortex_rs::nano::NanoAmpControl,
        value: u8,
    ) -> Result<(), CommandError> {
        self.client
            .request::<cortex_rs::nano::NanoCurrentState>(&Request::NanoSetAmp { control, value })
            .map(|_| ())
            .map_err(|error| CommandError::daemon(error.to_string()))
    }

    fn set_nano_bypass(
        &self,
        target: cortex_rs::nano::NanoBypassTarget,
        bypassed: bool,
    ) -> Result<(), CommandError> {
        self.client
            .request::<cortex_rs::nano::NanoCurrentState>(&Request::NanoSetBypass {
                target,
                bypassed,
            })
            .map(|_| ())
            .map_err(|error| CommandError::daemon(error.to_string()))
    }

    fn set_device(&self, device: Option<cortex_rs::DeviceKind>) -> Result<(), CommandError> {
        self.supervisor.set_device(device)
    }

    fn read_nano_fx_params(
        &self,
        slot: cortex_rs::nano::NanoFxSlot,
    ) -> Result<Vec<f32>, CommandError> {
        self.client
            .request::<Vec<f32>>(&Request::NanoReadFxParams { slot })
            .map_err(|error| CommandError::daemon(error.to_string()))
    }

    fn set_nano_fx_param(
        &self,
        slot: cortex_rs::nano::NanoFxSlot,
        param_index: u8,
        value: f32,
    ) -> Result<(), CommandError> {
        self.client
            .request::<Vec<f32>>(&Request::NanoSetFxParam {
                slot,
                param_index,
                value,
            })
            .map(|_| ())
            .map_err(|error| CommandError::daemon(error.to_string()))
    }

    fn reconnect_now(&self) -> Result<(), CommandError> {
        self.client
            .request::<bool>(&Request::ReconnectNow)
            .map(|_| ())
            .map_err(|error| CommandError::daemon(error.to_string()))
    }

    fn switch_scene(&self, scene: u32) -> Result<(), CommandError> {
        // The daemon answers a scene switch with the scene it acted on, not a
        // bare acknowledgement. Decoding that echo rather than discarding it
        // means a daemon that switched to a *different* scene is an error here
        // instead of a GUI that quietly displays the wrong thing.
        let acknowledged: SceneAck = self
            .client
            .request(&Request::SwitchScene { scene })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if acknowledged.scene != scene {
            return Err(CommandError::daemon(format!(
                "asked for scene {scene} but the session acknowledged {}",
                acknowledged.scene
            )));
        }
        Ok(())
    }

    fn recall_preset(&self, setlist: &str, slot: &str) -> Result<(), CommandError> {
        // Whether the target is the read-only factory library is derived here
        // from the path, not taken from the caller: the frontend must not be
        // able to describe a factory setlist as a user one.
        let acknowledged: RecallAck = self
            .client
            .request(&Request::RecallPreset {
                setlist: setlist.to_string(),
                slot: slot.to_string(),
                factory: is_factory_setlist(setlist),
            })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if acknowledged.slot != slot {
            return Err(CommandError::daemon(format!(
                "asked for slot {slot} but the session acknowledged {}",
                acknowledged.slot
            )));
        }
        Ok(())
    }

    fn set_bypass(&self, row: u32, column: u32, bypass: bool) -> Result<(), CommandError> {
        // The daemon reports whether it applied AND verified the change by
        // reading the cell back. Anything short of both is a failure, not a
        // silent no-op: a bypass that reports success while the block still
        // sounds is exactly the fault this project keeps finding.
        let acknowledged: BypassAck = self
            .client
            .request(&Request::SetBypass {
                row,
                column,
                bypass,
            })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if !acknowledged.applied || !acknowledged.verified {
            return Err(CommandError::daemon(format!(
                "the session did not confirm the bypass change at row {row}, column {column} \
                 (applied={}, verified={})",
                acknowledged.applied, acknowledged.verified
            )));
        }
        Ok(())
    }

    fn set_scene_label(&self, scene: u32, label: Option<String>) -> Result<(), CommandError> {
        let acknowledged: SceneAck = self
            .client
            .request(&Request::SetSceneLabel { scene, label })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if acknowledged.scene != scene {
            return Err(CommandError::daemon(format!(
                "asked to label scene {scene} but the session labelled {}",
                acknowledged.scene
            )));
        }
        Ok(())
    }

    fn set_scene_color(&self, scene: u32, color: u32) -> Result<(), CommandError> {
        let acknowledged: SceneAck = self
            .client
            .request(&Request::SetSceneColor { scene, color })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if acknowledged.scene != scene {
            return Err(CommandError::daemon(format!(
                "asked to recolour scene {scene} but the session recoloured {}",
                acknowledged.scene
            )));
        }
        Ok(())
    }

    fn block_parameters(&self, row: u32, column: u32) -> Result<Vec<ParameterView>, CommandError> {
        let catalog = self.catalog()?;
        let preset: Preset = self
            .client
            .request(&Request::CurrentPreset {
                with_params: true,
                timeout_seconds: 15,
            })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        let block = preset
            .blocks
            .iter()
            .find(|candidate| {
                u32::try_from(candidate.row) == Ok(row)
                    && u32::try_from(candidate.column) == Ok(column)
            })
            .ok_or_else(|| CommandError {
                code: "empty_cell",
                message: format!("no block at row {row}, column {column}"),
            })?;
        let model = catalog.get(block.model_id).ok_or_else(|| CommandError {
            code: "unknown_model",
            message: format!(
                "the catalog has no model {} for the block in row {row}, column {column}",
                block.model_id
            ),
        })?;
        Ok(parameter_views(model, &block.params))
    }

    fn set_parameter(
        &self,
        row: u32,
        column: u32,
        index: u32,
        input: cortex_rs::client::ParameterInput,
    ) -> Result<(), CommandError> {
        // The daemon answers with the concrete write it performed, after name
        // and unit resolution. Comparing the echoed index with the one asked
        // for turns "wrote a different parameter" into an error rather than a
        // control that appears to work while moving something else.
        let applied: cortex_rs::client::ParameterWrite = self
            .client
            .request(&Request::SetParam {
                row,
                column,
                target: cortex_rs::client::ParameterTarget::Index(index),
                input,
                // The active scene, and no promotion: changing which scenes a
                // parameter follows is a different decision from changing its
                // value, and doing it as a side effect of moving a control
                // would alter the preset in a way the user did not ask for.
                scene: None,
                promote: false,
                timeout_seconds: 15,
            })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if applied.index != index {
            return Err(CommandError::daemon(format!(
                "asked to write parameter {index} but the session wrote {}",
                applied.index
            )));
        }
        Ok(())
    }
}

/// The daemon's reply to a scene request: the scene it acted on.
#[derive(Debug, Clone, serde::Deserialize)]
struct SceneAck {
    scene: u32,
}

/// The daemon's catalog reply: the raw `ModelRepo` payload, which the crate
/// parses.
#[derive(Debug, Clone, serde::Deserialize)]
struct CatalogPayload {
    payload: Vec<u8>,
}

/// The daemon's reply to a bypass change: whether it applied it and confirmed
/// it by reading the cell back.
#[derive(Debug, Clone, serde::Deserialize)]
struct BypassAck {
    applied: bool,
    verified: bool,
}

/// The daemon's reply to a recall: the slot it acted on.
#[derive(Debug, Clone, serde::Deserialize)]
struct RecallAck {
    slot: String,
}

/// One editable parameter on a block, ready to render.
///
/// The join between the catalog's description of a parameter and the value the
/// preset stores happens in Rust, so the webview never has to know that the
/// wire carries a normalised 0..1 float while the unit displays real units.
/// **Measured on CorOS 4.0.1:** stored float values are normalised - a
/// compressor's `THRESHOLD` reads `0.1458`, not a dB figure - so `real` is the
/// converted value and `normalised` is what the device actually holds.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParameterView {
    /// Positional wire index, which is what a write addresses.
    pub index: u32,
    /// Display name as the unit shows it, e.g. `GAIN`.
    pub name: String,
    /// Control kind: `float`, `int`, `switch`, `str`, `fader`, `meter`, or
    /// `unknown`. `empty` slots are not returned at all.
    pub kind: String,
    /// Units string from the catalog, often empty.
    pub units: String,
    /// Range in the parameter's own units.
    pub min: f64,
    pub max: f64,
    /// Stored normalised value, when the parameter holds a number.
    pub normalised: Option<f64>,
    /// Stored value converted into the parameter's own units, when the catalog
    /// declares a usable range. `None` for a degenerate range, which some
    /// catalog entries genuinely have.
    pub real: Option<f64>,
    /// Stored value when the parameter holds a string instead of a number.
    pub text: Option<String>,
    /// Option labels, in order, for a switch.
    pub step_names: Vec<String>,
    /// True for a live meter, which is a reading and not a setting. Writing to
    /// one is meaningless, so it is shown but never offered as editable.
    pub read_only: bool,
    /// True when the device stores one value per scene for this parameter, so
    /// an edit only affects the active scene.
    pub per_scene: bool,
}

impl DaemonDashboardSource {
    /// The parsed catalog, fetched on first use.
    fn catalog(&self) -> Result<Arc<cortex_rs::Catalog>, CommandError> {
        if let Some(catalog) = self.catalog_cache.lock().unwrap().as_ref() {
            return Ok(Arc::clone(catalog));
        }
        let payload: CatalogPayload = self
            .client
            .request(&Request::Catalog {
                timeout_seconds: 30,
            })
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        let catalog = cortex_rs::Catalog::parse(&payload.payload).map_err(|error| {
            CommandError::daemon(format!("could not parse the device catalog: {error}"))
        })?;
        let catalog = Arc::new(catalog);
        *self.catalog_cache.lock().unwrap() = Some(Arc::clone(&catalog));
        Ok(catalog)
    }

    fn directory(&self, status: &Status) -> Vec<SetlistSnapshot> {
        let cache_key = (status.cache.generation, status.cache.storage_revision);
        if let Some((generation, revision, directory)) =
            self.directory_cache.lock().unwrap().as_ref()
        {
            if (*generation, *revision) == cache_key {
                return directory.clone();
            }
        }
        let mut keys = status.cache.listed_setlists.clone();
        if keys.is_empty() {
            keys.push(USER_SETLIST.into());
        }
        keys.sort();
        keys.dedup();
        let directory: Vec<SetlistSnapshot> = keys
            .into_iter()
            .filter_map(|key| {
                let slots = self
                    .client
                    .request::<Vec<PresetSlot>>(&Request::ListPresets {
                        setlist: key.clone(),
                        include_empty: false,
                        timeout_seconds: 25,
                    })
                    .ok()?;
                if slots.is_empty() {
                    return None;
                }
                let name = if is_factory_setlist(&key) {
                    "Factory Library".into()
                } else {
                    key.rsplit('/').next().unwrap_or(&key).to_string()
                };
                Some(SetlistSnapshot {
                    is_factory: is_factory_setlist(&key),
                    key,
                    name,
                    slots,
                })
            })
            .collect();
        *self.directory_cache.lock().unwrap() = Some((cache_key.0, cache_key.1, directory.clone()));
        directory
    }
}

fn status_is_live(status: &Status) -> bool {
    matches!(status.device, DeviceHealth::Connected { .. })
        && status.cache.phase == cortex_rs::CachePhase::Live
}

/// Join a model's catalog parameters with the values a block stores.
///
/// Driven by the CATALOG's parameter list, not the stored values: the catalog
/// is the description of what the model has, and a stored list can be shorter
/// or carry entries the catalog calls `Empty`. Walking the catalog keeps the
/// positional index meaning what a write will address.
///
/// `Empty` slots are dropped from the result but still consume their index, so
/// what remains addresses correctly. `Meter` entries are kept and marked
/// read-only, because they are worth showing and meaningless to write.
fn parameter_views(model: &cortex_rs::catalog::Model, stored: &[ParamValue]) -> Vec<ParameterView> {
    use cortex_rs::catalog::ParameterKind;

    model
        .parameters
        .iter()
        .filter(|parameter| parameter.kind != ParameterKind::Empty)
        .map(|parameter| {
            let index = u32::try_from(parameter.index).unwrap_or(u32::MAX);
            let held = stored.iter().find(|value| value.index == index);
            let (normalised, text) = match held.map(|value| &value.value) {
                Some(cortex_rs::view::ParamValueKind::Number(number)) => (Some(*number), None),
                Some(cortex_rs::view::ParamValueKind::Text(string)) => (None, Some(string.clone())),
                None => (None, None),
            };
            ParameterView {
                index,
                name: parameter.name.clone(),
                kind: parameter_kind_name(parameter.kind).into(),
                units: parameter.units.clone(),
                min: parameter.min,
                max: parameter.max,
                normalised,
                real: normalised.and_then(|value| parameter.from_normalised(value)),
                text,
                step_names: parameter.step_names.clone(),
                read_only: parameter.kind == ParameterKind::Meter,
                // The device stores one value per scene for a scene-following
                // parameter, so an edit reaches only the active scene.
                per_scene: held.is_some_and(|value| !value.per_scene.is_empty()),
            }
        })
        .collect()
}

fn parameter_kind_name(kind: cortex_rs::catalog::ParameterKind) -> &'static str {
    use cortex_rs::catalog::ParameterKind;
    match kind {
        ParameterKind::Float => "float",
        ParameterKind::Int => "int",
        ParameterKind::Switch => "switch",
        ParameterKind::Str => "str",
        ParameterKind::Fader => "fader",
        ParameterKind::Meter => "meter",
        ParameterKind::Empty => "empty",
        ParameterKind::Unknown => "unknown",
    }
}

fn scene_letter(scene: u32) -> String {
    char::from_u32(u32::from(b'A') + scene)
        .unwrap_or('?')
        .to_string()
}

/// Present all eight scenes, whether or not the preset carries an entry for
/// each. A preset that has never labelled scene H still has scene H, and a
/// selector that hid it would make a reachable scene unreachable.
fn scene_snapshots(scenes: &[cortex_rs::view::Scene]) -> Vec<SceneSnapshot> {
    (0..SCENE_COUNT)
        .map(|index| {
            let stored = scenes.iter().find(|scene| scene.index == index);
            SceneSnapshot {
                index,
                letter: scene_letter(index),
                label: stored.and_then(|scene| scene.label.clone()),
                color: stored.and_then(|scene| scene.color),
            }
        })
        .collect()
}

struct AppState {
    source: Arc<dyn DashboardSource>,
}

fn load_dashboard(source: &dyn DashboardSource) -> Result<DashboardSnapshot, CommandError> {
    source.dashboard()
}

fn request_reconnect(source: &dyn DashboardSource) -> Result<(), CommandError> {
    source.reconnect_now()
}

/// Switch the active scene.
///
/// Non-persistent: this changes the working copy's active scene and what the
/// unit is playing, and saves nothing. It is reversible by switching back.
///
/// The range check happens here rather than at the daemon so an out-of-range
/// index never reaches the device, and the caller gets a typed refusal instead
/// of a transport error.
fn request_switch_scene(source: &dyn DashboardSource, scene: u32) -> Result<(), CommandError> {
    if scene >= SCENE_COUNT {
        return Err(CommandError::invalid_scene(scene));
    }
    source.switch_scene(scene)
}

/// Recall a stored preset into the working copy.
///
/// Recall is free in this project's safety model - MCP-001.1, and what the CLI
/// and MCP already do - because it writes nothing to storage. It is not without
/// consequence: it changes what the unit is playing and replaces the working
/// copy, discarding unsaved edits, exactly as pressing the preset on the unit
/// does. Saving is the operation that requires confirmation.
///
/// Empty setlist or slot is refused here rather than sent, so a frontend bug
/// cannot turn into a device request.
fn request_recall_preset(
    source: &dyn DashboardSource,
    setlist: &str,
    slot: &str,
) -> Result<(), CommandError> {
    if setlist.trim().is_empty() || slot.trim().is_empty() {
        return Err(CommandError {
            code: "invalid_preset_target",
            message: "a recall needs both a setlist path and a slot".into(),
        });
    }
    source.recall_preset(setlist, slot)
}

#[tauri::command]
async fn dashboard(state: tauri::State<'_, AppState>) -> Result<DashboardSnapshot, CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || load_dashboard(source.as_ref()))
        .await
        .map_err(|error| CommandError::daemon(format!("dashboard task failed: {error}")))?
}

#[tauri::command]
async fn reconnect_now(state: tauri::State<'_, AppState>) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || request_reconnect(source.as_ref()))
        .await
        .map_err(|error| CommandError::daemon(format!("reconnect task failed: {error}")))?
}

#[tauri::command]
async fn switch_scene(state: tauri::State<'_, AppState>, scene: u32) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || request_switch_scene(source.as_ref(), scene))
        .await
        .map_err(|error| CommandError::daemon(format!("switch scene task failed: {error}")))?
}

/// Bypass or engage a block.
///
/// Non-persistent, and per-scene: the device stores bypass per scene, so this
/// reaches the ACTIVE scene only - the same rule the parameter editor's badge
/// states. `row` is the zero-based WIRE row, never the 1-4 screen row.
fn request_set_bypass(
    source: &dyn DashboardSource,
    row: u32,
    column: u32,
    bypass: bool,
) -> Result<(), CommandError> {
    if row >= GRID_ROWS || column >= GRID_COLUMNS {
        return Err(CommandError::invalid_cell(row, column));
    }
    source.set_bypass(row, column, bypass)
}

/// Rename a scene on the working copy, or clear its label.
///
/// Non-persistent, like every other edit here: it changes the working copy and
/// saves nothing. An empty or whitespace-only label is sent as "no label"
/// rather than as a blank string, because the unit stores a one-space value for
/// unlabelled and a GUI that wrote `""` would be inventing a third state.
fn request_set_scene_label(
    source: &dyn DashboardSource,
    scene: u32,
    label: Option<String>,
) -> Result<(), CommandError> {
    if scene >= SCENE_COUNT {
        return Err(CommandError::invalid_scene(scene));
    }
    let label = label.filter(|text| !text.trim().is_empty());
    source.set_scene_label(scene, label)
}

/// Recolour a scene on the working copy.
///
/// The unit accepts arbitrary ARGB, not just its own palette: CorOS 4.0.1
/// stored and read back off-palette `0xFF808080` exactly, and stepping through
/// eight scenes on real hardware showed the reported colours on the physical
/// LEDs (2026-08-16). So this offers full RGB rather than reproducing a fixed
/// set. Alpha is forced opaque, since a transparent scene LED is not a thing
/// the hardware can show and a zero alpha would silently read as black.
fn request_set_scene_color(
    source: &dyn DashboardSource,
    scene: u32,
    color: u32,
) -> Result<(), CommandError> {
    if scene >= SCENE_COUNT {
        return Err(CommandError::invalid_scene(scene));
    }
    source.set_scene_color(scene, 0xFF00_0000 | (color & 0x00FF_FFFF))
}

/// Read the editable parameters of one block.
///
/// `row` is the ZERO-BASED WIRE row, which is what the protocol addresses.
/// The unit shows rows as 1-4, and a write to the wrong row succeeds silently,
/// so the two are never interchanged: the frontend passes the `row` it was
/// given in the block, never the `screen_row` it displays.
fn request_block_parameters(
    source: &dyn DashboardSource,
    row: u32,
    column: u32,
) -> Result<Vec<ParameterView>, CommandError> {
    if row >= GRID_ROWS || column >= GRID_COLUMNS {
        return Err(CommandError::invalid_cell(row, column));
    }
    source.block_parameters(row, column)
}

/// Write one parameter on one block.
///
/// Non-persistent: this edits the working copy and changes what is heard, and
/// saves nothing.
fn request_set_parameter(
    source: &dyn DashboardSource,
    row: u32,
    column: u32,
    index: u32,
    input: cortex_rs::client::ParameterInput,
) -> Result<(), CommandError> {
    if row >= GRID_ROWS || column >= GRID_COLUMNS {
        return Err(CommandError::invalid_cell(row, column));
    }
    source.set_parameter(row, column, index, input)
}

#[tauri::command]
async fn set_bypass(
    state: tauri::State<'_, AppState>,
    row: u32,
    column: u32,
    bypass: bool,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || {
        request_set_bypass(source.as_ref(), row, column, bypass)
    })
    .await
    .map_err(|error| CommandError::daemon(format!("bypass task failed: {error}")))?
}

#[tauri::command]
async fn set_nano_amp(
    state: tauri::State<'_, AppState>,
    control: cortex_rs::nano::NanoAmpControl,
    value: u8,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || source.set_nano_amp(control, value))
        .await
        .map_err(|error| CommandError::daemon(format!("Nano amp write task failed: {error}")))?
}

#[tauri::command]
async fn set_nano_bypass(
    state: tauri::State<'_, AppState>,
    target: cortex_rs::nano::NanoBypassTarget,
    bypassed: bool,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || source.set_nano_bypass(target, bypassed))
        .await
        .map_err(|error| CommandError::daemon(format!("Nano bypass write task failed: {error}")))?
}

#[tauri::command]
async fn set_device(
    state: tauri::State<'_, AppState>,
    device: Option<cortex_rs::DeviceKind>,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || source.set_device(device))
        .await
        .map_err(|error| CommandError::daemon(format!("set_device task failed: {error}")))?
}

#[tauri::command]
async fn read_nano_fx_params(
    state: tauri::State<'_, AppState>,
    slot: cortex_rs::nano::NanoFxSlot,
) -> Result<Vec<f32>, CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || source.read_nano_fx_params(slot))
        .await
        .map_err(|error| CommandError::daemon(format!("Nano FX param read task failed: {error}")))?
}

#[tauri::command]
async fn set_nano_fx_param(
    state: tauri::State<'_, AppState>,
    slot: cortex_rs::nano::NanoFxSlot,
    param_index: u8,
    value: f32,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || source.set_nano_fx_param(slot, param_index, value))
        .await
        .map_err(|error| {
            CommandError::daemon(format!("Nano FX param write task failed: {error}"))
        })?
}

#[tauri::command]
async fn set_scene_label(
    state: tauri::State<'_, AppState>,
    scene: u32,
    label: Option<String>,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || {
        request_set_scene_label(source.as_ref(), scene, label)
    })
    .await
    .map_err(|error| CommandError::daemon(format!("scene label task failed: {error}")))?
}

#[tauri::command]
async fn set_scene_color(
    state: tauri::State<'_, AppState>,
    scene: u32,
    color: u32,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || {
        request_set_scene_color(source.as_ref(), scene, color)
    })
    .await
    .map_err(|error| CommandError::daemon(format!("scene colour task failed: {error}")))?
}

#[tauri::command]
async fn block_parameters(
    state: tauri::State<'_, AppState>,
    row: u32,
    column: u32,
) -> Result<Vec<ParameterView>, CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || {
        request_block_parameters(source.as_ref(), row, column)
    })
    .await
    .map_err(|error| CommandError::daemon(format!("parameter read task failed: {error}")))?
}

#[tauri::command]
async fn set_parameter(
    state: tauri::State<'_, AppState>,
    row: u32,
    column: u32,
    index: u32,
    input: cortex_rs::client::ParameterInput,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || {
        request_set_parameter(source.as_ref(), row, column, index, input)
    })
    .await
    .map_err(|error| CommandError::daemon(format!("parameter write task failed: {error}")))?
}

#[tauri::command]
async fn recall_preset(
    state: tauri::State<'_, AppState>,
    setlist: String,
    slot: String,
) -> Result<(), CommandError> {
    let source = Arc::clone(&state.source);
    tauri::async_runtime::spawn_blocking(move || {
        request_recall_preset(source.as_ref(), &setlist, &slot)
    })
    .await
    .map_err(|error| CommandError::daemon(format!("recall task failed: {error}")))?
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            source: Arc::new(DaemonDashboardSource::default()),
        })
        .invoke_handler(tauri::generate_handler![
            dashboard,
            reconnect_now,
            switch_scene,
            recall_preset,
            block_parameters,
            set_parameter,
            set_scene_label,
            set_scene_color,
            set_bypass,
            set_nano_amp,
            set_nano_bypass,
            set_device,
            read_nano_fx_params,
            set_nano_fx_param
        ])
        .run(tauri::generate_context!())
        .expect("Tauri application failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_letters_are_rendered_in_rust() {
        assert_eq!(scene_letter(0), "A");
        assert_eq!(scene_letter(7), "H");
    }

    #[test]
    fn reconnecting_status_is_not_live() {
        let status = Status {
            daemon_version: "4".into(),
            uptime_seconds: 1,
            auto_managed: false,
            idle_timeout_seconds: None,
            device_kind: cortex_rs::DeviceKind::QuadCortex,
            device: DeviceHealth::Reconnecting {
                attempts: 2,
                last_error: "device absent".into(),
            },
            cache: cortex_host::CacheStatus {
                phase: cortex_rs::CachePhase::Live,
                ..Default::default()
            },
        };
        assert!(!status_is_live(&status));
    }

    #[test]
    #[ignore = "requires a running Nano Cortex held session"]
    fn nano_dashboard_reads_the_typed_fixed_chain_from_the_daemon() {
        let snapshot = load_dashboard(&DaemonDashboardSource::default()).unwrap();
        assert_eq!(
            snapshot.status.device_kind,
            cortex_rs::DeviceKind::NanoCortex
        );
        assert!(snapshot.live.is_none());
        assert!(snapshot.directory.is_empty());
        let nano = snapshot.nano.expect("Nano state");
        assert_eq!(nano.slots.len(), 8);
        assert_eq!(nano.slots[0].role, cortex_rs::nano::NanoSlotRole::Gate);
        assert_eq!(nano.slots[7].role, cortex_rs::nano::NanoSlotRole::PostFx3);
    }

    #[test]
    #[ignore = "requires a running Nano Cortex held session; transiently changes gain by one raw step"]
    fn nano_amp_write_reaches_the_daemon_reads_back_and_restores() {
        let source = DaemonDashboardSource::default();
        let original = source
            .dashboard()
            .unwrap()
            .nano
            .unwrap()
            .amp
            .gain
            .expect("gain");
        let changed = if original == u8::MAX {
            original - 1
        } else {
            original + 1
        };
        let exercise = (|| {
            source.set_nano_amp(cortex_rs::nano::NanoAmpControl::Gain, changed)?;
            let actual = source.dashboard()?.nano.unwrap().amp.gain;
            assert_eq!(actual, Some(changed));
            Ok::<_, CommandError>(())
        })();
        source
            .set_nano_amp(cortex_rs::nano::NanoAmpControl::Gain, original)
            .expect("restore gain");
        assert_eq!(
            source.dashboard().unwrap().nano.unwrap().amp.gain,
            Some(original)
        );
        exercise.unwrap();
    }

    struct FailingSource;

    impl DashboardSource for FailingSource {
        fn dashboard(&self) -> Result<DashboardSnapshot, CommandError> {
            Err(CommandError::daemon("fixture must not replace this error"))
        }

        fn reconnect_now(&self) -> Result<(), CommandError> {
            Err(CommandError::daemon("fixture must not reconnect"))
        }

        fn switch_scene(&self, _scene: u32) -> Result<(), CommandError> {
            Err(CommandError::daemon("fixture must not switch scenes"))
        }

        fn recall_preset(&self, _setlist: &str, _slot: &str) -> Result<(), CommandError> {
            Err(CommandError::daemon("fixture must not recall"))
        }

        fn set_scene_label(&self, _scene: u32, _label: Option<String>) -> Result<(), CommandError> {
            Err(CommandError::daemon("fixture must not label scenes"))
        }

        fn set_bypass(&self, _row: u32, _column: u32, _bypass: bool) -> Result<(), CommandError> {
            Err(CommandError::daemon("fixture must not bypass blocks"))
        }

        fn set_scene_color(&self, _scene: u32, _color: u32) -> Result<(), CommandError> {
            Err(CommandError::daemon("fixture must not recolour scenes"))
        }

        fn block_parameters(
            &self,
            _row: u32,
            _column: u32,
        ) -> Result<Vec<ParameterView>, CommandError> {
            Err(CommandError::daemon("fixture must not read parameters"))
        }

        fn set_parameter(
            &self,
            _row: u32,
            _column: u32,
            _index: u32,
            _input: cortex_rs::client::ParameterInput,
        ) -> Result<(), CommandError> {
            Err(CommandError::daemon("fixture must not write parameters"))
        }
    }

    /// Records what reached the source, so a test can prove something did
    /// *not* reach the device.
    struct RecordingSource {
        switched: std::sync::Mutex<Vec<u32>>,
        recalled: std::sync::Mutex<Vec<(String, String)>>,
        written: std::sync::Mutex<Vec<(u32, u32, u32, cortex_rs::client::ParameterInput)>>,
        labelled: std::sync::Mutex<Vec<(u32, Option<String>)>>,
        coloured: std::sync::Mutex<Vec<(u32, u32)>>,
        bypassed: std::sync::Mutex<Vec<(u32, u32, bool)>>,
    }

    impl RecordingSource {
        fn new() -> Self {
            Self {
                switched: std::sync::Mutex::new(Vec::new()),
                recalled: std::sync::Mutex::new(Vec::new()),
                written: std::sync::Mutex::new(Vec::new()),
                labelled: std::sync::Mutex::new(Vec::new()),
                coloured: std::sync::Mutex::new(Vec::new()),
                bypassed: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl DashboardSource for RecordingSource {
        fn dashboard(&self) -> Result<DashboardSnapshot, CommandError> {
            Err(CommandError::daemon("not used by these tests"))
        }

        fn reconnect_now(&self) -> Result<(), CommandError> {
            Err(CommandError::daemon("not used by these tests"))
        }

        fn switch_scene(&self, scene: u32) -> Result<(), CommandError> {
            self.switched.lock().unwrap().push(scene);
            Ok(())
        }

        fn recall_preset(&self, setlist: &str, slot: &str) -> Result<(), CommandError> {
            self.recalled
                .lock()
                .unwrap()
                .push((setlist.to_string(), slot.to_string()));
            Ok(())
        }

        fn set_scene_label(&self, scene: u32, label: Option<String>) -> Result<(), CommandError> {
            self.labelled.lock().unwrap().push((scene, label));
            Ok(())
        }

        fn set_bypass(&self, row: u32, column: u32, bypass: bool) -> Result<(), CommandError> {
            self.bypassed.lock().unwrap().push((row, column, bypass));
            Ok(())
        }

        fn set_scene_color(&self, scene: u32, color: u32) -> Result<(), CommandError> {
            self.coloured.lock().unwrap().push((scene, color));
            Ok(())
        }

        fn block_parameters(
            &self,
            _row: u32,
            _column: u32,
        ) -> Result<Vec<ParameterView>, CommandError> {
            Ok(Vec::new())
        }

        fn set_parameter(
            &self,
            row: u32,
            column: u32,
            index: u32,
            input: cortex_rs::client::ParameterInput,
        ) -> Result<(), CommandError> {
            self.written
                .lock()
                .unwrap()
                .push((row, column, index, input));
            Ok(())
        }
    }

    #[test]
    fn backend_errors_do_not_fall_back_to_fixture_data() {
        let error = load_dashboard(&FailingSource).unwrap_err();
        assert_eq!(error.code, "daemon_request_failed");
        assert!(error.message.contains("must not replace"));
    }

    #[test]
    fn every_scene_is_selectable_even_when_the_preset_labels_none_of_them() {
        let snapshots = scene_snapshots(&[]);
        assert_eq!(snapshots.len(), 8);
        assert_eq!(snapshots[0].letter, "A");
        assert_eq!(snapshots[7].letter, "H");
        assert_eq!(snapshots[7].index, 7);
        assert!(snapshots.iter().all(|scene| scene.label.is_none()));
    }

    #[test]
    fn scene_snapshots_keep_the_zero_based_index_beside_the_displayed_letter() {
        let stored = vec![
            cortex_rs::view::Scene {
                index: 0,
                label: Some("Clean".into()),
                color: Some(0xFF00_FF00),
            },
            cortex_rs::view::Scene {
                index: 3,
                label: Some("Lead".into()),
                color: None,
            },
        ];
        let snapshots = scene_snapshots(&stored);

        // Scene 3 is the fourth scene and displays as D. An off-by-one here is
        // the same class of bug as the grid's row-numbering trap.
        assert_eq!(snapshots[3].index, 3);
        assert_eq!(snapshots[3].letter, "D");
        assert_eq!(snapshots[3].label.as_deref(), Some("Lead"));
        assert_eq!(snapshots[0].color, Some(0xFF00_FF00));
        // An unlabelled scene stays unlabelled rather than borrowing a neighbour's.
        assert_eq!(snapshots[1].label, None);
    }

    #[test]
    fn an_out_of_range_scene_is_refused_before_it_reaches_the_device() {
        let source = RecordingSource::new();
        let error = request_switch_scene(&source, 8).unwrap_err();
        assert_eq!(error.code, "invalid_scene");
        assert!(source.switched.lock().unwrap().is_empty());
    }

    /// The exact category vocabulary a real Quad Cortex reported on CorOS
    /// 4.0.1 (533 models across 31 categories). Kept verbatim so a CorOS
    /// update that renames a category fails this test rather than silently
    /// greying out a block.
    const MEASURED_CATEGORIES: [&str; 31] = [
        "Bass Amplifier",
        "Bass Overdrive",
        "Cabsim Bass (M)",
        "Cabsim Bass (ST)",
        "Cabsim Guitar (M)",
        "Cabsim Guitar (ST)",
        "Compressor",
        "Delay",
        "Equalizer",
        "FX Loop",
        "Filter",
        "Guitar Amplifier",
        "Guitar Overdrive",
        "IRLoaders",
        "InputGateControl",
        "Internal Routing",
        "Lane output control",
        "Loopers",
        "Mixer",
        "Modulation",
        "Morph",
        "Neural Capture",
        "Neural Capture Internal",
        "Pitch",
        "Reverb",
        "Splitter",
        "Synth",
        "Tempo control",
        "Utility",
        "Utility_Deprecated",
        "Wah",
    ];

    #[test]
    fn every_category_a_real_unit_reports_has_a_family() {
        let unclassified: Vec<&str> = MEASURED_CATEGORIES
            .into_iter()
            .filter(|category| block_family(category) == "other")
            .collect();
        assert!(
            unclassified.is_empty(),
            "these categories would be drawn as unknown: {unclassified:?}"
        );
    }

    #[test]
    fn families_follow_cortex_controls_own_grouping_not_intuition() {
        // Read off Cortex Control's block picker. Each of these is somewhere
        // a reasonable guess would have gone wrong.
        assert_eq!(
            block_family("Wah"),
            "utility",
            "the picker draws Wah grey, not as a filter"
        );
        assert_eq!(
            block_family("Loopers"),
            "amp",
            "the picker draws Looper red"
        );
        assert_eq!(block_family("Synth"), "amp", "the picker draws Synth red");
        assert_eq!(
            block_family("IRLoaders"),
            "cab",
            "IR loader shares the cab colour"
        );
        assert_eq!(block_family("Morph"), "utility");
        assert_eq!(block_family("Equalizer"), "eq");
        assert_eq!(block_family("Filter"), "filter");
    }

    #[test]
    fn an_unknown_future_category_is_drawn_neutrally_rather_than_guessed() {
        assert_eq!(block_family("Quantum Flux Capacitor"), "other");
        assert_eq!(block_family(""), "other");
    }

    fn parameter(
        index: usize,
        name: &str,
        kind: cortex_rs::catalog::ParameterKind,
        min: f64,
        max: f64,
    ) -> cortex_rs::catalog::Parameter {
        cortex_rs::catalog::Parameter {
            index,
            name: name.into(),
            kind,
            min,
            max,
            default: min,
            units: String::new(),
            step_names: Vec::new(),
        }
    }

    fn model_with(parameters: Vec<cortex_rs::catalog::Parameter>) -> cortex_rs::catalog::Model {
        cortex_rs::catalog::Model {
            id: 5007,
            name: "Test Model".into(),
            category_id: 1,
            category: "Compressor".into(),
            based_on: None,
            parameters,
        }
    }

    fn stored_number(index: u32, value: f64) -> ParamValue {
        ParamValue {
            index,
            name: None,
            value: cortex_rs::view::ParamValueKind::Number(value),
            per_scene: Vec::new(),
        }
    }

    #[test]
    fn a_stored_normalised_value_is_converted_into_the_parameters_own_units() {
        use cortex_rs::catalog::ParameterKind;
        // Measured on CorOS 4.0.1: the wire holds 0..1, not the displayed
        // figure. A GAIN of 0.5 over a 0..10 range is 5, not 0.5.
        let model = model_with(vec![parameter(0, "GAIN", ParameterKind::Float, 0.0, 10.0)]);
        let views = parameter_views(&model, &[stored_number(0, 0.5)]);

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].normalised, Some(0.5));
        assert_eq!(views[0].real, Some(5.0));
        assert!(!views[0].read_only);
    }

    #[test]
    fn a_degenerate_range_yields_no_real_value_rather_than_a_wrong_one() {
        use cortex_rs::catalog::ParameterKind;
        // Some catalog entries declare min == max. There is no conversion to
        // be had, and inventing one would put a confident wrong number on screen.
        let model = model_with(vec![parameter(0, "FIXED", ParameterKind::Float, 1.0, 1.0)]);
        let views = parameter_views(&model, &[stored_number(0, 0.5)]);

        assert_eq!(views[0].normalised, Some(0.5));
        assert_eq!(views[0].real, None);
    }

    #[test]
    fn empty_slots_are_dropped_but_do_not_shift_the_indices_that_remain() {
        use cortex_rs::catalog::ParameterKind;
        // `Empty` occupies a wire index. Renumbering what follows would make
        // every later write address the wrong parameter.
        let model = model_with(vec![
            parameter(0, "GAIN", ParameterKind::Float, 0.0, 10.0),
            parameter(1, "", ParameterKind::Empty, 0.0, 0.0),
            parameter(2, "TONE", ParameterKind::Float, 0.0, 10.0),
        ]);
        let views = parameter_views(&model, &[stored_number(0, 0.1), stored_number(2, 0.9)]);

        assert_eq!(views.len(), 2);
        assert_eq!(views[0].index, 0);
        assert_eq!(
            views[1].index, 2,
            "TONE keeps index 2, it does not become 1"
        );
        assert_eq!(views[1].real, Some(9.0));
    }

    #[test]
    fn a_meter_is_shown_but_never_offered_as_editable() {
        use cortex_rs::catalog::ParameterKind;
        let model = model_with(vec![parameter(
            0,
            "GAIN REDUCTION",
            ParameterKind::Meter,
            0.0,
            1.0,
        )]);
        let views = parameter_views(&model, &[stored_number(0, 1.0)]);

        assert_eq!(views.len(), 1, "a meter is worth showing");
        assert!(
            views[0].read_only,
            "but writing to a reading is meaningless"
        );
    }

    #[test]
    fn a_parameter_the_preset_does_not_store_is_still_described() {
        use cortex_rs::catalog::ParameterKind;
        // The catalog is the description of the model; a stored list can be
        // shorter. The control still has to appear, without a value.
        let model = model_with(vec![parameter(0, "GAIN", ParameterKind::Float, 0.0, 10.0)]);
        let views = parameter_views(&model, &[]);

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].normalised, None);
        assert_eq!(views[0].real, None);
    }

    #[test]
    fn a_string_parameter_survives_as_text_rather_than_a_number() {
        use cortex_rs::catalog::ParameterKind;
        let model = model_with(vec![parameter(0, "MIC", ParameterKind::Str, 0.0, 0.0)]);
        let views = parameter_views(
            &model,
            &[ParamValue {
                index: 0,
                name: None,
                value: cortex_rs::view::ParamValueKind::Text("SM57".into()),
                per_scene: Vec::new(),
            }],
        );

        assert_eq!(views[0].text.as_deref(), Some("SM57"));
        assert_eq!(views[0].normalised, None);
    }

    #[test]
    fn a_cell_outside_the_grid_is_refused_before_it_reaches_the_device() {
        let source = RecordingSource::new();
        for (row, column) in [(4, 0), (0, 8), (99, 99)] {
            let read = request_block_parameters(&source, row, column).unwrap_err();
            assert_eq!(read.code, "invalid_cell");
            let write = request_set_parameter(
                &source,
                row,
                column,
                0,
                cortex_rs::client::ParameterInput::Normalised(0.5),
            )
            .unwrap_err();
            assert_eq!(write.code, "invalid_cell");
        }
        assert!(source.written.lock().unwrap().is_empty());
    }

    #[test]
    fn a_parameter_write_reaches_the_device_on_the_wire_row_it_was_given() {
        let source = RecordingSource::new();
        // Row 3 is the fourth row, shown as 4 on the unit. A write that
        // "helpfully" converted between the two would edit the wrong row and
        // report success.
        request_set_parameter(
            &source,
            3,
            7,
            12,
            cortex_rs::client::ParameterInput::Real(6.0),
        )
        .unwrap();
        assert_eq!(
            *source.written.lock().unwrap(),
            vec![(3, 7, 12, cortex_rs::client::ParameterInput::Real(6.0))]
        );
    }

    #[test]
    fn a_blank_label_clears_the_scene_rather_than_writing_an_empty_string() {
        let source = RecordingSource::new();
        request_set_scene_label(&source, 0, Some("Lead".into())).unwrap();
        request_set_scene_label(&source, 1, Some("   ".into())).unwrap();
        request_set_scene_label(&source, 2, Some(String::new())).unwrap();
        request_set_scene_label(&source, 3, None).unwrap();

        // The unit stores a one-space value for "unlabelled". Sending "" would
        // invent a third state that neither the device nor the reader has.
        assert_eq!(
            *source.labelled.lock().unwrap(),
            vec![
                (0, Some("Lead".to_string())),
                (1, None),
                (2, None),
                (3, None),
            ]
        );
    }

    #[test]
    fn scene_colours_are_forced_opaque_so_a_zero_alpha_cannot_read_as_black() {
        let source = RecordingSource::new();
        request_set_scene_color(&source, 0, 0x0080_8080).unwrap();
        request_set_scene_color(&source, 1, 0x00FF_2727).unwrap();

        assert_eq!(
            *source.coloured.lock().unwrap(),
            vec![(0, 0xFF80_8080), (1, 0xFFFF_2727)],
            "an LED cannot be transparent; a zero alpha would silently render black"
        );
    }

    #[test]
    fn an_out_of_range_scene_is_refused_for_label_and_colour_too() {
        let source = RecordingSource::new();
        assert_eq!(
            request_set_scene_label(&source, 8, Some("x".into()))
                .unwrap_err()
                .code,
            "invalid_scene"
        );
        assert_eq!(
            request_set_scene_color(&source, 8, 0x00FF_FFFF)
                .unwrap_err()
                .code,
            "invalid_scene"
        );
        assert!(source.labelled.lock().unwrap().is_empty());
        assert!(source.coloured.lock().unwrap().is_empty());
    }

    #[test]
    fn bypass_carries_the_wire_row_and_refuses_a_cell_outside_the_grid() {
        let source = RecordingSource::new();
        request_set_bypass(&source, 3, 7, true).unwrap();
        request_set_bypass(&source, 0, 0, false).unwrap();
        assert_eq!(
            *source.bypassed.lock().unwrap(),
            vec![(3, 7, true), (0, 0, false)]
        );

        assert_eq!(
            request_set_bypass(&source, 4, 0, true).unwrap_err().code,
            "invalid_cell"
        );
        assert_eq!(source.bypassed.lock().unwrap().len(), 2);
    }

    #[test]
    fn an_incomplete_recall_target_is_refused_before_it_reaches_the_device() {
        let source = RecordingSource::new();
        for (setlist, slot) in [("", "1A"), ("/x", ""), ("   ", "1A"), ("/x", "  ")] {
            let error = request_recall_preset(&source, setlist, slot).unwrap_err();
            assert_eq!(error.code, "invalid_preset_target");
        }
        assert!(source.recalled.lock().unwrap().is_empty());
    }

    #[test]
    fn a_recall_passes_the_exact_setlist_and_slot_through() {
        let source = RecordingSource::new();
        request_recall_preset(&source, "/media/p4/Presets/My Presets", "12C").unwrap();
        assert_eq!(
            *source.recalled.lock().unwrap(),
            vec![(
                "/media/p4/Presets/My Presets".to_string(),
                "12C".to_string()
            )],
            "the slot must not be normalised, reformatted, or renumbered on the way out"
        );
    }

    #[test]
    fn in_range_scenes_reach_the_device_unchanged() {
        let source = RecordingSource::new();
        request_switch_scene(&source, 0).unwrap();
        request_switch_scene(&source, 7).unwrap();
        // Sent zero-based, exactly as received: no letter conversion on the way out.
        assert_eq!(*source.switched.lock().unwrap(), vec![0, 7]);
    }

    /// Drives the real `switch_scene` command path against the device and
    /// proves the unit *reports back* the scene that was asked for, rather
    /// than trusting that the write was accepted.
    ///
    /// Restores the scene it found. Needs no blocks, so it runs against an
    /// empty working grid.
    #[test]
    #[ignore = "requires a running held session and a real Quad Cortex; audibly changes the active scene"]
    fn switching_scenes_is_reflected_by_the_device() {
        let source = DaemonDashboardSource::default();
        let before = load_dashboard(&source)
            .unwrap()
            .live
            .expect("held session should expose live state");
        let original = before.active_scene;
        assert_eq!(before.scenes.len(), 8, "the unit has eight scenes");

        // Move somewhere that is definitely not where we started.
        let target = if original == 0 { 1 } else { 0 };
        request_switch_scene(&source, target).unwrap();

        // Poll: the switch is answered by a push, which takes a moment to
        // land in the cache.
        let mut observed = None;
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if let Some(live) = load_dashboard(&source).unwrap().live {
                if live.active_scene == target {
                    observed = Some(live);
                    break;
                }
            }
        }
        let live = observed.expect("device should report the scene that was requested");
        assert_eq!(live.active_scene, target);
        assert_eq!(
            live.scenes[target as usize].letter,
            scene_letter(target),
            "the displayed letter must track the zero-based index"
        );

        request_switch_scene(&source, original).unwrap();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if let Some(live) = load_dashboard(&source).unwrap().live {
                if live.active_scene == original {
                    return;
                }
            }
        }
        panic!("failed to restore the scene the unit started on ({original})");
    }

    /// A scene change the GUI did not cause must still appear in it. Here the
    /// change is made through a second daemon client, standing in for the
    /// footswitch: the point is that the GUI learns the state by reading, not
    /// by remembering what it sent.
    ///
    /// Restores the scene it found.
    #[test]
    #[ignore = "requires a running held session and a real Quad Cortex; audibly changes the active scene"]
    fn externally_originated_scene_changes_reach_the_gui() {
        let gui = DaemonDashboardSource::default();
        let elsewhere = DaemonDashboardSource::default();

        let original = load_dashboard(&gui)
            .unwrap()
            .live
            .expect("held session should expose live state")
            .active_scene;
        let target = if original == 0 { 1 } else { 0 };

        // Not through `gui`: this is someone else moving the unit.
        elsewhere.switch_scene(target).unwrap();

        let mut seen = false;
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if let Some(live) = load_dashboard(&gui).unwrap().live {
                if live.active_scene == target {
                    seen = true;
                    break;
                }
            }
        }
        assert!(
            seen,
            "the GUI never observed a scene change it did not make"
        );

        elsewhere.switch_scene(original).unwrap();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if let Some(live) = load_dashboard(&gui).unwrap().live {
                if live.active_scene == original {
                    return;
                }
            }
        }
        panic!("failed to restore the scene the unit started on ({original})");
    }

    /// Drives the real `recall_preset` command path and proves the device
    /// reports the recalled preset back, rather than trusting the write.
    ///
    /// The slot is named by the operator, not chosen here: a recall replaces
    /// the working copy, and only the operator knows whether the current one
    /// is disposable.
    ///
    /// ```sh
    /// CORTEX_GUI_RECALL_SETLIST="/media/p4/Presets/My Presets" \
    /// CORTEX_GUI_RECALL_SLOT=1A \
    ///   cargo test -p cortex-gui recalling_a_preset -- --ignored --exact --nocapture
    /// ```
    #[test]
    #[ignore = "requires a real Quad Cortex, a held session, and an operator-named slot; REPLACES the working copy"]
    fn recalling_a_preset_is_reflected_by_the_device() {
        let setlist = std::env::var("CORTEX_GUI_RECALL_SETLIST")
            .expect("set CORTEX_GUI_RECALL_SETLIST to the setlist path to recall from");
        let slot = std::env::var("CORTEX_GUI_RECALL_SLOT")
            .expect("set CORTEX_GUI_RECALL_SLOT to a slot you are willing to recall");

        let source = DaemonDashboardSource::default();
        let expected_name = load_dashboard(&source)
            .unwrap()
            .directory
            .iter()
            .find(|entry| entry.key == setlist)
            .and_then(|entry| {
                entry
                    .slots
                    .iter()
                    .find(|candidate| candidate.slot == slot)
                    .map(|candidate| candidate.name.clone())
            })
            .expect("the named slot should appear in the directory listing");

        request_recall_preset(&source, &setlist, &slot).unwrap();

        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if let Some(live) = load_dashboard(&source).unwrap().live {
                if live.preset_name == expected_name {
                    // The recalled preset is live, and it brought a grid with it.
                    assert!(
                        !live.blocks.is_empty(),
                        "a recalled preset should report its blocks"
                    );
                    assert_eq!(live.scenes.len(), 8);
                    return;
                }
            }
        }
        panic!("device never reported {expected_name} as the live preset after recall");
    }

    /// Reads a real block's parameters, writes one, and proves the device
    /// reports the new value back - then restores what it found.
    ///
    /// Needs a preset with blocks loaded; the operator names the cell, since
    /// this edits the working copy and changes what is heard.
    ///
    /// ```sh
    /// CORTEX_GUI_PARAM_ROW=0 CORTEX_GUI_PARAM_COLUMN=1 \
    ///   cargo test -p cortex-gui editing_a_parameter -- --ignored --exact --nocapture
    /// ```
    #[test]
    #[ignore = "requires a real Quad Cortex, a held session, and an operator-named cell; EDITS the working copy"]
    fn editing_a_parameter_is_reflected_by_the_device() {
        let row: u32 = std::env::var("CORTEX_GUI_PARAM_ROW")
            .expect("set CORTEX_GUI_PARAM_ROW to the zero-based WIRE row of a block")
            .parse()
            .expect("row must be 0-3");
        let column: u32 = std::env::var("CORTEX_GUI_PARAM_COLUMN")
            .expect("set CORTEX_GUI_PARAM_COLUMN to the column of a block")
            .parse()
            .expect("column must be 0-7");

        let source = DaemonDashboardSource::default();
        let before = request_block_parameters(&source, row, column).unwrap();
        assert!(
            !before.is_empty(),
            "the named cell should hold a block with parameters"
        );

        // Pick a writable numeric parameter with a usable range.
        let target = before
            .iter()
            .find(|parameter| {
                !parameter.read_only
                    && parameter.normalised.is_some()
                    && parameter.max != parameter.min
                    && matches!(parameter.kind.as_str(), "float" | "int" | "fader")
            })
            .expect("the block should have at least one writable numeric parameter")
            .clone();

        let original = target.normalised.unwrap();
        // Move somewhere clearly different, staying inside 0..1.
        let moved = if original > 0.5 { 0.25 } else { 0.75 };

        request_set_parameter(
            &source,
            row,
            column,
            target.index,
            cortex_rs::client::ParameterInput::Normalised(
                #[allow(clippy::cast_possible_truncation)]
                {
                    moved as f32
                },
            ),
        )
        .unwrap();

        let after = request_block_parameters(&source, row, column).unwrap();
        let observed = after
            .iter()
            .find(|parameter| parameter.index == target.index)
            .expect("the parameter should still be there")
            .normalised
            .expect("it should still hold a number");
        assert!(
            (observed - moved).abs() < 0.02,
            "device reported {observed} after writing {moved} to {} (index {})",
            target.name,
            target.index
        );

        // Put it back.
        request_set_parameter(
            &source,
            row,
            column,
            target.index,
            cortex_rs::client::ParameterInput::Normalised(
                #[allow(clippy::cast_possible_truncation)]
                {
                    original as f32
                },
            ),
        )
        .unwrap();
        let restored = request_block_parameters(&source, row, column)
            .unwrap()
            .iter()
            .find(|parameter| parameter.index == target.index)
            .and_then(|parameter| parameter.normalised)
            .expect("the parameter should read back after restoring");
        assert!(
            (restored - original).abs() < 0.02,
            "failed to restore {} to {original}, it reads {restored}",
            target.name
        );
    }

    #[test]
    #[ignore = "requires a running held session and a real Quad Cortex"]
    fn daemon_dashboard_reads_one_live_generation() {
        let snapshot = load_dashboard(&DaemonDashboardSource::default()).unwrap();
        let live = snapshot
            .live
            .expect("held session should expose live state");
        assert_eq!(live.generation, snapshot.status.cache.generation);
        assert_eq!(live.revision, snapshot.status.cache.revision);
        assert!(!live.blocks.is_empty());
        assert!(!snapshot.directory.is_empty());
    }
}

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::Arc;

use cortex_host::{DaemonClient, DeviceHealth, Request, Status};
use cortex_rs::client::{USER_SETLIST, is_factory_setlist};
use cortex_rs::view::{CpuLoad, ParamValue, Preset, PresetSlot};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardSnapshot {
    pub source: &'static str,
    pub status: Status,
    pub live: Option<LiveSnapshot>,
    pub directory: Vec<SetlistSnapshot>,
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

trait DashboardSource: Send + Sync {
    fn dashboard(&self) -> Result<DashboardSnapshot, CommandError>;
    fn reconnect_now(&self) -> Result<(), CommandError>;
    fn switch_scene(&self, scene: u32) -> Result<(), CommandError>;
    fn recall_preset(&self, setlist: &str, slot: &str) -> Result<(), CommandError>;
}

struct DaemonDashboardSource {
    client: DaemonClient,
    directory_cache: std::sync::Mutex<Option<(u64, u64, Vec<SetlistSnapshot>)>>,
}

impl Default for DaemonDashboardSource {
    fn default() -> Self {
        Self {
            client: DaemonClient::default(),
            directory_cache: std::sync::Mutex::new(None),
        }
    }
}

impl DashboardSource for DaemonDashboardSource {
    fn dashboard(&self) -> Result<DashboardSnapshot, CommandError> {
        let before = self
            .client
            .require_compatible()
            .map_err(|error| CommandError::daemon(error.to_string()))?;
        if !status_is_live(&before) {
            return Ok(DashboardSnapshot {
                source: "daemon",
                status: before,
                live: None,
                directory: Vec::new(),
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
        })
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
}

/// The daemon's reply to a scene request: the scene it acted on.
#[derive(Debug, Clone, serde::Deserialize)]
struct SceneAck {
    scene: u32,
}

/// The daemon's reply to a recall: the slot it acted on.
#[derive(Debug, Clone, serde::Deserialize)]
struct RecallAck {
    slot: String,
}

impl DaemonDashboardSource {
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
            recall_preset
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
    }

    /// Records what reached the source, so a test can prove something did
    /// *not* reach the device.
    struct RecordingSource {
        switched: std::sync::Mutex<Vec<u32>>,
        recalled: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl RecordingSource {
        fn new() -> Self {
            Self {
                switched: std::sync::Mutex::new(Vec::new()),
                recalled: std::sync::Mutex::new(Vec::new()),
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

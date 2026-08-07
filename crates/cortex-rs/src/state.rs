// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A subscribed session's latest device-reported state.
//!
//! This is a reducer, not an optimistic client-side model. It changes only
//! when the device sends state, and a sparse update that cannot be merged
//! without guessing invalidates the affected snapshot. That is the boundary
//! a GUI needs: show what the unit reported, never what a host merely tried to
//! send.
//!
//! Device pushes are reduced synchronously on the RX thread before any waiter
//! consumes the same message. Readers take snapshots, and slow readers can
//! wait on a revision number rather than queueing every message in a knob
//! sweep.
//!
//! @see spec/140-session/spec.md
//! @see spec/roadmap.md PROT-008.6.5

#![allow(clippy::missing_panics_doc)]

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::proto::cortex_message_type::Enum as MessageType;
use crate::proto::message_action::Enum as MessageAction;
use crate::proto::{
    BinaryPreset, Bypass, Chain, ColBypass, ConnectionMessage, CpuLoadMessage, FileMessage,
    FolderInfo, GridMessage, Model, ModelRepoMessage, NewModelsMessage, Param, PresetDirtyMessage,
    RecallPresetMessage, SceneMessage, SetlistPositionMessage, VersionMessage,
};

/// Whether a cache is ready to answer without device I/O.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CachePhase {
    /// The session has not subscribed to device state.
    #[default]
    Unsubscribed,
    /// The initial subscription dump is still arriving.
    Seeding,
    /// A complete baseline arrived and later pushes have been reducible.
    Live,
    /// Some state is usable, but at least one required field is absent.
    Incomplete,
    /// The inbound stream broke, so no old value may be trusted.
    Invalidated,
}

/// One value read from the cache, with its freshness tokens.
#[derive(Debug, Clone)]
pub struct Cached<T> {
    /// Session generation that reported this value.
    pub generation: u64,
    /// Cache revision at which this value was accepted.
    pub revision: u64,
    /// Device-reported value.
    pub value: T,
}

/// The preset slot the unit reports as selected.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PresetLocation {
    /// Absolute device folder key.
    pub setlist: String,
    /// Zero-based linear slot position.
    pub position: u32,
    /// Whether the folder is the factory library, when reported.
    pub is_factory: Option<bool>,
}

/// Counts that explain whether the subscribed stream is doing useful work.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CacheCounters {
    /// State-bearing messages observed in this generation.
    pub seen: u64,
    /// Messages accepted into the cache.
    pub applied: u64,
    /// Valid messages that carried no cacheable value.
    pub ignored: u64,
    /// Messages that could not be reduced without guessing.
    pub rejected: u64,
    /// Times a malformed or abandoned frame sequence made continuity unknowable.
    pub stream_gaps: u64,
}

/// Cheap serialisable status for daemon and GUI diagnostics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct DeviceStateStatus {
    /// Current session generation.
    pub generation: u64,
    /// Monotonic revision across all generations.
    pub revision: u64,
    /// Stored-preset mutation epoch within this generation.
    pub storage_revision: u64,
    /// Overall cache readiness.
    pub phase: CachePhase,
    /// Whether a full live preset is available.
    pub current_preset: bool,
    /// Whether the active scene is available.
    pub active_scene: bool,
    /// Whether dirty state is available.
    pub preset_dirty: bool,
    /// Whether the selected slot is available.
    pub preset_location: bool,
    /// Whether the model catalog payload is available.
    pub catalog: bool,
    /// Folder keys for which a complete listing has arrived.
    pub listed_setlists: Vec<String>,
    /// State-stream counters.
    pub counters: CacheCounters,
    /// Why the most recent message was rejected, if any.
    pub last_rejection: Option<String>,
}

#[derive(Default)]
struct StateInner {
    generation: u64,
    revision: u64,
    storage_revision: u64,
    phase: CachePhase,
    stream_valid: bool,
    current_preset: Option<Cached<BinaryPreset>>,
    active_scene: Option<Cached<u32>>,
    preset_dirty: Option<Cached<bool>>,
    preset_location: Option<Cached<PresetLocation>>,
    folders: HashMap<String, Cached<FolderInfo>>,
    model_repo: Option<Cached<Vec<u8>>>,
    cpu_load: Option<Cached<CpuLoadMessage>>,
    device_version: Option<Cached<VersionMessage>>,
    counters: CacheCounters,
    last_rejection: Option<String>,
}

struct StateShared {
    inner: Mutex<StateInner>,
    changed: Condvar,
}

/// Cloneable handle to the subscribed device-state reducer.
#[derive(Clone)]
pub struct DeviceStateCache {
    shared: Arc<StateShared>,
}

impl Default for DeviceStateCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceStateCache {
    /// Create an empty, unsubscribed cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shared: Arc::new(StateShared {
                inner: Mutex::new(StateInner::default()),
                changed: Condvar::new(),
            }),
        }
    }

    /// Start a new physical-session generation and invalidate every old value.
    pub(crate) fn begin_generation(&self) -> u64 {
        let mut inner = self.shared.inner.lock().unwrap();
        let generation = inner.generation.saturating_add(1);
        let revision = inner.revision.saturating_add(1);
        *inner = StateInner {
            generation,
            revision,
            ..StateInner::default()
        };
        self.shared.changed.notify_all();
        generation
    }

    /// Mark the start of the subscribed state dump.
    pub(crate) fn begin_subscription(&self, generation: u64) {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.generation != generation {
            return;
        }
        inner.revision = inner.revision.saturating_add(1);
        inner.phase = CachePhase::Seeding;
        inner.stream_valid = true;
        inner.current_preset = None;
        inner.active_scene = None;
        inner.preset_dirty = None;
        inner.preset_location = None;
        inner.folders.clear();
        inner.last_rejection = None;
        self.shared.changed.notify_all();
    }

    /// Finish the initial dump after the session's quiet-period barrier.
    pub(crate) fn finish_subscription(&self, generation: u64) {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.generation != generation {
            return;
        }
        if inner.phase == CachePhase::Invalidated {
            return;
        }
        inner.revision = inner.revision.saturating_add(1);
        inner.phase = if inner.current_preset.is_some() && inner.stream_valid {
            CachePhase::Live
        } else {
            CachePhase::Incomplete
        };
        self.shared.changed.notify_all();
    }

    /// Invalidate all state because at least one complete inbound message may
    /// have been lost.
    pub(crate) fn stream_gap(&self, generation: u64, reason: &str) {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.generation != generation || inner.phase == CachePhase::Invalidated {
            return;
        }
        inner.revision = inner.revision.saturating_add(1);
        inner.phase = CachePhase::Invalidated;
        inner.stream_valid = false;
        inner.current_preset = None;
        inner.active_scene = None;
        inner.preset_dirty = None;
        inner.preset_location = None;
        inner.folders.clear();
        inner.model_repo = None;
        inner.cpu_load = None;
        inner.device_version = None;
        inner.counters.stream_gaps = inner.counters.stream_gaps.saturating_add(1);
        inner.last_rejection = Some(reason.to_string());
        self.shared.changed.notify_all();
    }

    /// Invalidate every value before a host begins reconnecting.
    ///
    /// This is deliberately immediate: edits made while disconnected are
    /// unknowable, so retaining the previous generation during backoff would
    /// make a fast cache confidently return stale state.
    pub fn invalidate(&self, reason: impl Into<String>) {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.phase == CachePhase::Invalidated {
            return;
        }
        inner.revision = inner.revision.saturating_add(1);
        invalidate_all(&mut inner, reason.into());
        self.shared.changed.notify_all();
    }

    /// Reduce one decoded inbound message. Unrelated traffic is ignored
    /// without advancing the cache revision.
    pub(crate) fn observe(&self, generation: u64, message_type: MessageType, body: &[u8]) {
        match message_type {
            MessageType::Version => {
                self.decode_and_record::<VersionMessage, _>(
                    generation,
                    body,
                    DecodeImpact::Version,
                    apply_version,
                );
            }
            MessageType::ModelRepo => {
                self.decode_and_record::<ModelRepoMessage, _>(
                    generation,
                    body,
                    DecodeImpact::ModelRepo,
                    apply_model_repo,
                );
            }
            MessageType::CpuLoad => {
                self.decode_and_record::<CpuLoadMessage, _>(
                    generation,
                    body,
                    DecodeImpact::CpuLoad,
                    apply_cpu_load,
                );
            }
            MessageType::RecallPreset => self.decode_and_record::<RecallPresetMessage, _>(
                generation,
                body,
                DecodeImpact::CurrentPreset,
                apply_recall_preset,
            ),
            MessageType::Scene => {
                self.decode_and_record::<SceneMessage, _>(
                    generation,
                    body,
                    DecodeImpact::ActiveScene,
                    apply_scene,
                );
            }
            MessageType::PresetDirty => self.decode_and_record::<PresetDirtyMessage, _>(
                generation,
                body,
                DecodeImpact::PresetDirty,
                apply_preset_dirty,
            ),
            MessageType::SetlistPosition => self.decode_and_record::<SetlistPositionMessage, _>(
                generation,
                body,
                DecodeImpact::PresetLocation,
                apply_setlist_position,
            ),
            MessageType::File => {
                self.decode_and_record::<FileMessage, _>(
                    generation,
                    body,
                    DecodeImpact::Folders,
                    apply_file,
                );
            }
            MessageType::Grid => {
                self.decode_and_record::<GridMessage, _>(
                    generation,
                    body,
                    DecodeImpact::CurrentPreset,
                    apply_grid,
                );
            }
            MessageType::NewModels => {
                self.decode_and_record::<NewModelsMessage, _>(
                    generation,
                    body,
                    DecodeImpact::ModelRepo,
                    apply_new_models,
                );
            }
            MessageType::Connection => {
                self.decode_and_record::<ConnectionMessage, _>(
                    generation,
                    body,
                    DecodeImpact::All,
                    apply_connection,
                );
            }
            // These change the live preset structurally. Their exact merge
            // shape is not implemented, so retaining the old baseline would
            // be a lie.
            MessageType::GridMove | MessageType::DefaultParameters => {
                self.record(generation, |inner, _| {
                    invalidate_current(inner, format!("unsupported {message_type:?} state push"));
                    Apply::Rejected
                });
            }
            _ => {}
        }
    }

    fn decode_and_record<T, F>(&self, generation: u64, body: &[u8], impact: DecodeImpact, apply: F)
    where
        T: prost::Message + Default,
        F: FnOnce(&mut StateInner, u64, T) -> Apply,
    {
        match prost::Message::decode(body) {
            Ok(message) => self.record(generation, |inner, revision| {
                apply(inner, revision, message)
            }),
            Err(error) => self.record(generation, |inner, _| {
                decode_failed(inner, impact, format!("decoding cached state: {error}"));
                Apply::Rejected
            }),
        }
    }

    fn record(&self, generation: u64, apply: impl FnOnce(&mut StateInner, u64) -> Apply) {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.generation != generation {
            return;
        }
        inner.counters.seen = inner.counters.seen.saturating_add(1);
        inner.revision = inner.revision.saturating_add(1);
        let revision = inner.revision;
        match apply(&mut inner, revision) {
            Apply::Applied => {
                inner.counters.applied = inner.counters.applied.saturating_add(1);
                inner.last_rejection = None;
            }
            Apply::Ignored => {
                inner.counters.ignored = inner.counters.ignored.saturating_add(1);
            }
            Apply::Rejected => {
                inner.counters.rejected = inner.counters.rejected.saturating_add(1);
            }
        }
        self.shared.changed.notify_all();
    }

    /// Current full live preset, if no later message invalidated it.
    #[must_use]
    pub fn current_preset(&self) -> Option<Cached<BinaryPreset>> {
        self.shared.inner.lock().unwrap().current_preset.clone()
    }

    /// Current active scene, zero-based.
    #[must_use]
    pub fn active_scene(&self) -> Option<Cached<u32>> {
        self.shared.inner.lock().unwrap().active_scene.clone()
    }

    /// Whether the current working preset differs from its stored slot.
    #[must_use]
    pub fn preset_dirty(&self) -> Option<Cached<bool>> {
        self.shared.inner.lock().unwrap().preset_dirty.clone()
    }

    /// Currently selected setlist and slot.
    #[must_use]
    pub fn preset_location(&self) -> Option<Cached<PresetLocation>> {
        self.shared.inner.lock().unwrap().preset_location.clone()
    }

    /// Complete listing for one folder key, when announced.
    #[must_use]
    pub fn folder(&self, key: &str) -> Option<Cached<FolderInfo>> {
        self.shared
            .inner
            .lock()
            .unwrap()
            .folders
            .get(key.trim_end_matches('/'))
            .cloned()
    }

    /// The model catalog payload captured in this generation.
    #[must_use]
    pub fn model_repo(&self) -> Option<Cached<Vec<u8>>> {
        self.shared.inner.lock().unwrap().model_repo.clone()
    }

    /// Latest pushed CPU load.
    #[must_use]
    pub fn cpu_load(&self) -> Option<Cached<CpuLoadMessage>> {
        self.shared.inner.lock().unwrap().cpu_load.clone()
    }

    /// Device identity reported in this generation.
    #[must_use]
    pub fn device_version(&self) -> Option<Cached<VersionMessage>> {
        self.shared.inner.lock().unwrap().device_version.clone()
    }

    /// Current status without cloning any large cached payload.
    #[must_use]
    pub fn status(&self) -> DeviceStateStatus {
        let inner = self.shared.inner.lock().unwrap();
        let mut listed_setlists: Vec<String> = inner.folders.keys().cloned().collect();
        listed_setlists.sort();
        DeviceStateStatus {
            generation: inner.generation,
            revision: inner.revision,
            storage_revision: inner.storage_revision,
            phase: inner.phase,
            current_preset: inner.current_preset.is_some(),
            active_scene: inner.active_scene.is_some(),
            preset_dirty: inner.preset_dirty.is_some(),
            preset_location: inner.preset_location.is_some(),
            catalog: inner.model_repo.is_some(),
            listed_setlists,
            counters: inner.counters.clone(),
            last_rejection: inner.last_rejection.clone(),
        }
    }

    /// Wait until the cache advances beyond `after`, returning the latest
    /// revision. Multiple knob pushes naturally coalesce into one wake-up.
    #[must_use]
    pub fn wait_for_change(&self, after: u64, timeout: Duration) -> Option<u64> {
        let deadline = Instant::now() + timeout;
        let mut inner = self.shared.inner.lock().unwrap();
        while inner.revision <= after {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self.shared.changed.wait_timeout(inner, remaining).unwrap();
            inner = next;
            if result.timed_out() && inner.revision <= after {
                return None;
            }
        }
        Some(inner.revision)
    }
}

#[derive(Clone, Copy)]
enum Apply {
    Applied,
    Ignored,
    Rejected,
}

#[derive(Clone, Copy)]
enum DecodeImpact {
    Version,
    ModelRepo,
    CpuLoad,
    CurrentPreset,
    ActiveScene,
    PresetDirty,
    PresetLocation,
    Folders,
    All,
}

fn decode_failed(inner: &mut StateInner, impact: DecodeImpact, reason: String) {
    match impact {
        DecodeImpact::Version => inner.device_version = None,
        DecodeImpact::ModelRepo => inner.model_repo = None,
        DecodeImpact::CpuLoad => inner.cpu_load = None,
        DecodeImpact::CurrentPreset => {
            invalidate_current(inner, reason);
            return;
        }
        DecodeImpact::ActiveScene => inner.active_scene = None,
        DecodeImpact::PresetDirty => inner.preset_dirty = None,
        DecodeImpact::PresetLocation => inner.preset_location = None,
        DecodeImpact::Folders => inner.folders.clear(),
        DecodeImpact::All => {
            invalidate_all(inner, reason);
            return;
        }
    }
    inner.last_rejection = Some(reason);
}

fn cached<T>(inner: &StateInner, revision: u64, value: T) -> Cached<T> {
    Cached {
        generation: inner.generation,
        revision,
        value,
    }
}

fn apply_version(inner: &mut StateInner, revision: u64, message: VersionMessage) -> Apply {
    // The device later sends an id-less Version READ of its own. Caching that
    // empty request would erase the identity captured by the handshake.
    if message.device_type.is_none()
        && message.device_serial_number.is_none()
        && message.zenos_git_hash.is_none()
        && message.app_fw_version.is_none()
    {
        return Apply::Ignored;
    }
    inner.device_version = Some(cached(inner, revision, message));
    Apply::Applied
}

fn apply_model_repo(inner: &mut StateInner, revision: u64, message: ModelRepoMessage) -> Apply {
    let Some(crate::proto::model_repo_message::ModelRepoPayload::ModelRepoPayload(payload)) =
        message.model_repo_payload
    else {
        return Apply::Ignored;
    };
    if payload.is_empty() {
        return Apply::Ignored;
    }
    inner.model_repo = Some(cached(inner, revision, payload));
    Apply::Applied
}

fn apply_cpu_load(inner: &mut StateInner, revision: u64, message: CpuLoadMessage) -> Apply {
    if message.cpu_total_load.is_none() && message.chains.is_empty() {
        return Apply::Ignored;
    }
    inner.cpu_load = Some(cached(inner, revision, message));
    Apply::Applied
}

fn apply_recall_preset(
    inner: &mut StateInner,
    revision: u64,
    message: RecallPresetMessage,
) -> Apply {
    if crate::session::trace_enabled() {
        eprintln!(
            "cortex-trace: RecallPreset state action={} reason={:?} chains={} rows={:?}",
            message.action,
            message.reason,
            message.preset.as_ref().map_or(0, |preset| {
                let crate::proto::recall_preset_message::Preset::Preset(preset) = preset;
                preset.chains.len()
            }),
            message.preset.as_ref().map(|preset| {
                let crate::proto::recall_preset_message::Preset::Preset(preset) = preset;
                preset
                    .chains
                    .iter()
                    .map(|chain| chain.row)
                    .collect::<Vec<_>>()
            })
        );
    }
    let saved = message.reason.as_ref().is_some_and(|reason| {
        let crate::proto::recall_preset_message::Reason::Reason(reason) = reason;
        *reason == crate::proto::recall_preset_reason::Enum::Save as i32
    });
    if saved {
        inner.storage_revision = inner.storage_revision.saturating_add(1);
    }
    let Some(crate::proto::recall_preset_message::Preset::Preset(preset)) = message.preset else {
        return if saved {
            Apply::Applied
        } else {
            Apply::Ignored
        };
    };
    // Full recalled presets are positional and carry all four rows. A keyed or
    // partial shape is a delta, not a safe baseline.
    if preset.chains.len() != 4 || preset.chains.iter().any(|chain| chain.row.is_some()) {
        invalidate_current(
            inner,
            "RecallPreset did not carry a complete four-row baseline".into(),
        );
        return Apply::Rejected;
    }
    inner.current_preset = Some(cached(inner, revision, preset));
    if inner.stream_valid && inner.phase != CachePhase::Seeding {
        inner.phase = CachePhase::Live;
    }
    Apply::Applied
}

fn apply_scene(inner: &mut StateInner, revision: u64, message: SceneMessage) -> Apply {
    let Some(crate::proto::scene_message::SelectedScene::SelectedScene(scene)) =
        message.selected_scene
    else {
        return Apply::Ignored;
    };
    if scene > 7 {
        inner.last_rejection = Some(format!("device reported scene {scene}; scenes are 0-7"));
        return Apply::Rejected;
    }
    inner.active_scene = Some(cached(inner, revision, scene));
    Apply::Applied
}

fn apply_preset_dirty(inner: &mut StateInner, revision: u64, message: PresetDirtyMessage) -> Apply {
    if crate::session::trace_enabled() {
        eprintln!(
            "cortex-trace: PresetDirty state action={} dirty={}",
            message.action, message.is_dirty
        );
    }
    if message.action != MessageAction::Update as i32 {
        return Apply::Ignored;
    }
    // `is_dirty` is a plain proto3 bool: false has no field bytes but is still
    // a complete, meaningful UPDATE.
    inner.preset_dirty = Some(cached(inner, revision, message.is_dirty));
    Apply::Applied
}

fn apply_setlist_position(
    inner: &mut StateInner,
    revision: u64,
    message: SetlistPositionMessage,
) -> Apply {
    if message.action != MessageAction::Update as i32 {
        return Apply::Ignored;
    }
    let Some(crate::proto::setlist_position_message::FolderKey::FolderKey(setlist)) =
        message.folder_key
    else {
        return Apply::Ignored;
    };
    let Some(crate::proto::setlist_position_message::Position::Position(position)) =
        message.position
    else {
        return Apply::Ignored;
    };
    let is_factory = message.is_factory.map(|value| {
        let crate::proto::setlist_position_message::IsFactory::IsFactory(value) = value;
        value
    });
    inner.preset_location = Some(cached(
        inner,
        revision,
        PresetLocation {
            setlist,
            position,
            is_factory,
        },
    ));
    Apply::Applied
}

fn folder_key(folder: &FolderInfo) -> Option<String> {
    folder.key.as_ref().and_then(|key| {
        let crate::proto::folder_info::Key::Key(key) = key;
        let key = key.trim_end_matches('/');
        (!key.is_empty()).then(|| key.to_string())
    })
}

fn is_complete_preset_listing(folder: &FolderInfo) -> bool {
    if folder.files.len() != crate::client::SETLIST_SLOTS as usize {
        return false;
    }
    let mut seen = [false; crate::client::SETLIST_SLOTS as usize];
    for file in &folder.files {
        let Some(crate::proto::product_data::Index::Index(index)) = file.index else {
            return false;
        };
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        let Some(slot) = seen.get_mut(index) else {
            return false;
        };
        if *slot {
            return false;
        }
        *slot = true;
    }
    seen.into_iter().all(|present| present)
}

fn apply_file(inner: &mut StateInner, revision: u64, message: FileMessage) -> Apply {
    let folder = message.folder.map(|folder| {
        let crate::proto::file_message::Folder::Folder(folder) = folder;
        folder
    });
    let to_folder = message.to_folder.map(|folder| {
        let crate::proto::file_message::ToFolder::ToFolder(folder) = folder;
        folder
    });

    if message.action == MessageAction::Update as i32 {
        let Some(folder) = folder else {
            return Apply::Ignored;
        };
        let Some(key) = folder_key(&folder) else {
            return Apply::Ignored;
        };
        if is_complete_preset_listing(&folder) {
            if inner
                .folders
                .get(&key)
                .is_some_and(|previous| previous.value != folder)
            {
                inner.storage_revision = inner.storage_revision.saturating_add(1);
            }
            inner.folders.insert(key, cached(inner, revision, folder));
        } else {
            inner.folders.remove(&key);
            inner.storage_revision = inner.storage_revision.saturating_add(1);
        }
        return Apply::Applied;
    }

    if matches!(
        MessageAction::try_from(message.action),
        Ok(MessageAction::Create
            | MessageAction::Delete
            | MessageAction::Move
            | MessageAction::Copy
            | MessageAction::Upload
            | MessageAction::Download)
    ) {
        inner.storage_revision = inner.storage_revision.saturating_add(1);
        let keys: Vec<String> = [folder.as_ref(), to_folder.as_ref()]
            .into_iter()
            .flatten()
            .filter_map(folder_key)
            .collect();
        if keys.is_empty() {
            inner.folders.clear();
        } else {
            for key in keys {
                inner.folders.remove(&key);
            }
        }
        return Apply::Applied;
    }

    Apply::Ignored
}

fn apply_new_models(inner: &mut StateInner, _revision: u64, message: NewModelsMessage) -> Apply {
    let NewModelsMessage { models, .. } = message;
    if models.is_empty() {
        return Apply::Ignored;
    }
    inner.model_repo = None;
    Apply::Applied
}

fn apply_connection(inner: &mut StateInner, _revision: u64, message: ConnectionMessage) -> Apply {
    let disconnected = message.connected.is_some_and(|connected| {
        let crate::proto::connection_message::Connected::Connected(connected) = connected;
        !connected
    });
    if !disconnected {
        return Apply::Ignored;
    }
    invalidate_all(inner, "device announced disconnection".into());
    Apply::Applied
}

fn apply_grid(inner: &mut StateInner, revision: u64, message: GridMessage) -> Apply {
    if message.update_type.is_some() {
        return Apply::Ignored;
    }
    let Some(crate::proto::grid_message::Preset::Preset(delta)) = message.preset else {
        return Apply::Ignored;
    };
    let Some(current) = inner.current_preset.as_ref() else {
        if crate::session::trace_enabled() {
            eprintln!("cortex-trace: rejecting Grid: no full preset baseline");
        }
        invalidate_current(
            inner,
            "Grid delta arrived before a full preset baseline".into(),
        );
        return Apply::Rejected;
    };
    let active_scene = inner.active_scene.as_ref().map(|scene| scene.value);
    match merge_grid(&current.value, &delta, message.action, active_scene) {
        Ok(preset) => {
            inner.current_preset = Some(cached(inner, revision, preset));
            Apply::Applied
        }
        Err(reason) => {
            if crate::session::trace_enabled() {
                eprintln!("cortex-trace: rejecting Grid: {reason}");
            }
            invalidate_current(inner, reason);
            Apply::Rejected
        }
    }
}

fn invalidate_current(inner: &mut StateInner, reason: String) {
    inner.current_preset = None;
    if inner.phase != CachePhase::Invalidated {
        inner.phase = CachePhase::Incomplete;
    }
    inner.last_rejection = Some(reason);
}

fn invalidate_all(inner: &mut StateInner, reason: String) {
    inner.phase = CachePhase::Invalidated;
    inner.stream_valid = false;
    inner.current_preset = None;
    inner.active_scene = None;
    inner.preset_dirty = None;
    inner.preset_location = None;
    inner.folders.clear();
    inner.model_repo = None;
    inner.cpu_load = None;
    inner.device_version = None;
    inner.last_rejection = Some(reason);
}

fn merge_grid(
    baseline: &BinaryPreset,
    delta: &BinaryPreset,
    action: i32,
    active_scene: Option<u32>,
) -> Result<BinaryPreset, String> {
    let mut extra = delta.clone();
    extra.chains.clear();
    extra.bypass.clear();
    if extra != BinaryPreset::default() {
        return Err("Grid delta changed unsupported preset-level state".into());
    }
    if action != MessageAction::Update as i32 && action != MessageAction::Delete as i32 {
        return Err(format!("unsupported Grid action {action}"));
    }

    let mut next = baseline.clone();
    for chain in &delta.chains {
        merge_chain(&mut next, chain, action, active_scene)?;
    }
    if action == MessageAction::Delete as i32 && !delta.bypass.is_empty() {
        return Err("DELETE Grid carried an unsupported bypass delta".into());
    }
    for bypass in &delta.bypass {
        merge_bypass(&mut next, bypass, active_scene)?;
    }
    Ok(next)
}

fn chain_row(chain: &Chain) -> Option<u32> {
    chain.row.as_ref().map(|row| {
        let crate::proto::chain::Row::Row(row) = row;
        *row
    })
}

fn model_column(model: &Model) -> Option<u32> {
    model.column.as_ref().map(|column| {
        let crate::proto::model::Column::Column(column) = column;
        *column
    })
}

fn model_hash(model: &Model) -> Option<u32> {
    model.hash.as_ref().map(|hash| {
        let crate::proto::model::Hash::Hash(hash) = hash;
        *hash
    })
}

fn param_index(param: &Param) -> Option<u32> {
    param.index.as_ref().map(|index| {
        let crate::proto::param::Index::Index(index) = index;
        *index
    })
}

fn merge_chain(
    preset: &mut BinaryPreset,
    delta: &Chain,
    action: i32,
    active_scene: Option<u32>,
) -> Result<(), String> {
    let row = chain_row(delta).ok_or_else(|| "Grid chain carried no row key".to_string())?;
    let row = usize::try_from(row).map_err(|_| "Grid row does not fit in memory".to_string())?;
    let target = preset
        .chains
        .get_mut(row)
        .ok_or_else(|| format!("Grid delta addressed missing wire row {row}"))?;

    let mut extra = delta.clone();
    extra.row = None;
    extra.in_portid = None;
    extra.out_portid = None;
    extra.models.clear();
    extra.split_control_points.clear();
    extra.input_control.clear();
    if extra != Chain::default() {
        if crate::session::trace_enabled() {
            eprintln!("cortex-trace: unsupported Grid chain state: {extra:?}");
        }
        return Err(format!(
            "Grid delta changed unsupported state on wire row {row}"
        ));
    }

    if action == MessageAction::Delete as i32
        && (delta.in_portid.is_some()
            || delta.out_portid.is_some()
            || !delta.split_control_points.is_empty())
    {
        return Err("DELETE Grid carried non-model state".into());
    }
    if let Some(input) = &delta.in_portid {
        target.in_portid = Some(*input);
    }
    if let Some(output) = &delta.out_portid {
        target.out_portid = Some(*output);
    }
    if !delta.split_control_points.is_empty() {
        if delta.split_control_points.len() != 1 {
            return Err("Grid split delta carried more than one control point".into());
        }
        target
            .split_control_points
            .clone_from(&delta.split_control_points);
    }
    if action == MessageAction::Delete as i32 && !delta.input_control.is_empty() {
        return Err("DELETE Grid carried input-control state".into());
    }
    for (position, control) in delta.input_control.iter().enumerate() {
        let target = target
            .input_control
            .get_mut(position)
            .ok_or_else(|| format!("Grid input control addressed missing position {position}"))?;
        let mut extra = control.clone();
        extra.sidechain_source_flag = None;
        if extra != Model::default() {
            return Err(format!(
                "Grid input control changed unsupported state at position {position}"
            ));
        }
        if let Some(flag) = &control.sidechain_source_flag {
            target.sidechain_source_flag = Some(*flag);
        }
    }

    for model in &delta.models {
        merge_model(target, model, action, active_scene)?;
    }
    Ok(())
}

fn merge_model(
    chain: &mut Chain,
    delta: &Model,
    action: i32,
    active_scene: Option<u32>,
) -> Result<(), String> {
    let column =
        model_column(delta).ok_or_else(|| "Grid model carried no column key".to_string())?;
    let column =
        usize::try_from(column).map_err(|_| "Grid column does not fit in memory".to_string())?;
    let target = chain
        .models
        .get_mut(column)
        .ok_or_else(|| format!("Grid delta addressed missing column {column}"))?;

    let mut extra = delta.clone();
    extra.column = None;
    extra.hash = None;
    extra.params.clear();
    if extra != Model::default() {
        return Err(format!(
            "Grid delta changed unsupported state at column {column}"
        ));
    }

    if action == MessageAction::Delete as i32 {
        if model_hash(delta) != Some(0) || !delta.params.is_empty() {
            return Err("DELETE Grid did not carry one empty model cell".into());
        }
        *target = Model::default();
        return Ok(());
    }
    if delta.hash.is_some() {
        // A sparse placement identifies the new model but does not carry the
        // defaults the device instantiated, so replacing the cached model
        // would fabricate an incomplete block.
        return Err("block placement needs a full live-grid refresh".into());
    }
    for param in &delta.params {
        merge_param(target, param, active_scene)?;
    }
    Ok(())
}

fn merge_param(model: &mut Model, delta: &Param, active_scene: Option<u32>) -> Result<(), String> {
    let index =
        param_index(delta).ok_or_else(|| "Grid parameter carried no index key".to_string())?;
    let index = usize::try_from(index)
        .map_err(|_| "Grid parameter index does not fit in memory".to_string())?;
    let target = model
        .params
        .get_mut(index)
        .ok_or_else(|| format!("Grid delta addressed missing parameter {index}"))?;

    let mut extra = delta.clone();
    extra.index = None;
    extra.scene_mode = None;
    extra.param_values.clear();
    if extra != Param::default() {
        return Err(format!(
            "Grid delta changed unsupported parameter state at {index}"
        ));
    }
    if let Some(crate::proto::param::SceneMode::SceneMode(enabled)) = delta.scene_mode {
        if !delta.param_values.is_empty() {
            return Err(format!(
                "parameter {index} carried scene mode and a value together"
            ));
        }
        if !enabled {
            // Collapsing eight scene values does not report which one the
            // device retained, so this direction needs a full refresh.
            return Err(format!(
                "parameter {index} stopped following scenes; refresh required"
            ));
        }
        if target.param_values.len() == 1 {
            let value = target.param_values[0].clone();
            target.param_values.resize(8, value);
        } else if target.param_values.len() != 8 {
            return Err(format!(
                "parameter {index} has {} values and cannot be promoted safely",
                target.param_values.len()
            ));
        }
        target.scene_mode = Some(crate::proto::param::SceneMode::SceneMode(true));
        return Ok(());
    }
    if delta.param_values.len() != 1 || delta.param_values[0].value.is_none() {
        return Err(format!("parameter {index} did not carry exactly one value"));
    }

    let value = delta.param_values[0].clone();
    let follows_scenes = target.scene_mode.as_ref().is_some_and(|mode| {
        let crate::proto::param::SceneMode::SceneMode(mode) = mode;
        *mode
    });
    if follows_scenes {
        let scene = active_scene.ok_or_else(|| {
            format!("parameter {index} follows scenes but the active scene is unknown")
        })?;
        let scene = usize::try_from(scene)
            .map_err(|_| "active scene does not fit in memory".to_string())?;
        let target_value = target
            .param_values
            .get_mut(scene)
            .ok_or_else(|| format!("parameter {index} has no stored value for scene {scene}"))?;
        *target_value = value;
    } else {
        if target.param_values.is_empty() {
            return Err(format!("parameter {index} has no stored value to replace"));
        }
        for target_value in &mut target.param_values {
            *target_value = value.clone();
        }
    }
    Ok(())
}

fn bypass_row(bypass: &Bypass, position: usize) -> usize {
    bypass.row.as_ref().map_or(position, |row| {
        let crate::proto::bypass::Row::Row(row) = row;
        usize::try_from(*row).unwrap_or(usize::MAX)
    })
}

fn bypass_column(bypass: &ColBypass, position: usize) -> usize {
    bypass.column.as_ref().map_or(position, |column| {
        let crate::proto::col_bypass::Column::Column(column) = column;
        usize::try_from(*column).unwrap_or(usize::MAX)
    })
}

fn merge_bypass(
    preset: &mut BinaryPreset,
    delta: &Bypass,
    active_scene: Option<u32>,
) -> Result<(), String> {
    let Some(crate::proto::bypass::Row::Row(row)) = delta.row.as_ref() else {
        return Err("Grid bypass carried no row key".into());
    };
    let row = usize::try_from(*row).map_err(|_| "bypass row does not fit in memory".to_string())?;

    let mut extra = delta.clone();
    extra.row = None;
    extra.col_bypass.clear();
    if extra != Bypass::default() {
        return Err(format!(
            "Grid bypass changed unsupported state on row {row}"
        ));
    }

    let position = preset
        .bypass
        .iter()
        .enumerate()
        .position(|(position, entry)| bypass_row(entry, position) == row)
        .ok_or_else(|| format!("Grid bypass addressed missing row {row}"))?;
    let target_row = &mut preset.bypass[position];
    for column in &delta.col_bypass {
        merge_column_bypass(target_row, column, active_scene)?;
    }
    Ok(())
}

fn merge_column_bypass(
    row: &mut Bypass,
    delta: &ColBypass,
    active_scene: Option<u32>,
) -> Result<(), String> {
    let Some(crate::proto::col_bypass::Column::Column(column)) = delta.column.as_ref() else {
        return Err("Grid column bypass carried no column key".into());
    };
    let column =
        usize::try_from(*column).map_err(|_| "bypass column does not fit in memory".to_string())?;

    let mut extra = delta.clone();
    extra.column = None;
    extra.scene_mode = None;
    extra.scene_bypass.clear();
    if extra != ColBypass::default() {
        return Err(format!(
            "Grid bypass changed unsupported state at column {column}"
        ));
    }
    if delta.scene_mode.is_some() {
        return Err(format!(
            "bypass scene mode changed at column {column}; refresh required"
        ));
    }
    if delta.scene_bypass.len() != 1 {
        return Err(format!(
            "column {column} did not carry exactly one bypass value"
        ));
    }

    let position = row
        .col_bypass
        .iter()
        .enumerate()
        .position(|(position, entry)| bypass_column(entry, position) == column)
        .ok_or_else(|| format!("Grid bypass addressed missing column {column}"))?;
    let target = &mut row.col_bypass[position];
    let bypassed = delta.scene_bypass[0].bypass;
    let follows_scenes = target.scene_mode.as_ref().is_some_and(|mode| {
        let crate::proto::col_bypass::SceneMode::SceneMode(mode) = mode;
        *mode
    });
    if follows_scenes {
        let scene = active_scene
            .ok_or_else(|| format!("bypass at column {column} follows an unknown scene"))?;
        let scene = usize::try_from(scene)
            .map_err(|_| "active scene does not fit in memory".to_string())?;
        let target_scene = target
            .scene_bypass
            .get_mut(scene)
            .ok_or_else(|| format!("bypass at column {column} has no value for scene {scene}"))?;
        target_scene.bypass = bypassed;
    } else {
        if target.scene_bypass.is_empty() {
            return Err(format!("bypass at column {column} has no stored value"));
        }
        for target_scene in &mut target.scene_bypass {
            target_scene.bypass = bypassed;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{
        ParamValue, ProductData, SceneBypass, chain, col_bypass, grid_message, model, param,
    };

    fn full_preset(parameter_scene_mode: bool, bypass_scene_mode: bool) -> BinaryPreset {
        let values = (0_u8..8)
            .map(|index| ParamValue {
                value: Some(crate::proto::param_value::Value::FloatValue(
                    f32::from(index) / 10.0,
                )),
            })
            .collect();
        let model = Model {
            hash: Some(model::Hash::Hash(42)),
            params: vec![Param {
                scene_mode: Some(param::SceneMode::SceneMode(parameter_scene_mode)),
                param_values: values,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut chains = vec![Chain::default(); 4];
        chains[0].models = vec![model];
        BinaryPreset {
            chains,
            bypass: vec![Bypass {
                col_bypass: vec![ColBypass {
                    scene_mode: Some(col_bypass::SceneMode::SceneMode(bypass_scene_mode)),
                    scene_bypass: vec![SceneBypass { bypass: false }; 8],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn observe<T: prost::Message>(
        cache: &DeviceStateCache,
        generation: u64,
        message_type: MessageType,
        message: &T,
    ) {
        cache.observe(generation, message_type, &message.encode_to_vec());
    }

    fn seeded_cache(
        parameter_scene_mode: bool,
        bypass_scene_mode: bool,
    ) -> (DeviceStateCache, u64) {
        let cache = DeviceStateCache::new();
        let generation = cache.begin_generation();
        cache.begin_subscription(generation);
        observe(
            &cache,
            generation,
            MessageType::RecallPreset,
            &RecallPresetMessage {
                action: MessageAction::Update as i32,
                preset: Some(crate::proto::recall_preset_message::Preset::Preset(
                    full_preset(parameter_scene_mode, bypass_scene_mode),
                )),
                ..Default::default()
            },
        );
        cache.finish_subscription(generation);
        (cache, generation)
    }

    #[test]
    fn a_full_recall_seeds_a_live_baseline() {
        let (cache, _) = seeded_cache(false, false);
        assert_eq!(cache.status().phase, CachePhase::Live);
        assert_eq!(cache.current_preset().unwrap().value.chains.len(), 4);
    }

    #[test]
    fn finishing_subscription_cannot_hide_a_stream_gap() {
        let cache = DeviceStateCache::new();
        let generation = cache.begin_generation();
        cache.begin_subscription(generation);
        cache.stream_gap(generation, "fictional lost report");
        cache.finish_subscription(generation);
        assert_eq!(cache.status().phase, CachePhase::Invalidated);
    }

    #[test]
    fn a_global_parameter_push_replaces_every_stored_scene_value() {
        let (cache, generation) = seeded_cache(false, false);
        let update = crate::grid::set_param(
            crate::Row::from_wire(0),
            0,
            0,
            crate::Value::Normalised(0.75),
        );
        observe(&cache, generation, MessageType::Grid, &update);
        let preset = cache.current_preset().unwrap().value;
        let values = &preset.chains[0].models[0].params[0].param_values;
        assert!(values.iter().all(|value| {
            value.value == Some(crate::proto::param_value::Value::FloatValue(0.75))
        }));
    }

    #[test]
    fn a_recall_input_control_flag_keeps_the_full_baseline_live() {
        let mut preset = full_preset(false, false);
        preset.chains[0].input_control.push(Model {
            sidechain_source_flag: Some(model::SidechainSourceFlag::SidechainSourceFlag(true)),
            ..Default::default()
        });
        let delta = BinaryPreset {
            chains: vec![Chain {
                row: Some(chain::Row::Row(0)),
                input_control: vec![Model {
                    sidechain_source_flag: Some(model::SidechainSourceFlag::SidechainSourceFlag(
                        false,
                    )),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let merged = merge_grid(&preset, &delta, MessageAction::Update as i32, Some(0)).unwrap();
        assert!(matches!(
            merged.chains[0].input_control[0].sidechain_source_flag,
            Some(model::SidechainSourceFlag::SidechainSourceFlag(false))
        ));
    }

    #[test]
    fn a_scene_parameter_push_changes_only_the_active_scene() {
        let (cache, generation) = seeded_cache(true, false);
        observe(
            &cache,
            generation,
            MessageType::Scene,
            &SceneMessage {
                action: MessageAction::Update as i32,
                selected_scene: Some(crate::proto::scene_message::SelectedScene::SelectedScene(3)),
                ..Default::default()
            },
        );
        let update = crate::grid::set_param(
            crate::Row::from_wire(0),
            0,
            0,
            crate::Value::Normalised(0.9),
        );
        observe(&cache, generation, MessageType::Grid, &update);
        let values = cache.current_preset().unwrap().value.chains[0].models[0].params[0]
            .param_values
            .clone();
        assert_eq!(
            values[3].value,
            Some(crate::proto::param_value::Value::FloatValue(0.9))
        );
        assert_eq!(
            values[2].value,
            Some(crate::proto::param_value::Value::FloatValue(0.2))
        );
    }

    #[test]
    fn promoting_a_parameter_preserves_a_complete_per_scene_baseline() {
        let (cache, generation) = seeded_cache(false, false);
        let update = crate::grid::set_param_scene_mode(crate::Row::from_wire(0), 0, 0, true);
        observe(&cache, generation, MessageType::Grid, &update);
        let parameter = &cache.current_preset().unwrap().value.chains[0].models[0].params[0];
        assert!(parameter.scene_mode.as_ref().is_some_and(|mode| {
            let crate::proto::param::SceneMode::SceneMode(enabled) = mode;
            *enabled
        }));
        assert_eq!(parameter.param_values.len(), 8);
    }

    #[test]
    fn an_unknown_active_scene_invalidates_a_scene_parameter_delta() {
        let (cache, generation) = seeded_cache(true, false);
        let update = crate::grid::set_param(
            crate::Row::from_wire(0),
            0,
            0,
            crate::Value::Normalised(0.9),
        );
        observe(&cache, generation, MessageType::Grid, &update);
        assert!(cache.current_preset().is_none());
        assert_eq!(cache.status().phase, CachePhase::Incomplete);
    }

    #[test]
    fn a_scene_bypass_push_changes_only_the_active_scene() {
        let (cache, generation) = seeded_cache(false, true);
        observe(
            &cache,
            generation,
            MessageType::Scene,
            &SceneMessage {
                action: MessageAction::Update as i32,
                selected_scene: Some(crate::proto::scene_message::SelectedScene::SelectedScene(5)),
                ..Default::default()
            },
        );
        let update = crate::grid::set_bypass(crate::Row::from_wire(0), 0, true);
        observe(&cache, generation, MessageType::Grid, &update);
        let bypass = &cache.current_preset().unwrap().value.bypass[0].col_bypass[0].scene_bypass;
        assert!(bypass[5].bypass);
        assert!(!bypass[4].bypass);
    }

    #[test]
    fn an_ambiguous_block_placement_invalidates_instead_of_inventing_defaults() {
        let (cache, generation) = seeded_cache(false, false);
        let update = crate::grid::set_block(crate::Row::from_wire(0), 0, 99);
        observe(&cache, generation, MessageType::Grid, &update);
        assert!(cache.current_preset().is_none());
        assert_eq!(cache.status().counters.rejected, 1);
    }

    #[test]
    fn dirty_false_is_a_real_value_despite_proto3_omission() {
        let (cache, generation) = seeded_cache(false, false);
        observe(
            &cache,
            generation,
            MessageType::PresetDirty,
            &PresetDirtyMessage {
                action: MessageAction::Update as i32,
                is_dirty: false,
                request_id: None,
            },
        );
        assert!(!cache.preset_dirty().unwrap().value);
    }

    #[test]
    fn a_file_mutation_invalidates_instead_of_caching_its_one_item_ack() {
        let (cache, generation) = seeded_cache(false, false);
        let folder = FolderInfo {
            key: Some(crate::proto::folder_info::Key::Key(
                "/fictional/list".into(),
            )),
            files: (0..i32::try_from(crate::client::SETLIST_SLOTS).unwrap())
                .map(|index| ProductData {
                    index: Some(crate::proto::product_data::Index::Index(index)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        observe(
            &cache,
            generation,
            MessageType::File,
            &FileMessage {
                action: MessageAction::Update as i32,
                folder: Some(crate::proto::file_message::Folder::Folder(folder.clone())),
                ..Default::default()
            },
        );
        assert!(cache.folder("/fictional/list").is_some());
        observe(
            &cache,
            generation,
            MessageType::File,
            &FileMessage {
                action: MessageAction::Create as i32,
                folder: Some(crate::proto::file_message::Folder::Folder(folder)),
                ..Default::default()
            },
        );
        assert!(cache.folder("/fictional/list").is_none());
        assert_eq!(cache.status().storage_revision, 1);
    }

    #[test]
    fn a_materially_changed_complete_listing_advances_storage_revision() {
        let (cache, generation) = seeded_cache(false, false);
        let mut folder = FolderInfo {
            key: Some(crate::proto::folder_info::Key::Key(
                "/fictional/list".into(),
            )),
            files: (0..i32::try_from(crate::client::SETLIST_SLOTS).unwrap())
                .map(|index| ProductData {
                    index: Some(crate::proto::product_data::Index::Index(index)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let update = |folder| FileMessage {
            action: MessageAction::Update as i32,
            folder: Some(crate::proto::file_message::Folder::Folder(folder)),
            ..Default::default()
        };
        observe(
            &cache,
            generation,
            MessageType::File,
            &update(folder.clone()),
        );
        assert_eq!(cache.status().storage_revision, 0);

        folder.files[1].name = Some(crate::proto::product_data::Name::Name(
            "Fictional Moved".into(),
        ));
        observe(
            &cache,
            generation,
            MessageType::File,
            &update(folder.clone()),
        );
        assert_eq!(cache.status().storage_revision, 1);

        observe(&cache, generation, MessageType::File, &update(folder));
        assert_eq!(
            cache.status().storage_revision,
            1,
            "an identical refresh is not another storage mutation"
        );
    }

    #[test]
    fn a_partial_file_update_invalidates_the_cached_listing() {
        let (cache, generation) = seeded_cache(false, false);
        let folder = FolderInfo {
            key: Some(crate::proto::folder_info::Key::Key(
                "/fictional/list".into(),
            )),
            files: vec![ProductData {
                index: Some(crate::proto::product_data::Index::Index(0)),
                ..Default::default()
            }],
            ..Default::default()
        };
        observe(
            &cache,
            generation,
            MessageType::File,
            &FileMessage {
                action: MessageAction::Update as i32,
                folder: Some(crate::proto::file_message::Folder::Folder(folder)),
                ..Default::default()
            },
        );
        assert!(cache.folder("/fictional/list").is_none());
        assert_eq!(cache.status().storage_revision, 1);
    }

    #[test]
    fn a_save_push_advances_the_storage_revision() {
        let (cache, generation) = seeded_cache(false, false);
        observe(
            &cache,
            generation,
            MessageType::RecallPreset,
            &RecallPresetMessage {
                action: MessageAction::Update as i32,
                reason: Some(crate::proto::recall_preset_message::Reason::Reason(
                    crate::proto::recall_preset_reason::Enum::Save as i32,
                )),
                ..Default::default()
            },
        );
        assert_eq!(cache.status().storage_revision, 1);
    }

    #[test]
    fn an_old_session_generation_cannot_repopulate_new_state() {
        let (cache, old_generation) = seeded_cache(false, false);
        let new_generation = cache.begin_generation();
        assert_ne!(old_generation, new_generation);
        observe(
            &cache,
            old_generation,
            MessageType::RecallPreset,
            &RecallPresetMessage {
                action: MessageAction::Update as i32,
                preset: Some(crate::proto::recall_preset_message::Preset::Preset(
                    full_preset(false, false),
                )),
                ..Default::default()
            },
        );
        assert!(cache.current_preset().is_none());
        assert_eq!(cache.status().generation, new_generation);
    }

    #[test]
    fn the_devices_later_empty_version_read_does_not_erase_its_identity() {
        let (cache, generation) = seeded_cache(false, false);
        observe(
            &cache,
            generation,
            MessageType::Version,
            &VersionMessage {
                device_serial_number: Some(
                    crate::proto::version_message::DeviceSerialNumber::DeviceSerialNumber(
                        "QA00AB123".into(),
                    ),
                ),
                ..Default::default()
            },
        );
        observe(
            &cache,
            generation,
            MessageType::Version,
            &VersionMessage {
                action: MessageAction::Read as i32,
                ..Default::default()
            },
        );
        let version = cache.device_version().unwrap().value;
        assert!(version.device_serial_number.is_some());
    }

    #[test]
    fn an_undecodable_grid_delta_invalidates_the_baseline() {
        let (cache, generation) = seeded_cache(false, false);
        cache.observe(generation, MessageType::Grid, &[0x0F]);
        assert!(cache.current_preset().is_none());
        assert_eq!(cache.status().counters.rejected, 1);
    }

    #[test]
    fn explicit_invalidation_clears_every_generation_value() {
        let (cache, generation) = seeded_cache(false, false);
        observe(
            &cache,
            generation,
            MessageType::Scene,
            &SceneMessage {
                action: MessageAction::Update as i32,
                selected_scene: Some(crate::proto::scene_message::SelectedScene::SelectedScene(2)),
                ..Default::default()
            },
        );
        cache.invalidate("fictional disconnect");
        let status = cache.status();
        assert_eq!(status.phase, CachePhase::Invalidated);
        assert!(!status.current_preset);
        assert!(!status.active_scene);
    }

    #[test]
    fn a_knob_burst_coalesces_to_the_latest_value_without_a_message_queue() {
        let (cache, generation) = seeded_cache(false, false);
        for step in 0_u8..135 {
            let update = crate::grid::set_param(
                crate::Row::from_wire(0),
                0,
                0,
                crate::Value::Normalised(f32::from(step) / 134.0),
            );
            observe(&cache, generation, MessageType::Grid, &update);
        }
        let value = &cache.current_preset().unwrap().value.chains[0].models[0].params[0]
            .param_values[0]
            .value;
        assert_eq!(
            *value,
            Some(crate::proto::param_value::Value::FloatValue(1.0))
        );
        assert_eq!(cache.status().counters.applied, 136);
    }

    #[test]
    fn waiters_coalesce_changes_by_revision() {
        let (cache, generation) = seeded_cache(false, false);
        let before = cache.status().revision;
        observe(
            &cache,
            generation,
            MessageType::Scene,
            &SceneMessage {
                action: MessageAction::Update as i32,
                selected_scene: Some(crate::proto::scene_message::SelectedScene::SelectedScene(1)),
                ..Default::default()
            },
        );
        assert!(cache.wait_for_change(before, Duration::ZERO).is_some());
        assert!(
            cache
                .wait_for_change(cache.status().revision, Duration::from_millis(1))
                .is_none()
        );
    }

    #[test]
    fn a_delete_removes_only_the_addressed_model() {
        let (cache, generation) = seeded_cache(false, false);
        let mut chain = Chain {
            row: Some(chain::Row::Row(0)),
            ..Default::default()
        };
        chain.models.push(Model {
            column: Some(model::Column::Column(0)),
            hash: Some(model::Hash::Hash(0)),
            ..Default::default()
        });
        let message = GridMessage {
            action: MessageAction::Delete as i32,
            preset: Some(grid_message::Preset::Preset(BinaryPreset {
                chains: vec![chain],
                ..Default::default()
            })),
            ..Default::default()
        };
        observe(&cache, generation, MessageType::Grid, &message);
        assert!(
            cache.current_preset().unwrap().value.chains[0].models[0]
                .hash
                .is_none()
        );
    }
}

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
use std::time::Duration;

use crate::DeviceKind;
use crate::grid::{Row, Value};
use crate::proto::cortex_message_type::Enum as MessageType;
use crate::proto::message_action::Enum as MessageAction;
use crate::proto::{
    BinaryPreset, FileMessage, RecallPresetMessage, SceneMessage, SetlistPositionMessage,
    VersionMessage,
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
    pub instrument: Option<i32>,
}

impl PresetEntry {
    /// Build from a `ProductData` listing entry, or `None` if it has no name
    /// (which is how the device reports an EMPTY slot - every setlist always
    /// reports its full complement of 256 slots).
    fn from_proto(pd: &crate::proto::ProductData) -> Option<Self> {
        use crate::proto::product_data;
        let name = match pd.name.as_ref()? {
            product_data::Name::Name(n) if !n.is_empty() => n.clone(),
            product_data::Name::Name(_) => return None,
        };
        let index = match pd.index.as_ref() {
            Some(product_data::Index::Index(i)) => u32::try_from(*i).ok()?,
            None => return None,
        };
        let key = pd.key.as_ref().map(|k| {
            let product_data::Key::Key(v) = k;
            v.clone()
        });
        let instrument = pd.instrument.as_ref().map(|i| {
            let product_data::Instrument::Instrument(v) = i;
            *v
        });
        Some(Self {
            index,
            name,
            key,
            instrument,
        })
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
            .filter(|pd| PresetEntry::from_proto(pd).is_some())
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

/// How a block placement was confirmed.
///
/// Worth distinguishing, because the two carry different confidence: an echo
/// is the device telling us it accepted the cell, while a read-back is us
/// observing the grid afterwards. Both mean the block is there; only the
/// second survives a slow device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Placement {
    /// The device echoed a `Grid` broadcast naming the cell.
    EchoConfirmed,
    /// No echo arrived in time, but a read-back found the block in place.
    ReadBackConfirmed,
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

    /// Open a transport, start the session, run the connect handshake, and
    /// return a ready-to-use `QuadCortex`. This is the Rust equivalent of
    /// `pyquadcortex.connect()`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::DeviceNotFound`] if no matching device is on
    /// the bus, or [`crate::Error::ReadTimeout`] if the handshake reply does
    /// not arrive within `timeout`.
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
    /// This sends `SetlistPosition{UPDATE}`. It does NOT wait for a reply
    /// (writes are stalled); the recall triggers a `RecallPreset` push that
    /// `read_preset` captures.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidSlot`] if `position` is not a valid slot
    /// name. Otherwise returns `Ok` even on a write error, since the USB
    /// status-stage stall makes every write appear to fail.
    pub fn recall_preset(
        &self,
        setlist_path: &str,
        position: &str,
        is_factory: bool,
    ) -> crate::Result<()> {
        let payload = build_recall(setlist_path, position, is_factory, None)?;
        self.session.send(MessageType::SetlistPosition, &payload)
    }

    /// Switch the active scene. Scenes are 0-based.
    ///
    /// # Errors
    ///
    /// Currently always returns `Ok`.
    pub fn switch_scene(&self, scene: u32) -> crate::Result<()> {
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
            || {
                let _ = self.session.send(MessageType::RecallPreset, &payload);
            },
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
    /// The device services the push lazily (10-25 s observed), hence the
    /// generous timeout callers should pass.
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
            || {
                let _ = self.session.send(MessageType::SetlistPosition, &recall);
            },
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
    /// `setlist` is any folder KEY the device reports, not only the two
    /// setlists: plugin artist folders and the Captures Library work too.
    ///
    /// Note the trailing-slash asymmetry this absorbs: recalls need the
    /// factory path WITH its trailing slash, but the device reports that same
    /// folder's listing key WITHOUT one. Keys are compared with trailing
    /// slashes normalised away.
    ///
    /// A listing that arrives is COMPLETE, but a READ does not reliably
    /// produce one promptly - delivery is lazy. Treat a timeout as "ask
    /// again", not as an answer about the setlist's contents.
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
            || {
                let _ = self.session.send(MessageType::File, &payload);
            },
            timeout,
            move |m| {
                prost::Message::decode(m.body.as_ref())
                    .ok()
                    .and_then(|f: FileMessage| f.folder)
                    .is_some_and(|crate::proto::file_message::Folder::Folder(folder)| {
                        folder_key(&folder).is_some_and(|k| k == match_key)
                            && !folder.files.is_empty()
                    })
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
        let mut entries: Vec<PresetEntry> = folder
            .files
            .iter()
            .filter_map(PresetEntry::from_proto)
            .collect();
        entries.sort_by_key(|e| e.index);
        if include_empty {
            // Re-expand to the full slot map, filling gaps with blank entries,
            // so a caller looking for a free slot can see one.
            let occupied: std::collections::HashMap<u32, PresetEntry> =
                entries.into_iter().map(|e| (e.index, e)).collect();
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
            || {
                let _ = self.session.send(MessageType::File, &payload);
            },
            window,
            |_| true,
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
                // The device re-announces folders; keep the first sighting.
                if !folders.iter().any(|existing| existing.key == folder.key) {
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
    /// device, so it covers whatever this unit actually has - purchased
    /// plugin models and the player's own Neural Captures included - which no
    /// hard-coded table could know.
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

        // The handshake already asked for this, so the payload has usually
        // arrived by now. Asking again makes the device rebuild and resend
        // 46 KB, which it does at roughly 82 reports per second.
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
            || {
                let _ = self.session.send(MessageType::ModelRepo, &payload);
            },
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
    /// Measured from a capture of Cortex Control on `CorOS` 4.0.1; see
    /// `docs/protocol.md`.
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
        timeout: Duration,
    ) -> crate::Result<()> {
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
        let entry = crate::proto::ProductData {
            index: Some(crate::proto::product_data::Index::Index(
                i32::try_from(index).unwrap_or(i32::MAX),
            )),
            name: name.map(|n| crate::proto::product_data::Name::Name(n.to_string())),
            // Carried because Cortex Control carries it. Its meaning is not
            // established, so it is copied rather than reasoned about.
            instrument: Some(crate::proto::product_data::Instrument::Instrument(1)),
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

        // Wait for the acknowledging File reply rather than firing and
        // hoping. This is the one operation where "did it land" matters.
        self.session.await_broadcast(
            MessageType::File,
            || {
                let _ = self.session.send(MessageType::File, &payload);
            },
            timeout,
            |m| !m.body.is_empty(),
        )?;
        Ok(())
    }

    /// Tell the device this client is going away. Sends
    /// `Connection{connected: false}` (best effort).
    pub fn disconnect(&self) {
        self.session.disconnect();
    }

    /// Send `Connection{connected: false}` and stop the session.
    pub fn close(&mut self) {
        self.disconnect();
        self.session.stop();
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
// until a save, which this crate does not yet implement.
// ---------------------------------------------------------------------------

impl QuadCortex {
    /// Re-point one grid row's input.
    ///
    /// # Errors
    ///
    /// Returns `Ok` even on a write error, since the USB status-stage stall
    /// makes every write appear to fail.
    pub fn set_chain_input(&self, row: Row, in_portid: u32) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_chain_input(row, in_portid))
    }

    /// Re-point one grid row's output.
    ///
    /// The device does NOT validate this field: an id that means nothing is
    /// stored rather than rejected, so a typo reads back cleanly.
    ///
    /// # Errors
    ///
    /// As [`QuadCortex::set_chain_input`].
    pub fn set_chain_output(&self, row: Row, out_portid: u32) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_chain_output(row, out_portid))
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

    /// Make a block parameter follow scenes, or stop it following them.
    ///
    /// The flag travels alone; see [`crate::grid::set_param_scene_mode`].
    ///
    /// # Errors
    ///
    /// As [`QuadCortex::set_chain_input`].
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
        if promote {
            self.set_param_scene_mode(row, column, param_index, true)?;
        }
        self.switch_scene(scene)?;
        self.set_param(row, column, param_index, value)
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
        self.send_grid(&crate::grid::set_bypass(row, column, bypassed))
    }

    /// Remove the block at a cell.
    ///
    /// # Errors
    ///
    /// As [`QuadCortex::set_chain_input`].
    pub fn remove_block(&self, row: Row, column: u32) -> crate::Result<()> {
        self.send_grid(&crate::grid::remove_block(row, column))
    }

    /// Set a row's split and mix points, activating a parallel branch.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidRow`] for an odd row, which has no
    /// splitter.
    pub fn set_split(&self, row: Row, split: i32, mix: i32) -> crate::Result<()> {
        self.send_grid(&crate::grid::set_split(row, split, mix)?)
    }

    /// Place a model in a grid cell, verifying the device accepted it.
    ///
    /// **A placement can be refused for want of DSP capacity.** The preset has
    /// a processing budget; a block that does not fit is accepted on the wire
    /// like any other write and is simply absent afterwards. Nothing in the
    /// reply says so - every host write is stalled and there is no per-block
    /// error message.
    ///
    /// So this verifies, which is possible without saving: the device echoes
    /// a `Grid` broadcast naming the cell it accepted, and a refused block
    /// produces no echo at all. When none arrives within `timeout` this
    /// returns [`crate::Error::BlockRefused`].
    ///
    /// Use [`QuadCortex::set_block_unverified`] to send and return
    /// immediately, in which case a save and read-back is the only way to
    /// learn whether the block is there.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::BlockRefused`] if no echo naming the cell
    /// arrives within `timeout`.
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
            || {
                let _ = self.session.send(MessageType::Grid, &payload);
            },
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
                    // tell. Say so rather than guessing either way.
                    Err(e) => Err(crate::Error::BlockRefused(format!(
                        "could not determine whether model {model_id} was placed at \
                         wire row {wire_row} (screen row {}) column {column}: no echo \
                         within {timeout:?}, and the confirming read-back also failed \
                         ({e}). Check with `cortex grid`",
                        row.screen()
                    ))),
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
    let Some(chain) = preset.chains.get(row as usize) else {
        return false;
    };
    let Some(model) = chain.models.get(column as usize) else {
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
    use crate::proto::{GridMessage, chain, grid_message, model};

    let Ok(decoded) = prost::Message::decode(message.body.as_ref()) as Result<GridMessage, _>
    else {
        return false;
    };
    let Some(grid_message::Preset::Preset(preset)) = decoded.preset else {
        return false;
    };

    for (chain_index, ch) in preset.chains.iter().enumerate() {
        // A grid has four rows and eight columns, so an index never
        // approaches u32; try_from documents that without an allow.
        let fallback = u32::try_from(chain_index).unwrap_or(u32::MAX);
        let echoed_row = ch.row.as_ref().map_or(fallback, |r| {
            let chain::Row::Row(v) = r;
            *v
        });
        if echoed_row != row {
            continue;
        }
        for (model_index, m) in ch.models.iter().enumerate() {
            let fallback = u32::try_from(model_index).unwrap_or(u32::MAX);
            let echoed_column = m.column.as_ref().map_or(fallback, |c| {
                let model::Column::Column(v) = c;
                *v
            });
            if echoed_column != column {
                continue;
            }
            if let Some(model::Hash::Hash(hash)) = m.hash {
                if hash == model_id {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let entry = PresetEntry::from_proto(&product(218, "Plexi Sunrise")).unwrap();
        assert_eq!(entry.index, 218);
        assert_eq!(entry.name, "Plexi Sunrise");
        assert_eq!(position_to_slot(entry.index), "28C");
    }

    #[test]
    fn preset_entry_rejects_empty_slots() {
        // The device always reports a setlist's full 256 slots; an empty one
        // is signalled by an absent or blank name, NOT by omitting the entry.
        assert!(PresetEntry::from_proto(&product(5, "")).is_none());
        let nameless = ProductData {
            index: Some(product_data::Index::Index(5)),
            ..Default::default()
        };
        assert!(PresetEntry::from_proto(&nameless).is_none());
    }

    #[test]
    fn preset_entry_rejects_entry_without_index() {
        let no_index = ProductData {
            name: Some(product_data::Name::Name("Orphan".into())),
            ..Default::default()
        };
        assert!(PresetEntry::from_proto(&no_index).is_none());
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
    use std::sync::{Arc, Mutex};

    fn client() -> QuadCortex {
        let device: Arc<Mutex<dyn crate::link::HidLink>> = Arc::new(Mutex::new(FakeLink::new()));
        QuadCortex::new(Arc::new(
            crate::Session::over(device).expect("session over a fake link"),
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
            .save_current_preset(USER_SETLIST, "99Z", None, Duration::from_millis(50))
            .expect_err("99Z is not a slot");
        assert!(
            matches!(err, crate::Error::InvalidSlot(_)),
            "expected InvalidSlot, got {err:?}"
        );
        qc.session.stop();
    }
}

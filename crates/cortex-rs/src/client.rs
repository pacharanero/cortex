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
/// Total slots per setlist (256).
pub const SETLIST_SLOTS: u32 = BANKS * SLOTS_PER_BANK;

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
        let request = FileMessage {
            action: MessageAction::Read as i32,
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
        let entry = PresetEntry::from_proto(&product(218, "BelAir (e609)")).unwrap();
        assert_eq!(entry.index, 218);
        assert_eq!(entry.name, "BelAir (e609)");
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
}

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared save safety for CLI, MCP, and GUI hosts.
//!
//! Saving commits the unit's current working grid and overwrites a stored
//! slot. The subtle ordering constraint is that reading an occupied target to
//! back it up also RECALLS it, replacing that working grid. Therefore an
//! occupied target must be prepared before editing starts; a target first
//! selected after edits exist must be empty.
//!
//! This module supplies the policy and preparation token. It deliberately
//! chooses no default scratch range: only the user knows which of their 256
//! USER slots are disposable.
//!
//! @see spec/300-mcp/spec.md [FR-1] [FR-4]
//! @see spec/400-gui/spec.md

use std::time::Duration;

use crate::client::{
    PresetEntry, QuadCortex, USER_SETLIST_ROOT, is_factory_setlist, slot_to_position_checked,
};
use crate::proto::BinaryPreset;
use crate::state::CachePhase;

fn is_user_setlist(setlist: &str) -> bool {
    let normalized = setlist.trim_end_matches('/');
    let Some(name) = normalized.strip_prefix(&format!("{USER_SETLIST_ROOT}/")) else {
        return false;
    };
    // Reject `.` and `..`, which would escape the USER root if the device
    // normalises filesystem paths. A direct child has a real name and no
    // slash; a dot component is neither.
    !name.is_empty() && !name.contains('/') && name != "." && name != ".."
}

/// One inclusive range of save-safe USER slots.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ScratchRange {
    /// First displayed slot, inclusive.
    pub start: String,
    /// Last displayed slot, inclusive.
    pub end: String,
    start_position: u32,
    end_position: u32,
}

impl ScratchRange {
    /// Reconstruct from serialised `start`/`end` fields, validating as
    /// [`ScratchRange::new`] does. Used by `serde::Deserialize`.
    fn try_from_fields(start: &str, end: &str) -> crate::Result<Self> {
        let start_position = slot_to_position_checked(start)
            .ok_or_else(|| crate::Error::InvalidSlot(start.to_string()))?;
        let end_position = slot_to_position_checked(end)
            .ok_or_else(|| crate::Error::InvalidSlot(end.to_string()))?;
        if start_position > end_position {
            return Err(crate::Error::InvalidSlot(format!(
                "scratch range {start}-{end} is reversed"
            )));
        }
        Ok(Self {
            start: start.to_ascii_uppercase(),
            end: end.to_ascii_uppercase(),
            start_position,
            end_position,
        })
    }
}

impl<'de> serde::Deserialize<'de> for ScratchRange {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Fields {
            start: String,
            end: String,
        }
        let f = Fields::deserialize(deserializer)?;
        ScratchRange::try_from_fields(&f.start, &f.end).map_err(serde::de::Error::custom)
    }
}

impl ScratchRange {
    /// Construct an inclusive range from displayed slot names.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidSlot`] for malformed or reversed ranges.
    pub fn new(start: &str, end: &str) -> crate::Result<Self> {
        let start_position = slot_to_position_checked(start)
            .ok_or_else(|| crate::Error::InvalidSlot(start.to_string()))?;
        let end_position = slot_to_position_checked(end)
            .ok_or_else(|| crate::Error::InvalidSlot(end.to_string()))?;
        if start_position > end_position {
            return Err(crate::Error::InvalidSlot(format!(
                "scratch range {start}-{end} is reversed"
            )));
        }
        Ok(Self {
            start: start.to_ascii_uppercase(),
            end: end.to_ascii_uppercase(),
            start_position,
            end_position,
        })
    }

    fn contains(&self, position: u32) -> bool {
        (self.start_position..=self.end_position).contains(&position)
    }
}

/// Host-configured boundary for ordinary saves.
#[derive(Debug, Clone)]
pub struct SavePolicy {
    scratch_setlist: String,
    scratch_ranges: Vec<ScratchRange>,
}

impl SavePolicy {
    /// Define the user's scratch space. No default is supplied by the crate.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsafeSave`] for anything other than a direct
    /// USER setlist or for an empty range list.
    pub fn new(
        scratch_setlist: impl Into<String>,
        scratch_ranges: Vec<ScratchRange>,
    ) -> crate::Result<Self> {
        let scratch_setlist = scratch_setlist.into();
        if is_factory_setlist(&scratch_setlist) {
            return Err(crate::Error::UnsafeSave(
                "the factory library can never be configured as scratch space".into(),
            ));
        }
        if !is_user_setlist(&scratch_setlist) {
            return Err(crate::Error::UnsafeSave(format!(
                "{scratch_setlist} is not a USER setlist under {USER_SETLIST_ROOT}"
            )));
        }
        if scratch_ranges.is_empty() {
            return Err(crate::Error::UnsafeSave(
                "configure at least one USER scratch-slot range".into(),
            ));
        }
        Ok(Self {
            scratch_setlist,
            scratch_ranges,
        })
    }

    /// The setlist configured as scratch space.
    #[must_use]
    pub fn scratch_setlist(&self) -> &str {
        &self.scratch_setlist
    }

    /// Configured inclusive ranges.
    #[must_use]
    pub fn scratch_ranges(&self) -> &[ScratchRange] {
        &self.scratch_ranges
    }

    fn authorize(
        &self,
        target: &SaveTarget,
        override_scratch: ScratchOverride,
    ) -> crate::Result<()> {
        if is_factory_setlist(&target.setlist) {
            return Err(crate::Error::UnsafeSave(format!(
                "{} is the factory library and can never be a save target; choose a USER slot",
                target.setlist
            )));
        }
        if !is_user_setlist(&target.setlist) {
            return Err(crate::Error::UnsafeSave(format!(
                "{} is not a USER setlist under {USER_SETLIST_ROOT}",
                target.setlist
            )));
        }
        let in_scratch = target.setlist.trim_end_matches('/')
            == self.scratch_setlist.trim_end_matches('/')
            && self
                .scratch_ranges
                .iter()
                .any(|range| range.contains(target.position));
        if !in_scratch && override_scratch != ScratchOverride::AllowOutsideScratch {
            return Err(crate::Error::UnsafeSave(format!(
                "{} in {} is outside the configured scratch range; choose a scratch slot or explicitly allow an outside-scratch save",
                target.slot, target.setlist
            )));
        }
        Ok(())
    }
}

/// Explicit opt-in for a USER slot outside configured scratch space.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScratchOverride {
    /// Refuse anything outside the configured range.
    #[default]
    ScratchOnly,
    /// Deliberately allow another USER slot. Never permits factory writes.
    AllowOutsideScratch,
}

/// Whether preparation may discard an existing working copy by recalling an
/// occupied target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallConsent {
    /// Recall only when the live cache positively reports a clean grid.
    RequireClean,
    /// Explicitly accept that any current unsaved edits will be discarded.
    DiscardWorkingCopy,
}

/// Active confirmation required by a destructive save.
#[derive(Debug)]
pub struct SaveConfirmation(());

impl SaveConfirmation {
    /// Convert a host's explicit confirmation input into an unforgeable token.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsafeSave`] unless `confirmed` is true.
    pub fn explicit(confirmed: bool) -> crate::Result<Self> {
        if !confirmed {
            return Err(crate::Error::UnsafeSave(
                "saving requires explicit confirmation".into(),
            ));
        }
        Ok(Self(()))
    }
}

/// Validated save destination.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SaveTarget {
    /// Absolute device setlist key.
    pub setlist: String,
    /// Displayed slot name, e.g. `31A`.
    pub slot: String,
    /// Zero-based linear position.
    pub position: u32,
}

impl SaveTarget {
    /// Validate one destination without applying scratch policy yet.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidSlot`] for a malformed slot.
    pub fn new(setlist: impl Into<String>, slot: &str) -> crate::Result<Self> {
        let position = slot_to_position_checked(slot)
            .ok_or_else(|| crate::Error::InvalidSlot(slot.to_string()))?;
        Ok(Self {
            setlist: setlist.into(),
            slot: slot.to_ascii_uppercase(),
            position,
        })
    }
}

/// Serialisable summary returned to a host while the Rust side retains the
/// actual backup token.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SavePreparationView {
    /// Prepared destination.
    pub target: SaveTarget,
    /// Whether the listing reported the destination empty. This is the
    /// listing's view, not a proof: file listings are eventually consistent,
    /// so a stale listing can say empty when the slot is occupied. The backup
    /// is always attempted regardless.
    pub target_was_empty: bool,
    /// Previous stored name, when the listing reported the slot occupied.
    pub previous_name: Option<String>,
    /// Whether a full stored preset is retained in memory. Always true after
    /// preparation, because the backup read is always attempted.
    pub backup_retained: bool,
}

/// Prepared destination consumed by [`QuadCortex::save_prepared`].
///
/// Keep this on the Rust side of a Tauri/MCP host; expose
/// [`SavePreparation::view`] to the webview or agent.
pub struct SavePreparation {
    target: SaveTarget,
    expected_entry: PresetEntry,
    generation: u64,
    storage_revision: u64,
    previous_name: Option<String>,
    backup: Option<BinaryPreset>,
    override_scratch: ScratchOverride,
}

impl SavePreparation {
    /// Safe serialisable description without the raw preset blob.
    #[must_use]
    pub fn view(&self) -> SavePreparationView {
        SavePreparationView {
            target: self.target.clone(),
            target_was_empty: self.previous_name.is_none(),
            previous_name: self.previous_name.clone(),
            backup_retained: self.backup.is_some(),
        }
    }

    /// The retained pre-edit preset, when the target was occupied.
    #[must_use]
    pub fn backup(&self) -> Option<&BinaryPreset> {
        self.backup.as_ref()
    }

    /// Encode the retained preset for host-side persistence.
    #[must_use]
    pub fn backup_bytes(&self) -> Option<Vec<u8>> {
        self.backup.as_ref().map(prost::Message::encode_to_vec)
    }

    fn validate_current(
        &self,
        status: &crate::DeviceStateStatus,
        current_entry: &PresetEntry,
    ) -> crate::Result<()> {
        if status.generation != self.generation
            || status.storage_revision != self.storage_revision
            || status.phase == CachePhase::Invalidated
        {
            return Err(crate::Error::UnsafeSave(
                "the prepared target may have changed; prepare it again before saving".into(),
            ));
        }
        if current_entry != &self.expected_entry {
            return Err(crate::Error::UnsafeSave(
                "the prepared target changed before confirmation; prepare it again before saving"
                    .into(),
            ));
        }
        Ok(())
    }
}

/// Result of a gated save, retaining the preparation and its backup.
pub struct SaveReceipt {
    preparation: SavePreparation,
}

impl SaveReceipt {
    /// Prepared-target summary.
    #[must_use]
    pub fn view(&self) -> SavePreparationView {
        self.preparation.view()
    }

    /// Retained pre-edit preset, if the overwritten target was occupied.
    #[must_use]
    pub fn backup(&self) -> Option<&BinaryPreset> {
        self.preparation.backup()
    }

    /// Encoded backup suitable for writing to host-controlled storage.
    #[must_use]
    pub fn backup_bytes(&self) -> Option<Vec<u8>> {
        self.preparation.backup_bytes()
    }
}

impl QuadCortex {
    /// Prepare one save destination before making working-copy edits.
    ///
    /// The target is always recalled and read, regardless of what the listing
    /// says. This is because file listings are eventually consistent
    /// (`docs/protocol.md`): a slot filled moments earlier can still appear
    /// empty in a fresh listing. Using "the listing says empty" as the reason
    /// to skip the backup would overwrite a real preset with no backup in
    /// exactly the case the backup exists for. So the backup decision comes
    /// from the read, never from the listing.
    ///
    /// The listing still determines `previous_name` (for the host's UI), but
    /// the backup is attempted unconditionally. If the slot is genuinely
    /// empty, the read returns an empty preset and no backup is retained.
    ///
    /// Because recalling the target replaces the current working grid, this
    /// method is named and ordered "before editing". If the subscribed cache
    /// reports the current grid dirty, or cannot establish that it is clean,
    /// [`RecallConsent::DiscardWorkingCopy`] is required - regardless of
    /// whether the target appears empty. This prevents a host from presenting
    /// backup as safety while silently losing the work being saved.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsafeSave`] for a disallowed target or a
    /// recall without the required consent. Propagates listing and recall
    /// errors.
    pub fn prepare_save_before_editing(
        &self,
        policy: &SavePolicy,
        setlist: &str,
        slot: &str,
        override_scratch: ScratchOverride,
        recall_consent: RecallConsent,
        timeout: Duration,
    ) -> crate::Result<SavePreparation> {
        let target = SaveTarget::new(setlist, slot)?;
        policy.authorize(&target, override_scratch)?;
        let entries = self.list_presets(setlist, timeout, true)?;
        let entry = entries
            .into_iter()
            .find(|entry| entry.index == target.position)
            .ok_or_else(|| {
                crate::Error::UnsafeSave(format!(
                    "{} did not appear in the complete listing for {setlist}",
                    target.slot
                ))
            })?;

        // The consent gate applies to every target, not only occupied ones.
        // A listing that says "empty" cannot be trusted to prove it: file
        // listings are eventually consistent, so a slot filled moments ago
        // can still read empty. If we skipped the consent check for
        // apparently-empty targets, a stale listing would let us recall and
        // destroy a dirty working grid without the host having said that was
        // acceptable.
        if recall_consent == RecallConsent::RequireClean {
            let state = self.state_cache();
            let status = state.status();
            let clean = status.phase == crate::CachePhase::Live
                && state.preset_dirty().is_some_and(|dirty| !dirty.value);
            if !clean {
                return Err(crate::Error::UnsafeSave(
                    "backing up the target recalls it, which replaces the working grid. \
                     The live grid is dirty or its dirty state is unknown; \
                     choose a target before editing, or explicitly allow discarding the \
                     working copy"
                        .into(),
                ));
            }
        }

        // Always attempt the backup read. If the slot is genuinely empty the
        // device pushes an empty four-row preset; if the listing was stale and
        // the slot is occupied, we get the real preset as the backup we would
        // otherwise have skipped.
        let backup = self.read_preset(setlist, slot, false, timeout)?;
        let previous_name = if entry.name.is_empty() {
            None
        } else {
            Some(entry.name.clone())
        };
        let status = self.state_cache().status();
        Ok(SavePreparation {
            target,
            previous_name,
            expected_entry: entry,
            generation: status.generation,
            storage_revision: status.storage_revision,
            backup: Some(backup),
            override_scratch,
        })
    }

    /// Commit the current working grid to a previously prepared target.
    ///
    /// The preparation is consumed so it cannot accidentally authorize a
    /// second overwrite after the target has changed. The returned receipt
    /// retains any pre-edit backup for host-side persistence. Before writing,
    /// this re-lists the target and checks the preparation's physical-session
    /// generation and stored-preset mutation epoch. A reconnect, intervening
    /// save/delete/move, or changed listing entry makes it stale.
    ///
    /// # Errors
    ///
    /// Revalidates policy, refuses stale preparations with
    /// [`crate::Error::UnsafeSave`], and propagates listing and
    /// [`Self::save_current_preset`] errors.
    pub fn save_prepared(
        &self,
        policy: &SavePolicy,
        preparation: SavePreparation,
        _confirmation: SaveConfirmation,
        name: Option<&str>,
        timeout: Duration,
    ) -> crate::Result<SaveReceipt> {
        policy.authorize(&preparation.target, preparation.override_scratch)?;
        let current_entry = self
            .list_presets(&preparation.target.setlist, timeout, true)?
            .into_iter()
            .find(|entry| entry.index == preparation.target.position)
            .ok_or_else(|| {
                crate::Error::UnsafeSave(format!(
                    "{} did not appear in the current setlist listing",
                    preparation.target.slot
                ))
            })?;
        let status = self.state_cache().status();
        preparation.validate_current(&status, &current_entry)?;
        self.save_current_preset(
            &preparation.target.setlist,
            &preparation.target.slot,
            name,
            timeout,
        )?;
        Ok(SaveReceipt { preparation })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::{Frame, encode_message};
    use crate::link::FakeLink;
    use crate::message::Message;
    use crate::proto::{
        FileMessage, FolderInfo, ProductData, RecallPresetMessage, SetlistPositionMessage,
        cortex_message_type, file_message, folder_info, product_data, recall_preset_message,
        setlist_position_message,
    };
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    const USER: &str = "/media/p4/Presets/My Presets";

    fn policy() -> SavePolicy {
        SavePolicy::new(USER, vec![ScratchRange::new("31A", "32H").unwrap()]).unwrap()
    }

    fn client() -> (QuadCortex, Arc<crate::Session>, FakeLink) {
        let link = FakeLink::new();
        let device: Arc<Mutex<dyn crate::link::HidLink>> = Arc::new(Mutex::new(link.clone()));
        let session = Arc::new(crate::Session::over(device).unwrap());
        (QuadCortex::new(session.clone()), session, link)
    }

    fn wait_for_write(link: &FakeLink, index: usize) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if let Some(report) = link.written().get(index) {
                return report.clone();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("timed out waiting for fake-device write {index}");
    }

    fn push_message<T: prost::Message>(
        link: &FakeLink,
        message_type: cortex_message_type::Enum,
        message: &T,
    ) {
        for mut report in encode_message(message_type as u16, &message.encode_to_vec()) {
            report[0] = crate::ReportId::Input as u8;
            link.push_inbound(report);
        }
    }

    fn listing(name: Option<&str>) -> FileMessage {
        FileMessage {
            action: crate::proto::message_action::Enum::Update as i32,
            folder: Some(file_message::Folder::Folder(FolderInfo {
                key: Some(folder_info::Key::Key(USER.into())),
                files: vec![ProductData {
                    index: Some(product_data::Index::Index(0)),
                    name: name.map(|name| product_data::Name::Name(name.into())),
                    ..Default::default()
                }],
                ..Default::default()
            })),
            ..Default::default()
        }
    }

    fn request_id(report: &[u8]) -> u64 {
        let frame = Frame::parse(report).unwrap();
        let message = Message::parse(&frame.data).unwrap();
        assert_eq!(
            message.message_type,
            cortex_message_type::Enum::SetlistPosition as u16
        );
        let request: SetlistPositionMessage = prost::Message::decode(message.body).unwrap();
        let Some(setlist_position_message::RequestId::RequestId(request_id)) = request.request_id
        else {
            panic!("recall request carried no request id");
        };
        request_id
    }

    #[test]
    fn scratch_ranges_use_the_units_real_slot_bounds() {
        let range = ScratchRange::new("31A", "32H").unwrap();
        assert!(range.contains(slot_to_position_checked("31A").unwrap()));
        assert!(range.contains(slot_to_position_checked("32H").unwrap()));
        assert!(ScratchRange::new("32H", "31A").is_err());
        assert!(ScratchRange::new("33A", "33H").is_err());
    }

    #[test]
    fn there_is_no_implicit_or_empty_scratch_policy() {
        assert!(SavePolicy::new(USER, Vec::new()).is_err());
        assert!(
            SavePolicy::new(
                "/opt/neuraldsp/Factory Library",
                vec![ScratchRange::new("1A", "1H").unwrap()]
            )
            .is_err()
        );
    }

    #[test]
    fn outside_scratch_requires_an_explicit_override() {
        let target = SaveTarget::new(USER, "1A").unwrap();
        assert!(
            policy()
                .authorize(&target, ScratchOverride::ScratchOnly)
                .is_err()
        );
        assert!(
            policy()
                .authorize(&target, ScratchOverride::AllowOutsideScratch)
                .is_ok()
        );
    }

    #[test]
    fn factory_refusal_has_no_override() {
        let target = SaveTarget::new("/opt/neuraldsp/Factory Library", "1A").unwrap();
        assert!(
            policy()
                .authorize(&target, ScratchOverride::AllowOutsideScratch)
                .is_err()
        );
    }

    #[test]
    fn an_override_cannot_escape_the_user_setlist_root() {
        for setlist in [
            "/media/p4/Captures",
            "/media/p4/Presets/My Presets/Nested Folder",
            "/media/p4/Presets/.",
            "/media/p4/Presets/..",
            "/tmp/fictional-setlist",
        ] {
            let target = SaveTarget::new(setlist, "1A").unwrap();
            assert!(
                policy()
                    .authorize(&target, ScratchOverride::AllowOutsideScratch)
                    .is_err(),
                "unexpectedly accepted {setlist}"
            );
            assert!(
                SavePolicy::new(setlist, vec![ScratchRange::new("1A", "1A").unwrap()]).is_err()
            );
        }
    }

    #[test]
    fn confirmation_is_active_opt_in() {
        assert!(SaveConfirmation::explicit(false).is_err());
        assert!(SaveConfirmation::explicit(true).is_ok());
    }

    #[test]
    fn a_preparation_view_never_serialises_the_raw_backup() {
        let preparation = SavePreparation {
            target: SaveTarget::new(USER, "31A").unwrap(),
            expected_entry: PresetEntry {
                index: slot_to_position_checked("31A").unwrap(),
                name: "Fictional Original".into(),
                key: None,
                instrument: None,
            },
            generation: 1,
            storage_revision: 0,
            previous_name: Some("Fictional Original".into()),
            backup: Some(BinaryPreset::default()),
            override_scratch: ScratchOverride::ScratchOnly,
        };
        let json = serde_json::to_string(&preparation.view()).unwrap();
        assert!(json.contains("backup_retained"));
        assert!(!json.contains("chains"));
        assert!(preparation.backup_bytes().is_some());
    }

    #[test]
    fn stale_preparations_fail_closed() {
        let entry = PresetEntry {
            index: slot_to_position_checked("31A").unwrap(),
            name: String::new(),
            key: None,
            instrument: None,
        };
        let preparation = SavePreparation {
            target: SaveTarget::new(USER, "31A").unwrap(),
            expected_entry: entry.clone(),
            generation: 4,
            storage_revision: 7,
            previous_name: None,
            backup: None,
            override_scratch: ScratchOverride::ScratchOnly,
        };
        let mut status = crate::DeviceStateCache::new().status();
        status.generation = 4;
        status.storage_revision = 7;
        assert!(preparation.validate_current(&status, &entry).is_ok());

        status.storage_revision = 8;
        assert!(preparation.validate_current(&status, &entry).is_err());
        status.storage_revision = 7;
        status.generation = 5;
        assert!(preparation.validate_current(&status, &entry).is_err());

        status.generation = 4;
        let occupied = PresetEntry {
            name: "Fictional Replacement".into(),
            ..entry
        };
        assert!(preparation.validate_current(&status, &occupied).is_err());
    }

    #[test]
    fn an_empty_target_is_recalled_to_prove_emptiness_and_saved() {
        // PROT-009.4: a listing that says "empty" is not trusted to skip the
        // backup, because file listings are eventually consistent. The target
        // is always recalled. If the slot is genuinely empty, the device
        // pushes an empty four-row preset and no backup is retained.
        let (qc, session, link) = client();
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            // 1. prepare: list
            wait_for_write(&fake, 0);
            push_message(&fake, cortex_message_type::Enum::File, &listing(None));
            // 2. prepare: recall (always attempted now)
            let recall = wait_for_write(&fake, 1);
            let request_id = request_id(&recall);
            push_message(
                &fake,
                cortex_message_type::Enum::RecallPreset,
                &RecallPresetMessage {
                    request_id: Some(recall_preset_message::RequestId::RequestId(request_id)),
                    preset: Some(recall_preset_message::Preset::Preset(
                        BinaryPreset::default(),
                    )),
                    ..Default::default()
                },
            );
            // 3. commit: re-list
            wait_for_write(&fake, 2);
            push_message(&fake, cortex_message_type::Enum::File, &listing(None));
            // 4. commit: save
            wait_for_write(&fake, 3);
            push_message(
                &fake,
                cortex_message_type::Enum::File,
                &FileMessage {
                    folder: Some(file_message::Folder::Folder(FolderInfo {
                        key: Some(folder_info::Key::Key(USER.into())),
                        files: vec![ProductData {
                            index: Some(product_data::Index::Index(0)),
                            name: Some(product_data::Name::Name("Fictional Saved".into())),
                            ..Default::default()
                        }],
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            );
        });
        let policy = SavePolicy::new(USER, vec![ScratchRange::new("1A", "1A").unwrap()]).unwrap();
        let preparation = qc
            .prepare_save_before_editing(
                &policy,
                USER,
                "1A",
                ScratchOverride::ScratchOnly,
                RecallConsent::DiscardWorkingCopy,
                Duration::from_secs(1),
            )
            .unwrap();
        // The listing said empty, so previous_name is None. The backup is
        // retained regardless (an empty preset if the slot was genuinely
        // empty), because the read - not the listing - decides whether to
        // back up.
        assert!(preparation.view().target_was_empty);
        assert!(preparation.backup().is_some());
        let receipt = qc
            .save_prepared(
                &policy,
                preparation,
                SaveConfirmation::explicit(true).unwrap(),
                Some("Fictional Saved"),
                Duration::from_secs(1),
            )
            .unwrap();
        assert!(receipt.backup().is_some());
        responder.join().unwrap();
        assert_eq!(link.write_count(), 4, "list, recall, re-list, save");
        session.stop();
    }

    #[test]
    fn an_occupied_target_is_recalled_and_retained_only_with_discard_consent() {
        let (qc, session, link) = client();
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            wait_for_write(&fake, 0);
            push_message(
                &fake,
                cortex_message_type::Enum::File,
                &listing(Some("Fictional Original")),
            );
            let recall = wait_for_write(&fake, 1);
            let request_id = request_id(&recall);
            push_message(
                &fake,
                cortex_message_type::Enum::RecallPreset,
                &RecallPresetMessage {
                    request_id: Some(recall_preset_message::RequestId::RequestId(request_id)),
                    preset: Some(recall_preset_message::Preset::Preset(
                        BinaryPreset::default(),
                    )),
                    ..Default::default()
                },
            );
        });
        let policy = SavePolicy::new(USER, vec![ScratchRange::new("1A", "1A").unwrap()]).unwrap();
        let preparation = qc
            .prepare_save_before_editing(
                &policy,
                USER,
                "1A",
                ScratchOverride::ScratchOnly,
                RecallConsent::DiscardWorkingCopy,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(
            preparation.view().previous_name.as_deref(),
            Some("Fictional Original")
        );
        assert!(preparation.backup().is_some());
        responder.join().unwrap();
        session.stop();
    }

    #[test]
    fn an_occupied_target_is_not_recalled_when_cleanliness_is_unknown() {
        let (qc, session, link) = client();
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            wait_for_write(&fake, 0);
            push_message(
                &fake,
                cortex_message_type::Enum::File,
                &listing(Some("Fictional Original")),
            );
        });
        let policy = SavePolicy::new(USER, vec![ScratchRange::new("1A", "1A").unwrap()]).unwrap();
        let Err(error) = qc.prepare_save_before_editing(
            &policy,
            USER,
            "1A",
            ScratchOverride::ScratchOnly,
            RecallConsent::RequireClean,
            Duration::from_secs(1),
        ) else {
            panic!("unknown working-copy state must not be discarded");
        };
        assert!(matches!(error, crate::Error::UnsafeSave(_)));
        responder.join().unwrap();
        assert_eq!(link.write_count(), 1, "the occupied target was only listed");
        session.stop();
    }

    #[test]
    fn a_stale_listing_saying_empty_still_backs_up_the_real_preset() {
        // PROT-009.4: the listing says the slot is empty, but the recall
        // reveals it is occupied. The backup is retained because the read,
        // not the listing, decides whether to back up.
        let (qc, session, link) = client();
        let fake = link.clone();
        let responder = std::thread::spawn(move || {
            // Listing says empty (stale).
            wait_for_write(&fake, 0);
            push_message(&fake, cortex_message_type::Enum::File, &listing(None));
            // But the recall reveals a real preset.
            let recall = wait_for_write(&fake, 1);
            let request_id = request_id(&recall);
            push_message(
                &fake,
                cortex_message_type::Enum::RecallPreset,
                &RecallPresetMessage {
                    request_id: Some(recall_preset_message::RequestId::RequestId(request_id)),
                    preset: Some(recall_preset_message::Preset::Preset(BinaryPreset {
                        chains: vec![crate::proto::Chain::default(); 4],
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            );
        });
        let policy = SavePolicy::new(USER, vec![ScratchRange::new("1A", "1A").unwrap()]).unwrap();
        let preparation = qc
            .prepare_save_before_editing(
                &policy,
                USER,
                "1A",
                ScratchOverride::ScratchOnly,
                RecallConsent::DiscardWorkingCopy,
                Duration::from_secs(1),
            )
            .unwrap();
        // The listing said empty, but the read proved occupied: the backup
        // is retained even though the listing said otherwise.
        assert!(
            preparation.backup().is_some(),
            "a stale listing must not cause the backup to be skipped"
        );
        responder.join().unwrap();
        session.stop();
    }
}

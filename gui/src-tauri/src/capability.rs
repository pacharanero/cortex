// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Honest verified-vs-provisional labelling for GUI operations, following the
//! `deskop-nano-cortex` capability-matrix discipline (see
//! `spec/prior-art.md#the-idea-most-worth-stealing`).
//!
//! The default matters more than the entries: a GUI operation that is not in
//! [`default_matrix`] reports [`CapabilityStatus::Unverified`] rather than
//! silently reading as confirmed. An operation is promoted only on the
//! strength of a recorded hardware pass in `spec/roadmap.md`, never because it
//! is implemented or has passed only offline/fixture verification - several
//! commands in this crate (`set_bypass`, `set_scene_label`, `set_scene_color`)
//! are exactly that: implemented, offline-verified, and still `unverified`
//! here because no hardware pass has confirmed them yet.
//!
//! @see spec/400-gui/spec.md
//! @see spec/400-gui/design.md [DES-CAPABILITY]

use std::collections::BTreeMap;

/// How confident this project is that a GUI operation behaves as documented,
/// against a real device rather than a fixture or reasoning about the wire.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityStatus {
    /// A read path confirmed against real hardware.
    ConfirmedReadable,
    /// A write path confirmed against real hardware, including read-back.
    ConfirmedWritable,
    /// Not itself hardware-tested, but implied by a confirmed sibling
    /// operation (e.g. the same request type, a different argument).
    Inferred,
    /// Known not to work, or deliberately unavailable.
    Unsupported,
    /// No hardware evidence either way. The default: an operation absent from
    /// the matrix, or one only offline/fixture-verified, is unverified.
    #[default]
    Unverified,
}

/// A bounded lookup from GUI operation name to its [`CapabilityStatus`].
///
/// Operation names match the Tauri command names in `lib.rs`
/// (`recall_preset`, `switch_scene`, ...), so a caller can key off the same
/// identifier used to invoke the command.
#[derive(Debug, Clone, Default)]
pub struct CapabilityMatrix(BTreeMap<&'static str, CapabilityStatus>);

impl CapabilityMatrix {
    /// Status for `operation`. An operation absent from the matrix is
    /// [`CapabilityStatus::Unverified`], never inferred as confirmed by its
    /// absence - the default is enforced here, not left to callers to
    /// remember.
    #[must_use]
    pub fn status(&self, operation: &str) -> CapabilityStatus {
        self.0.get(operation).copied().unwrap_or_default()
    }

    fn insert(mut self, operation: &'static str, status: CapabilityStatus) -> Self {
        self.0.insert(operation, status);
        self
    }
}

/// Every currently implemented Quad and Nano operation surface this matrix
/// labels, keyed by the exact Tauri command name it is exposed as. Extending
/// this list is how a new rendered control gains an evidence label - an
/// operation missing from it is simply not shown one, never silently
/// confirmed. `dashboard` and `reconnect_now` are deliberately absent: they
/// are the always-on read/health path rather than a discrete user-triggered
/// operation, and have no single per-operation evidence claim to label.
pub const OPERATIONS: &[&str] = &[
    "switch_scene",
    "recall_preset",
    "block_parameters",
    "set_parameter",
    "set_bypass",
    "set_scene_label",
    "set_scene_color",
    "set_nano_amp",
    "set_nano_gate_reduction",
    "set_nano_bypass",
    "read_nano_fx_params",
    "set_nano_fx_param",
];

/// The matrix seeded from what `spec/roadmap.md` records as hardware-verified
/// today. Extend this only alongside a roadmap entry recording the same
/// evidence - promoting an operation here without one is exactly the ad-hoc
/// claim this matrix exists to prevent.
#[must_use]
pub fn default_matrix() -> CapabilityMatrix {
    use CapabilityStatus::{ConfirmedReadable, ConfirmedWritable};

    CapabilityMatrix::default()
        // GUI-003.4, hardware-verified 2026-08-16: switch, then re-read; the
        // daemon's echoed scene is compared against the one requested.
        .insert("switch_scene", ConfirmedWritable)
        // GUI-003.1, hardware-verified 2026-08-17 through the sidebar: the
        // daemon's echoed slot matched and the GUI re-read the working copy.
        .insert("recall_preset", ConfirmedWritable)
        // GUI-003.3, hardware-verified 2026-08-17: read a real block's
        // parameters through the catalog join.
        .insert("block_parameters", ConfirmedReadable)
        // GUI-003.3, hardware-verified 2026-08-17: wrote one parameter, saw
        // the device report the new value back, restored the original.
        .insert("set_parameter", ConfirmedWritable)
        // NANO-001.5, Tauri backend hardware-verified 2026-08-18: a Tauri amp
        // write changed Gain by one raw step, independently read it back,
        // restored it and verified restoration in 12.15 seconds.
        .insert("set_nano_amp", ConfirmedWritable)
        // NANO-001.6, hardware-verified 2026-08-21: all five editable slots
        // returned non-empty normalized vectors, and the rendered native
        // Linux FX control displayed device-confirmed values.
        .insert("read_nano_fx_params", ConfirmedReadable)
        // NANO-001.6, hardware-verified 2026-08-21: the rendered native Linux
        // FX control changed one Pre FX 2 slider by 0.001, displayed
        // device-confirmed convergence, and restored through the same
        // control with matching device/draft values.
        .insert("set_nano_fx_param", ConfirmedWritable)
    // set_nano_gate_reduction and set_nano_bypass stay unverified: the
    // 2026-09-04 Gate-reduction pass explicitly records the Tauri boundary as
    // hardware-verified while "the rendered Gate control remains
    // provisional", and the Nano bypass entry (NANO-001.6) never itemises an
    // exact Tauri/rendered-control pass the way amp and FX do - both are
    // exactly the ambiguous-evidence case this matrix must not paper over.
}

/// One operation's evidence label for the typed Tauri/frontend contract.
///
/// The frontend renders exactly this - operation name paired with status -
/// and must not maintain its own copy of which operations are confirmed;
/// [`labels`] is the one place that decision is made.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct CapabilityLabel {
    pub operation: String,
    pub status: CapabilityStatus,
}

/// The complete evidence-label list for every operation in [`OPERATIONS`],
/// looked up against [`default_matrix`]. An operation absent from the seed
/// still appears here, labelled [`CapabilityStatus::Unverified`] - the
/// frontend never has to guess which operations exist or invent a status for
/// one the backend has not labelled.
#[must_use]
pub fn labels() -> Vec<CapabilityLabel> {
    let matrix = default_matrix();
    OPERATIONS
        .iter()
        .map(|&operation| CapabilityLabel {
            operation: operation.to_owned(),
            status: matrix.status(operation),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_operation_absent_from_the_matrix_is_unverified_not_confirmed() {
        let matrix = default_matrix();
        assert_eq!(
            matrix.status("an_operation_nobody_has_added_yet"),
            CapabilityStatus::Unverified
        );
    }

    #[test]
    fn an_empty_matrix_confirms_nothing() {
        let matrix = CapabilityMatrix::default();
        assert_eq!(matrix.status("switch_scene"), CapabilityStatus::Unverified);
    }

    #[test]
    fn the_default_matrix_only_confirms_what_the_roadmap_records_as_hardware_verified() {
        let matrix = default_matrix();
        assert_eq!(
            matrix.status("switch_scene"),
            CapabilityStatus::ConfirmedWritable
        );
        assert_eq!(
            matrix.status("recall_preset"),
            CapabilityStatus::ConfirmedWritable
        );
        assert_eq!(
            matrix.status("block_parameters"),
            CapabilityStatus::ConfirmedReadable
        );
        assert_eq!(
            matrix.status("set_parameter"),
            CapabilityStatus::ConfirmedWritable
        );
        assert_eq!(
            matrix.status("set_nano_amp"),
            CapabilityStatus::ConfirmedWritable
        );
        assert_eq!(
            matrix.status("read_nano_fx_params"),
            CapabilityStatus::ConfirmedReadable
        );
        assert_eq!(
            matrix.status("set_nano_fx_param"),
            CapabilityStatus::ConfirmedWritable
        );
    }

    /// `set_bypass`, `set_scene_label` and `set_scene_color` are implemented
    /// and offline-verified (GUI-003.3, GUI-003.4), which is exactly the trap
    /// this matrix exists to avoid: "it works" is not "hardware confirmed".
    /// None has a recorded hardware pass, so all three stay unverified.
    #[test]
    fn offline_verified_operations_are_not_promoted_on_the_strength_of_appearing_to_work() {
        let matrix = default_matrix();
        for operation in ["set_bypass", "set_scene_label", "set_scene_color"] {
            assert_eq!(
                matrix.status(operation),
                CapabilityStatus::Unverified,
                "{operation} has no recorded hardware pass and must not be promoted"
            );
        }
    }

    /// `set_nano_gate_reduction` and `set_nano_bypass` each have SOME
    /// hardware evidence in `spec/roadmap.md` (NANO-001.5/.6), but neither
    /// citation is the exact rendered-GUI-control pass the matrix requires:
    /// the Gate entry explicitly says "the rendered Gate control remains
    /// provisional", and the bypass entry never itemises a Tauri/rendered
    /// pass at all. Promoting either would be exactly the ambiguous-evidence
    /// claim this matrix must refuse to make.
    #[test]
    fn nano_operations_with_ambiguous_rendered_evidence_stay_unverified() {
        let matrix = default_matrix();
        for operation in ["set_nano_gate_reduction", "set_nano_bypass"] {
            assert_eq!(
                matrix.status(operation),
                CapabilityStatus::Unverified,
                "{operation} has no unambiguous exact-GUI-path hardware pass and must not be promoted"
            );
        }
    }

    #[test]
    fn labels_covers_every_operation_including_ones_absent_from_the_seed() {
        let all = labels();
        assert_eq!(all.len(), OPERATIONS.len());
        let confirmed = all
            .iter()
            .find(|label| label.operation == "set_nano_amp")
            .expect("set_nano_amp is a known operation");
        assert_eq!(confirmed.status, CapabilityStatus::ConfirmedWritable);
        let unverified = all
            .iter()
            .find(|label| label.operation == "set_nano_bypass")
            .expect("set_nano_bypass is a known operation");
        assert_eq!(unverified.status, CapabilityStatus::Unverified);
    }

    #[test]
    fn capability_status_serialises_with_the_roadmap_vocabulary() {
        let cases = [
            (
                CapabilityStatus::ConfirmedReadable,
                "\"confirmed-readable\"",
            ),
            (
                CapabilityStatus::ConfirmedWritable,
                "\"confirmed-writable\"",
            ),
            (CapabilityStatus::Inferred, "\"inferred\""),
            (CapabilityStatus::Unsupported, "\"unsupported\""),
            (CapabilityStatus::Unverified, "\"unverified\""),
        ];
        for (status, expected) in cases {
            assert_eq!(serde_json::to_string(&status).unwrap(), expected);
        }
    }
}

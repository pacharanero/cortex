// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Process tests proving selected preset-copy and setlist dry runs need no
//! daemon or device, and that protected or escaping targets are refused
//! before operation dispatch.
//!
//! @see spec/200-cli/spec.md [FR-24] [FR-25]

use std::process::Command;

fn cortex(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(args)
        .output()
        .expect("run cortex process")
}

#[test]
fn file_operation_dry_runs_need_no_daemon_or_device() {
    for args in [
        &[
            "preset",
            "copy",
            "--from",
            "6A",
            "--to-setlist",
            "/media/p4/Presets/Fictional Destination",
            "--to",
            "1A",
            "--instrument",
            "bass",
            "--dry-run",
            "--format",
            "json",
        ][..],
        &[
            "setlist",
            "create",
            "--name",
            "Fictional Temp",
            "--dry-run",
            "--format",
            "json",
        ],
        &[
            "setlist",
            "duplicate",
            "--source",
            "My Presets",
            "--destination",
            "Fictional Duplicate",
            "--limit",
            "2",
            "--dry-run",
            "--format",
            "json",
        ],
    ] {
        let output = cortex(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["dry_run"], true, "{args:?}: {value}");
    }
}

#[test]
fn protected_or_escaping_setlist_names_fail_before_process_io() {
    for args in [
        &["setlist", "delete", "--name", "My Presets", "--dry-run"][..],
        &[
            "setlist",
            "delete",
            "--name",
            "/media/p4/Presets",
            "--dry-run",
        ],
        &["setlist", "create", "--name", "Nested/Name", "--dry-run"],
        &[
            "preset",
            "copy",
            "--from",
            "6A",
            "--to-setlist",
            "/opt/neuraldsp/Factory Library",
            "--to",
            "1A",
            "--dry-run",
        ],
    ] {
        let output = cortex(args);
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        assert!(output.stdout.is_empty(), "refusal wrote data to stdout");
    }
}

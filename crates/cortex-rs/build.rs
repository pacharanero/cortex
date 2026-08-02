// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Build script for `cortex-rs`: compiles the recovered Cortex Control
//! protobuf schema (`proto/Preset.proto`, `proto/ProductionAutomation.proto`)
//! into typed Rust types via `prost`.
//!
//! The schema was recovered from Neural DSP's Cortex Control application by
//! the MIT-licensed `stokes-audio/pyquadcortex` project
//! (https://github.com/stokes-audio/pyquadcortex). We vendor only the schema
//! definitions needed for interoperability - no Cortex Control binaries,
//! firmware, or artwork are redistributed. See `NOTICE` and
//! `THIRD-PARTY-NOTICES.md` at the repo root for attribution.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_dir = PathBuf::from("proto");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);

    prost_build::Config::new()
        .out_dir(&out_dir)
        .compile_protos(
            &[
                proto_dir.join("Preset.proto"),
                proto_dir.join("ProductionAutomation.proto"),
            ],
            &[proto_dir],
        )?;

    // Tell Cargo to recompile if the schema changes.
    println!("cargo::rerun-if-changed=proto/Preset.proto");
    println!("cargo::rerun-if-changed=proto/ProductionAutomation.proto");
    Ok(())
}

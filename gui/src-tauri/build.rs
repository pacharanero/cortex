// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Build script for the Tauri backend: applies `tauri-build`'s Cargo helpers
//! and validates the application configuration during native builds.
//!
//! @see spec/400-gui/spec.md
//! @see spec/600-ci-release/spec.md [FR-14]

fn main() {
    tauri_build::build()
}

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Build script for the Tauri backend: runs `tauri-build` to parse
//! `tauri.conf.json`, generate icons, and wire the frontend build.
//!
//! @see spec/400-gui/spec.md
//! @see spec/400-gui/design.md

fn main() {
    tauri_build::build()
}

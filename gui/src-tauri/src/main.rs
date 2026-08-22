// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tauri entry point; behaviour lives in the `cortex_gui` library target.
//!
//! @see spec/400-gui/spec.md
//! @see spec/400-gui/design.md

fn main() {
    cortex_gui::run();
}

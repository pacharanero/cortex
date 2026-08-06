// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Dashboard {
    health: &'static str,
    coros_version: Option<String>,
    preset_name: &'static str,
    active_scene: u8,
    cpu_load: Option<f32>,
    blocks: Vec<Block>,
}
#[derive(serde::Serialize)]
struct Block {
    row: u8,
    column: u8,
    name: &'static str,
    category: &'static str,
    bypassed: bool,
}

#[tauri::command]
fn dashboard() -> Dashboard {
    Dashboard {
        health: "demo",
        coros_version: None,
        preset_name: "Demo working grid",
        active_scene: 0,
        cpu_load: None,
        blocks: vec![],
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![dashboard])
        .run(tauri::generate_context!())
        .expect("Tauri application failed")
}

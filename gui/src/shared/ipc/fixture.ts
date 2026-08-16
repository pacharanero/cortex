// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { CortexApi, DashboardSnapshot, SceneSnapshot } from "./types";

/** Mirrors the eight zero-based scenes the unit exposes as A-H. */
const scenes: SceneSnapshot[] = [
  { index: 0, letter: "A", label: "Clean", color: 0xff3fa9f5 },
  { index: 1, letter: "B", label: "Crunch", color: 0xffffc107 },
  { index: 2, letter: "C", label: "Lead", color: 0xffe53935 },
  // Deliberately unlabelled from D onward: a real preset often labels only the
  // scenes it uses, and the selector still has to reach the rest.
  { index: 3, letter: "D", label: null, color: null },
  { index: 4, letter: "E", label: null, color: null },
  { index: 5, letter: "F", label: null, color: null },
  { index: 6, letter: "G", label: null, color: null },
  { index: 7, letter: "H", label: null, color: null },
];

const dashboard: DashboardSnapshot = {
  source: "fixture",
  status: {
    daemon_version: "fixture",
    uptime_seconds: 0,
    device: { state: "connected", serial: null, coros_version: null, last_message_seconds: 0 },
    cache: {
      generation: 1, revision: 1, storage_revision: 1, phase: "live", catalog: true,
      current_preset: true, active_scene: true, preset_dirty: true, preset_location: true,
      listed_setlists: ["/media/p4/Presets/My Presets"], pushes_applied: 1,
      messages_seen: 1, messages_rejected: 0, stream_gaps: 0, last_rejection: null,
    },
  },
  live: {
    generation: 1,
    revision: 1,
    storage_revision: 1,
    preset_name: "Demo working grid",
    active_scene: 0,
    active_scene_label: "A",
    preset_dirty: false,
    cpu_load: {
      total: 41.2,
      chains: [[
        { load: 4.2, on_core2: false }, { load: 16.8, on_core2: false },
        { load: 8.1, on_core2: true }, { load: 12.1, on_core2: true },
      ]],
    },
    blocks: [
      { row: 0, screen_row: 1, column: 0, model_id: 1, name: "Input", category: "I/O", based_on: null, bypassed: false, params: [] },
      { row: 0, screen_row: 1, column: 1, model_id: 1001, name: "Brit 2203", category: "Amplifier", based_on: null, bypassed: false, params: [] },
      { row: 0, screen_row: 1, column: 3, model_id: 2001, name: "Cab", category: "Cabinet", based_on: null, bypassed: false, params: [] },
      { row: 0, screen_row: 1, column: 5, model_id: 3001, name: "Delay", category: "Delay", based_on: null, bypassed: true, params: [] },
      { row: 0, screen_row: 1, column: 7, model_id: 2, name: "Output", category: "I/O", based_on: null, bypassed: false, params: [] },
    ],
    scenes,
  },
  directory: [{
    key: "/media/p4/Presets/My Presets",
    name: "My Presets",
    is_factory: false,
    slots: [{ index: 0, slot: "1A", name: "Demo working grid" }],
  }],
};

export const fixtureApi: CortexApi = {
  async dashboard() { return structuredClone(dashboard); },
  async reconnectNow() {},
  async switchScene(scene: number) {
    // Refuse the same range the Rust boundary refuses, so fixture mode cannot
    // make an interaction look workable that production would reject.
    const target = scenes.find((candidate) => candidate.index === scene);
    if (!target) throw new Error(`scene ${scene} is out of range; scenes are zero-based 0-7 and display as A-H`);
    if (!dashboard.live) return;
    dashboard.live.active_scene = target.index;
    dashboard.live.active_scene_label = target.label ?? target.letter;
    // The device answers a switch with a new revision; the header shows it.
    dashboard.live.revision += 1;
    dashboard.status.cache.revision = dashboard.live.revision;
  },
};

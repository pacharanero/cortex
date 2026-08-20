// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { CortexApi, DashboardSnapshot, LiveBlock, NanoCurrentState, ParameterInput, ParameterView, SceneSnapshot } from "./types";

/**
 * Parameters per block cell, keyed "row,column".
 *
 * Shaped like the real join: values are stored NORMALISED 0..1 as the device
 * holds them, with `real` the catalog conversion - so browser mode exercises
 * the same conversion the hardware path does rather than a tidier fiction. One
 * meter and one switch are included because those are the cases most easily got
 * wrong.
 */
const blockParameters: Record<string, ParameterView[]> = {
  "0,1": [
    { index: 0, name: "GAIN", kind: "float", units: "", min: 0, max: 10, normalised: 0.62, real: 6.2, text: null, step_names: [], read_only: false, per_scene: true },
    { index: 1, name: "BASS", kind: "float", units: "", min: 0, max: 10, normalised: 0.5, real: 5, text: null, step_names: [], read_only: false, per_scene: false },
    { index: 2, name: "MID", kind: "float", units: "", min: 0, max: 10, normalised: 0.44, real: 4.4, text: null, step_names: [], read_only: false, per_scene: false },
    { index: 3, name: "TREBLE", kind: "float", units: "", min: 0, max: 10, normalised: 0.7, real: 7, text: null, step_names: [], read_only: false, per_scene: false },
    { index: 4, name: "MASTER", kind: "fader", units: "dB", min: -60, max: 0, normalised: 0.8, real: -12, text: null, step_names: [], read_only: false, per_scene: false },
    { index: 5, name: "BRIGHT", kind: "switch", units: "", min: 0, max: 1, normalised: 0, real: 0, text: null, step_names: ["Off", "On"], read_only: false, per_scene: false },
    { index: 6, name: "OUTPUT LEVEL", kind: "meter", units: "dB", min: -60, max: 0, normalised: 0.55, real: -27, text: null, step_names: [], read_only: true, per_scene: false },
  ],
  "0,3": [
    { index: 0, name: "MIC", kind: "str", units: "", min: 0, max: 0, normalised: null, real: null, text: "SM57", step_names: [], read_only: false, per_scene: false },
    { index: 1, name: "DISTANCE", kind: "float", units: "cm", min: 0, max: 30, normalised: 0.2, real: 6, text: null, step_names: [], read_only: false, per_scene: false },
  ],
};

/** The one USER setlist browser mode pretends the unit has. */
const FIXTURE_SETLIST = "/media/p4/Presets/My Presets";

/**
 * Stored presets the fixture can recall, so the preset directory is worth
 * clicking in browser mode. Names are invented: real preset names say what
 * someone plays and do not belong in the repository.
 */
const storedPresets: Record<string, { name: string; blocks: LiveBlock[] }> = {
  "1A": {
    name: "Demo working grid",
    blocks: [
      { row: 0, screen_row: 1, column: 0, model_id: 1, name: "Input", category: "I/O", based_on: null, bypassed: false, params: [], family: "utility" },
      { row: 0, screen_row: 1, column: 1, model_id: 1001, name: "Brit 2203", category: "Amplifier", based_on: null, bypassed: false, params: [], family: "amp" },
      { row: 0, screen_row: 1, column: 3, model_id: 2001, name: "Cab", category: "Cabinet", based_on: null, bypassed: false, params: [], family: "cab" },
      { row: 0, screen_row: 1, column: 5, model_id: 3001, name: "Delay", category: "Delay", based_on: null, bypassed: true, params: [], family: "delay" },
      { row: 0, screen_row: 1, column: 7, model_id: 2, name: "Output", category: "I/O", based_on: null, bypassed: false, params: [], family: "utility" },
    ],
  },
  "1B": {
    name: "Fixture clean platform",
    blocks: [
      { row: 0, screen_row: 1, column: 0, model_id: 1, name: "Input", category: "I/O", based_on: null, bypassed: false, params: [], family: "utility" },
      { row: 0, screen_row: 1, column: 2, model_id: 1091, name: "Brit Plexi 100 Bright", category: "Amplifier", based_on: null, bypassed: false, params: [], family: "amp" },
      { row: 0, screen_row: 1, column: 4, model_id: 12006, name: "412 Brit 60B GB 71 (M)", category: "Cabinet", based_on: null, bypassed: false, params: [], family: "cab" },
      { row: 0, screen_row: 1, column: 7, model_id: 2, name: "Output", category: "I/O", based_on: null, bypassed: false, params: [], family: "utility" },
    ],
  },
  "2A": {
    name: "Fixture ambient bed",
    blocks: [
      { row: 0, screen_row: 1, column: 0, model_id: 1, name: "Input", category: "I/O", based_on: null, bypassed: false, params: [], family: "utility" },
      { row: 0, screen_row: 1, column: 3, model_id: 3001, name: "Delay", category: "Delay", based_on: null, bypassed: false, params: [], family: "delay" },
      { row: 1, screen_row: 2, column: 3, model_id: 4001, name: "Reverb", category: "Reverb", based_on: null, bypassed: false, params: [], family: "reverb" },
      { row: 0, screen_row: 1, column: 7, model_id: 2, name: "Output", category: "I/O", based_on: null, bypassed: false, params: [], family: "utility" },
    ],
  },
};

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
    device_kind: "quad_cortex",
    device: { state: "connected", serial: null, coros_version: null, last_message_seconds: 0 },
    cache: {
      generation: 1, revision: 1, storage_revision: 1, phase: "live", catalog: true,
      current_preset: true, active_scene: true, preset_dirty: true, preset_location: true,
      listed_setlists: [FIXTURE_SETLIST], pushes_applied: 1,
      messages_seen: 1, messages_rejected: 0, stream_gaps: 0, last_rejection: null,
    },
  },
  live: {
    generation: 1,
    revision: 1,
    storage_revision: 1,
    preset_name: storedPresets["1A"].name,
    active_scene: 0,
    // Matches scene 0's own label below; the daemon returns the label when the
    // preset carries one, so a fixture that said "A" here would be describing
    // state the real boundary cannot produce.
    active_scene_label: "Clean",
    preset_dirty: false,
    cpu_load: {
      total: 41.2,
      chains: [[
        { load: 4.2, on_core2: false }, { load: 16.8, on_core2: false },
        { load: 8.1, on_core2: true }, { load: 12.1, on_core2: true },
      ]],
    },
    blocks: structuredClone(storedPresets["1A"].blocks),
    scenes,
  },
  directory: [{
    key: FIXTURE_SETLIST,
    name: "My Presets",
    is_factory: false,
    slots: [
      { index: 0, slot: "1A", name: storedPresets["1A"].name },
      { index: 1, slot: "1B", name: storedPresets["1B"].name },
      { index: 8, slot: "2A", name: storedPresets["2A"].name },
    ],
  }],
  nano: null,
};

const nanoState: NanoCurrentState = {
  firmware: "NC-FICTION-1.2.3",
  amp: { gain: 101, level: 102, bass: 103, mid: 104, treble: 105 },
  capture_slot: 2, capture_volume: 128, gate_reduction: 42,
  footswitch_assignments: { ia: 1, ib: 2, iia: 3, iib: 4 },
  slots: [
    { role: "gate", loaded_name: null, model_id: null, bypassed: false },
    { role: "pre_fx1", loaded_name: null, model_id: 1001, bypassed: false },
    { role: "pre_fx2", loaded_name: null, model_id: 1002, bypassed: true },
    { role: "capture", loaded_name: "Fictional Capture", model_id: null, bypassed: null },
    { role: "ir_cab", loaded_name: "Fictional Cabinet", model_id: null, bypassed: false },
    { role: "post_fx1", loaded_name: null, model_id: 1003, bypassed: false },
    { role: "post_fx2", loaded_name: null, model_id: 1004, bypassed: false },
    { role: "post_fx3", loaded_name: null, model_id: 1005, bypassed: false },
  ],
};

/** The device answers an edit with a new revision; the header shows it. */
function bumpRevision() {
  if (!dashboard.live) return;
  dashboard.live.revision += 1;
  dashboard.status.cache.revision = dashboard.live.revision;
}

export const fixtureApi: CortexApi = {
  async dashboard() {
    if (new URLSearchParams(window.location.search).get("device") === "nano") {
      return structuredClone({
        ...dashboard,
        status: { ...dashboard.status, device_kind: "nano_cortex", cache: { ...dashboard.status.cache, phase: "unsubscribed" } },
        live: null, directory: [], nano: nanoState,
      });
    }
    return structuredClone(dashboard);
  },
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
  async recallPreset(setlist: string, slot: string) {
    // Refuse what the Rust boundary refuses, so browser mode cannot make an
    // unworkable interaction look workable.
    if (!setlist.trim() || !slot.trim()) throw new Error("a recall needs both a setlist path and a slot");
    if (setlist !== FIXTURE_SETLIST) throw new Error(`unknown setlist ${setlist}`);
    const stored = storedPresets[slot];
    if (!stored) throw new Error(`slot ${slot} is empty in the fixture setlist`);
    if (!dashboard.live) return;
    // A recall replaces the whole working copy, so the fixture replaces it too
    // rather than patching the name and leaving a stale grid on screen.
    dashboard.live.preset_name = stored.name;
    dashboard.live.blocks = structuredClone(stored.blocks);
    dashboard.live.active_scene = 0;
    dashboard.live.active_scene_label = scenes[0].label ?? scenes[0].letter;
    dashboard.live.preset_dirty = false;
    dashboard.live.revision += 1;
    dashboard.status.cache.revision = dashboard.live.revision;
  },
  async setSceneLabel(scene: number, label: string | null) {
    const target = scenes.find((candidate) => candidate.index === scene);
    if (!target) throw new Error(`scene ${scene} is out of range`);
    // Blank means unlabelled, as Rust does: the unit has no empty-string state.
    target.label = label && label.trim() ? label : null;
    if (dashboard.live && dashboard.live.active_scene === scene) {
      dashboard.live.active_scene_label = target.label ?? target.letter;
    }
    bumpRevision();
  },
  async setSceneColor(scene: number, color: number) {
    const target = scenes.find((candidate) => candidate.index === scene);
    if (!target) throw new Error(`scene ${scene} is out of range`);
    target.color = 0xff000000 | (color & 0x00ffffff);
    bumpRevision();
  },
  async setBypass(row: number, column: number, bypass: boolean) {
    const block = dashboard.live?.blocks.find((b) => b.row === row && b.column === column);
    if (!block) throw new Error(`no block at row ${row}, column ${column}`);
    block.bypassed = bypass;
    if (dashboard.live) dashboard.live.preset_dirty = true;
    bumpRevision();
  },
  async setNanoAmp(control, value) {
    if (!Number.isInteger(value) || value < 0 || value > 255) throw new Error("Nano amp value must be an integer from 0 to 255");
    nanoState.amp[control] = value;
  },
  async blockParameters(row: number, column: number) {
    if (row < 0 || row > 3 || column < 0 || column > 7) throw new Error(`row ${row}, column ${column} is outside the grid`);
    const found = blockParameters[`${row},${column}`];
    if (!found) throw new Error(`no block at row ${row}, column ${column}`);
    return structuredClone(found);
  },
  async setParameter(row: number, column: number, index: number, input: ParameterInput) {
    if (row < 0 || row > 3 || column < 0 || column > 7) throw new Error(`row ${row}, column ${column} is outside the grid`);
    const params = blockParameters[`${row},${column}`];
    const target = params?.find((candidate) => candidate.index === index);
    if (!target) throw new Error(`no parameter ${index} on the block at row ${row}, column ${column}`);
    if (target.read_only) throw new Error(`${target.name} is a meter, not a setting`);
    // Apply in the same terms the device stores: normalised is authoritative,
    // and `real` is derived from it, never the other way round.
    if (input.kind === "text") {
      target.text = input.value;
    } else {
      const normalised = input.kind === "normalised"
        ? input.value
        : (input.value - target.min) / (target.max - target.min);
      target.normalised = Math.min(1, Math.max(0, normalised));
      target.real = target.max === target.min ? null : target.min + target.normalised * (target.max - target.min);
    }
    if (dashboard.live) {
      dashboard.live.preset_dirty = true;
      dashboard.live.revision += 1;
      dashboard.status.cache.revision = dashboard.live.revision;
    }
  },
};

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

export type CachePhase = "unsubscribed" | "seeding" | "live" | "incomplete" | "invalidated";

export type DeviceHealth =
  | { state: "connected"; serial: string | null; coros_version: string | null; last_message_seconds: number }
  | { state: "reconnecting"; attempts: number; last_error: string }
  | { state: "failed"; error: string };

export interface CacheStatus {
  generation: number;
  revision: number;
  storage_revision: number;
  phase: CachePhase;
  catalog: boolean;
  current_preset: boolean;
  active_scene: boolean;
  preset_dirty: boolean;
  preset_location: boolean;
  listed_setlists: string[];
  pushes_applied: number;
  messages_seen: number;
  messages_rejected: number;
  stream_gaps: number;
  last_rejection: string | null;
}

export interface DaemonStatus {
  daemon_version: string;
  uptime_seconds: number;
  device_kind: "quad_cortex" | "nano_cortex";
  device: DeviceHealth;
  cache: CacheStatus;
}

export type NanoSlotRole = "gate" | "pre_fx1" | "pre_fx2" | "capture" | "ir_cab" | "post_fx1" | "post_fx2" | "post_fx3";
export type NanoAmpControl = "gain" | "level" | "bass" | "mid" | "treble";
export interface NanoSlotState { role: NanoSlotRole; loaded_name: string | null; model_id: number | null; bypassed: boolean | null }
export interface NanoCurrentState {
  firmware: string | null;
  amp: { gain: number | null; level: number | null; bass: number | null; mid: number | null; treble: number | null };
  capture_slot: number | null;
  capture_volume: number | null;
  gate_reduction: number | null;
  footswitch_assignments: { ia: number; ib: number; iia: number; iib: number } | null;
  slots: NanoSlotState[];
}

export interface ParamValue {
  index: number;
  name?: string | null;
  value: number | string;
  per_scene?: Array<number | string>;
}

export interface LiveBlock {
  row: number;
  screen_row: number;
  column: number;
  model_id: number;
  name: string;
  category: string;
  based_on: string | null;
  bypassed: boolean;
  params: ParamValue[];
  /**
   * Visual family, classified in Rust from the block's catalog category.
   *
   * The device does not tell us what colour a block is - the grid palette is a
   * Cortex Control UI convention - so this is the vendor's own grouping read
   * off its block picker, not an invented one. An unrecognised category
   * arrives as `other` and is drawn neutrally.
   */
  family: string;
}

export interface CpuColumn { load: number; on_core2: boolean }
export interface CpuLoad { total: number | null; chains: CpuColumn[][] }
export interface PresetSlot { index: number; slot: string; name: string }
export interface SetlistSnapshot { key: string; name: string; is_factory: boolean; slots: PresetSlot[] }

/**
 * One scene. `index` is the zero-based protocol value; `letter` is the A-H the
 * unit shows. Both come from Rust so the mapping has one implementation - send
 * `index`, display `letter`, never the other way round.
 */
export interface SceneSnapshot {
  index: number;
  letter: string;
  label: string | null;
  color: number | null;
}

export interface LiveSnapshot {
  generation: number;
  revision: number;
  storage_revision: number;
  preset_name: string;
  active_scene: number;
  active_scene_label: string;
  preset_dirty: boolean | null;
  cpu_load: CpuLoad | null;
  blocks: LiveBlock[];
  scenes: SceneSnapshot[];
}

export interface DashboardSnapshot {
  source: "daemon" | "fixture";
  status: DaemonStatus;
  live: LiveSnapshot | null;
  directory: SetlistSnapshot[];
  nano: NanoCurrentState | null;
}

/** How a parameter value is supplied to a write. Mirrors Rust's `ParameterInput`. */
export type ParameterInput =
  | { kind: "normalised"; value: number }
  | { kind: "real"; value: number }
  | { kind: "text"; value: string };

/**
 * One editable parameter on a block, already joined against the device catalog
 * in Rust.
 *
 * The wire carries a normalised 0..1 float while the unit displays real units,
 * so both are present: `normalised` is what the device holds and `real` is that
 * value in `units`. `real` is null when the catalog declares a degenerate
 * range, which some entries genuinely do - better nothing than a confident
 * wrong number.
 */
export interface ParameterView {
  index: number;
  name: string;
  kind: "float" | "int" | "switch" | "str" | "fader" | "meter" | "unknown";
  units: string;
  min: number;
  max: number;
  normalised: number | null;
  real: number | null;
  text: string | null;
  step_names: string[];
  /** A live reading, not a setting. Shown, never editable. */
  read_only: boolean;
  /** The device stores one value per scene, so an edit reaches only the active one. */
  per_scene: boolean;
}

export interface CortexApi {
  dashboard(): Promise<DashboardSnapshot>;
  reconnectNow(): Promise<void>;
  /**
   * Switch the active scene. Takes the zero-based index, never the letter.
   *
   * Non-persistent: this changes what the unit is playing and saves nothing,
   * and is reversible by switching back.
   */
  switchScene(scene: number): Promise<void>;
  /**
   * Recall a stored preset into the working copy.
   *
   * Recall is free in this project's safety model: it writes nothing to
   * storage. It is not without consequence - it changes what the unit is
   * playing and replaces the working copy, discarding unsaved edits, exactly
   * as pressing the preset on the unit does.
   *
   * Whether the setlist is the read-only factory library is derived in Rust
   * from the path, so it is deliberately not a parameter here.
   */
  recallPreset(setlist: string, slot: string): Promise<void>;
  /**
   * Read the editable parameters of one block.
   *
   * `row` is the ZERO-BASED WIRE row the block reports, never the 1-4
   * `screen_row` shown to the user. A write to the wrong row succeeds silently.
   */
  blockParameters(row: number, column: number): Promise<ParameterView[]>;
  /**
   * Write one parameter. Non-persistent: it edits the working copy and changes
   * what is heard, and saves nothing.
   */
  setParameter(row: number, column: number, index: number, input: ParameterInput): Promise<void>;
  /**
   * Rename a scene on the working copy, or clear its label by passing null.
   * Non-persistent, like every edit here.
   */
  setSceneLabel(scene: number, label: string | null): Promise<void>;
  /**
   * Recolour a scene. Takes 0xRRGGBB; Rust forces the alpha opaque, since an
   * LED cannot be transparent. The unit accepts arbitrary RGB rather than a
   * fixed palette - confirmed on hardware 2026-08-16.
   */
  setSceneColor(scene: number, color: number): Promise<void>;
  /**
   * Bypass or engage a block.
   *
   * Per-scene: the device stores bypass per scene, so this reaches the ACTIVE
   * scene only. `row` is the zero-based WIRE row, never the 1-4 screen row.
   */
  setBypass(row: number, column: number, bypass: boolean): Promise<void>;
  /** Set one Nano amp control and wait for exact fresh read-back. */
  setNanoAmp(control: NanoAmpControl, value: number): Promise<void>;
}

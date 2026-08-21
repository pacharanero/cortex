// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import type {
  DashboardSnapshot,
  DeviceKind,
  NanoAmpControl,
  NanoBypassTarget,
  NanoFxSlot,
  ParameterInput,
  ParameterView,
} from "./generated";

export type { DashboardSnapshot };
export type { DeviceKind };
export type { LiveBlock } from "./generated";
export type { NanoAmpControl };
export type { NanoBypassTarget };
export type { NanoCurrentState } from "./generated";
export type { NanoFxSlot };
export type { NanoSlotRole } from "./generated";
export type { ParameterInput };
export type { ParameterView };
export type { SceneSnapshot } from "./generated";

/** The bounded command surface implemented by both the Tauri and fixture adapters. */
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
   * Recall writes nothing to storage, but it changes what the unit is playing
   * and replaces the working copy, discarding unsaved edits.
   */
  recallPreset(setlist: string, slot: string): Promise<void>;
  /**
   * Read one block using its zero-based wire row, never its 1-4 screen row. A
   * write to the wrong row succeeds silently.
   */
  blockParameters(row: number, column: number): Promise<ParameterView[]>;
  /** Edit the working copy and change what is heard without saving it. */
  setParameter(row: number, column: number, index: number, input: ParameterInput): Promise<void>;
  setSceneLabel(scene: number, label: string | null): Promise<void>;
  /** Set a scene colour as 0xRRGGBB; Rust forces its alpha channel opaque. */
  setSceneColor(scene: number, color: number): Promise<void>;
  /** Set bypass in the active scene using the block's zero-based wire row. */
  setBypass(row: number, column: number, bypass: boolean): Promise<void>;
  /** Set one Nano amp control and wait for exact fresh read-back. */
  setNanoAmp(control: NanoAmpControl, value: number): Promise<void>;
  setNanoBypass(target: NanoBypassTarget, bypassed: boolean): Promise<void>;
  readNanoFxParams(slot: NanoFxSlot): Promise<number[]>;
  setNanoFxParam(slot: NanoFxSlot, paramIndex: number, value: number): Promise<void>;
  setDevice(device: DeviceKind | null): Promise<void>;
}

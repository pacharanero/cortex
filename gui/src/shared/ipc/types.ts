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
  device: DeviceHealth;
  cache: CacheStatus;
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
}

export interface CpuColumn { load: number; on_core2: boolean }
export interface CpuLoad { total: number | null; chains: CpuColumn[][] }
export interface PresetSlot { index: number; slot: string; name: string }
export interface SetlistSnapshot { key: string; name: string; is_factory: boolean; slots: PresetSlot[] }

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
}

export interface DashboardSnapshot {
  source: "daemon" | "fixture";
  status: DaemonStatus;
  live: LiveSnapshot | null;
  directory: SetlistSnapshot[];
}

export interface CortexApi {
  dashboard(): Promise<DashboardSnapshot>;
}

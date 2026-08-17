// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { invoke } from "@tauri-apps/api/core";
import type { CortexApi, DashboardSnapshot } from "./types";

export const tauriApi: CortexApi = {
  dashboard() { return invoke<DashboardSnapshot>("dashboard"); },
  reconnectNow() { return invoke<void>("reconnect_now"); },
  switchScene(scene: number) { return invoke<void>("switch_scene", { scene }); },
  recallPreset(setlist: string, slot: string) { return invoke<void>("recall_preset", { setlist, slot }); },
};

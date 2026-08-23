// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { invoke } from "@tauri-apps/api/core";
import type { CortexApi, DashboardSnapshot, DeviceKind, ParameterInput, ParameterView } from "./types";

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (reason) {
    if (typeof reason === "object" && reason !== null && "message" in reason && typeof reason.message === "string") {
      throw new Error(reason.message);
    }
    throw reason;
  }
}

export const tauriApi: CortexApi = {
  dashboard() { return invokeCommand<DashboardSnapshot>("dashboard"); },
  reconnectNow() { return invokeCommand<void>("reconnect_now"); },
  switchScene(scene: number) { return invokeCommand<void>("switch_scene", { scene }); },
  recallPreset(setlist: string, slot: string) { return invokeCommand<void>("recall_preset", { setlist, slot }); },
  blockParameters(row: number, column: number) { return invokeCommand<ParameterView[]>("block_parameters", { row, column }); },
  setParameter(row: number, column: number, index: number, input: ParameterInput) {
    return invokeCommand<void>("set_parameter", { row, column, index, input });
  },
  setSceneLabel(scene: number, label: string | null) { return invokeCommand<void>("set_scene_label", { scene, label }); },
  setSceneColor(scene: number, color: number) { return invokeCommand<void>("set_scene_color", { scene, color }); },
  setBypass(row: number, column: number, bypass: boolean) { return invokeCommand<void>("set_bypass", { row, column, bypass }); },
  setNanoAmp(control, value) { return invokeCommand<void>("set_nano_amp", { control, value }); },
  setNanoGateReduction(percent) { return invokeCommand<void>("set_nano_gate_reduction", { percent }); },
  setNanoBypass(target, bypassed) { return invokeCommand<void>("set_nano_bypass", { target, bypassed }); },
  readNanoFxParams(slot) { return invokeCommand<number[]>("read_nano_fx_params", { slot }); },
  setNanoFxParam(slot, paramIndex, value) { return invokeCommand<void>("set_nano_fx_param", { slot, paramIndex, value }); },
  setDevice(device) { return invokeCommand<void>("set_device", { device }); },
};

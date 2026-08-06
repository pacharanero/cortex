// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

export type DeviceHealth = "demo" | "connected" | "reconnecting" | "disconnected";

export interface GridBlock {
  row: number;
  column: number;
  name: string;
  category: string;
  bypassed: boolean;
}

export interface CortexDashboard {
  health: DeviceHealth;
  corosVersion: string | null;
  presetName: string;
  activeScene: number;
  cpuLoad: number | null;
  blocks: GridBlock[];
}

const demo: CortexDashboard = {
  health: "demo",
  corosVersion: null,
  presetName: "Demo working grid",
  activeScene: 0,
  cpuLoad: null,
  blocks: [
    { row: 0, column: 0, name: "Input", category: "I/O", bypassed: false },
    { row: 0, column: 1, name: "Brit 2203", category: "Amplifier", bypassed: false },
    { row: 0, column: 3, name: "Cab", category: "Cabinet", bypassed: false },
    { row: 0, column: 5, name: "Delay", category: "Delay", bypassed: true },
    { row: 0, column: 7, name: "Output", category: "I/O", bypassed: false },
  ],
};

export interface CortexApi {
  dashboard(): Promise<CortexDashboard>;
}

/** Browser-mode API. Tauri will replace this with daemon-backed commands. */
export const cortexApi: CortexApi = {
  async dashboard() {
    return demo;
  },
};

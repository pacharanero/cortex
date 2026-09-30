// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";

export interface WindowSizePreset {
  label: string;
  width: number;
  height: number;
}

// A stable 16:10 desktop canvas gives the console predictable geometry without
// preventing users or window managers from choosing an arbitrary window size.
export const windowSizePresets: readonly WindowSizePreset[] = [
  { label: "75% (1080 x 675)", width: 1080, height: 675 },
  { label: "100% (1440 x 900)", width: 1440, height: 900 },
  { label: "125% (1800 x 1125)", width: 1800, height: 1125 },
  { label: "150% (2160 x 1350)", width: 2160, height: 1350 },
  { label: "200% (2880 x 1800)", width: 2880, height: 1800 },
];

export async function applyWindowSize(preset: WindowSizePreset): Promise<void> {
  await getCurrentWindow().setSize(new LogicalSize(preset.width, preset.height));
}

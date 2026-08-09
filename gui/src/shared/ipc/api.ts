// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { fixtureApi } from "./fixture";
import { tauriApi } from "./tauri";
import type { CortexApi } from "./types";

export function apiForMode(mode: string): CortexApi {
  if (mode === "fixture") return fixtureApi;
  if (mode === "tauri") return tauriApi;
  throw new Error(`Unsupported Cortex GUI mode ${JSON.stringify(mode)}; use fixture or tauri`);
}

export const cortexApi = apiForMode(import.meta.env.MODE);

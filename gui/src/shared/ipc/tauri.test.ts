// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { tauriApi } from "./tauri";

describe("Tauri IPC adapter", () => {
  beforeEach(() => invoke.mockReset());

  it("sends the complete identified real-unit parameter request", async () => {
    invoke.mockResolvedValue(undefined);
    const identity = {
      catalog_generation: 3,
      catalog_revision: 17,
      model_id: 5007,
      index: 4,
      name: "GAIN",
      catalog_descriptor: "test descriptor",
    };

    await tauriApi.setParameter(1, 2, identity, { kind: "real", value: 7.5 });

    expect(invoke).toHaveBeenCalledWith("set_parameter", {
      row: 1,
      column: 2,
      identity,
      input: { kind: "real", value: 7.5 },
    });
  });
});

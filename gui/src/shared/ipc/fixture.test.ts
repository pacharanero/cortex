// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it } from "vitest";
import { fixtureApi } from "./fixture";

describe("Nano fixture API", () => {
  afterEach(() => window.history.replaceState({}, "", "/"));

  it("exercises every Nano operation through the production CortexApi contract", async () => {
    window.history.replaceState({}, "", "/?device=nano");

    await fixtureApi.setNanoAmp("gain", 127);
    await fixtureApi.setNanoGateReduction(43);
    await fixtureApi.setNanoBypass("post_fx3", true);
    expect(await fixtureApi.readNanoFxParams("pre_fx1")).toEqual([
      { index: 0, name: "Overdrive", normalized: 0.5 },
      { index: 1, name: "Tone", normalized: 0.25 },
      { index: 2, name: "Level", normalized: 0.75 },
    ]);
    expect((await fixtureApi.setNanoFxParam("pre_fx1", 27, 1, 0.5))[1]).toEqual({ index: 1, name: "Tone", normalized: 0.5 });
    expect((await fixtureApi.readNanoFxParams("pre_fx1"))[1].normalized).toBe(0.5);

    const snapshot = await fixtureApi.dashboard();
    expect(snapshot.status.device_kind).toBe("nano_cortex");
    expect(snapshot.nano?.amp.gain).toBe(127);
    expect(snapshot.nano?.gate_reduction).toBe(43);
    expect(snapshot.nano?.slots.find((slot) => slot.role === "post_fx3")?.bypassed).toBe(true);
  });

  it("rejects invalid Nano values instead of accepting fixture-only state", async () => {
    await expect(fixtureApi.setNanoAmp("gain", 256)).rejects.toThrow("integer from 0 to 255");
    await expect(fixtureApi.setNanoGateReduction(101)).rejects.toThrow("integer from 0 to 100");
    await expect(fixtureApi.setNanoFxParam("pre_fx1", 27, 99, 0.5)).rejects.toThrow("index 99 is out of range");
    await expect(fixtureApi.setNanoFxParam("pre_fx1", 27, 0, 1.5)).rejects.toThrow("normalized from 0 to 1");
    await expect(fixtureApi.setNanoFxParam("pre_fx1", 99, 0, 0.5)).rejects.toThrow("model changed");
  });
});

// GUI-004.2: fixture mode must offer the same evidence labels production
// would, so browser-mode inspection is not exercising a fiction.
describe("capability evidence labels", () => {
  it("mirrors the Rust-owned matrix rather than inventing its own", async () => {
    const labels = await fixtureApi.capabilities();
    const byOperation = Object.fromEntries(labels.map((label) => [label.operation, label.status]));

    expect(byOperation.switch_scene).toBe("confirmed-writable");
    expect(byOperation.recall_preset).toBe("confirmed-writable");
    expect(byOperation.block_parameters).toBe("confirmed-readable");
    expect(byOperation.set_parameter).toBe("confirmed-writable");
    expect(byOperation.set_nano_amp).toBe("confirmed-writable");
    expect(byOperation.read_nano_fx_params).toBe("confirmed-readable");
    expect(byOperation.set_nano_fx_param).toBe("confirmed-writable");

    // Offline/fixture-verified only, and ambiguous Nano evidence: none of
    // these may be promoted (see capability.rs's own tests for why).
    for (const operation of ["set_bypass", "set_scene_label", "set_scene_color", "set_nano_gate_reduction", "set_nano_bypass"]) {
      expect(byOperation[operation]).toBe("unverified");
    }
  });
});

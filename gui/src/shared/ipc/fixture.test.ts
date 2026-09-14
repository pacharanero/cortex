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

describe("Quad fixture parameter identity", () => {
  it("converts a real-unit write carrying the identity returned by Rust", async () => {
    const [gain] = await fixtureApi.blockParameters(0, 1);

    await fixtureApi.setParameter(0, 1, gain.identity, { kind: "real", value: 7.5 });

    const [updated] = await fixtureApi.blockParameters(0, 1);
    expect(updated.real).toBe(7.5);
    expect(updated.normalised).toBe(0.75);

    await fixtureApi.setParameter(0, 1, gain.identity, { kind: "real", value: gain.real! });
  });

  it("rejects stale or mismatched identities", async () => {
    const [gain] = await fixtureApi.blockParameters(0, 1);

    await expect(fixtureApi.setParameter(0, 1, { ...gain.identity, model_id: 9999 }, { kind: "real", value: 5 })).rejects.toThrow("block model changed");
    await expect(fixtureApi.setParameter(0, 1, { ...gain.identity, catalog_revision: 99 }, { kind: "real", value: 5 })).rejects.toThrow("model catalog changed");
    await expect(fixtureApi.setParameter(0, 1, { ...gain.identity, index: 99 }, { kind: "real", value: 5 })).rejects.toThrow("no parameter 99");
    await expect(fixtureApi.setParameter(0, 1, { ...gain.identity, name: "LEVEL" }, { kind: "real", value: 5 })).rejects.toThrow("parameter identity changed");
    await expect(fixtureApi.setParameter(0, 1, { ...gain.identity, catalog_descriptor: "stale descriptor" }, { kind: "real", value: 5 })).rejects.toThrow("catalog metadata changed");
  });

  it("rejects values and input kinds that production rejects", async () => {
    const [gain] = await fixtureApi.blockParameters(0, 1);
    const [mic] = await fixtureApi.blockParameters(0, 3);

    await expect(fixtureApi.setParameter(0, 1, gain.identity, { kind: "normalised", value: 1.1 })).rejects.toThrow("within 0-1");
    await expect(fixtureApi.setParameter(0, 1, gain.identity, { kind: "normalised", value: Number.NaN })).rejects.toThrow("finite");
    await expect(fixtureApi.setParameter(0, 1, gain.identity, { kind: "text", value: "wrong kind" })).rejects.toThrow("numeric parameter");
    await expect(fixtureApi.setParameter(0, 3, mic.identity, { kind: "real", value: 1 })).rejects.toThrow("string parameter");
  });
});

// GUI-004.2/GUI-004.5: fixture diagnostics claim no hardware evidence.
describe("capability evidence", () => {
  it("returns no claimed hardware capabilities", async () => {
    expect(await fixtureApi.capabilities()).toEqual([]);
  });
});

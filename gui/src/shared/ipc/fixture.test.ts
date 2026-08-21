// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it } from "vitest";
import { fixtureApi } from "./fixture";

describe("Nano fixture API", () => {
  afterEach(() => window.history.replaceState({}, "", "/"));

  it("exercises every Nano operation through the production CortexApi contract", async () => {
    window.history.replaceState({}, "", "/?device=nano");

    await fixtureApi.setNanoAmp("gain", 127);
    await fixtureApi.setNanoBypass("post_fx3", true);
    expect(await fixtureApi.readNanoFxParams("pre_fx1")).toEqual([0.5, 0.25, 0.75, 0, 1]);
    await fixtureApi.setNanoFxParam("pre_fx1", 3, 0.5);

    const snapshot = await fixtureApi.dashboard();
    expect(snapshot.status.device_kind).toBe("nano_cortex");
    expect(snapshot.nano?.amp.gain).toBe(127);
    expect(snapshot.nano?.slots.find((slot) => slot.role === "post_fx3")?.bypassed).toBe(true);
  });

  it("rejects invalid amp values instead of accepting fixture-only state", async () => {
    await expect(fixtureApi.setNanoAmp("gain", 256)).rejects.toThrow("integer from 0 to 255");
  });
});

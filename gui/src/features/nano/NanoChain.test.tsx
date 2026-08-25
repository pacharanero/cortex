// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { NanoCurrentState } from "../../shared/ipc/types";
import { NanoChain } from "./NanoChain";

const state: NanoCurrentState = {
  firmware: "4.0.1",
  amp: { gain: 100, level: 110, bass: 120, mid: 130, treble: 140 },
  capture_slot: 1,
  capture_volume: 100,
  gate_reduction: null,
  footswitch_assignments: null,
  slots: [
    { role: "gate", loaded_name: null, model_id: null, bypassed: false },
    { role: "pre_fx1", loaded_name: "Fictional Drive", model_id: 1, bypassed: false },
    { role: "pre_fx2", loaded_name: "Fictional Chorus", model_id: 2, bypassed: true },
    { role: "capture", loaded_name: "Fictional Capture", model_id: null, bypassed: false },
    { role: "ir_cab", loaded_name: "Fictional IR", model_id: null, bypassed: false },
    { role: "post_fx1", loaded_name: "Fictional Delay", model_id: 3, bypassed: false },
    { role: "post_fx2", loaded_name: "Fictional Reverb", model_id: 4, bypassed: false },
    { role: "post_fx3", loaded_name: "Fictional EQ", model_id: 5, bypassed: false },
  ],
};

describe("NanoChain", () => {
  it("serializes parameter reads and gives every control a target-specific name", async () => {
    let finishRead: (values: number[]) => void = () => undefined;
    const readFx = vi.fn(() => new Promise<number[]>((resolve) => { finishRead = resolve; }));
    render(<MantineProvider>
      <NanoChain
        onReadFxParameters={readFx}
        onSetAmp={vi.fn()}
        onSetBypass={vi.fn()}
        onSetFxParameter={vi.fn()}
        state={state}
      />
    </MantineProvider>);

    fireEvent.click(screen.getByRole("button", { name: /Position 2: Fictional Drive/ }));
    fireEvent.click(screen.getByRole("button", { name: /Position 3: Fictional Chorus/ }));
    expect(readFx).toHaveBeenCalledOnce();
    expect(readFx).toHaveBeenCalledWith("pre_fx1");

    await act(async () => finishRead([0.25]));
    await waitFor(() => expect(screen.getByRole("button", { name: "Apply Pre FX 1 parameter 0" })).toBeDefined());
    expect(screen.getByRole("button", { name: "Apply gain" })).toBeDefined();
    expect(screen.getByRole("switch", { name: "Bypass Gate" })).toBeDefined();
  });

  it("keeps focus on the control that started an operation", async () => {
    let finishWrite: () => void = () => undefined;
    const setAmp = vi.fn(() => new Promise<void>((resolve) => { finishWrite = resolve; }));
    render(<MantineProvider>
      <NanoChain
        onReadFxParameters={vi.fn()}
        onSetAmp={setAmp}
        onSetBypass={vi.fn()}
        onSetFxParameter={vi.fn()}
        state={state}
      />
    </MantineProvider>);

    const applyGain = screen.getByRole("button", { name: "Apply gain" });
    applyGain.focus();
    fireEvent.click(applyGain);
    expect(setAmp).toHaveBeenCalledWith("gain", 100);
    expect(document.activeElement).toBe(applyGain);
    expect(applyGain.hasAttribute("disabled")).toBe(false);
    expect(applyGain.getAttribute("aria-busy")).toBe("true");

    await act(async () => finishWrite());
  });
});

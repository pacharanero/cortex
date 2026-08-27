// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { NanoCurrentState, NanoFxParameter } from "../../shared/ipc/types";
import { NanoChain } from "./NanoChain";

const state: NanoCurrentState = {
  firmware: "4.0.1",
  amp: { gain: 100, level: 110, bass: 120, mid: 130, treble: 140 },
  capture_slot: 1,
  capture_volume: 100,
  gate_reduction: null,
  footswitch_assignments: null,
  slots: [
    { role: "gate", loaded_name: null, model_id: null, model_name: null, bypassed: false },
    { role: "pre_fx1", loaded_name: null, model_id: 1, model_name: "Fictional Drive", bypassed: false },
    { role: "pre_fx2", loaded_name: null, model_id: 2, model_name: "Fictional Chorus", bypassed: true },
    { role: "capture", loaded_name: "Fictional Capture", model_id: null, model_name: null, bypassed: false },
    { role: "ir_cab", loaded_name: "Fictional IR", model_id: null, model_name: null, bypassed: false },
    { role: "post_fx1", loaded_name: null, model_id: 3, model_name: "Fictional Delay", bypassed: false },
    { role: "post_fx2", loaded_name: null, model_id: 4, model_name: "Fictional Reverb", bypassed: false },
    { role: "post_fx3", loaded_name: null, model_id: 5, model_name: "Fictional EQ", bypassed: false },
  ],
};

function fx(normalized: number, name: string | null = "Gain", index = 0): NanoFxParameter {
  return { index, name, normalized };
}

function props(overrides: Record<string, unknown> = {}) {
  return {
    onReadFxParams: vi.fn(async () => []),
    onSetAmp: vi.fn(async () => {}),
    onSetGateReduction: vi.fn(async () => {}),
    onSetBypass: vi.fn(async () => {}),
    onSetFxParam: vi.fn(async () => []),
    ...overrides,
  };
}

function renderNano(current: NanoCurrentState = state, overrides: Record<string, unknown> = {}) {
  return render(<MantineProvider><NanoChain {...props(overrides)} state={current} /></MantineProvider>);
}

describe("NanoChain", () => {
  it("shows resolved effect names and keeps an unknown model explicit", () => {
    const unknown: NanoCurrentState = {
      ...state,
      slots: state.slots.map((slot) => slot.role === "pre_fx1" ? { ...slot, model_id: 99999, model_name: null } : slot),
    };

    renderNano(unknown);

    expect(screen.getByRole("button", { name: /Position 2: Unknown model 99999/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Position 3: Fictional Chorus/ })).toBeTruthy();
  });

  it("uses the shared editor canvas and gives every control a target-specific name", async () => {
    renderNano(state, { onReadFxParams: vi.fn(async () => [fx(0.25)]) });

    expect(screen.getByLabelText("Nano Cortex fixed signal chain").getAttribute("data-topology")).toBe("nano-chain");
    fireEvent.click(screen.getByRole("button", { name: /Position 2: Fictional Drive/ }));

    expect(await screen.findByRole("button", { name: "Apply Pre FX 1 Gain" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Apply gain" })).toBeDefined();
    expect(screen.getByRole("switch", { name: "Gate bypass, on" })).toBeDefined();
  });

  it("renders semantic FX names and preserves an explicit fallback for unknown parameters", async () => {
    renderNano(state, { onReadFxParams: vi.fn(async () => [fx(0.25, "Gain"), fx(0.5, null, 3)]) });

    fireEvent.click(screen.getByRole("button", { name: /Position 2: Fictional Drive/ }));

    expect(await screen.findByText("Gain")).toBeTruthy();
    expect(screen.getByText("Param 3")).toBeTruthy();
    expect(screen.getByText(/parameter 3 \| device: 0\.500/)).toBeTruthy();
  });

  it("keeps focus on the control that started an operation", async () => {
    let finishWrite: () => void = () => undefined;
    const onSetAmp = vi.fn(() => new Promise<void>((resolve) => { finishWrite = resolve; }));
    renderNano(state, { onSetAmp });

    const applyGain = screen.getByRole("button", { name: "Apply gain" });
    applyGain.focus();
    fireEvent.click(applyGain);
    expect(onSetAmp).toHaveBeenCalledWith("gain", 100);
    expect(document.activeElement).toBe(applyGain);
    expect(applyGain.hasAttribute("disabled")).toBe(false);
    expect(applyGain.getAttribute("aria-busy")).toBe("true");

    await act(async () => finishWrite());
    expect(document.activeElement).toBe(applyGain);
  });

  it("disables block selection while a write is pending", async () => {
    let finishWrite: () => void = () => undefined;
    const onSetAmp = vi.fn(() => new Promise<void>((resolve) => { finishWrite = resolve; }));
    renderNano(state, { onSetAmp });

    fireEvent.click(screen.getByRole("button", { name: "Apply gain" }));
    const drive = screen.getByRole("button", { name: /Position 2: Fictional Drive/ });
    expect(drive.hasAttribute("disabled")).toBe(true);
    expect(drive.getAttribute("aria-busy")).toBe("true");

    await act(async () => finishWrite());
    expect(drive.hasAttribute("disabled")).toBe(false);
  });

  it("preserves an unsubmitted amp draft across a dashboard refresh", () => {
    const nanoProps = props();
    const view = render(<MantineProvider><NanoChain {...nanoProps} state={state} /></MantineProvider>);
    const gain = screen.getByLabelText("Gain") as HTMLInputElement;

    fireEvent.change(gain, { target: { value: "121" } });
    expect(gain.value).toBe("121");

    view.rerender(<MantineProvider><NanoChain {...nanoProps} state={{ ...state, amp: { ...state.amp, gain: 99 } }} /></MantineProvider>);
    expect((screen.getByLabelText("Gain") as HTMLInputElement).value).toBe("121");
  });

  it("closes stale FX controls after a model change without discarding amp drafts", async () => {
    const nanoProps = props({ onReadFxParams: vi.fn(async () => [fx(0.5)]) });
    const view = render(<MantineProvider><NanoChain {...nanoProps} state={state} /></MantineProvider>);
    const gain = screen.getByLabelText("Gain") as HTMLInputElement;
    fireEvent.change(gain, { target: { value: "121" } });
    fireEvent.click(screen.getByRole("button", { name: /Position 2: Fictional Drive/ }));
    expect(await screen.findByRole("slider", { name: "Pre FX 1 Gain normalized value" })).toBeTruthy();

    const changed = {
      ...state,
      slots: state.slots.map((slot) => slot.role === "pre_fx1" ? { ...slot, model_id: 99, model_name: "Fictional Replacement" } : slot),
    };
    view.rerender(<MantineProvider><NanoChain {...nanoProps} state={changed} /></MantineProvider>);

    expect(await screen.findByText("Pre FX 1 model changed; select it again to load the new parameters.")).toBeTruthy();
    expect(screen.queryByRole("slider", { name: "Pre FX 1 Gain normalized value" })).toBeNull();
    expect((screen.getByLabelText("Gain") as HTMLInputElement).value).toBe("121");
  });

  it("exposes FX cards as keyboard-operable native buttons", async () => {
    const onReadFxParams = vi.fn(async () => [fx(0.5)]);
    renderNano(state, { onReadFxParams });
    const card = screen.getByRole("button", { name: /Position 2: Fictional Drive/ });

    expect(card.tagName).toBe("BUTTON");
    expect(card.tabIndex).toBe(0);
    card.focus();
    fireEvent.click(card);

    expect(onReadFxParams).toHaveBeenCalledWith("pre_fx1");
    expect(await screen.findByRole("slider", { name: "Pre FX 1 Gain normalized value" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Apply Pre FX 1 Gain" })).toBeTruthy();
  });

  it("serializes uncorrelated FX reads and applies only the latest selection", async () => {
    const finishReads: ((values: NanoFxParameter[]) => void)[] = [];
    const onReadFxParams = vi.fn(() => new Promise<NanoFxParameter[]>((resolve) => { finishReads.push(resolve); }));
    renderNano(state, { onReadFxParams });

    fireEvent.click(screen.getByRole("button", { name: /Position 2: Fictional Drive/ }));
    fireEvent.click(screen.getByRole("button", { name: /Position 3: Fictional Chorus/ }));
    expect(onReadFxParams).toHaveBeenCalledOnce();
    expect(onReadFxParams).toHaveBeenCalledWith("pre_fx1");

    await act(async () => finishReads[0]([fx(0.25)]));
    await waitFor(() => expect(onReadFxParams).toHaveBeenCalledTimes(2));
    expect(onReadFxParams).toHaveBeenLastCalledWith("pre_fx2");
    expect(screen.queryByRole("button", { name: "Apply Pre FX 1 Gain" })).toBeNull();

    await act(async () => finishReads[1]([fx(0.75, "Mix")]));
    expect(await screen.findByRole("button", { name: "Apply Pre FX 2 Mix" })).toBeTruthy();
  });

  it("reports an FX write failure even if the inspector closes", async () => {
    let rejectWrite: (reason: Error) => void = () => undefined;
    const onSetFxParam = vi.fn(() => new Promise<NanoFxParameter[]>((_resolve, reject) => { rejectWrite = reject; }));
    renderNano(state, {
      onReadFxParams: vi.fn(async () => [fx(0.5)]),
      onSetFxParam,
    });

    fireEvent.click(screen.getByRole("button", { name: /Position 2: Fictional Drive/ }));
    const slider = await screen.findByRole("slider", { name: "Pre FX 1 Gain normalized value" });
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    fireEvent.click(screen.getByRole("button", { name: "Apply Pre FX 1 Gain" }));
    expect(onSetFxParam).toHaveBeenCalledWith("pre_fx1", 1, 0, 0.501);
    fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));

    await act(async () => rejectWrite(new Error("confirmation failed")));
    expect(await screen.findByText("confirmation failed")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("Nano operation failed");
  });

  it("refreshes device values after an unconfirmed FX write", async () => {
    const onReadFxParams = vi.fn()
      .mockResolvedValueOnce([fx(0.5)])
      .mockResolvedValueOnce([fx(0.6)]);
    renderNano(state, {
      onReadFxParams,
      onSetFxParam: vi.fn(async () => { throw new Error("confirmation failed"); }),
    });

    fireEvent.click(screen.getByRole("button", { name: /Position 2: Fictional Drive/ }));
    const slider = await screen.findByRole("slider", { name: "Pre FX 1 Gain normalized value" });
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    fireEvent.click(screen.getByRole("button", { name: "Apply Pre FX 1 Gain" }));

    expect(await screen.findByText("confirmation failed")).toBeTruthy();
    expect(onReadFxParams).toHaveBeenCalledTimes(2);
    expect(screen.getByText(/device: 0\.600/)).toBeTruthy();
  });

  it("preserves a newer amp edit made while the submitted value is pending", async () => {
    let finishWrite: () => void = () => undefined;
    const onSetAmp = vi.fn(() => new Promise<void>((resolve) => { finishWrite = resolve; }));
    const nanoProps = props({ onSetAmp });
    const view = render(<MantineProvider><NanoChain {...nanoProps} state={state} /></MantineProvider>);
    const gain = screen.getByLabelText("Gain") as HTMLInputElement;

    fireEvent.change(gain, { target: { value: "121" } });
    const applyGain = screen.getByRole("button", { name: "Apply gain" });
    applyGain.focus();
    fireEvent.click(applyGain);
    expect(document.activeElement).toBe(applyGain);
    expect(applyGain.hasAttribute("disabled")).toBe(false);
    fireEvent.change(gain, { target: { value: "122" } });
    await act(async () => finishWrite());

    view.rerender(<MantineProvider><NanoChain {...nanoProps} state={{ ...state, amp: { ...state.amp, gain: 121 } }} /></MantineProvider>);
    expect((screen.getByLabelText("Gain") as HTMLInputElement).value).toBe("122");
  });

  it("preserves a newer FX edit while the submitted value is pending", async () => {
    let finishWrite: (values: NanoFxParameter[]) => void = () => undefined;
    const onSetFxParam = vi.fn(() => new Promise<NanoFxParameter[]>((resolve) => { finishWrite = resolve; }));
    renderNano(state, {
      onReadFxParams: vi.fn(async () => [fx(0.5)]),
      onSetFxParam,
    });

    fireEvent.click(screen.getByRole("button", { name: /Position 2: Fictional Drive/ }));
    const slider = await screen.findByRole("slider", { name: "Pre FX 1 Gain normalized value" });
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    fireEvent.click(screen.getByRole("button", { name: "Apply Pre FX 1 Gain" }));
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    await act(async () => finishWrite([fx(0.501)]));

    expect(screen.getByText(/device: 0\.501/)).toBeTruthy();
    expect(screen.getByText(/draft: 0\.502/)).toBeTruthy();
  });

  it("names each amp and bypass action for its control", () => {
    renderNano();

    expect(screen.getByRole("button", { name: "Apply gain" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Apply treble" })).toBeTruthy();
    expect(screen.getByRole("switch", { name: "Pre FX 1 bypass, on" })).toBeTruthy();
  });

  it("submits Gate reduction as an explicit percentage", () => {
    const onSetGateReduction = vi.fn(async () => {});
    renderNano({ ...state, gate_reduction: 42 }, { onSetGateReduction });

    fireEvent.change(screen.getByLabelText("Gate reduction"), { target: { value: "43" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply Gate reduction" }));

    expect(onSetGateReduction).toHaveBeenCalledWith(43);
  });

  it("announces operation progress and completion", async () => {
    renderNano();

    fireEvent.click(screen.getByRole("button", { name: "Apply gain" }));

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("gain applied."));
    expect(screen.getByRole("status").getAttribute("aria-live")).toBe("polite");
  });
});

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NanoChain } from "./NanoChain";
import type { NanoCurrentState } from "../../shared/ipc/types";

const state: NanoCurrentState = {
  firmware: null,
  amp: { gain: 100, level: 101, bass: 102, mid: 103, treble: 104 },
  capture_slot: null,
  capture_volume: null,
  gate_reduction: null,
  footswitch_assignments: null,
  slots: [],
};

function renderNano(current: NanoCurrentState) {
  return render(
    <MantineProvider>
      <NanoChain
        onReadFxParams={vi.fn(async () => [])}
        onSetAmp={vi.fn(async () => {})}
        onSetGateReduction={vi.fn(async () => {})}
        onSetBypass={vi.fn(async () => {})}
        onSetFxParam={vi.fn(async () => [])}
        state={current}
      />
    </MantineProvider>,
  );
}

describe("NanoChain", () => {
  it("preserves an unsubmitted amp draft across a dashboard refresh", () => {
    const view = renderNano(state);
    const gain = screen.getByLabelText("Gain") as HTMLInputElement;

    fireEvent.change(gain, { target: { value: "121" } });
    expect(gain.value).toBe("121");

    view.rerender(
      <MantineProvider>
        <NanoChain
          onReadFxParams={vi.fn(async () => [])}
          onSetAmp={vi.fn(async () => {})}
          onSetGateReduction={vi.fn(async () => {})}
          onSetBypass={vi.fn(async () => {})}
          onSetFxParam={vi.fn(async () => [])}
          state={{ ...state, amp: { ...state.amp, gain: 99 } }}
        />
      </MantineProvider>,
    );

    expect((screen.getByLabelText("Gain") as HTMLInputElement).value).toBe("121");
  });

  it("opens an FX parameter inspector from the keyboard", async () => {
    const onReadFxParams = vi.fn(async () => [0.5]);
    render(
      <MantineProvider>
        <NanoChain
          onReadFxParams={onReadFxParams}
          onSetAmp={vi.fn(async () => {})}
          onSetGateReduction={vi.fn(async () => {})}
          onSetBypass={vi.fn(async () => {})}
          onSetFxParam={vi.fn(async () => [])}
          state={{
            ...state,
            slots: [{ role: "pre_fx1", loaded_name: null, model_id: 1001, bypassed: false }],
          }}
        />
      </MantineProvider>,
    );

    fireEvent.keyDown(screen.getByRole("button", { name: /Pre FX 1/i }), { key: "Enter" });

    expect(onReadFxParams).toHaveBeenCalledWith("pre_fx1");
    expect(await screen.findByText("Param 0")).toBeTruthy();
    expect(screen.getByRole("slider", { name: "FX parameter 0 normalized value" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Apply FX parameter 0" })).toBeTruthy();
  });

  it("serializes uncorrelated FX reads and applies only the latest selection", async () => {
    const finishReads: ((values: number[]) => void)[] = [];
    const onReadFxParams = vi.fn(() => new Promise<number[]>((resolve) => { finishReads.push(resolve); }));
    render(
      <MantineProvider>
        <NanoChain
          onReadFxParams={onReadFxParams}
          onSetAmp={vi.fn(async () => {})}
          onSetGateReduction={vi.fn(async () => {})}
          onSetBypass={vi.fn(async () => {})}
          onSetFxParam={vi.fn(async () => [])}
          state={{
            ...state,
            slots: [
              { role: "pre_fx1", loaded_name: "Fictional Drive", model_id: 1001, bypassed: false },
              { role: "pre_fx2", loaded_name: "Fictional Chorus", model_id: 1002, bypassed: false },
            ],
          }}
        />
      </MantineProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: /Pre FX 1/i }));
    fireEvent.click(screen.getByRole("button", { name: /Pre FX 2/i }));
    expect(onReadFxParams).toHaveBeenCalledOnce();
    expect(onReadFxParams).toHaveBeenCalledWith("pre_fx1");

    await act(async () => finishReads[0]([0.25]));
    await waitFor(() => expect(onReadFxParams).toHaveBeenCalledTimes(2));
    expect(onReadFxParams).toHaveBeenLastCalledWith("pre_fx2");
    expect(screen.queryByText("FX parameters: Pre FX 1")).toBeNull();

    await act(async () => finishReads[1]([0.75]));
    expect(await screen.findByText("FX parameters: Pre FX 2")).toBeTruthy();
  });

  it("reports an FX write failure even if the inspector closes", async () => {
    let rejectWrite: (reason: Error) => void = () => undefined;
    const onSetFxParam = vi.fn(() => new Promise<number[]>((_resolve, reject) => { rejectWrite = reject; }));
    render(
      <MantineProvider>
        <NanoChain
          onReadFxParams={vi.fn(async () => [0.5])}
          onSetAmp={vi.fn(async () => {})}
          onSetGateReduction={vi.fn(async () => {})}
          onSetBypass={vi.fn(async () => {})}
          onSetFxParam={onSetFxParam}
          state={{
            ...state,
            slots: [{ role: "pre_fx1", loaded_name: "Fictional Drive", model_id: 1001, bypassed: false }],
          }}
        />
      </MantineProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: /Pre FX 1/i }));
    const slider = await screen.findByRole("slider", { name: "FX parameter 0 normalized value" });
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    fireEvent.click(screen.getByRole("button", { name: "Apply FX parameter 0" }));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    await act(async () => rejectWrite(new Error("confirmation failed")));
    expect(await screen.findByText("confirmation failed")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("Nano operation failed");
  });

  it("refreshes device values after an unconfirmed FX write", async () => {
    const onReadFxParams = vi.fn()
      .mockResolvedValueOnce([0.5])
      .mockResolvedValueOnce([0.6]);
    render(
      <MantineProvider>
        <NanoChain
          onReadFxParams={onReadFxParams}
          onSetAmp={vi.fn(async () => {})}
          onSetGateReduction={vi.fn(async () => {})}
          onSetBypass={vi.fn(async () => {})}
          onSetFxParam={vi.fn(async () => { throw new Error("confirmation failed"); })}
          state={{
            ...state,
            slots: [{ role: "pre_fx1", loaded_name: "Fictional Drive", model_id: 1001, bypassed: false }],
          }}
        />
      </MantineProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: /Pre FX 1/i }));
    const slider = await screen.findByRole("slider", { name: "FX parameter 0 normalized value" });
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    fireEvent.click(screen.getByRole("button", { name: "Apply FX parameter 0" }));

    expect(await screen.findByText("confirmation failed")).toBeTruthy();
    expect(onReadFxParams).toHaveBeenCalledTimes(2);
    expect(screen.getByText(/device: 0\.600/)).toBeTruthy();
  });

  it("preserves a newer amp edit made while the submitted value is pending", async () => {
    let finishWrite: () => void = () => undefined;
    const onSetAmp = vi.fn(() => new Promise<void>((resolve) => { finishWrite = resolve; }));
    const props = {
      onReadFxParams: vi.fn(async () => []),
      onSetAmp,
      onSetGateReduction: vi.fn(async () => {}),
      onSetBypass: vi.fn(async () => {}),
      onSetFxParam: vi.fn(async () => []),
    };
    const view = render(<MantineProvider><NanoChain {...props} state={state} /></MantineProvider>);
    const gain = screen.getByLabelText("Gain") as HTMLInputElement;

    fireEvent.change(gain, { target: { value: "121" } });
    const applyGain = screen.getByRole("button", { name: "Apply gain" });
    applyGain.focus();
    fireEvent.click(applyGain);
    expect(document.activeElement).toBe(applyGain);
    expect(applyGain.hasAttribute("disabled")).toBe(false);
    fireEvent.change(gain, { target: { value: "122" } });
    await act(async () => finishWrite());

    view.rerender(<MantineProvider><NanoChain {...props} state={{ ...state, amp: { ...state.amp, gain: 121 } }} /></MantineProvider>);
    expect((screen.getByLabelText("Gain") as HTMLInputElement).value).toBe("122");
  });

  it("names each amp action for its control", () => {
    renderNano({
      ...state,
      slots: [{ role: "pre_fx1", loaded_name: null, model_id: 1001, bypassed: false }],
    });

    expect(screen.getByRole("button", { name: "Apply gain" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Apply treble" })).toBeTruthy();
    expect(screen.getByRole("switch", { name: "Pre FX 1 bypass, on" })).toBeTruthy();
  });

  it("submits Gate reduction as an explicit percentage", () => {
    const onSetGateReduction = vi.fn(async () => {});
    render(
      <MantineProvider>
        <NanoChain
          onReadFxParams={vi.fn(async () => [])}
          onSetAmp={vi.fn(async () => {})}
          onSetGateReduction={onSetGateReduction}
          onSetBypass={vi.fn(async () => {})}
          onSetFxParam={vi.fn(async () => [])}
          state={{ ...state, gate_reduction: 42 }}
        />
      </MantineProvider>,
    );

    fireEvent.change(screen.getByLabelText("Gate reduction"), { target: { value: "43" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply Gate reduction" }));

    expect(onSetGateReduction).toHaveBeenCalledWith(43);
  });
});

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen } from "@testing-library/react";
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
        onSetBypass={vi.fn(async () => {})}
        onSetFxParam={vi.fn(async () => {})}
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
          onSetBypass={vi.fn(async () => {})}
          onSetFxParam={vi.fn(async () => {})}
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
          onSetBypass={vi.fn(async () => {})}
          onSetFxParam={vi.fn(async () => {})}
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

  it("names each amp action for its control", () => {
    renderNano({
      ...state,
      slots: [{ role: "pre_fx1", loaded_name: null, model_id: 1001, bypassed: false }],
    });

    expect(screen.getByRole("button", { name: "Apply gain" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Apply treble" })).toBeTruthy();
    expect(screen.getByRole("switch", { name: "Pre FX 1 bypass, on" })).toBeTruthy();
  });
});

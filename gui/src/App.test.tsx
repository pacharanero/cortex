// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DashboardSnapshot } from "./shared/ipc/types";

const api = vi.hoisted(() => ({
  dashboard: vi.fn(),
  setDevice: vi.fn(),
  reconnectNow: vi.fn(),
  switchScene: vi.fn(),
  recallPreset: vi.fn(),
  blockParameters: vi.fn(),
  setParameter: vi.fn(),
  setSceneLabel: vi.fn(),
  setSceneColor: vi.fn(),
  setBypass: vi.fn(),
  setNanoAmp: vi.fn(),
  setNanoBypass: vi.fn(),
  readNanoFxParams: vi.fn(),
  setNanoFxParam: vi.fn(),
}));

vi.mock("./shared/ipc/api", () => ({ cortexApi: api }));

import { App } from "./App";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function snapshot(device: "quad_cortex" | "nano_cortex"): DashboardSnapshot {
  return {
    source: "daemon",
    status: {
      daemon_version: "fixture",
      uptime_seconds: 1,
      device_kind: device,
      device: { state: "connected", serial: null, coros_version: null, last_message_seconds: 0 },
      cache: {
        generation: device === "quad_cortex" ? 1 : 2,
        revision: 1,
        storage_revision: 1,
        phase: "unsubscribed",
        catalog: false,
        current_preset: false,
        active_scene: false,
        preset_dirty: false,
        preset_location: false,
        listed_setlists: [],
        pushes_applied: 0,
        messages_seen: 0,
        messages_rejected: 0,
        stream_gaps: 0,
        last_rejection: null,
      },
    },
    live: null,
    directory: [],
    nano: device === "nano_cortex" ? {
      firmware: null,
      amp: { gain: null, level: null, bass: null, mid: null, treble: null },
      capture_slot: null,
      capture_volume: null,
      gate_reduction: null,
      footswitch_assignments: null,
      slots: [],
    } : null,
  };
}

function renderApp() {
  return render(<MantineProvider><App /></MantineProvider>);
}

async function chooseDevice(name: "Quad Cortex" | "Nano Cortex") {
  const badge = screen.getByText(/Cortex$/, { selector: ".mantine-Badge-label" }).parentElement!;
  if (badge.getAttribute("aria-expanded") !== "true") fireEvent.click(badge);
  fireEvent.click(await screen.findByRole("menuitem", { name }));
  await waitFor(() => expect(badge.getAttribute("aria-expanded")).toBe("false"));
}

describe("device switching", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("serializes rapid switches so the latest selection wins", async () => {
    const firstSwitch = deferred<void>();
    api.dashboard.mockResolvedValue(snapshot("quad_cortex"));
    api.setDevice
      .mockImplementationOnce(() => firstSwitch.promise)
      .mockResolvedValueOnce(undefined);
    renderApp();
    await screen.findByText("Quad Cortex", { selector: ".mantine-Badge-label" });

    const badge = screen.getByText("Quad Cortex", { selector: ".mantine-Badge-label" }).parentElement!;
    fireEvent.click(badge);
    const nanoItem = await screen.findByRole("menuitem", { name: "Nano Cortex" });
    const quadItem = [...document.querySelectorAll<HTMLElement>("[role=menuitem]")]
      .find((item) => item.textContent?.includes("Quad Cortex"))!;
    fireEvent.click(nanoItem);
    fireEvent.click(quadItem);
    await act(async () => firstSwitch.resolve());

    await waitFor(() => expect(api.setDevice.mock.calls).toEqual([["nano_cortex"], ["quad_cortex"]]));
    expect(screen.getByText("Quad Cortex", { selector: ".mantine-Badge-label" })).toBeTruthy();
  });

  it("does not let an older dashboard poll overwrite a completed switch", async () => {
    const stalePoll = deferred<DashboardSnapshot>();
    api.dashboard
      .mockResolvedValue(snapshot("nano_cortex"))
      .mockResolvedValueOnce(snapshot("quad_cortex"))
      .mockImplementationOnce(() => stalePoll.promise);
    api.setDevice.mockResolvedValue(undefined);
    renderApp();
    await screen.findByText("Quad Cortex", { selector: ".mantine-Badge-label" });
    await waitFor(() => expect(api.dashboard).toHaveBeenCalledTimes(2), { timeout: 2_000 });

    await chooseDevice("Nano Cortex");
    await screen.findByText("Nano Cortex", { selector: ".mantine-Badge-label" });
    await act(async () => stalePoll.resolve(snapshot("quad_cortex")));

    expect(screen.getByText("Nano Cortex", { selector: ".mantine-Badge-label" })).toBeTruthy();
  });
});

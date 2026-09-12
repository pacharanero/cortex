// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CapabilityLabel, DashboardSnapshot } from "./shared/ipc/types";

const api = vi.hoisted(() => ({
  dashboard: vi.fn(),
  capabilities: vi.fn(),
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
  setNanoGateReduction: vi.fn(),
  setNanoBypass: vi.fn(),
  readNanoFxParams: vi.fn(),
  setNanoFxParam: vi.fn(),
}));

vi.mock("./shared/ipc/api", () => ({ cortexApi: api }));

import { App } from "./App";

// Every test mounts `App`, which fetches capabilities once on mount; give it a
// harmless empty default so tests that do not care about evidence labels
// (the vast majority) do not each have to stub it themselves. Runs after each
// describe block's own `vi.resetAllMocks()`, so the default is always back in
// place before the next test's render.
beforeEach(() => {
  api.capabilities.mockResolvedValue([]);
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function snapshot(device: "quad_cortex" | "nano_cortex"): DashboardSnapshot {
  return {
    source: "daemon",
    status: {
      daemon_version: "fixture",
      uptime_seconds: 1,
      auto_managed: false,
      idle_timeout_seconds: null,
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

// Two entries whose old "<setlist> <slot>" delimiter-joined identity collided
// on the same string ("Live Set 1A") despite naming different presets, so a
// space-joined pending identity could not tell them apart.
function directorySnapshot(): DashboardSnapshot {
  const base = snapshot("quad_cortex");
  base.live = {
    generation: 1, revision: 1, storage_revision: 1, preset_name: "Preset One",
    active_scene: 0, active_scene_label: "A", preset_dirty: false, cpu_load: null,
    blocks: [], scenes: [],
  };
  base.directory = [
    { key: "Live Set", name: "Live Set", is_factory: false, slots: [{ index: 0, slot: "1A", name: "Preset One" }] },
    { key: "Live", name: "Live", is_factory: false, slots: [{ index: 0, slot: "Set 1A", name: "Preset Two" }] },
  ];
  return base;
}

async function chooseDevice(name: "Quad Cortex" | "Nano Cortex") {
  const selector = screen.getByRole("button", { name: /Select device, current/ });
  if (selector.getAttribute("aria-expanded") !== "true") fireEvent.click(selector);
  fireEvent.click(await screen.findByRole("menuitem", { name }));
  await waitFor(() => expect(selector.getAttribute("aria-expanded")).toBe("false"));
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
    const selector = await screen.findByRole("button", { name: "Select device, current Quad Cortex" });

    expect(selector.tagName).toBe("BUTTON");
    expect(selector.tabIndex).toBe(0);
    fireEvent.click(selector);
    const nanoItem = await screen.findByRole("menuitem", { name: "Nano Cortex" });
    const quadItem = [...document.querySelectorAll<HTMLElement>("[role=menuitem]")]
      .find((item) => item.textContent?.includes("Quad Cortex"))!;
    fireEvent.click(nanoItem);
    fireEvent.click(quadItem);
    await act(async () => firstSwitch.resolve());

    await waitFor(() => expect(api.setDevice.mock.calls).toEqual([["nano_cortex"], ["quad_cortex"]]));
    expect(screen.getByRole("button", { name: "Select device, current Quad Cortex" })).toBeTruthy();
  });

  it("does not let an older dashboard poll overwrite a completed switch", async () => {
    const stalePoll = deferred<DashboardSnapshot>();
    api.dashboard
      .mockResolvedValue(snapshot("nano_cortex"))
      .mockResolvedValueOnce(snapshot("quad_cortex"))
      .mockImplementationOnce(() => stalePoll.promise);
    api.setDevice.mockResolvedValue(undefined);
    renderApp();
    await screen.findByRole("button", { name: "Select device, current Quad Cortex" });
    await waitFor(() => expect(api.dashboard).toHaveBeenCalledTimes(2), { timeout: 2_000 });

    await chooseDevice("Nano Cortex");
    await screen.findByRole("button", { name: "Select device, current Nano Cortex" });
    await act(async () => stalePoll.resolve(snapshot("quad_cortex")));

    expect(screen.getByRole("button", { name: "Select device, current Nano Cortex" })).toBeTruthy();
  });

  it("does not let an action started during a switch restore the old dashboard", async () => {
    const setDevice = deferred<void>();
    const staleRecallRead = deferred<DashboardSnapshot>();
    api.dashboard
      .mockResolvedValue(snapshot("nano_cortex"))
      .mockResolvedValueOnce(directorySnapshot())
      .mockImplementationOnce(() => staleRecallRead.promise);
    api.setDevice.mockReturnValue(setDevice.promise);
    api.recallPreset.mockResolvedValue(undefined);
    renderApp();
    await screen.findByRole("button", { name: "Select device, current Quad Cortex" });

    await chooseDevice("Nano Cortex");
    fireEvent.click(screen.getByRole("button", { name: "1A Preset One" }));
    await waitFor(() => expect(api.dashboard).toHaveBeenCalledTimes(2));

    await act(async () => setDevice.resolve());
    await screen.findByRole("button", { name: "Select device, current Nano Cortex" });

    await act(async () => staleRecallRead.resolve(directorySnapshot()));
    expect(screen.getByRole("button", { name: "Select device, current Nano Cortex" })).toBeTruthy();
  });

  it("keeps Nano identity and exposes its failure when state is unavailable", async () => {
    const unavailable = snapshot("nano_cortex");
    unavailable.nano = null;
    unavailable.status.device = { state: "failed", error: "Nano Cortex editor channel is owned by another transport" };
    api.dashboard.mockResolvedValue(unavailable);

    renderApp();

    expect(await screen.findByRole("button", { name: "Select device, current Nano Cortex" })).toBeTruthy();
    expect(screen.getByText("Nano Cortex editor channel is owned by another transport")).toBeTruthy();
  });

  it("keeps a Nano write failure visible across a generation remount", async () => {
    const initial = snapshot("nano_cortex");
    initial.nano!.amp.gain = 10;
    const recovered = snapshot("nano_cortex");
    recovered.status.cache.generation = 3;
    recovered.nano!.amp.gain = 10;
    api.dashboard.mockResolvedValue(recovered).mockResolvedValueOnce(initial);
    api.setNanoAmp.mockRejectedValue(new Error("write outcome was not confirmed"));
    renderApp();

    fireEvent.click(await screen.findByRole("button", { name: "Apply gain" }));
    await screen.findAllByText("write outcome was not confirmed");
    await waitFor(() => expect(api.dashboard.mock.calls.length).toBeGreaterThan(1), { timeout: 2_000 });
    await waitFor(() => expect(screen.getAllByText("write outcome was not confirmed")).toHaveLength(1));
  });

  it("does not report a confirmed Nano write as failed when the follow-up dashboard refresh fails", async () => {
    const initial = snapshot("nano_cortex");
    initial.nano!.amp.gain = 10;
    api.dashboard
      .mockResolvedValue(initial)
      .mockResolvedValueOnce(initial)
      .mockRejectedValueOnce(new Error("secondary dashboard refresh failed"));
    api.setNanoAmp.mockResolvedValue(undefined);
    renderApp();

    fireEvent.click(await screen.findByRole("button", { name: "Apply gain" }));

    expect(await screen.findByText("gain applied.")).toBeTruthy();
    expect(screen.queryByText("secondary dashboard refresh failed")).toBeNull();
  });
});

describe("preset recall pending state", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("marks only the invoked slot as Recalling and clears it once the recall resolves", async () => {
    const recall = deferred<void>();
    api.dashboard.mockResolvedValue(directorySnapshot());
    api.recallPreset.mockReturnValue(recall.promise);
    renderApp();

    const invoked = await screen.findByRole("button", { name: "1A Preset One" });
    const other = screen.getByRole("button", { name: "Set 1A Preset Two" });
    expect(invoked.tabIndex).toBe(0);
    fireEvent.click(invoked);

    await screen.findByText("Recalling...");
    // The other setlist's slot must not also read pending, even though its
    // space-joined "<setlist> <slot>" identity ("Live Set 1A") collides with
    // the invoked one.
    expect(screen.getAllByText("Recalling...")).toHaveLength(1);
    expect(invoked.textContent).toContain("Recalling...");
    expect(other.textContent).not.toContain("Recalling...");
    expect(api.recallPreset).toHaveBeenCalledWith("Live Set", "1A");
    expect(api.recallPreset).not.toHaveBeenCalledWith("Live", "Set 1A");

    await act(async () => recall.resolve());

    await waitFor(() => expect(screen.queryByText("Recalling...")).toBeNull());
  });

  it("distinguishes two slots and only shows the second slot's own invocation as pending", async () => {
    const recall = deferred<void>();
    api.dashboard.mockResolvedValue(directorySnapshot());
    api.recallPreset.mockReturnValue(recall.promise);
    renderApp();

    const other = await screen.findByRole("button", { name: "1A Preset One" });
    const invoked = screen.getByRole("button", { name: "Set 1A Preset Two" });
    fireEvent.click(invoked);

    await screen.findByText("Recalling...");
    expect(screen.getAllByText("Recalling...")).toHaveLength(1);
    expect(invoked.textContent).toContain("Recalling...");
    expect(other.textContent).not.toContain("Recalling...");
    expect(api.recallPreset).toHaveBeenCalledWith("Live", "Set 1A");

    await act(async () => recall.resolve());
    await waitFor(() => expect(screen.queryByText("Recalling...")).toBeNull());
  });

  it("clears the pending indicator and surfaces the error when the recall rejects", async () => {
    const recall = deferred<void>();
    api.dashboard.mockResolvedValue(directorySnapshot());
    api.recallPreset.mockReturnValue(recall.promise);
    renderApp();

    fireEvent.click(await screen.findByText("1A Preset One"));
    await screen.findByText("Recalling...");

    await act(async () => {
      recall.reject(new Error("recall failed"));
      await recall.promise.catch(() => {});
    });

    await waitFor(() => expect(screen.queryByText("Recalling...")).toBeNull());
    expect(await screen.findByText("recall failed")).toBeTruthy();
  });
});

// GUI-003.1 (Night: slice): a local, already-loaded filter over the
// directory - no daemon call, no persistence, no favourites. Uses its own
// fixture rather than `directorySnapshot()` so query strings that
// deliberately overlap another test's collision-tolerant identities cannot
// also start matching multiple slots here by accident.
function searchDirectorySnapshot(): DashboardSnapshot {
  const base = snapshot("quad_cortex");
  base.directory = [
    {
      key: "Main", name: "Main", is_factory: false,
      slots: [
        { index: 0, slot: "1A", name: "Clean Tone" },
        { index: 1, slot: "1B", name: "Crunch Rock" },
      ],
    },
    {
      key: "Backup", name: "Backup", is_factory: false,
      slots: [{ index: 8, slot: "2A", name: "Ambient Pad" }],
    },
  ];
  return base;
}

describe("preset directory search", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("filters by preset name, case-insensitively, and omits empty setlist groups", async () => {
    api.dashboard.mockResolvedValue(searchDirectorySnapshot());
    renderApp();

    await screen.findByRole("button", { name: "1A Clean Tone" });
    fireEvent.change(screen.getByLabelText("Search presets by name or slot"), { target: { value: "CLEAN" } });

    expect(screen.getByRole("button", { name: "1A Clean Tone" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "1B Crunch Rock" })).toBeNull();
    expect(screen.queryByText("Backup")).toBeNull();
  });

  it("filters by displayed slot", async () => {
    api.dashboard.mockResolvedValue(searchDirectorySnapshot());
    renderApp();

    await screen.findByRole("button", { name: "2A Ambient Pad" });
    fireEvent.change(screen.getByLabelText("Search presets by name or slot"), { target: { value: "2a" } });

    expect(screen.getByRole("button", { name: "2A Ambient Pad" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "1A Clean Tone" })).toBeNull();
    expect(screen.queryByRole("button", { name: "1B Crunch Rock" })).toBeNull();
    expect(screen.queryByText("Main")).toBeNull();
  });

  it("shows a distinct no-match message rather than the unavailable-directory message", async () => {
    api.dashboard.mockResolvedValue(searchDirectorySnapshot());
    renderApp();

    await screen.findByRole("button", { name: "1A Clean Tone" });
    fireEvent.change(screen.getByLabelText("Search presets by name or slot"), { target: { value: "zzz-no-such-preset" } });

    expect(await screen.findByText("No presets match.")).toBeTruthy();
    expect(screen.queryByText("Unavailable for this session generation.")).toBeNull();
    expect(screen.queryByRole("button", { name: "1A Clean Tone" })).toBeNull();
  });

  it("restores the complete directory when the search is cleared", async () => {
    api.dashboard.mockResolvedValue(searchDirectorySnapshot());
    renderApp();

    const search = await screen.findByLabelText("Search presets by name or slot");
    fireEvent.change(search, { target: { value: "clean" } });
    expect(screen.queryByRole("button", { name: "1B Crunch Rock" })).toBeNull();

    fireEvent.change(search, { target: { value: "" } });
    expect(await screen.findByRole("button", { name: "1B Crunch Rock" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "2A Ambient Pad" })).toBeTruthy();
  });

  it("preserves the exact setlist key and slot for recall while filtered", async () => {
    api.dashboard.mockResolvedValue(directorySnapshot());
    api.recallPreset.mockResolvedValue(undefined);
    renderApp();

    await screen.findByRole("button", { name: "1A Preset One" });
    fireEvent.change(screen.getByLabelText("Search presets by name or slot"), { target: { value: "preset two" } });

    fireEvent.click(await screen.findByRole("button", { name: "Set 1A Preset Two" }));
    await waitFor(() => expect(api.recallPreset).toHaveBeenCalledWith("Live", "Set 1A"));
  });
});

// GUI-001.9: the poll and a command's own read-back both call
// `cortexApi.dashboard()` independently, so their replies can settle in a
// different order than they were issued in. These prove a read issued before
// one that has already won cannot overwrite it later, whether it eventually
// succeeds or fails.
// GUI-004.2: the frontend renders exactly what `cortexApi.capabilities()`
// returns and holds no independent opinion of its own about which operations
// are confirmed - these test that end to end through two always-rendered
// evidence labels (the sidebar's `recall_preset` badge and the scene
// selector's `switch_scene` badge), rather than duplicating the Rust-side
// seed-content assertions already covered in `capability.rs`.
describe("capability evidence labels", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("renders each operation's fetched status as text", async () => {
    const labels: CapabilityLabel[] = [
      { operation: "recall_preset", status: "confirmed-writable" },
      { operation: "switch_scene", status: "unverified" },
    ];
    api.dashboard.mockResolvedValue(directorySnapshot());
    api.capabilities.mockResolvedValue(labels);
    renderApp();

    await screen.findByText("Preset One");
    expect(await screen.findByText("Preset recall: Hardware-verified")).toBeTruthy();
    expect(screen.getByText("Scene switching: Not yet hardware-verified")).toBeTruthy();
  });

  it("renders every operation as not-yet-hardware-verified when the fetch fails", async () => {
    api.dashboard.mockResolvedValue(directorySnapshot());
    api.capabilities.mockRejectedValue(new Error("capability fetch failed"));
    renderApp();

    await screen.findByText("Preset One");
    // recall_preset and switch_scene both fall back to unverified, and
    // nothing else surfaces a status label in this snapshot (no block or
    // scene is selected, and no Nano state is present).
    expect(await screen.findAllByText(/Not yet hardware-verified$/)).toHaveLength(2);
  });
});

describe("dashboard read ordering", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("does not let a stale poll reply overwrite a newer recall read-back", async () => {
    const stalePoll = deferred<DashboardSnapshot>();
    const initial = directorySnapshot();
    const recalled = directorySnapshot();
    recalled.live!.preset_name = "Recalled Preset";
    api.dashboard
      .mockResolvedValue(initial)
      .mockResolvedValueOnce(initial)
      .mockImplementationOnce(() => stalePoll.promise)
      .mockResolvedValueOnce(recalled);
    api.recallPreset.mockResolvedValue(undefined);
    renderApp();

    await screen.findByText("Preset One");
    // Let the poll issue its second, stalled request before the recall issues
    // a third, so the third is the newer of the two in flight.
    await waitFor(() => expect(api.dashboard).toHaveBeenCalledTimes(2), { timeout: 2_000 });

    fireEvent.click(screen.getByRole("button", { name: "1A Preset One" }));
    await screen.findByText("Recalled Preset");
    expect(api.dashboard).toHaveBeenCalledTimes(3);

    await act(async () => stalePoll.resolve(initial));

    expect(screen.getByText("Recalled Preset")).toBeTruthy();
  });

  it("does not surface a stale poll failure once a newer recall read-back has already succeeded", async () => {
    const stalePoll = deferred<DashboardSnapshot>();
    const initial = directorySnapshot();
    const recalled = directorySnapshot();
    recalled.live!.preset_name = "Recalled Preset";
    api.dashboard
      .mockResolvedValue(initial)
      .mockResolvedValueOnce(initial)
      .mockImplementationOnce(() => stalePoll.promise)
      .mockResolvedValueOnce(recalled);
    api.recallPreset.mockResolvedValue(undefined);
    renderApp();

    await screen.findByText("Preset One");
    await waitFor(() => expect(api.dashboard).toHaveBeenCalledTimes(2), { timeout: 2_000 });

    fireEvent.click(screen.getByRole("button", { name: "1A Preset One" }));
    await screen.findByText("Recalled Preset");

    await act(async () => {
      stalePoll.reject(new Error("stale poll failure"));
      await stalePoll.promise.catch(() => {});
    });

    expect(screen.getByText("Recalled Preset")).toBeTruthy();
    expect(screen.queryByText("stale poll failure")).toBeNull();
  });
});

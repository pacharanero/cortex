// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";

const { LogicalSize, getCurrentWindow, setSize } = vi.hoisted(() => {
  const setSize = vi.fn();
  return {
    LogicalSize: class LogicalSize {
      constructor(readonly width: number, readonly height: number) {}
    },
    getCurrentWindow: vi.fn(() => ({ setSize })),
    setSize,
  };
});

vi.mock("@tauri-apps/api/dpi", () => ({ LogicalSize }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow }));

import { applyWindowSize, windowSizePresets } from "./windowSizing";

describe("window sizing", () => {
  it("uses logical 16:10 presets", () => {
    expect(windowSizePresets).toEqual([
      { label: "75% (1080 x 675)", width: 1080, height: 675 },
      { label: "100% (1440 x 900)", width: 1440, height: 900 },
      { label: "125% (1800 x 1125)", width: 1800, height: 1125 },
      { label: "150% (2160 x 1350)", width: 2160, height: 1350 },
      { label: "200% (2880 x 1800)", width: 2880, height: 1800 },
    ]);
  });

  it("resizes only the current native window", async () => {
    await applyWindowSize(windowSizePresets[1]);

    expect(getCurrentWindow).toHaveBeenCalledOnce();
    expect(setSize).toHaveBeenCalledWith(expect.objectContaining({ width: 1440, height: 900 }));
  });
});

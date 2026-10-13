// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import type { LiveBlock } from "../../shared/ipc/types";
import { diffSingleChangedCell } from "./liveBlockDiff";

function block(overrides: Partial<LiveBlock>): LiveBlock {
  return {
    row: 0,
    screen_row: 1,
    column: 0,
    model_id: 1,
    name: "Fictional Drive",
    category: "Drive",
    based_on: null,
    bypassed: false,
    params: [],
    family: "drive",
    ...overrides,
  };
}

describe("diffSingleChangedCell", () => {
  it("returns null for identical snapshots", () => {
    const blocks = [block({ row: 1, column: 2 })];
    expect(diffSingleChangedCell(blocks, blocks)).toBeNull();
  });

  it("returns null for two structurally equal but distinct arrays", () => {
    const a = [block({ row: 1, column: 2 })];
    const b = [block({ row: 1, column: 2 })];
    expect(diffSingleChangedCell(a, b)).toBeNull();
  });

  it("finds the one cell whose occupying block changed", () => {
    const previous = [block({ row: 0, column: 0, name: "Fictional Drive" }), block({ row: 1, column: 3, name: "Fictional Delay", family: "delay" })];
    const next = [block({ row: 0, column: 0, name: "Fictional Drive" }), block({ row: 1, column: 3, name: "Fictional Chorus", family: "modulation" })];
    expect(diffSingleChangedCell(previous, next)).toEqual({ row: 1, column: 3 });
  });

  it("finds a cell that newly became occupied", () => {
    const previous = [block({ row: 0, column: 0 })];
    const next = [block({ row: 0, column: 0 }), block({ row: 2, column: 5, name: "Fictional Reverb", family: "reverb" })];
    expect(diffSingleChangedCell(previous, next)).toEqual({ row: 2, column: 5 });
  });

  it("finds a cell that newly became empty", () => {
    const previous = [block({ row: 0, column: 0 }), block({ row: 2, column: 5 })];
    const next = [block({ row: 0, column: 0 })];
    expect(diffSingleChangedCell(previous, next)).toEqual({ row: 2, column: 5 });
  });

  it("returns null when two cells change at once rather than guessing one", () => {
    const previous = [block({ row: 0, column: 0, name: "Fictional Drive" }), block({ row: 1, column: 1, name: "Fictional Delay" })];
    const next = [block({ row: 0, column: 0, name: "Fictional Chorus" }), block({ row: 1, column: 1, name: "Fictional Reverb" })];
    expect(diffSingleChangedCell(previous, next)).toBeNull();
  });

  it("returns null for an empty grid compared to itself", () => {
    expect(diffSingleChangedCell([], [])).toBeNull();
  });
});

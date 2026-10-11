// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { LiveBlock } from "../../shared/ipc/types";

export interface GridCell {
  row: number;
  column: number;
}

function cellKey(row: number, column: number): string {
  return `${row},${column}`;
}

/**
 * The exact single grid cell that differs between two block snapshots, or
 * `null` if zero or more than one cell differs.
 *
 * Pure and position-keyed: an occupied and an empty cell at the same
 * `(row, column)` compare unequal just like two different occupied blocks
 * do, so a block appearing, disappearing, or having any reported field
 * change at one position all count as that one cell changing. Comparing
 * exactly one changed cell is the whole contract - two or more changed
 * cells collapse to `null` rather than picking one arbitrarily, because
 * guessing a single target among several unrelated changes is unsafe
 * (GUI-003.6). Callers decide separately whether the two snapshots being
 * compared are even eligible to diff (same generation, strictly newer
 * revision); this helper only compares the block arrays it is given.
 */
export function diffSingleChangedCell(previous: LiveBlock[], next: LiveBlock[]): GridCell | null {
  const previousByCell = new Map(previous.map((block) => [cellKey(block.row, block.column), block]));
  const nextByCell = new Map(next.map((block) => [cellKey(block.row, block.column), block]));
  const keys = new Set([...previousByCell.keys(), ...nextByCell.keys()]);
  const changedKeys = [...keys].filter((key) => JSON.stringify(previousByCell.get(key)) !== JSON.stringify(nextByCell.get(key)));
  if (changedKeys.length !== 1) return null;
  const [row, column] = changedKeys[0].split(",").map(Number);
  return { row, column };
}

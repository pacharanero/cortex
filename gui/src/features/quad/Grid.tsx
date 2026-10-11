// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Text } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import type { LiveBlock } from "../../shared/ipc/types";
import { EditorBlockCard, EditorCanvas } from "../../shared/editor/EditorCanvas";
import { diffSingleChangedCell, type GridCell } from "./liveBlockDiff";

interface GridProps {
  blocks: LiveBlock[];
  selected: LiveBlock | null;
  onSelect: (block: LiveBlock) => void;
  /** Physical-session generation these blocks were read under (`LiveSnapshot.generation`). */
  generation: number;
  /** Monotonic state revision within that generation (`LiveSnapshot.revision`). */
  revision: number;
}

/**
 * Indicate, without stealing focus or selection, that the device's own latest
 * report changed exactly one grid cell since its previous report in the same
 * session (GUI-003.6).
 *
 * The device-reported snapshot is the sole source of truth: this only reads
 * `blocks`/`generation`/`revision` as given and never fetches parameters,
 * changes `selected`, or moves focus. The initial snapshot, a generation
 * replacement (a reconnect or device switch), an unchanged or stale report,
 * and a report that changed more than one cell all become the new baseline
 * silently, with no cell-specific indication - guessing a single changed
 * target among several unrelated changes is unsafe, and nothing here is
 * labelled "external" because the GUI cannot tell its own write apart from
 * one made by another client.
 */
export function Grid({ blocks, selected, onSelect, generation, revision }: GridProps) {
  const [changedCell, setChangedCell] = useState<GridCell | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const lastReport = useRef<{ generation: number; revision: number; blocks: LiveBlock[] } | null>(null);

  useEffect(() => {
    const previous = lastReport.current;
    lastReport.current = { generation, revision, blocks };
    if (!previous || previous.generation !== generation || revision <= previous.revision) {
      setChangedCell(null);
      return;
    }
    const cell = diffSingleChangedCell(previous.blocks, blocks);
    setChangedCell(cell);
    if (cell) {
      const occupant = blocks.find((candidate) => candidate.row === cell.row && candidate.column === cell.column);
      // Matches `positionLabel` below: row is shown 1-based, column 0-based,
      // exactly as every other on-screen grid position in this component.
      setAnnouncement(`Row ${cell.row + 1}, column ${cell.column} changed${occupant ? ` to ${occupant.name}` : ", now empty"}.`);
    }
    // Deliberately no focus change and no selection change: the device-reported
    // snapshot is the sole source of truth and must not steal focus from
    // whatever control the user is actually driving.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [generation, revision, blocks]);

  return (
    <>
      <EditorCanvas label="Quad Cortex signal grid" topology="quad-grid">
        {Array.from({ length: 32 }, (_, index) => {
          const row = Math.floor(index / 8);
          const column = index % 8;
          const block = blocks.find((candidate) => candidate.row === row && candidate.column === column);
          return (
            <EditorBlockCard
              changed={changedCell?.row === row && changedCell?.column === column}
              detail={block ? `Model ${block.model_id}` : undefined}
              eyebrow={block?.category ?? "Available position"}
              family={block?.family}
              inspectorId="quad-block-inspector"
              key={`${row}-${column}`}
              onSelect={block ? () => onSelect(block) : undefined}
              positionLabel={`Row ${row + 1}, column ${column}`}
              selected={selected?.row === row && selected?.column === column}
              state={block ? block.bypassed ? "bypassed" : "engaged" : "empty"}
              title={block?.name ?? "Empty cell"}
            />
          );
        })}
      </EditorCanvas>
      {/* No visual equivalent of its own: the changed cell's border already
          shows sightedly, so this exists purely for a screen reader. */}
      <Text aria-live="polite" className="visually-hidden" role="status">{announcement}</Text>
    </>
  );
}

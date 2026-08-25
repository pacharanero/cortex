// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { LiveBlock } from "../../shared/ipc/types";
import { EditorBlockCard, EditorCanvas } from "../../shared/editor/EditorCanvas";

interface GridProps {
  blocks: LiveBlock[];
  selected: LiveBlock | null;
  onSelect: (block: LiveBlock) => void;
}

export function Grid({ blocks, selected, onSelect }: GridProps) {
  return (
    <EditorCanvas label="Quad Cortex signal grid" topology="quad-grid">
      {Array.from({ length: 32 }, (_, index) => {
        const row = Math.floor(index / 8);
        const column = index % 8;
        const block = blocks.find((candidate) => candidate.row === row && candidate.column === column);
        return (
          <EditorBlockCard
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
  );
}

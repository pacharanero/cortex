// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Badge, Button, Paper, Stack, Text } from "@mantine/core";
import type { GridBlock } from "../../shared/ipc/cortex";

interface GridProps {
  blocks: GridBlock[];
  selected: GridBlock | null;
  onSelect: (block: GridBlock) => void;
}

export function Grid({ blocks, selected, onSelect }: GridProps) {
  return (
    <div className="grid" aria-label="Quad Cortex signal grid">
      {Array.from({ length: 32 }, (_, index) => {
        const row = Math.floor(index / 8);
        const column = index % 8;
        const block = blocks.find((candidate) => candidate.row === row && candidate.column === column);
        return (
          <Button
            aria-pressed={selected?.row === row && selected?.column === column}
            className="grid-cell"
            color={block?.bypassed ? "gray" : block ? "orange" : "dark"}
            disabled={!block}
            key={`${row}-${column}`}
            onClick={() => block && onSelect(block)}
            variant={block ? "filled" : "subtle"}
          >
            {block ? <Stack gap={2}><Text size="xs">{block.name}</Text><Badge color="dark" size="xs">{block.category}</Badge></Stack> : ""}
          </Button>
        );
      })}
    </div>
  );
}

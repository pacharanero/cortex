// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Badge, Button, Paper, Stack, Text } from "@mantine/core";
import type { LiveBlock } from "../../shared/ipc/types";

interface GridProps {
  blocks: LiveBlock[];
  selected: LiveBlock | null;
  onSelect: (block: LiveBlock) => void;
}

export function Grid({ blocks, selected, onSelect }: GridProps) {
  return (
    <div className="grid" aria-label="Quad Cortex signal grid" role="group">
      {Array.from({ length: 32 }, (_, index) => {
        const row = Math.floor(index / 8);
        const column = index % 8;
        const block = blocks.find((candidate) => candidate.row === row && candidate.column === column);
        return (
          <Button
            aria-label={block
              ? `${block.name}, ${block.category}, row ${block.screen_row}, column ${block.column}, ${block.bypassed ? "bypassed" : "engaged"}`
              : `Empty, row ${row + 1}, column ${column}`}
            aria-pressed={selected?.row === row && selected?.column === column}
            className="grid-cell"
            color="dark"
            // The family drives the border colour in CSS; the name and category
            // stay as text so the colour is never the only thing carrying it.
            data-bypassed={block?.bypassed ? "true" : undefined}
            data-family={block?.family}
            disabled={!block}
            key={`${row}-${column}`}
            onClick={() => block && onSelect(block)}
            variant={block ? "filled" : "subtle"}
          >
            {block ? (
              <Stack gap={2}>
                <Text size="xs">{block.name}</Text>
                <Badge color="dark" size="xs">{block.category}</Badge>
                {block.bypassed && <Text c="dimmed" size="xs">bypassed</Text>}
              </Stack>
            ) : ""}
          </Button>
        );
      })}
    </div>
  );
}

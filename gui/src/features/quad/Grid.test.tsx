// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LiveBlock } from "../../shared/ipc/types";
import { Grid } from "./Grid";

const block: LiveBlock = {
  row: 2,
  screen_row: 3,
  column: 5,
  model_id: 42,
  name: "Fictional Delay",
  category: "Delay",
  based_on: null,
  bypassed: true,
  params: [],
  family: "delay",
};

function otherBlock(overrides: Partial<LiveBlock>): LiveBlock {
  return { ...block, row: 0, screen_row: 1, column: 0, name: "Fictional Drive", family: "drive", bypassed: false, ...overrides };
}

describe("Grid", () => {
  it("preserves all 32 positions and selects the reported wire coordinate", () => {
    const select = vi.fn();
    const { container } = render(<MantineProvider>
      <Grid blocks={[block]} generation={1} onSelect={select} revision={1} selected={block} />
    </MantineProvider>);

    expect(container.querySelectorAll(".editor-block-card")).toHaveLength(32);
    const card = screen.getByRole("button", { name: "Row 3, column 5: Fictional Delay, Delay, bypassed" });
    expect(card.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(card);
    expect(select).toHaveBeenCalledWith(block);
  });

  // GUI-003.6: a pure, polite single-cell change indication, gated to a
  // strictly newer report in the same generation that changes exactly one
  // cell. Every guarded case renders no "| changed" state text anywhere.
  describe("snapshot-diff indication (GUI-003.6)", () => {
    function renderGrid(blocks: LiveBlock[], generation: number, revision: number, select = vi.fn()) {
      return render(<MantineProvider>
        <button type="button">unrelated control</button>
        <Grid blocks={blocks} generation={generation} onSelect={select} revision={revision} selected={null} />
      </MantineProvider>);
    }

    it("shows no indication on the initial snapshot", () => {
      renderGrid([otherBlock({})], 1, 1);
      expect(document.querySelector('[data-changed="true"]')).toBeNull();
    });

    it("shows no indication across a generation replacement, even if blocks differ", () => {
      const { rerender } = renderGrid([otherBlock({ name: "Fictional Drive" })], 1, 1);
      rerender(<MantineProvider>
        <button type="button">unrelated control</button>
        <Grid blocks={[otherBlock({ name: "Fictional Chorus", family: "modulation" })]} generation={2} onSelect={vi.fn()} revision={1} selected={null} />
      </MantineProvider>);
      expect(document.querySelector('[data-changed="true"]')).toBeNull();
    });

    it("shows no indication when the grid is unchanged", () => {
      const { rerender } = renderGrid([otherBlock({})], 1, 1);
      rerender(<MantineProvider>
        <button type="button">unrelated control</button>
        <Grid blocks={[otherBlock({})]} generation={1} onSelect={vi.fn()} revision={2} selected={null} />
      </MantineProvider>);
      expect(document.querySelector('[data-changed="true"]')).toBeNull();
    });

    it("shows no indication for a stale or replayed report at or below the stored revision", () => {
      const { rerender } = renderGrid([otherBlock({ name: "Fictional Drive" })], 1, 2);
      rerender(<MantineProvider>
        <button type="button">unrelated control</button>
        <Grid blocks={[otherBlock({ name: "Fictional Chorus", family: "modulation" })]} generation={1} onSelect={vi.fn()} revision={2} selected={null} />
      </MantineProvider>);
      expect(document.querySelector('[data-changed="true"]')).toBeNull();
    });

    it("shows no indication when more than one cell changes at once", () => {
      const { rerender } = renderGrid([otherBlock({ row: 0, column: 0 }), otherBlock({ row: 1, column: 1, name: "Fictional Delay" })], 1, 1);
      rerender(<MantineProvider>
        <button type="button">unrelated control</button>
        <Grid
          blocks={[otherBlock({ row: 0, column: 0, name: "Fictional Chorus" }), otherBlock({ row: 1, column: 1, name: "Fictional Reverb" })]}
          generation={1}
          onSelect={vi.fn()}
          revision={2}
          selected={null}
        />
      </MantineProvider>);
      expect(document.querySelector('[data-changed="true"]')).toBeNull();
    });

    it("indicates the single changed cell, announces it politely, and leaves focus on an unrelated control", () => {
      const { rerender } = renderGrid([otherBlock({ row: 0, column: 0, name: "Fictional Drive" })], 1, 1);
      const unrelated = screen.getByRole("button", { name: "unrelated control" });
      unrelated.focus();
      expect(document.activeElement).toBe(unrelated);

      rerender(<MantineProvider>
        <button type="button">unrelated control</button>
        <Grid blocks={[otherBlock({ row: 0, column: 0, name: "Fictional Chorus", family: "modulation" })]} generation={1} onSelect={vi.fn()} revision={2} selected={null} />
      </MantineProvider>);

      const changedCard = document.querySelector('[data-changed="true"]');
      expect(changedCard).not.toBeNull();
      expect(changedCard?.getAttribute("aria-label")).toBe("Row 1, column 0: Fictional Chorus, Delay, engaged, changed");
      expect(screen.getByRole("status").textContent).toBe("Row 1, column 0 changed to Fictional Chorus.");
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "unrelated control" }));
    });

    it("clears a previous indication once a later unchanged report arrives", () => {
      const { rerender } = renderGrid([otherBlock({ row: 0, column: 0, name: "Fictional Drive" })], 1, 1);
      const changedBlocks = [otherBlock({ row: 0, column: 0, name: "Fictional Chorus", family: "modulation" })];
      rerender(<MantineProvider>
        <button type="button">unrelated control</button>
        <Grid blocks={changedBlocks} generation={1} onSelect={vi.fn()} revision={2} selected={null} />
      </MantineProvider>);
      expect(document.querySelector('[data-changed="true"]')).not.toBeNull();

      rerender(<MantineProvider>
        <button type="button">unrelated control</button>
        <Grid blocks={changedBlocks} generation={1} onSelect={vi.fn()} revision={3} selected={null} />
      </MantineProvider>);
      expect(document.querySelector('[data-changed="true"]')).toBeNull();
      expect(screen.getByRole("status").textContent).toBe("");
    });
  });
});

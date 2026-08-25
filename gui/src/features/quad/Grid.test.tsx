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

describe("Grid", () => {
  it("preserves all 32 positions and selects the reported wire coordinate", () => {
    const select = vi.fn();
    const { container } = render(<MantineProvider>
      <Grid blocks={[block]} onSelect={select} selected={block} />
    </MantineProvider>);

    expect(container.querySelectorAll(".editor-block-card")).toHaveLength(32);
    const card = screen.getByRole("button", { name: "Row 3, column 5: Fictional Delay, Delay, bypassed" });
    expect(card.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(card);
    expect(select).toHaveBeenCalledWith(block);
  });
});

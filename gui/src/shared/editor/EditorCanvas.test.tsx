// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { EditorBlockCard, EditorCanvas, InspectorPanel } from "./EditorCanvas";

describe("EditorCanvas", () => {
  it("keeps topology explicit and exposes block position, role, and state", () => {
    const select = vi.fn();
    render(<MantineProvider>
      <EditorCanvas label="Nano Cortex fixed signal chain" topology="nano-chain">
        <EditorBlockCard
          eyebrow="Pre FX 1"
          inspectorId="inspector"
          onSelect={select}
          positionLabel="Position 2"
          selected
          state="bypassed"
          title="Fictional Drive"
        />
      </EditorCanvas>
      <InspectorPanel id="inspector" title="Fictional Drive" />
    </MantineProvider>);

    const canvas = screen.getByLabelText("Nano Cortex fixed signal chain");
    expect(canvas.getAttribute("data-topology")).toBe("nano-chain");
    const card = screen.getByRole("button", { name: "Position 2: Fictional Drive, Pre FX 1, bypassed" });
    expect(card.getAttribute("aria-pressed")).toBe("true");
    expect(card.getAttribute("aria-controls")).toBe("inspector");
    expect(screen.getByText("bypassed | selected")).toBeDefined();
    fireEvent.click(card);
    expect(select).toHaveBeenCalledOnce();
  });

  it("renders an empty Quad position as content rather than a disabled action", () => {
    render(<MantineProvider>
      <EditorCanvas label="Quad Cortex signal grid" topology="quad-grid">
        <EditorBlockCard
          eyebrow="Available position"
          inspectorId="inspector"
          positionLabel="Row 1, column 2"
          selected={false}
          state="empty"
          title="Empty cell"
        />
      </EditorCanvas>
    </MantineProvider>);

    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByRole("group", { name: "Row 1, column 2: Empty cell, Available position, empty" })).toBeDefined();
    expect(screen.getByText("Row 1, column 2")).toBeDefined();
    expect(screen.getByText("Empty cell")).toBeDefined();
    expect(screen.getByText("Available position")).toBeDefined();
  });
});

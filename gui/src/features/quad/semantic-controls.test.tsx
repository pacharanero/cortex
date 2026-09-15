// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LiveBlock, ParameterView } from "../../shared/ipc/types";
import { Grid } from "./Grid";
import { ParameterEditor } from "./ParameterEditor";

const parameter = (overrides: Partial<ParameterView>): ParameterView => {
  const view = {
    index: 0,
    name: "GAIN",
    kind: "float" as const,
    units: "dB",
    min: 0,
    max: 10,
    normalised: 0.5,
    real: 5,
    text: null,
    step_names: [],
    read_only: false,
    per_scene: false,
    ...overrides,
  };
  return {
    ...view,
    identity: overrides.identity ?? {
      catalog_generation: 3,
      catalog_revision: 17,
      model_id: 5007,
      index: view.index,
      name: view.name,
      catalog_descriptor: "test descriptor",
    },
  };
};

describe("Quad semantic controls", () => {
  it("names parameter inputs with their action and units", () => {
    render(
      <MantineProvider>
        <ParameterEditor
          disabled={false}
          onWrite={vi.fn(async () => {})}
          parameters={[
            parameter({}),
            parameter({ index: 1, kind: "str", name: "LABEL", units: "", normalised: null, real: null, text: "Lead" }),
            parameter({ index: 2, kind: "switch", name: "MODE", units: "", real: 0, step_names: ["Normal", "Bright"] }),
          ]}
        />
      </MantineProvider>,
    );

    expect(screen.getByRole("slider", { name: "GAIN (dB) slider" })).toBeTruthy();
    expect(screen.getByLabelText("GAIN (dB) numeric input")).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "LABEL" })).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "MODE" })).toBeTruthy();
  });

  it("returns the Rust-owned identity with a real-unit edit", async () => {
    const identity = {
      catalog_generation: 3,
      catalog_revision: 17,
      model_id: 5007,
      index: 4,
      name: "LEVEL",
      catalog_descriptor: "test descriptor",
    };
    const onWrite = vi.fn(async () => {});
    render(
      <MantineProvider>
        <ParameterEditor
          disabled={false}
          onWrite={onWrite}
          parameters={[parameter({ identity, index: 4, name: "LEVEL", real: 5 })]}
        />
      </MantineProvider>,
    );

    const input = screen.getByLabelText("LEVEL (dB) numeric input");
    fireEvent.change(input, { target: { value: "7.5" } });
    fireEvent.blur(input);

    await waitFor(() => expect(onWrite).toHaveBeenCalledWith(identity, { kind: "real", value: 7.5 }));
  });

  it("exposes ordered grid cells with coordinates and block state", () => {
    const block: LiveBlock = {
      row: 0,
      screen_row: 1,
      column: 2,
      model_id: 1001,
      name: "Brit 2203",
      category: "Amplifier",
      based_on: null,
      bypassed: true,
      params: [],
      family: "amp",
    };
    render(
      <MantineProvider>
        <Grid blocks={[block]} onSelect={vi.fn()} selected={block} />
      </MantineProvider>,
    );

    expect(screen.getByRole("group", { name: "Quad Cortex signal grid" })).toBeTruthy();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Row 1, column 2: Brit 2203, Amplifier, bypassed" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("group", { name: "Row 1, column 0: Empty cell, Available position, empty" })).toBeTruthy();
  });
});

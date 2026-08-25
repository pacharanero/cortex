// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LiveBlock, ParameterView } from "../../shared/ipc/types";
import { Grid } from "./Grid";
import { ParameterEditor } from "./ParameterEditor";

const parameter = (overrides: Partial<ParameterView>): ParameterView => ({
  index: 0,
  name: "GAIN",
  kind: "float",
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
});

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

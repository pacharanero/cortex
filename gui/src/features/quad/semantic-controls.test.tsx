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

  it("commits only the stepped value when leaving a numeric input for a stepper", async () => {
    const onWrite = vi.fn(async () => {});
    render(
      <MantineProvider>
        <ParameterEditor disabled={false} onWrite={onWrite} parameters={[parameter({})]} />
      </MantineProvider>,
    );

    const input = screen.getByLabelText("GAIN (dB) numeric input");
    const increase = screen.getByRole("button", { name: "Increase GAIN (dB)" });
    fireEvent.change(input, { target: { value: "7.5" } });
    fireEvent.blur(input, { relatedTarget: increase });
    fireEvent.click(increase);

    await waitFor(() => expect(onWrite).toHaveBeenCalledTimes(1));
    expect(onWrite).toHaveBeenCalledWith(expect.anything(), { kind: "real", value: 7.6 });
  });

  it("filters the already fetched parameter list by its Rust-provided display names", () => {
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

    const search = screen.getByRole("searchbox", { name: "Search parameters" });
    fireEvent.change(search, { target: { value: " label " } });
    expect(screen.getByRole("textbox", { name: "LABEL" })).toBeTruthy();
    expect(screen.queryByRole("slider", { name: "GAIN (dB) slider" })).toBeNull();
    expect(screen.getByText("1 of 3 parameters shown.")).toBeTruthy();

    fireEvent.change(search, { target: { value: "missing" } });
    expect(screen.getByText("No parameters match.")).toBeTruthy();

    fireEvent.change(search, { target: { value: "   " } });
    expect(screen.getByRole("slider", { name: "GAIN (dB) slider" })).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "MODE" })).toBeTruthy();
  });

  it("keeps an in-flight parameter write visible when a filter would otherwise hide it", async () => {
    const write = new Promise<void>(() => {});
    const onWrite = vi.fn(() => write);
    render(
      <MantineProvider>
        <ParameterEditor
          disabled={false}
          onWrite={onWrite}
          parameters={[parameter({}), parameter({ index: 1, name: "MODE" })]}
        />
      </MantineProvider>,
    );

    const input = screen.getByLabelText("GAIN (dB) numeric input");
    fireEvent.change(input, { target: { value: "7" } });
    fireEvent.blur(input);
    await waitFor(() => expect(onWrite).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByRole("searchbox", { name: "Search parameters" }), { target: { value: "mode" } });
    expect(screen.getByRole("slider", { name: "GAIN (dB) slider" })).toBeTruthy();
    expect(screen.getByRole("slider", { name: "MODE (dB) slider" })).toBeTruthy();
  });

  it("keeps the search mounted and reapplies its query when parameters refresh", () => {
    const { rerender } = render(
      <MantineProvider>
        <ParameterEditor disabled={false} onWrite={vi.fn(async () => {})} parameters={[parameter({ name: "GAIN" })]} />
      </MantineProvider>,
    );
    const search = screen.getByRole("searchbox", { name: "Search parameters" });
    search.focus();
    fireEvent.change(search, { target: { value: "level" } });

    rerender(
      <MantineProvider>
        <ParameterEditor disabled={false} onWrite={vi.fn(async () => {})} parameters={[parameter({ name: "LEVEL" })]} />
      </MantineProvider>,
    );

    expect(screen.getByRole("searchbox", { name: "Search parameters" })).toBe(search);
    expect(document.activeElement).toBe(search);
    expect(screen.getByRole("slider", { name: "LEVEL (dB) slider" })).toBeTruthy();
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
        <Grid blocks={[block]} generation={1} onSelect={vi.fn()} revision={1} selected={block} />
      </MantineProvider>,
    );

    expect(screen.getByRole("group", { name: "Quad Cortex signal grid" })).toBeTruthy();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Row 1, column 2: Brit 2203, Amplifier, bypassed" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("group", { name: "Row 1, column 0: Empty cell, Available position, empty" })).toBeTruthy();
  });
});

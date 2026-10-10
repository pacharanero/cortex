// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

function Thrower(): never {
  throw new Error("boom from Thrower");
}

function Fine() {
  return <div>fine</div>;
}

// The fallback renders Mantine components, which need a MantineProvider
// ancestor even outside the real app shell.
function renderWithMantine(ui: ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

describe("ErrorBoundary", () => {
  let consoleError: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    // React logs caught render errors to console.error; spying (rather than
    // silencing) both keeps test output honest and lets the "does not
    // swallow" assertion below check something real.
    consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    consoleError.mockRestore();
  });

  it("renders children when nothing throws", () => {
    renderWithMantine(
      <ErrorBoundary name="Grid">
        <Fine />
      </ErrorBoundary>,
    );
    expect(screen.getByText("fine")).toBeDefined();
  });

  it("offers a consumer-facing recovery action and keeps the render error collapsed", () => {
    renderWithMantine(
      <ErrorBoundary name="Grid">
        <Thrower />
      </ErrorBoundary>,
    );
    expect(screen.getByText("Grid needs to reload")).toBeDefined();
    expect(screen.getByText("This part of Cortex stopped working. Reload Cortex to continue.")).toBeDefined();
    expect(screen.getByRole("button", { name: "Reload" })).toBeDefined();
    const details = screen.getByText("boom from Thrower").closest("details");
    expect(details?.open).toBe(false);
    fireEvent.click(screen.getByText("Show technical details"));
    expect(details?.open).toBe(true);
  });

  it("does not swallow the error from the console", () => {
    renderWithMantine(
      <ErrorBoundary name="Grid">
        <Thrower />
      </ErrorBoundary>,
    );
    expect(consoleError).toHaveBeenCalled();
  });

  it("isolates the failure to the boundary that caught it", () => {
    renderWithMantine(
      <>
        <ErrorBoundary name="Grid">
          <Thrower />
        </ErrorBoundary>
        <ErrorBoundary name="Inspector">
          <Fine />
        </ErrorBoundary>
      </>,
    );
    expect(screen.getByText("Grid needs to reload")).toBeDefined();
    expect(screen.getByText("fine")).toBeDefined();
  });
});

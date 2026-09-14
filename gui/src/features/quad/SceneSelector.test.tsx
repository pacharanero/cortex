// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SceneSnapshot } from "../../shared/ipc/types";
import { SceneSelector } from "./SceneSelector";

const scenes: SceneSnapshot[] = [
  { index: 0, letter: "A", label: null, color: null },
  { index: 1, letter: "B", label: "Lead", color: null },
];

function props(overrides: Record<string, unknown> = {}) {
  return {
    scenes,
    activeScene: 0,
    generation: 1,
    revision: 1,
    disabled: false,
    onSwitch: vi.fn(async () => {}),
    onRename: vi.fn(async () => {}),
    onRecolour: vi.fn(async () => {}),
    onCopySwap: vi.fn(async () => {}),
    ...overrides,
  };
}

/** An unrelated focusable control, to prove a device report never steals focus. */
function renderWithOutsideControl(overrides: Record<string, unknown> = {}) {
  const result = render(
    <MantineProvider>
      <button type="button">Outside control</button>
      <SceneSelector {...props(overrides)} />
    </MantineProvider>,
  );
  return result;
}

const status = () => screen.getByRole("status");

describe("SceneSelector device-reported transitions", () => {
  it("stays silent on mount", () => {
    renderWithOutsideControl();
    expect(status().textContent).toBe("");
  });

  it("announces a strictly newer same-generation report without moving focus", () => {
    const { rerender } = renderWithOutsideControl();
    const outside = screen.getByRole("button", { name: "Outside control" });
    outside.focus();
    expect(document.activeElement).toBe(outside);

    rerender(
      <MantineProvider>
        <button type="button">Outside control</button>
        <SceneSelector {...props({ activeScene: 1, revision: 2 })} />
      </MantineProvider>,
    );

    expect(status().textContent).toBe("Scene B - Lead active");
    expect(document.activeElement).toBe(outside);
  });

  it("ignores an unchanged poll (same generation, revision and scene)", () => {
    const { rerender } = renderWithOutsideControl();
    rerender(
      <MantineProvider>
        <button type="button">Outside control</button>
        <SceneSelector {...props()} />
      </MantineProvider>,
    );
    expect(status().textContent).toBe("");
  });

  it("ignores a metadata-only revision bump that leaves the active scene unchanged", () => {
    const { rerender } = renderWithOutsideControl();
    rerender(
      <MantineProvider>
        <button type="button">Outside control</button>
        <SceneSelector {...props({ revision: 2 })} />
      </MantineProvider>,
    );
    expect(status().textContent).toBe("");
  });

  it("ignores a stale report at or below the last accepted revision", () => {
    const { rerender } = renderWithOutsideControl({ revision: 3 });
    rerender(
      <MantineProvider>
        <button type="button">Outside control</button>
        {/* A same-or-lower revision reporting a different scene is a late or
            replayed reply, not a newer transition. */}
        <SceneSelector {...props({ activeScene: 1, revision: 3 })} />
      </MantineProvider>,
    );
    expect(status().textContent).toBe("");
  });

  it("ignores a replacement generation rather than announcing its first report", () => {
    const { rerender } = renderWithOutsideControl();
    rerender(
      <MantineProvider>
        <button type="button">Outside control</button>
        {/* A reconnect starts a new generation at its own revision numbering;
            comparing it against the previous generation's revision would be
            meaningless. */}
        <SceneSelector {...props({ activeScene: 1, generation: 2, revision: 1 })} />
      </MantineProvider>,
    );
    expect(status().textContent).toBe("");
  });

  it("does not let a local switch's own read-back re-announce or override its result", async () => {
    const onSwitch = vi.fn(async () => {});
    const { rerender } = renderWithOutsideControl({ onSwitch });

    fireEvent.click(screen.getByRole("radio", { name: "B - Lead" }));
    await waitFor(() => expect(onSwitch).toHaveBeenCalledWith(1));
    await waitFor(() => expect(status().textContent).toBe("Scene B - Lead active"));

    // The read-back lands with a relabelled scene B - a concurrent edit, or
    // just a fuller snapshot. If the device-report effect were not guarded,
    // it would overwrite the switch's own announcement with this new label.
    const relabelled: SceneSnapshot[] = [scenes[0], { ...scenes[1], label: "Lead (renamed)" }];
    rerender(
      <MantineProvider>
        <button type="button">Outside control</button>
        <SceneSelector {...props({ activeScene: 1, revision: 2, onSwitch, scenes: relabelled })} />
      </MantineProvider>,
    );

    expect(status().textContent).toBe("Scene B - Lead active");
  });

  it("preserves the immediate refusal announcement, and does not swallow the next real report", async () => {
    const onSwitch = vi.fn(async () => { throw new Error("refused"); });
    const { rerender } = renderWithOutsideControl({ onSwitch });

    fireEvent.click(screen.getByRole("radio", { name: "B - Lead" }));
    await waitFor(() => expect(status().textContent).toBe("Scene B was refused: refused"));

    // Nothing was switched, so activeScene/revision are unchanged - a later,
    // genuinely newer report must still be able to announce.
    rerender(
      <MantineProvider>
        <button type="button">Outside control</button>
        <SceneSelector {...props({ onSwitch, activeScene: 1, revision: 2 })} />
      </MantineProvider>,
    );

    expect(status().textContent).toBe("Scene B - Lead active");
  });
});

describe("SceneSelector copy/swap scenes", () => {
  const copyButton = () => screen.getByRole("button", { name: "Copy" });
  const swapButton = () => screen.getByRole("button", { name: "Swap" });
  const chooseFromTo = (from: string, to: string) => {
    fireEvent.change(screen.getByLabelText("From scene"), { target: { value: from } });
    fireEvent.change(screen.getByLabelText("To scene"), { target: { value: to } });
  };

  it("disables copy and swap until two scenes are chosen", () => {
    renderWithOutsideControl();
    expect(copyButton().hasAttribute("disabled")).toBe(true);
    expect(swapButton().hasAttribute("disabled")).toBe(true);
  });

  it("stays disabled, with an explanatory note, when the same scene is chosen twice", () => {
    renderWithOutsideControl();
    chooseFromTo("0", "0");
    expect(copyButton().hasAttribute("disabled")).toBe(true);
    expect(swapButton().hasAttribute("disabled")).toBe(true);
    expect(screen.getByText("Choose two different scenes.")).toBeTruthy();
  });

  it("copies the exact zero-based indices chosen, never a letter", async () => {
    const onCopySwap = vi.fn(async () => {});
    renderWithOutsideControl({ onCopySwap });
    chooseFromTo("0", "1");
    fireEvent.click(copyButton());
    await waitFor(() => expect(onCopySwap).toHaveBeenCalledWith(0, 1, false));
  });

  it("swaps with the swap flag set", async () => {
    const onCopySwap = vi.fn(async () => {});
    renderWithOutsideControl({ onCopySwap });
    chooseFromTo("1", "0");
    fireEvent.click(swapButton());
    await waitFor(() => expect(onCopySwap).toHaveBeenCalledWith(1, 0, true));
  });

  it("shows a failure banner rather than pretending success on a mismatched or unverified reply", async () => {
    const onCopySwap = vi.fn(async () => {
      throw new Error("asked to copy scene 0 and 1 but the session reported from=0, to=1, swap=false, verified=false");
    });
    renderWithOutsideControl({ onCopySwap });
    chooseFromTo("0", "1");
    fireEvent.click(copyButton());
    await waitFor(() => expect(screen.getByText(/verified=false/)).toBeTruthy());
    // The failed attempt must not clear the operator's selection or disable
    // retrying: the buttons and note both remain live for the same choice.
    expect(copyButton().hasAttribute("disabled")).toBe(false);
  });

  it("does not let a rejected request reach the handler with an incomplete or equal selection", () => {
    const onCopySwap = vi.fn(async () => {});
    renderWithOutsideControl({ onCopySwap });
    chooseFromTo("2", "2");
    fireEvent.click(copyButton());
    fireEvent.click(swapButton());
    expect(onCopySwap).not.toHaveBeenCalled();
  });
});

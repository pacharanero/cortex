// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// @testing-library/react's auto-cleanup only self-registers when it finds a
// global `afterEach` (e.g. Jest, or Vitest's `globals: true`). This project
// imports test hooks explicitly instead, so without this, one test's DOM
// output would still be present when the next test's queries run.
afterEach(() => {
  cleanup();
});

// jsdom has no `window.matchMedia` implementation, which Mantine's
// MantineProvider queries on mount to resolve the color scheme. Without this,
// every test that renders a Mantine component throws before assertions run.
if (!window.matchMedia) {
  window.matchMedia = (query: string): MediaQueryList => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  }) as MediaQueryList;
}

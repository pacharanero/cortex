// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Button, Group, Stack, Text } from "@mantine/core";
import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";

interface ErrorBoundaryProps {
  /** Named in the fallback so the failure points at one panel, not the whole app. */
  name: string;
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * A thrown render error here used to unmount the whole panel silently while
 * the daemon and its writes kept working - GUI-001.7 recorded two same-day
 * incidents (SceneSelector, then a Rules-of-Hooks fault) that read as "the
 * keyboard stopped working" rather than an obvious crash. This makes that
 * failure visible instead: name the panel, offer a reload, and let React's
 * own console logging through rather than swallowing it.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  // React already logs the error and component stack to the console by
  // itself; this only exists to satisfy the lint rule that the caught error
  // be observed, not to add a second log line.
  componentDidCatch(error: Error, info: ErrorInfo): void {
    void info;
    void error;
  }

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <Alert color="red" title={`${this.props.name} failed`}>
        <Stack gap="xs">
          <Text size="sm">{error.message}</Text>
          <Group>
            <Button color="red" onClick={() => window.location.reload()} size="xs" variant="outline">
              Reload
            </Button>
          </Group>
        </Stack>
      </Alert>
    );
  }
}

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Code, Stack, Text } from "@mantine/core";
import type { AlertProps } from "@mantine/core";
import type { ReactNode } from "react";

interface ConsumerErrorAlertProps {
  title: string;
  children: ReactNode;
  error: unknown;
  color?: AlertProps["color"];
  action?: ReactNode;
}

/**
 * Player-facing failures explain what happened and what remains safe. Raw IPC,
 * daemon, and render diagnostics stay available for troubleshooting but never
 * become the default text a player has to parse.
 */
export function ConsumerErrorAlert({ title, children, error, color = "red", action }: ConsumerErrorAlertProps) {
  const detail = technicalDetail(error);

  return (
    <Alert color={color} title={title}>
      <Stack gap="xs">
        <Text size="sm">{children}</Text>
        {action}
        {detail && (
          <details>
            <Text c="dimmed" component="summary" size="sm" style={{ cursor: "pointer" }}>
              Show technical details
            </Text>
            <Code block mt="xs">{detail}</Code>
          </details>
        )}
      </Stack>
    </Alert>
  );
}

export function technicalDetail(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  if (typeof reason === "string") return reason;
  if (reason === null || reason === undefined) return "";
  try {
    return JSON.stringify(reason);
  } catch {
    return String(reason);
  }
}

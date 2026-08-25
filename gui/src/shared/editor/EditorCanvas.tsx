// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Button, Group, Paper, Stack, Text, Title } from "@mantine/core";
import type { ReactNode } from "react";

export type EditorTopology = "quad-grid" | "nano-chain";
export type BlockOperationalState = "engaged" | "bypassed" | "unknown" | "empty";

interface EditorCanvasProps {
  topology: EditorTopology;
  label: string;
  children: ReactNode;
}

export function EditorCanvas({ topology, label, children }: EditorCanvasProps) {
  return <div className="editor-canvas" aria-label={label} data-topology={topology} role="group">
    <div className="editor-canvas__items">{children}</div>
  </div>;
}

interface EditorBlockCardProps {
  positionLabel: string;
  eyebrow: string;
  title: string;
  detail?: string;
  state: BlockOperationalState;
  family?: string;
  selected: boolean;
  inspectorId: string;
  disabled?: boolean;
  busy?: boolean;
  onSelect?: () => void;
}

const stateLabels: Record<BlockOperationalState, string> = {
  engaged: "engaged",
  bypassed: "bypassed",
  unknown: "state unavailable",
  empty: "empty",
};

export function EditorBlockCard({ positionLabel, eyebrow, title, detail, state, family, selected, inspectorId, disabled = false, busy = false, onSelect }: EditorBlockCardProps) {
  const accessibleName = `${positionLabel}: ${title}, ${eyebrow}, ${stateLabels[state]}`;
  const content = <>
    <Text className="editor-block-card__position" size="xs">{positionLabel}</Text>
    <Text className="editor-block-card__title" fw={650} size="sm">{title}</Text>
    <Text className="editor-block-card__eyebrow" size="xs">{eyebrow}</Text>
    {detail && <Text className="editor-block-card__detail" size="xs">{detail}</Text>}
    <Text className="editor-block-card__state" size="xs">{stateLabels[state]}{selected ? " | selected" : ""}</Text>
  </>;
  const common = {
    className: "editor-block-card",
    "data-family": family,
    "data-selected": selected ? "true" : undefined,
    "data-state": state,
  };

  if (!onSelect) return <div {...common} aria-label={accessibleName} role="group">{content}</div>;
  return <button
    {...common}
    aria-controls={inspectorId}
    aria-busy={busy || undefined}
    aria-label={accessibleName}
    aria-pressed={selected}
    disabled={disabled}
    onClick={onSelect}
    type="button"
  >{content}</button>;
}

interface InspectorPanelProps {
  id: string;
  title: string;
  summary?: ReactNode;
  aside?: ReactNode;
  onClose?: () => void;
  children?: ReactNode;
}

export function InspectorPanel({ id, title, summary, aside, onClose, children }: InspectorPanelProps) {
  return <Paper id={id} p="md" withBorder>
    <Stack gap="md">
      <Group align="flex-start" justify="space-between" wrap="wrap">
        <div>
          <Text c="dimmed" size="sm">Inspector</Text>
          <Title order={4}>{title}</Title>
          {summary}
        </div>
        <Group align="flex-start" gap="md">
          {aside}
          {onClose && <Button onClick={onClose} size="xs" variant="subtle">Clear selection</Button>}
        </Group>
      </Group>
      {children}
    </Stack>
  </Paper>;
}

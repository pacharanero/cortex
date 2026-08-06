// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, AppShell, Badge, Group, Paper, SimpleGrid, Stack, Text, Title } from "@mantine/core";
import { useEffect, useState } from "react";
import { Grid } from "./features/quad/Grid";
import { cortexApi, type CortexDashboard, type GridBlock } from "./shared/ipc/cortex";

export function App() {
  const [dashboard, setDashboard] = useState<CortexDashboard | null>(null);
  const [selected, setSelected] = useState<GridBlock | null>(null);

  useEffect(() => {
    void cortexApi.dashboard().then(setDashboard);
  }, []);

  if (!dashboard) return <Text p="xl">Loading Cortex state...</Text>;

  return (
    <AppShell header={{ height: 64 }} padding="md">
      <AppShell.Header p="md">
        <Group justify="space-between">
          <Group gap="xs"><Title order={2}>cortex</Title><Badge color="orange">Quad Cortex</Badge></Group>
          <Badge color={dashboard.health === "demo" ? "yellow" : "green"}>{dashboard.health}</Badge>
        </Group>
      </AppShell.Header>
      <AppShell.Main>
        <Stack gap="md">
          <Alert color="yellow" title="Demo surface">
            This first draft is interactive but does not own USB HID or write to the device. The production backend will query the held `cortex` session daemon.
          </Alert>
          <Group justify="space-between"><div><Text c="dimmed" size="sm">Working grid</Text><Title order={3}>{dashboard.presetName}</Title></div><Text>Scene {String.fromCharCode(65 + dashboard.activeScene)}</Text></Group>
          <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="md">
            <Paper p="md" withBorder><Grid blocks={dashboard.blocks} selected={selected} onSelect={setSelected} /></Paper>
            <Paper p="md" withBorder><Text c="dimmed" size="sm">Inspector</Text><Title order={4}>{selected?.name ?? "Select a block"}</Title><Text mt="sm">{selected ? `${selected.category} at row ${selected.row + 1}, column ${selected.column + 1}.` : "Block details will appear here."}</Text><Text mt="md">CPU: {dashboard.cpuLoad === null ? "awaiting device session" : `${dashboard.cpuLoad}%`}</Text></Paper>
          </SimpleGrid>
        </Stack>
      </AppShell.Main>
    </AppShell>
  );
}

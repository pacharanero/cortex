// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, AppShell, Badge, Button, Group, NavLink, Paper, ScrollArea, SimpleGrid, Stack, Text, Title } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import { Grid } from "./features/quad/Grid";
import { SceneSelector } from "./features/quad/SceneSelector";
import { cortexApi } from "./shared/ipc/api";
import type { DashboardSnapshot, LiveBlock } from "./shared/ipc/types";

interface Cell { row: number; column: number }

function healthLabel(snapshot: DashboardSnapshot): string {
  if (snapshot.source === "fixture") return "fixture";
  return snapshot.status.device.state;
}

export function App() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [selectedCell, setSelectedCell] = useState<Cell | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);
  const generation = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const refresh = async () => {
      try {
        const next = await cortexApi.dashboard();
        if (cancelled) return;
        if (generation.current !== null && generation.current !== next.status.cache.generation) setSelectedCell(null);
        generation.current = next.status.cache.generation;
        setSnapshot(next);
        setError(null);
      } catch (reason) {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        if (!cancelled) timer = window.setTimeout(refresh, 1000);
      }
    };
    void refresh();
    return () => { cancelled = true; if (timer !== undefined) window.clearTimeout(timer); };
  }, []);

  if (!snapshot && !error) return <Text p="xl">Loading Cortex state...</Text>;
  if (!snapshot) return <Alert color="red" m="xl" title="Cortex session unavailable">{error}</Alert>;

  const live = snapshot.live;
  const selected: LiveBlock | null = selectedCell && live
    ? live.blocks.find((block) => block.row === selectedCell.row && block.column === selectedCell.column) ?? null
    : null;
  const health = healthLabel(snapshot);
  const connected = live !== null;
  const reconnectState = snapshot.source === "daemon" && snapshot.status.device.state === "reconnecting" ? snapshot.status.device : null;
  // Switch, then re-read. The device is the authority on which scene is
  // active, so nothing is updated optimistically: if the unit refuses or
  // lands somewhere else, that is what appears. A failed re-read is left to
  // the poll rather than reported as a failed switch.
  const switchScene = async (scene: number) => {
    await cortexApi.switchScene(scene);
    try {
      const next = await cortexApi.dashboard();
      generation.current = next.status.cache.generation;
      setSnapshot(next);
    } catch {
      /* the one-second poll re-reads and surfaces any error */
    }
  };

  const reconnectNow = async () => {
    setRetrying(true);
    try {
      await cortexApi.reconnectNow();
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setRetrying(false);
    }
  };

  return (
    <AppShell header={{ height: 64 }} navbar={{ width: 250, breakpoint: "sm" }} padding="md">
      <AppShell.Header p="md">
        <Group justify="space-between">
          <Group gap="xs"><Title order={2}>cortex</Title><Badge color="orange">Quad Cortex</Badge></Group>
          <Group gap="xs"><Badge color={snapshot.source === "fixture" ? "yellow" : connected ? "green" : "orange"}>{health}</Badge><Badge variant="outline">gen {snapshot.status.cache.generation} / rev {snapshot.status.cache.revision}</Badge></Group>
        </Group>
      </AppShell.Header>
      <AppShell.Navbar p="sm">
        <Text c="dimmed" fw={700} mb="xs" size="xs" tt="uppercase">Preset directory</Text>
        <ScrollArea>
          {snapshot.directory.map((setlist) => (
            <NavLink defaultOpened key={setlist.key} label={setlist.name}>
              {setlist.slots.map((slot) => <NavLink key={`${setlist.key}-${slot.index}`} label={`${slot.slot}  ${slot.name}`} />)}
            </NavLink>
          ))}
          {snapshot.directory.length === 0 && <Text c="dimmed" size="sm">Unavailable for this session generation.</Text>}
        </ScrollArea>
      </AppShell.Navbar>
      <AppShell.Main>
        <Stack gap="md">
          {snapshot.source === "fixture" && <Alert color="yellow" title="Fixture mode">Browser development data is active. Fixture mode never falls back from a daemon error.</Alert>}
          {error && <Alert color="red" title="Refresh failed">{error}</Alert>}
          {!live && <Alert color="orange" title={`Device ${snapshot.status.device.state}`}>
            <Stack gap="xs">
              <Text>Live state is hidden until the daemon reports a connected, complete generation.</Text>
              {reconnectState && <>
                <Text size="sm">Attempt {reconnectState.attempts}: {reconnectState.last_error}</Text>
                <Group gap="sm"><Button color="orange" loading={retrying} onClick={() => void reconnectNow()} size="xs">Reconnect now</Button><Text c="dimmed" size="sm">Automatic retries continue in the background.</Text></Group>
              </>}
            </Stack>
          </Alert>}
          {live && <>
            <Group justify="space-between"><div><Text c="dimmed" size="sm">Working grid</Text><Title order={3}>{live.preset_name}{live.preset_dirty ? " *" : ""}</Title></div><Text>Scene {live.active_scene_label}</Text></Group>
            <Paper p="md" withBorder>
              <SceneSelector
                activeScene={live.active_scene}
                disabled={!connected}
                onSwitch={switchScene}
                scenes={live.scenes}
              />
            </Paper>
            <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="md">
              <Paper p="md" withBorder><Grid blocks={live.blocks} selected={selected} onSelect={(block) => setSelectedCell({ row: block.row, column: block.column })} /></Paper>
              <Paper p="md" withBorder><Text c="dimmed" size="sm">Inspector</Text><Title order={4}>{selected?.name ?? "Select a block"}</Title><Text mt="sm">{selected ? `${selected.category} at row ${selected.screen_row}, column ${selected.column}.` : "Block details will appear here."}</Text><Text mt="md">CPU: {live.cpu_load?.total == null ? "awaiting device push" : `${live.cpu_load.total.toFixed(1)}%`}</Text>{live.cpu_load?.chains.map((chain, row) => <Text key={row} size="sm">Row {row + 1}: {chain.map((column) => `${column.load.toFixed(1)}${column.on_core2 ? "*" : ""}`).join("  ")}</Text>)}</Paper>
            </SimpleGrid>
          </>}
        </Stack>
      </AppShell.Main>
    </AppShell>
  );
}

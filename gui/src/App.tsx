// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, AppShell, Badge, Button, Divider, Group, Menu, NavLink, Paper, ScrollArea, Stack, Switch, Text, Title } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import { Grid } from "./features/quad/Grid";
import { ParameterEditor } from "./features/quad/ParameterEditor";
import { SceneSelector } from "./features/quad/SceneSelector";
import { ErrorBoundary } from "./shared/ErrorBoundary";
import { NanoChain } from "./features/nano/NanoChain";
import { cortexApi } from "./shared/ipc/api";
import type { DashboardSnapshot, DeviceKind, LiveBlock, NanoAmpControl, NanoBypassTarget, NanoFxSlot, ParameterInput, ParameterView } from "./shared/ipc/types";

interface Cell { row: number; column: number }

/**
 * Name the active scene as the unit does, by its letter, adding the label when
 * the preset carries one. `active_scene_label` alone returns the label in place
 * of the letter, which hides *which* of A-H is live - the one thing the
 * hardware always shows.
 */
function activeSceneName(live: NonNullable<DashboardSnapshot["live"]>): string {
  const scene = live.scenes.find((candidate) => candidate.index === live.active_scene);
  if (!scene) return `${live.active_scene}`;
  return scene.label ? `${scene.letter} - ${scene.label}` : scene.letter;
}

function healthLabel(snapshot: DashboardSnapshot): string {
  if (snapshot.source === "fixture") return "fixture";
  return snapshot.status.device.state;
}

export function App() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [selectedCell, setSelectedCell] = useState<Cell | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);
  // Which slot is mid-recall, as "<setlist> <slot>", so only the clicked entry
  // shows as pending rather than the whole directory.
  const [recalling, setRecalling] = useState<string | null>(null);
  const [parameters, setParameters] = useState<ParameterView[] | null>(null);
  const [parameterError, setParameterError] = useState<string | null>(null);
  const generation = useRef<number | null>(null);
  const dashboardEpoch = useRef(0);
  const pendingDeviceSwitch = useRef<{ device: DeviceKind | null; epoch: number } | null>(null);
  const deviceSwitchRunning = useRef(false);
  const [deviceSwitchInProgress, setDeviceSwitchInProgress] = useState(false);
  // Pauses the auto-refresh while a Nano write is in progress. The write
  // itself takes ~6 seconds and returns updated state, so the manual
  // refresh after it completes is sufficient.
  const [nanoWriteInProgress, setNanoWriteInProgress] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const refresh = async () => {
      const requestEpoch = dashboardEpoch.current;
      try {
        const next = await cortexApi.dashboard();
        if (cancelled || requestEpoch !== dashboardEpoch.current) return;
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
    if (!nanoWriteInProgress && !deviceSwitchInProgress) void refresh();
    return () => { cancelled = true; if (timer !== undefined) window.clearTimeout(timer); };
  }, [deviceSwitchInProgress, nanoWriteInProgress]);

  // Parameters are fetched for the selected cell only, not carried in the
  // one-second dashboard poll: they need the model catalog joined in, and
  // sending every block's full parameter set every second would be wasteful.
  //
  // This sits above the early returns below, with the other hooks. Placing it
  // after them changes the number of hooks between the loading render and the
  // loaded one, which React rejects outright - and which crashes the component
  // rather than degrading, so it presents as a panel that silently stops
  // working.
  const selectedCellKey = selectedCell ? `${selectedCell.row},${selectedCell.column}` : null;
  const liveRevision = snapshot?.live?.revision ?? null;
  useEffect(() => {
    if (!selectedCell) { setParameters(null); setParameterError(null); return; }
    let cancelled = false;
    // The zero-based WIRE row the block reported, never the 1-4 screen row: a
    // read or write addressed to the wrong row succeeds silently.
    cortexApi.blockParameters(selectedCell.row, selectedCell.column)
      .then((next) => { if (!cancelled) { setParameters(next); setParameterError(null); } })
      .catch((reason) => {
        if (cancelled) return;
        setParameters(null);
        setParameterError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => { cancelled = true; };
    // Keyed by the cell and the device revision, so an edit or an externally
    // originated change refreshes the values on show.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedCellKey, liveRevision]);

  if (!snapshot && !error) return <Text p="xl">Loading Cortex state...</Text>;
  if (!snapshot) return <Alert color="red" m="xl" title="Cortex session unavailable">{error}</Alert>;

  const live = snapshot.live;
  const nano = snapshot.nano;
  const selected: LiveBlock | null = selectedCell && live
    ? live.blocks.find((block) => block.row === selectedCell.row && block.column === selectedCell.column) ?? null
    : null;
  const health = healthLabel(snapshot);
  const connected = live !== null || nano !== null;
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

  const writeParameter = async (index: number, input: ParameterInput) => {
    if (!selectedCell) return;
    await cortexApi.setParameter(selectedCell.row, selectedCell.column, index, input);
    // Re-read rather than assume: the device may clamp or refuse, and the
    // control should show what it actually holds.
    const next = await cortexApi.blockParameters(selectedCell.row, selectedCell.column);
    setParameters(next);
  };

  // Scene metadata edits follow the same shape as every other write here:
  // act, then re-read, so the panel shows what the device holds.
  const afterDeviceEdit = async () => {
    const next = await cortexApi.dashboard();
    generation.current = next.status.cache.generation;
    setSnapshot(next);
  };
  // Bypass reaches the active scene only, because that is how the device
  // stores it. Act, then re-read: the grid shows what the unit reports.
  const toggleBypass = async (bypass: boolean) => {
    if (!selectedCell) return;
    await cortexApi.setBypass(selectedCell.row, selectedCell.column, bypass);
    await afterDeviceEdit();
  };
  const renameScene = async (scene: number, label: string | null) => {
    await cortexApi.setSceneLabel(scene, label);
    await afterDeviceEdit();
  };
  const recolourScene = async (scene: number, color: number) => {
    await cortexApi.setSceneColor(scene, color);
    await afterDeviceEdit();
  };

  // Recalling replaces the working copy and changes what the unit plays, so it
  // is followed by a re-read rather than an optimistic update: the grid shown
  // is the one the device reports, not the one that was asked for.
  const recall = async (setlist: string, slot: string) => {
    setRecalling(`${setlist}\u0000${slot}`);
    try {
      await cortexApi.recallPreset(setlist, slot);
      setSelectedCell(null);
      const next = await cortexApi.dashboard();
      generation.current = next.status.cache.generation;
      setSnapshot(next);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setRecalling(null);
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
  const setNanoAmp = async (control: NanoAmpControl, value: number) => {
    setNanoWriteInProgress(true);
    try {
      await cortexApi.setNanoAmp(control, value);
      const next = await cortexApi.dashboard();
      setSnapshot(next);
    } finally {
      setNanoWriteInProgress(false);
    }
  };
  const setNanoGateReduction = async (percent: number) => {
    setNanoWriteInProgress(true);
    try {
      await cortexApi.setNanoGateReduction(percent);
      const next = await cortexApi.dashboard();
      setSnapshot(next);
    } finally {
      setNanoWriteInProgress(false);
    }
  };
  const setNanoBypass = async (target: NanoBypassTarget, bypassed: boolean) => {
    setNanoWriteInProgress(true);
    try {
      await cortexApi.setNanoBypass(target, bypassed);
      const next = await cortexApi.dashboard();
      setSnapshot(next);
    } finally {
      setNanoWriteInProgress(false);
    }
  };
  const setNanoFxParam = async (slot: NanoFxSlot, paramIndex: number, value: number) => {
    setNanoWriteInProgress(true);
    try {
      await cortexApi.setNanoFxParam(slot, paramIndex, value);
      const next = await cortexApi.dashboard();
      setSnapshot(next);
    } finally {
      setNanoWriteInProgress(false);
    }
  };
  const switchDevice = async (device: "quad_cortex" | "nano_cortex" | "auto") => {
    const epoch = dashboardEpoch.current + 1;
    dashboardEpoch.current = epoch;
    pendingDeviceSwitch.current = { device: device === "auto" ? null : device, epoch };
    if (deviceSwitchRunning.current) return;

    deviceSwitchRunning.current = true;
    setDeviceSwitchInProgress(true);
    try {
      while (pendingDeviceSwitch.current) {
        const selection = pendingDeviceSwitch.current;
        pendingDeviceSwitch.current = null;
        try {
          await cortexApi.setDevice(selection.device);
          if (selection.epoch !== dashboardEpoch.current) continue;
          const next = await cortexApi.dashboard();
          if (selection.epoch !== dashboardEpoch.current) continue;
          generation.current = next.status.cache.generation;
          setSnapshot(next);
          setSelectedCell(null);
          setError(null);
        } catch (reason) {
          if (selection.epoch === dashboardEpoch.current) {
            setError(reason instanceof Error ? reason.message : String(reason));
          }
        }
      }
    } finally {
      deviceSwitchRunning.current = false;
      setDeviceSwitchInProgress(false);
    }
  };

  const currentDeviceLabel = nano ? "Nano Cortex" : "Quad Cortex";
  const currentDeviceKind = nano ? "nano_cortex" : "quad_cortex";

  return (
    <AppShell header={{ height: 64 }} navbar={{ width: 250, breakpoint: "sm" }} padding="md">
      <AppShell.Header p="md">
        <Group justify="space-between">
          <Group gap="xs">
            <Title order={2}>cortex</Title>
            <Menu shadow="md" position="bottom-start" width={200}>
              <Menu.Target>
                <Button aria-label={`Select device, current ${currentDeviceLabel}`} color="orange" size="compact-xs" tt="uppercase" variant="filled">{currentDeviceLabel}</Button>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Label>Device</Menu.Label>
                <Menu.Item leftSection={currentDeviceKind === "quad_cortex" ? "●" : undefined} onClick={() => void switchDevice("quad_cortex")}>
                  Quad Cortex
                </Menu.Item>
                <Menu.Item leftSection={currentDeviceKind === "nano_cortex" ? "●" : undefined} onClick={() => void switchDevice("nano_cortex")}>
                  Nano Cortex
                </Menu.Item>
                <Menu.Divider />
                <Menu.Item onClick={() => void switchDevice("auto")}>
                  Auto-detect
                </Menu.Item>
              </Menu.Dropdown>
            </Menu>
          </Group>
          <Group gap="xs"><Badge color={snapshot.source === "fixture" ? "yellow" : connected ? "green" : "orange"}>{health}</Badge><Badge variant="outline">gen {snapshot.status.cache.generation} / rev {snapshot.status.cache.revision}</Badge></Group>
        </Group>
      </AppShell.Header>
      <AppShell.Navbar p="sm">
        <Text c="dimmed" fw={700} mb="xs" size="xs" tt="uppercase">Preset directory</Text>
        <ScrollArea>
          {snapshot.directory.map((setlist) => (
            <NavLink defaultOpened key={setlist.key} label={setlist.name}>
              {setlist.slots.map((slot) => (
                <NavLink
                  active={live?.preset_name === slot.name}
                  // A recall replaces the working copy and changes what is
                  // heard, exactly as pressing the preset on the unit does.
                  // Recall is free here because it writes nothing to storage;
                  // saving is the operation that asks first.
                  description={recalling === `${setlist.key} ${slot.slot}` ? "Recalling..." : undefined}
                  disabled={recalling !== null || !connected}
                  key={`${setlist.key}-${slot.index}`}
                  label={`${slot.slot}  ${slot.name}`}
                  onClick={() => void recall(setlist.key, slot.slot)}
                />
              ))}
            </NavLink>
          ))}
          {snapshot.directory.length === 0 && <Text c="dimmed" size="sm">Unavailable for this session generation.</Text>}
        </ScrollArea>
      </AppShell.Navbar>
      <AppShell.Main>
        <Stack gap="md">
          {snapshot.source === "fixture" && <Alert color="yellow" title="Fixture mode">Browser development data is active. Fixture mode never falls back from a daemon error.</Alert>}
          {error && <Alert color="red" title="Refresh failed">{error}</Alert>}
          {!live && !nano && <Alert color="orange" title={`Device ${snapshot.status.device.state}`}>
            <Stack gap="xs">
              <Text>Live state is hidden until the daemon reports a connected, complete generation.</Text>
              {reconnectState && <>
                <Text size="sm">Attempt {reconnectState.attempts}: {reconnectState.last_error}</Text>
                <Group gap="sm"><Button color="orange" loading={retrying} onClick={() => void reconnectNow()} size="xs">Reconnect now</Button><Text c="dimmed" size="sm">Automatic retries continue in the background.</Text></Group>
              </>}
            </Stack>
          </Alert>}
          {nano && <NanoChain
            onReadFxParams={cortexApi.readNanoFxParams}
            onSetAmp={setNanoAmp}
            onSetGateReduction={setNanoGateReduction}
            onSetBypass={setNanoBypass}
            onSetFxParam={setNanoFxParam}
            state={nano}
          />}
          {live && <>
            <Group justify="space-between"><div><Text c="dimmed" size="sm">Working grid</Text><Title order={3}>{live.preset_name}{live.preset_dirty ? " *" : ""}</Title></div><Text>Scene {activeSceneName(live)}</Text></Group>
            <Paper p="md" withBorder>
              <ErrorBoundary name="Scene selector">
                <SceneSelector
                  activeScene={live.active_scene}
                  disabled={!connected}
                  onRecolour={recolourScene}
                  onRename={renameScene}
                  onSwitch={switchScene}
                  scenes={live.scenes}
                />
              </ErrorBoundary>
            </Paper>
            {/* Grid first and full width, inspector beneath it. The grid is
                the thing being read at a glance and benefits from the width;
                the inspector will grow parameter controls, which need room to
                lay out horizontally rather than in a narrow column. */}
            <Paper p="md" withBorder>
              <ErrorBoundary name="Grid">
                <Grid blocks={live.blocks} selected={selected} onSelect={(block) => setSelectedCell({ row: block.row, column: block.column })} />
              </ErrorBoundary>
            </Paper>
            <Paper p="md" withBorder>
              <ErrorBoundary name="Inspector">
                <Group align="flex-start" justify="space-between" wrap="wrap">
                  <div>
                    <Text c="dimmed" size="sm">Inspector</Text>
                    <Title order={4}>{selected?.name ?? "Select a block"}</Title>
                    <Text mt="sm">
                      {selected
                        ? `${selected.category} at row ${selected.screen_row}, column ${selected.column}.`
                        : "Block details will appear here."}
                    </Text>
                    {selected?.based_on && <Text c="dimmed" mt="xs" size="sm">{selected.based_on}</Text>}
                    {selected && (
                      <Switch
                        aria-label={`${selected.name} bypass, ${selected.bypassed ? "bypassed" : "engaged"}`}
                        // Not disabled while writing: a disabled control cannot
                        // hold focus, which is the fault recorded in
                        // SceneSelector and ParameterEditor.
                        checked={selected.bypassed}
                        description="Applies to the active scene only, as the device stores it"
                        disabled={!connected}
                        label={selected.bypassed ? "Bypassed" : "Engaged"}
                        mt="md"
                        onChange={(event) => void toggleBypass(event.currentTarget.checked)}
                      />
                    )}
                  </div>
                  <div>
                    <Text c="dimmed" size="sm">DSP load</Text>
                    <Text>{live.cpu_load?.total == null ? "awaiting device push" : `${live.cpu_load.total.toFixed(1)}%`}</Text>
                    {live.cpu_load?.chains.map((chain, row) => (
                      <Text key={row} size="sm">
                        Row {row + 1}: {chain.map((column) => `${column.load.toFixed(1)}${column.on_core2 ? "*" : ""}`).join("  ")}
                      </Text>
                    ))}
                  </div>
                </Group>

                {selected && (
                  <>
                    <Divider label="Parameters" labelPosition="left" my="md" />
                    {parameterError && <Alert color="orange" title="Parameters unavailable">{parameterError}</Alert>}
                    {!parameterError && parameters === null && <Text c="dimmed" size="sm">Reading parameters...</Text>}
                    {!parameterError && parameters !== null && (
                      <ParameterEditor
                        disabled={!connected}
                        onWrite={writeParameter}
                        parameters={parameters}
                      />
                    )}
                  </>
                )}
              </ErrorBoundary>
            </Paper>
          </>}
        </Stack>
      </AppShell.Main>
    </AppShell>
  );
}

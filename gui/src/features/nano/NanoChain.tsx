// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Badge, Button, Group, NumberInput, Paper, SimpleGrid, Slider, Stack, Switch, Text, Title } from "@mantine/core";
import { useEffect, useState } from "react";
import type { NanoAmpControl, NanoBypassTarget, NanoCurrentState, NanoFxSlot, NanoSlotRole } from "../../shared/ipc/types";

const roleNames: Record<NanoSlotRole, string> = {
  gate: "Gate", pre_fx1: "Pre FX 1", pre_fx2: "Pre FX 2", capture: "Capture",
  ir_cab: "IR / Cab", post_fx1: "Post FX 1", post_fx2: "Post FX 2", post_fx3: "Post FX 3",
};

const bypassTargets: { role: NanoSlotRole; target: NanoBypassTarget }[] = [
  { role: "gate", target: "gate" },
  { role: "pre_fx1", target: "pre_fx1" },
  { role: "pre_fx2", target: "pre_fx2" },
  { role: "post_fx1", target: "post_fx1" },
  { role: "post_fx2", target: "post_fx2" },
  { role: "post_fx3", target: "post_fx3" },
];

const fxSlots: { role: NanoSlotRole; slot: NanoFxSlot }[] = [
  { role: "pre_fx1", slot: "pre_fx1" },
  { role: "pre_fx2", slot: "pre_fx2" },
  { role: "post_fx1", slot: "post_fx1" },
  { role: "post_fx2", slot: "post_fx2" },
  { role: "post_fx3", slot: "post_fx3" },
];

interface NanoChainProps {
  state: NanoCurrentState;
  onSetAmp: (control: NanoAmpControl, value: number) => Promise<void>;
  onSetBypass: (target: NanoBypassTarget, bypassed: boolean) => Promise<void>;
  onReadFxParams: (slot: NanoFxSlot) => Promise<number[]>;
  onSetFxParam: (slot: NanoFxSlot, paramIndex: number, value: number) => Promise<void>;
}

export function NanoChain({ state, onSetAmp, onSetBypass, onReadFxParams, onSetFxParam }: NanoChainProps) {
  const [draft, setDraft] = useState(state.amp);
  const [dirtyAmpControls, setDirtyAmpControls] = useState<Set<NanoAmpControl>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedSlot, setSelectedSlot] = useState<NanoFxSlot | null>(null);
  const [fxParams, setFxParams] = useState<number[] | null>(null);
  const [fxDraft, setFxDraft] = useState<number[]>([]);
  useEffect(() => {
    if (busy) return;
    setDraft((current) => {
      const next = { ...current };
      let changed = false;
      for (const control of Object.keys(state.amp) as NanoAmpControl[]) {
        if (!dirtyAmpControls.has(control) && next[control] !== state.amp[control]) {
          next[control] = state.amp[control];
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }, [state.amp, busy, dirtyAmpControls]);

  const apply = async (control: NanoAmpControl) => {
    const value = draft[control];
    if (value == null) return;
    setBusy(`amp:${control}`); setError(null);
    try {
      await onSetAmp(control, value);
      setDirtyAmpControls((current) => {
        const next = new Set(current);
        next.delete(control);
        return next;
      });
    }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  };

  const toggleBypass = async (target: NanoBypassTarget, bypassed: boolean) => {
    setBusy(`bypass:${target}`); setError(null);
    try { await onSetBypass(target, bypassed); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  };

  const loadFxParams = async (slot: NanoFxSlot) => {
    setSelectedSlot(slot);
    setFxParams(null);
    setBusy(`fx-read:${slot}`); setError(null);
    try {
      const values = await onReadFxParams(slot);
      setFxParams(values);
      setFxDraft(values);
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  };

  const applyFxParam = async (slot: NanoFxSlot, paramIndex: number) => {
    const value = fxDraft[paramIndex];
    if (value == null) return;
    setBusy(`fx-write:${slot}:${paramIndex}`); setError(null);
    try {
      await onSetFxParam(slot, paramIndex, value);
      const values = await onReadFxParams(slot);
      setFxParams(values);
      setFxDraft(values);
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  };

  return <Stack gap="md">
    <Group justify="space-between">
      <div><Text c="dimmed" size="sm">Fixed signal chain</Text><Title order={3}>Nano Cortex</Title></div>
      <Badge color="orange" variant="outline">Amp and bypass editing hardware verified</Badge>
    </Group>
    <SimpleGrid cols={{ base: 2, sm: 4, lg: 8 }} spacing="xs">
      {state.slots.map((slot) => {
        const fxSlot = fxSlots.find((f) => f.role === slot.role);
        const isSelected = fxSlot && selectedSlot === fxSlot.slot;
        return <Paper key={slot.role} p="sm" withBorder data-bypassed={slot.bypassed || undefined}
          style={fxSlot ? { cursor: "pointer", borderColor: isSelected ? "var(--mantine-color-blue-5)" : undefined } : undefined}
          onClick={fxSlot ? () => void loadFxParams(fxSlot.slot) : undefined}>
          <Text c="dimmed" fw={700} size="xs" tt="uppercase">{roleNames[slot.role]}</Text>
          <Text fw={600} mt="xs">{slot.loaded_name ?? (slot.model_id == null ? "Assigned by device" : `Model ${slot.model_id}`)}</Text>
          <Text c={slot.bypassed ? "orange" : "dimmed"} size="xs">
            {slot.bypassed == null ? "state unavailable" : slot.bypassed ? "bypassed" : "on"}
          </Text>
          {fxSlot && <Text c={isSelected ? "blue" : "dimmed"} size="xs" mt={2}>{isSelected ? "editing" : "click to edit"}</Text>}
        </Paper>;
      })}
    </SimpleGrid>
    <Paper p="md" withBorder>
      <Text c="dimmed" fw={700} size="xs" tt="uppercase">Amp controls (raw 0-255)</Text>
      <SimpleGrid cols={{ base: 1, sm: 3, lg: 5 }} mt="sm">
        {(Object.keys(state.amp) as NanoAmpControl[]).map((control) => <Group align="flex-end" key={control} wrap="nowrap">
          <NumberInput aria-busy={busy === `amp:${control}`} clampBehavior="strict" label={control[0].toUpperCase() + control.slice(1)} max={255} min={0} onChange={(value) => {
            setDraft((current) => ({ ...current, [control]: typeof value === "number" ? value : null }));
            setDirtyAmpControls((current) => new Set(current).add(control));
          }} value={draft[control] ?? ""} />
          <Button disabled={draft[control] == null || busy !== null} loading={busy === `amp:${control}`} onClick={() => void apply(control)}>Apply</Button>
        </Group>)}
      </SimpleGrid>
      <Text c="dimmed" mt="sm" size="xs">Changes heard working state and saves nothing. Apply waits about six seconds for fresh device read-back.</Text>
    </Paper>
    <Paper p="md" withBorder>
      <Text c="dimmed" fw={700} size="xs" tt="uppercase">Gate / FX bypass</Text>
      <SimpleGrid cols={{ base: 2, sm: 3, lg: 6 }} mt="sm">
        {bypassTargets.map(({ role, target }) => {
          const slot = state.slots.find((s) => s.role === role);
          const bypassed = slot?.bypassed;
          return <Group key={target} justify="space-between" wrap="nowrap">
            <div><Text size="sm" fw={600}>{roleNames[role]}</Text></div>
            <Switch
              aria-busy={busy === `bypass:${target}`}
              checked={bypassed ?? false}
              disabled={bypassed == null || busy !== null}
              label={bypassed == null ? "unknown" : bypassed ? "bypassed" : "on"}
              onChange={(event) => void toggleBypass(target, event.currentTarget.checked)}
            />
          </Group>;
        })}
      </SimpleGrid>
      <Text c="dimmed" mt="sm" size="xs">Toggling bypass changes heard working state and saves nothing. Each toggle waits about six seconds for fresh device read-back. The Gate&apos;s &quot;on&quot; state may read back as unknown because the device represents it by omitting the field.</Text>
    </Paper>
    {selectedSlot && fxParams != null && <Paper p="md" withBorder>
      <Group justify="space-between">
        <Text c="dimmed" fw={700} size="xs" tt="uppercase">FX parameters: {roleNames[fxSlots.find((f) => f.slot === selectedSlot)?.role ?? "pre_fx1"]}</Text>
        <Button size="xs" variant="subtle" onClick={() => { setSelectedSlot(null); setFxParams(null); }}>Close</Button>
      </Group>
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} mt="sm">
        {fxParams.map((value, i) => <Group align="flex-end" key={i} wrap="nowrap">
          <div style={{ flex: 1 }}>
            <Text size="xs" fw={600}>Param {i}</Text>
            <Slider aria-busy={busy === `fx-write:${selectedSlot}:${i}`} disabled={busy !== null} label={(v) => v.toFixed(2)} max={1} min={0} onChange={(v) => setFxDraft((current) => { const next = [...current]; next[i] = v; return next; })} size="sm" step={0.001} value={fxDraft[i] ?? value} />
            <Text c="dimmed" size="xs">device: {value.toFixed(3)} | draft: {(fxDraft[i] ?? value).toFixed(3)}</Text>
          </div>
          <Button disabled={busy !== null || Math.abs((fxDraft[i] ?? value) - value) < 0.0005} loading={busy === `fx-write:${selectedSlot}:${i}`} onClick={() => selectedSlot && void applyFxParam(selectedSlot, i)} size="xs">Apply</Button>
        </Group>)}
      </SimpleGrid>
      <Text c="dimmed" mt="sm" size="xs">Provisional: this normalized 0.0-1.0 read/write path is offline-tested but not hardware-verified. Values vary by loaded model.</Text>
    </Paper>}
    {error && <Alert color="red" title="Nano write failed">{error}</Alert>}
  </Stack>;
}

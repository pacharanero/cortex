// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Badge, Button, Group, NumberInput, Paper, SimpleGrid, Slider, Stack, Switch, Text, Title } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
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
  onSetGateReduction: (percent: number) => Promise<void>;
  onSetBypass: (target: NanoBypassTarget, bypassed: boolean) => Promise<void>;
  onReadFxParams: (slot: NanoFxSlot) => Promise<number[]>;
  onSetFxParam: (slot: NanoFxSlot, paramIndex: number, value: number) => Promise<number[]>;
}

export function NanoChain({ state, onSetAmp, onSetGateReduction, onSetBypass, onReadFxParams, onSetFxParam }: NanoChainProps) {
  const [draft, setDraft] = useState(state.amp);
  const [dirtyAmpControls, setDirtyAmpControls] = useState<Set<NanoAmpControl>>(new Set());
  const [gateDraft, setGateDraft] = useState<number | string>(state.gate_reduction ?? "");
  const [gateDirty, setGateDirty] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [selectedSlot, setSelectedSlot] = useState<NanoFxSlot | null>(null);
  const [fxParams, setFxParams] = useState<number[] | null>(null);
  const [fxDraft, setFxDraft] = useState<number[]>([]);
  const selectionEpoch = useRef(0);
  const ampEditEpoch = useRef(new Map<NanoAmpControl, number>());
  const gateEditEpoch = useRef(0);
  const fxEditEpoch = useRef<number[]>([]);
  const pendingFxRead = useRef<{ slot: NanoFxSlot; epoch: number } | null>(null);
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
  useEffect(() => {
    if (!busy && !gateDirty) setGateDraft(state.gate_reduction ?? "");
  }, [state.gate_reduction, busy, gateDirty]);

  const apply = async (control: NanoAmpControl) => {
    if (busy) return;
    const value = draft[control];
    if (value == null) return;
    const editEpoch = ampEditEpoch.current.get(control) ?? 0;
    setBusy(`amp:${control}`); setError(null); setStatus(`Applying ${control}...`);
    try {
      await onSetAmp(control, value);
      if ((ampEditEpoch.current.get(control) ?? 0) === editEpoch) {
        setDirtyAmpControls((current) => {
          const next = new Set(current);
          next.delete(control);
          return next;
        });
      }
      setStatus(`${control} applied.`);
    }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); setStatus(`${control} failed.`); }
    finally { setBusy(null); }
  };

  const applyGateReduction = async () => {
    if (busy) return;
    if (typeof gateDraft !== "number") return;
    const editEpoch = gateEditEpoch.current;
    setBusy("gate:reduction"); setError(null); setStatus("Applying Gate reduction...");
    try {
      await onSetGateReduction(gateDraft);
      if (gateEditEpoch.current === editEpoch) setGateDirty(false);
      setStatus("Gate reduction applied.");
    }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); setStatus("Gate reduction failed."); }
    finally { setBusy(null); }
  };

  const toggleBypass = async (target: NanoBypassTarget, bypassed: boolean) => {
    if (busy) return;
    setBusy(`bypass:${target}`); setError(null); setStatus(`Applying ${roleNames[target]} bypass...`);
    try { await onSetBypass(target, bypassed); setStatus(`${roleNames[target]} bypass applied.`); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); setStatus(`${roleNames[target]} bypass failed.`); }
    finally { setBusy(null); }
  };

  const runFxReads = async (first: { slot: NanoFxSlot; epoch: number }) => {
    let request: { slot: NanoFxSlot; epoch: number } | null = first;
    while (request) {
      const { slot, epoch } = request;
      const operation = `fx-read:${slot}`;
      setBusy(operation); setError(null); setStatus(`Reading ${roleNames[fxSlots.find((item) => item.slot === slot)?.role ?? "pre_fx1"]} parameters...`);
      try {
        const values = await onReadFxParams(slot);
        if (selectionEpoch.current === epoch) {
          setFxParams(values);
          setFxDraft(values);
          fxEditEpoch.current = values.map(() => 0);
          setStatus(`${roleNames[fxSlots.find((item) => item.slot === slot)?.role ?? "pre_fx1"]} parameters loaded.`);
        }
      } catch (reason) {
        if (selectionEpoch.current === epoch) {
          setError(reason instanceof Error ? reason.message : String(reason));
          setStatus("Nano FX parameter read failed.");
        }
      }
      request = pendingFxRead.current;
      pendingFxRead.current = null;
    }
    setBusy((current) => current?.startsWith("fx-read:") ? null : current);
  };

  const loadFxParams = (slot: NanoFxSlot) => {
    if (busy && !busy.startsWith("fx-read:")) return;
    const request = { slot, epoch: ++selectionEpoch.current };
    setSelectedSlot(slot);
    setFxParams(null);
    setFxDraft([]);
    fxEditEpoch.current = [];
    setError(null);
    if (busy?.startsWith("fx-read:")) {
      pendingFxRead.current = request;
      return;
    }
    void runFxReads(request);
  };

  const applyFxParam = async (slot: NanoFxSlot, paramIndex: number) => {
    if (busy) return;
    const value = fxDraft[paramIndex];
    if (value == null) return;
    const epoch = selectionEpoch.current;
    const editEpoch = fxEditEpoch.current[paramIndex] ?? 0;
    const operation = `fx-write:${slot}:${paramIndex}`;
    setBusy(operation); setError(null); setStatus(`Applying FX parameter ${paramIndex}...`);
    try {
      const values = await onSetFxParam(slot, paramIndex, value);
      if (selectionEpoch.current !== epoch) return;
      setFxParams(values);
      setFxDraft((current) => values.map((confirmed, index) => {
        if (index === paramIndex && (fxEditEpoch.current[index] ?? 0) === editEpoch) return confirmed;
        return current[index] ?? confirmed;
      }));
      setStatus(`FX parameter ${paramIndex} applied.`);
    } catch (reason) {
      try {
        const values = await onReadFxParams(slot);
        if (selectionEpoch.current === epoch) {
          setFxParams(values);
          setFxDraft((current) => values.map((confirmed, index) => {
            if (index === paramIndex && (fxEditEpoch.current[index] ?? 0) !== editEpoch) return current[index] ?? confirmed;
            return confirmed;
          }));
        }
      } catch {
        if (selectionEpoch.current === epoch) {
          setFxParams(null);
          setFxDraft([]);
        }
      }
      setError(reason instanceof Error ? reason.message : String(reason));
      setStatus(`FX parameter ${paramIndex} failed.`);
    }
    finally { setBusy((current) => current === operation ? null : current); }
  };

  return <Stack gap="md">
    <Group justify="space-between">
      <div><Text c="dimmed" size="sm">Fixed signal chain</Text><Title order={3}>Nano Cortex</Title></div>
      <Badge color="orange" variant="outline">Amp, bypass and FX paths hardware verified</Badge>
    </Group>
    <SimpleGrid cols={{ base: 2, sm: 4, lg: 8 }} spacing="xs">
      {state.slots.map((slot) => {
        const fxSlot = fxSlots.find((f) => f.role === slot.role);
        const isSelected = fxSlot?.slot === selectedSlot;
        return <Paper key={slot.role} p="sm" withBorder data-bypassed={slot.bypassed || undefined}
          aria-disabled={fxSlot ? busy !== null && !busy.startsWith("fx-read:") : undefined}
          aria-pressed={fxSlot ? isSelected : undefined}
          role={fxSlot ? "button" : undefined}
          style={fxSlot ? { cursor: "pointer", borderColor: isSelected ? "var(--mantine-color-blue-5)" : undefined } : undefined}
          tabIndex={fxSlot ? 0 : undefined}
          onClick={fxSlot ? () => void loadFxParams(fxSlot.slot) : undefined}
          onKeyDown={fxSlot ? (event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              void loadFxParams(fxSlot.slot);
            }
          } : undefined}>
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
            ampEditEpoch.current.set(control, (ampEditEpoch.current.get(control) ?? 0) + 1);
            setDraft((current) => ({ ...current, [control]: typeof value === "number" ? value : null }));
            setDirtyAmpControls((current) => new Set(current).add(control));
          }} value={draft[control] ?? ""} />
          <Button aria-busy={busy === `amp:${control}`} aria-label={`Apply ${control}`} disabled={draft[control] == null || (busy !== null && busy !== `amp:${control}`)} onClick={() => void apply(control)}>{busy === `amp:${control}` ? "Applying..." : "Apply"}</Button>
        </Group>)}
      </SimpleGrid>
      <Text c="dimmed" mt="sm" size="xs">Changes heard working state and saves nothing. Apply waits about six seconds for fresh device read-back.</Text>
    </Paper>
    <Paper p="md" withBorder>
      <Text c="dimmed" fw={700} size="xs" tt="uppercase">Gate / FX bypass</Text>
      <Group align="flex-end" mt="sm">
        <NumberInput aria-busy={busy === "gate:reduction"} clampBehavior="strict" label="Gate reduction" max={100} min={0} onChange={(value) => {
          gateEditEpoch.current += 1;
          setGateDraft(value);
          setGateDirty(true);
        }} style={{ flex: "1 1 160px" }} suffix="%" value={gateDraft} />
        <Button aria-busy={busy === "gate:reduction"} aria-label="Apply Gate reduction" disabled={typeof gateDraft !== "number" || !gateDirty || (busy !== null && busy !== "gate:reduction")} onClick={() => void applyGateReduction()}>{busy === "gate:reduction" ? "Applying..." : "Apply"}</Button>
        <Badge color="yellow" variant="outline">provisional</Badge>
      </Group>
      <SimpleGrid cols={{ base: 2, sm: 3, lg: 6 }} mt="sm">
        {bypassTargets.map(({ role, target }) => {
          const slot = state.slots.find((s) => s.role === role);
          const bypassed = slot?.bypassed;
          return <Group key={target} justify="space-between" wrap="nowrap">
            <div><Text size="sm" fw={600}>{roleNames[role]}</Text></div>
            <Switch
              aria-label={`${roleNames[role]} bypass, ${bypassed == null ? "state unknown" : bypassed ? "bypassed" : "on"}`}
              aria-busy={busy === `bypass:${target}`}
              checked={bypassed ?? false}
              disabled={bypassed == null || (busy !== null && busy !== `bypass:${target}`)}
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
        <Button size="xs" variant="subtle" onClick={() => { selectionEpoch.current += 1; pendingFxRead.current = null; setSelectedSlot(null); setFxParams(null); }}>Close</Button>
      </Group>
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} mt="sm">
        {fxParams.map((value, i) => <Group align="flex-end" key={i} wrap="nowrap">
          <div style={{ flex: 1 }}>
            <Text size="xs" fw={600}>Param {i}</Text>
            <Slider aria-busy={busy === `fx-write:${selectedSlot}:${i}`} disabled={busy !== null && busy !== `fx-write:${selectedSlot}:${i}`} label={(v) => v.toFixed(2)} max={1} min={0} onChange={(v) => {
              fxEditEpoch.current[i] = (fxEditEpoch.current[i] ?? 0) + 1;
              setFxDraft((current) => { const next = [...current]; next[i] = v; return next; });
            }} size="sm" step={0.001} thumbLabel={`FX parameter ${i} normalized value`} value={fxDraft[i] ?? value} />
            <Text c="dimmed" size="xs">device: {value.toFixed(3)} | draft: {(fxDraft[i] ?? value).toFixed(3)}</Text>
          </div>
          <Button aria-busy={busy === `fx-write:${selectedSlot}:${i}`} aria-label={`Apply FX parameter ${i}`} disabled={(busy !== null && busy !== `fx-write:${selectedSlot}:${i}`) || Math.abs((fxDraft[i] ?? value) - value) < 0.0005} onClick={() => selectedSlot && void applyFxParam(selectedSlot, i)} size="xs">{busy === `fx-write:${selectedSlot}:${i}` ? "Applying..." : "Apply"}</Button>
        </Group>)}
      </SimpleGrid>
      <Text c="dimmed" mt="sm" size="xs">The normalized 0.0-1.0 path and this rendered Linux control are hardware-verified with device read-back and restoration. Values vary by loaded model.</Text>
    </Paper>}
    {status && <Text aria-live="polite" role="status" size="sm">{status}</Text>}
    {error && <Alert color="red" title="Nano operation failed">{error}</Alert>}
  </Stack>;
}

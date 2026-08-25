// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Badge, Button, Group, NumberInput, Paper, SimpleGrid, Slider, Stack, Switch, Text, Title } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import { EditorBlockCard, EditorCanvas, InspectorPanel } from "../../shared/editor/EditorCanvas";
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
  onReadFxParameters: (slot: NanoFxSlot) => Promise<number[]>;
  onSetAmp: (control: NanoAmpControl, value: number) => Promise<void>;
  onSetBypass: (target: NanoBypassTarget, bypassed: boolean) => Promise<void>;
  onSetFxParameter: (slot: NanoFxSlot, paramIndex: number, value: number) => Promise<number[]>;
}

export function NanoChain({ state, onReadFxParameters, onSetAmp, onSetBypass, onSetFxParameter }: NanoChainProps) {
  const [draft, setDraft] = useState(state.amp);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedRole, setSelectedRole] = useState<NanoSlotRole | null>(null);
  const [fxParams, setFxParams] = useState<number[] | null>(null);
  const [fxDraft, setFxDraft] = useState<number[]>([]);
  const selectionEpoch = useRef(0);
  useEffect(() => { if (!busy) setDraft(state.amp); }, [state.amp, busy]);

  const selectedState = state.slots.find((slot) => slot.role === selectedRole) ?? null;
  const selectedFxSlot = fxSlots.find((slot) => slot.role === selectedRole)?.slot ?? null;

  const selectRole = async (role: NanoSlotRole) => {
    if (busy) return;
    const epoch = ++selectionEpoch.current;
    setSelectedRole(role);
    setFxParams(null);
    setFxDraft([]);
    setError(null);
    const fxSlot = fxSlots.find((candidate) => candidate.role === role)?.slot;
    if (!fxSlot) return;
    const operation = `fx-read:${fxSlot}`;
    setBusy(operation);
    try {
      const values = await onReadFxParameters(fxSlot);
      if (selectionEpoch.current !== epoch) return;
      setFxParams(values);
      setFxDraft(values);
    } catch (reason) {
      if (selectionEpoch.current === epoch) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy((current) => current === operation ? null : current);
    }
  };

  const apply = async (control: NanoAmpControl) => {
    if (busy) return;
    const value = draft[control];
    if (value == null) return;
    setBusy(`amp:${control}`); setError(null);
    try { await onSetAmp(control, value); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  };

  const toggleBypass = async (target: NanoBypassTarget, bypassed: boolean) => {
    if (busy) return;
    setBusy(`bypass:${target}`); setError(null);
    try { await onSetBypass(target, bypassed); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  };

  const applyFxParam = async (slot: NanoFxSlot, paramIndex: number) => {
    if (busy) return;
    const value = fxDraft[paramIndex];
    if (value == null) return;
    setBusy(`fx-write:${slot}:${paramIndex}`); setError(null);
    try {
      const values = await onSetFxParameter(slot, paramIndex, value);
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
    <Paper p="md" withBorder>
      <EditorCanvas label="Nano Cortex fixed signal chain" topology="nano-chain">
        {state.slots.map((slot, index) => <EditorBlockCard
          detail={slot.model_id == null ? undefined : `Model ${slot.model_id}`}
          eyebrow={roleNames[slot.role]}
          inspectorId="nano-slot-inspector"
          key={slot.role}
          onSelect={() => void selectRole(slot.role)}
          positionLabel={`Position ${index + 1}`}
          selected={selectedRole === slot.role}
          state={slot.bypassed == null ? "unknown" : slot.bypassed ? "bypassed" : "engaged"}
          title={slot.loaded_name ?? "Assigned by device"}
        />)}
      </EditorCanvas>
    </Paper>
    <InspectorPanel
      id="nano-slot-inspector"
      onClose={selectedRole ? () => { selectionEpoch.current += 1; setSelectedRole(null); setFxParams(null); } : undefined}
      summary={selectedState && <Text mt="sm">{roleNames[selectedState.role]} is {selectedState.bypassed == null ? "in an unknown state" : selectedState.bypassed ? "bypassed" : "engaged"}.</Text>}
      title={selectedState?.loaded_name ?? (selectedRole ? roleNames[selectedRole] : "Select a chain block")}
    >
      {!selectedRole && <Text c="dimmed" size="sm">Block details and available controls will appear here.</Text>}
      {selectedRole && !selectedFxSlot && <Text c="dimmed" size="sm">No editable parameters are available for this role yet.</Text>}
      {selectedFxSlot && busy === `fx-read:${selectedFxSlot}` && <Text c="dimmed" size="sm">Reading parameters...</Text>}
      {selectedFxSlot && fxParams != null && <>
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }}>
          {fxParams.map((value, i) => <Group align="flex-end" key={i} wrap="nowrap">
            <div style={{ flex: 1 }}>
              <Text size="xs" fw={600}>Param {i}</Text>
              <Slider aria-busy={busy === `fx-write:${selectedFxSlot}:${i}`} aria-label={`${roleNames[selectedRole ?? "pre_fx1"]} parameter ${i}`} disabled={busy !== null} label={(v) => v.toFixed(2)} max={1} min={0} onChange={(v) => setFxDraft((current) => { const next = [...current]; next[i] = v; return next; })} size="sm" step={0.001} value={fxDraft[i] ?? value} />
              <Text c="dimmed" size="xs">device: {value.toFixed(3)} | draft: {(fxDraft[i] ?? value).toFixed(3)}</Text>
            </div>
            <Button aria-busy={busy === `fx-write:${selectedFxSlot}:${i}`} aria-label={`Apply ${roleNames[selectedRole ?? "pre_fx1"]} parameter ${i}`} disabled={(busy !== null && busy !== `fx-write:${selectedFxSlot}:${i}`) || Math.abs((fxDraft[i] ?? value) - value) < 0.0005} onClick={() => void applyFxParam(selectedFxSlot, i)} size="xs">{busy === `fx-write:${selectedFxSlot}:${i}` ? "Applying..." : "Apply"}</Button>
          </Group>)}
        </SimpleGrid>
        <Text c="dimmed" size="xs">Provisional model-specific controls. Values are normalized 0.0-1.0; each Apply writes one parameter and verifies through fresh read-back.</Text>
      </>}
      {error && <Alert color="red" title="Nano operation failed">{error}</Alert>}
    </InspectorPanel>
    <Paper p="md" withBorder>
      <Text c="dimmed" fw={700} size="xs" tt="uppercase">Amp controls (raw 0-255)</Text>
      <SimpleGrid cols={{ base: 1, sm: 3, lg: 5 }} mt="sm">
        {(Object.keys(state.amp) as NanoAmpControl[]).map((control) => <Group align="flex-end" key={control} wrap="nowrap">
          <NumberInput aria-busy={busy === `amp:${control}`} clampBehavior="strict" label={control[0].toUpperCase() + control.slice(1)} max={255} min={0} onChange={(value) => setDraft((current) => ({ ...current, [control]: typeof value === "number" ? value : null }))} value={draft[control] ?? ""} />
          <Button aria-busy={busy === `amp:${control}`} aria-label={`Apply ${control}`} disabled={draft[control] == null || (busy !== null && busy !== `amp:${control}`)} onClick={() => void apply(control)}>{busy === `amp:${control}` ? "Applying..." : "Apply"}</Button>
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
              aria-label={`Bypass ${roleNames[role]}`}
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
  </Stack>;
}

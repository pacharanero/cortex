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

function slotName(slot: NanoCurrentState["slots"][number]): string {
  return slot.loaded_name ?? slot.model_name ?? (slot.model_id == null ? roleNames[slot.role] : `Unknown model ${slot.model_id}`);
}

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
  const [selectedRole, setSelectedRole] = useState<NanoSlotRole | null>(null);
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

  const selectedState = state.slots.find((slot) => slot.role === selectedRole) ?? null;
  const selectedFxSlot = fxSlots.find((slot) => slot.role === selectedRole)?.slot ?? null;

  const runFxReads = async (first: { slot: NanoFxSlot; epoch: number }) => {
    let request: { slot: NanoFxSlot; epoch: number } | null = first;
    while (request) {
      const { slot, epoch } = request;
      const role = fxSlots.find((item) => item.slot === slot)?.role ?? "pre_fx1";
      const operation = `fx-read:${slot}`;
      setBusy(operation);
      setError(null);
      setStatus(`Reading ${roleNames[role]} parameters...`);
      try {
        const values = await onReadFxParams(slot);
        if (selectionEpoch.current === epoch) {
          setFxParams(values);
          setFxDraft(values);
          fxEditEpoch.current = values.map(() => 0);
          setStatus(`${roleNames[role]} parameters loaded.`);
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

  const selectRole = (role: NanoSlotRole) => {
    if (busy && !busy.startsWith("fx-read:")) return;
    const request = {
      slot: fxSlots.find((candidate) => candidate.role === role)?.slot,
      epoch: ++selectionEpoch.current,
    };
    setSelectedRole(role);
    setFxParams(null);
    setFxDraft([]);
    fxEditEpoch.current = [];
    setError(null);
    if (!request.slot) {
      pendingFxRead.current = null;
      return;
    }
    const fxRequest = { slot: request.slot, epoch: request.epoch };
    if (busy?.startsWith("fx-read:")) {
      pendingFxRead.current = fxRequest;
      return;
    }
    void runFxReads(fxRequest);
  };

  const apply = async (control: NanoAmpControl) => {
    if (busy) return;
    const value = draft[control];
    if (value == null) return;
    const editEpoch = ampEditEpoch.current.get(control) ?? 0;
    setBusy(`amp:${control}`);
    setError(null);
    setStatus(`Applying ${control}...`);
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
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setStatus(`${control} failed.`);
    } finally {
      setBusy(null);
    }
  };

  const applyGateReduction = async () => {
    if (busy || typeof gateDraft !== "number") return;
    const editEpoch = gateEditEpoch.current;
    setBusy("gate:reduction");
    setError(null);
    setStatus("Applying Gate reduction...");
    try {
      await onSetGateReduction(gateDraft);
      if (gateEditEpoch.current === editEpoch) setGateDirty(false);
      setStatus("Gate reduction applied.");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setStatus("Gate reduction failed.");
    } finally {
      setBusy(null);
    }
  };

  const toggleBypass = async (target: NanoBypassTarget, bypassed: boolean) => {
    if (busy) return;
    setBusy(`bypass:${target}`);
    setError(null);
    setStatus(`Applying ${roleNames[target]} bypass...`);
    try {
      await onSetBypass(target, bypassed);
      setStatus(`${roleNames[target]} bypass applied.`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setStatus(`${roleNames[target]} bypass failed.`);
    } finally {
      setBusy(null);
    }
  };

  const applyFxParam = async (slot: NanoFxSlot, paramIndex: number) => {
    if (busy) return;
    const value = fxDraft[paramIndex];
    if (value == null) return;
    const epoch = selectionEpoch.current;
    const editEpoch = fxEditEpoch.current[paramIndex] ?? 0;
    const operation = `fx-write:${slot}:${paramIndex}`;
    setBusy(operation);
    setError(null);
    setStatus(`Applying FX parameter ${paramIndex}...`);
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
    } finally {
      setBusy((current) => current === operation ? null : current);
    }
  };

  const clearSelection = () => {
    selectionEpoch.current += 1;
    pendingFxRead.current = null;
    setSelectedRole(null);
    setFxParams(null);
    setFxDraft([]);
  };

  return <Stack gap="md">
    <Group justify="space-between">
      <div><Text c="dimmed" size="sm">Fixed signal chain</Text><Title order={3}>Nano Cortex</Title></div>
      <Badge color="orange" variant="outline">Amp, bypass and FX paths hardware verified</Badge>
    </Group>
    <Paper p="md" withBorder>
      <EditorCanvas label="Nano Cortex fixed signal chain" topology="nano-chain">
        {state.slots.map((slot, index) => <EditorBlockCard
          busy={busy !== null}
          detail={slot.model_id == null ? undefined : `Model ${slot.model_id}`}
          disabled={busy !== null && !busy.startsWith("fx-read:")}
          eyebrow={roleNames[slot.role]}
          inspectorId="nano-slot-inspector"
          key={slot.role}
          onSelect={() => selectRole(slot.role)}
          positionLabel={`Position ${index + 1}`}
          selected={selectedRole === slot.role}
          state={slot.bypassed == null ? "unknown" : slot.bypassed ? "bypassed" : "engaged"}
          title={slotName(slot)}
        />)}
      </EditorCanvas>
    </Paper>
    <InspectorPanel
      id="nano-slot-inspector"
      onClose={selectedRole ? clearSelection : undefined}
      summary={selectedState && <Text mt="sm">{roleNames[selectedState.role]} is {selectedState.bypassed == null ? "in an unknown state" : selectedState.bypassed ? "bypassed" : "engaged"}.</Text>}
      title={selectedState ? slotName(selectedState) : "Select a chain block"}
    >
      {!selectedRole && <Text c="dimmed" size="sm">Block details and available controls will appear here.</Text>}
      {selectedRole && !selectedFxSlot && <Text c="dimmed" size="sm">No editable parameters are available for this role yet.</Text>}
      {selectedFxSlot && fxParams === null && <Text c="dimmed" size="sm">Reading parameters...</Text>}
      {selectedFxSlot && fxParams != null && <>
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }}>
          {fxParams.map((value, index) => <Group align="flex-end" key={index} wrap="nowrap">
            <div style={{ flex: 1 }}>
              <Text fw={600} size="xs">Param {index}</Text>
              <Slider
                aria-busy={busy === `fx-write:${selectedFxSlot}:${index}`}
                disabled={busy !== null && busy !== `fx-write:${selectedFxSlot}:${index}`}
                label={(value) => value.toFixed(2)}
                max={1}
                min={0}
                onChange={(nextValue) => {
                  fxEditEpoch.current[index] = (fxEditEpoch.current[index] ?? 0) + 1;
                  setFxDraft((current) => { const next = [...current]; next[index] = nextValue; return next; });
                }}
                size="sm"
                step={0.001}
                thumbLabel={`${roleNames[selectedRole ?? "pre_fx1"]} parameter ${index} normalized value`}
                value={fxDraft[index] ?? value}
              />
              <Text c="dimmed" size="xs">device: {value.toFixed(3)} | draft: {(fxDraft[index] ?? value).toFixed(3)}</Text>
            </div>
            <Button
              aria-busy={busy === `fx-write:${selectedFxSlot}:${index}`}
              aria-label={`Apply ${roleNames[selectedRole ?? "pre_fx1"]} parameter ${index}`}
              disabled={(busy !== null && busy !== `fx-write:${selectedFxSlot}:${index}`) || Math.abs((fxDraft[index] ?? value) - value) < 0.0005}
              onClick={() => void applyFxParam(selectedFxSlot, index)}
              size="xs"
            >{busy === `fx-write:${selectedFxSlot}:${index}` ? "Applying..." : "Apply"}</Button>
          </Group>)}
        </SimpleGrid>
        <Text c="dimmed" size="xs">The normalized 0.0-1.0 path and this rendered Linux control are hardware-verified with device read-back and restoration. Values vary by loaded model.</Text>
      </>}
    </InspectorPanel>
    <Paper p="md" withBorder>
      <Text c="dimmed" fw={700} size="xs" tt="uppercase">Amp controls (raw 0-255)</Text>
      <SimpleGrid cols={{ base: 1, sm: 3, lg: 5 }} mt="sm">
        {(Object.keys(state.amp) as NanoAmpControl[]).map((control) => <Group align="flex-end" key={control} wrap="nowrap">
          <NumberInput
            aria-busy={busy === `amp:${control}`}
            clampBehavior="strict"
            label={control[0].toUpperCase() + control.slice(1)}
            max={255}
            min={0}
            onChange={(value) => {
              ampEditEpoch.current.set(control, (ampEditEpoch.current.get(control) ?? 0) + 1);
              setDraft((current) => ({ ...current, [control]: typeof value === "number" ? value : null }));
              setDirtyAmpControls((current) => new Set(current).add(control));
            }}
            value={draft[control] ?? ""}
          />
          <Button aria-busy={busy === `amp:${control}`} aria-label={`Apply ${control}`} disabled={draft[control] == null || (busy !== null && busy !== `amp:${control}`)} onClick={() => void apply(control)}>{busy === `amp:${control}` ? "Applying..." : "Apply"}</Button>
        </Group>)}
      </SimpleGrid>
      <Text c="dimmed" mt="sm" size="xs">Changes heard working state and saves nothing. Apply waits about six seconds for fresh device read-back.</Text>
    </Paper>
    <Paper p="md" withBorder>
      <Text c="dimmed" fw={700} size="xs" tt="uppercase">Gate / FX bypass</Text>
      <Group align="flex-end" mt="sm">
        <NumberInput
          aria-busy={busy === "gate:reduction"}
          clampBehavior="strict"
          label="Gate reduction"
          max={100}
          min={0}
          onChange={(value) => {
            gateEditEpoch.current += 1;
            setGateDraft(value);
            setGateDirty(true);
          }}
          style={{ flex: "1 1 160px" }}
          suffix="%"
          value={gateDraft}
        />
        <Button aria-busy={busy === "gate:reduction"} aria-label="Apply Gate reduction" disabled={typeof gateDraft !== "number" || !gateDirty || (busy !== null && busy !== "gate:reduction")} onClick={() => void applyGateReduction()}>{busy === "gate:reduction" ? "Applying..." : "Apply"}</Button>
        <Badge color="yellow" variant="outline">provisional</Badge>
      </Group>
      <SimpleGrid cols={{ base: 2, sm: 3, lg: 6 }} mt="sm">
        {bypassTargets.map(({ role, target }) => {
          const slot = state.slots.find((candidate) => candidate.role === role);
          const bypassed = slot?.bypassed;
          const stateLabel = bypassed == null ? "state unknown" : bypassed ? "bypassed" : "on";
          return <Group justify="space-between" key={target} wrap="nowrap">
            <Text fw={600} size="sm">{roleNames[role]}</Text>
            <Switch
              aria-busy={busy === `bypass:${target}`}
              aria-label={`${roleNames[role]} bypass, ${stateLabel}`}
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
    {status && <Text aria-live="polite" role="status" size="sm">{status}</Text>}
    {error && <Alert color="red" title="Nano operation failed">{error}</Alert>}
  </Stack>;
}

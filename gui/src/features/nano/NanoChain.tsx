// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Badge, Button, Group, NumberInput, Paper, SimpleGrid, Stack, Text, Title } from "@mantine/core";
import { useEffect, useState } from "react";
import type { NanoAmpControl, NanoCurrentState, NanoSlotRole } from "../../shared/ipc/types";

const roleNames: Record<NanoSlotRole, string> = {
  gate: "Gate", pre_fx1: "Pre FX 1", pre_fx2: "Pre FX 2", capture: "Capture",
  ir_cab: "IR / Cab", post_fx1: "Post FX 1", post_fx2: "Post FX 2", post_fx3: "Post FX 3",
};

interface NanoChainProps { state: NanoCurrentState; onSetAmp: (control: NanoAmpControl, value: number) => Promise<void> }

export function NanoChain({ state, onSetAmp }: NanoChainProps) {
  const [draft, setDraft] = useState(state.amp);
  const [busy, setBusy] = useState<NanoAmpControl | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (!busy) setDraft(state.amp); }, [state.amp, busy]);

  const apply = async (control: NanoAmpControl) => {
    const value = draft[control];
    if (value == null) return;
    setBusy(control); setError(null);
    try { await onSetAmp(control, value); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  };

  return <Stack gap="md">
    <Group justify="space-between">
      <div><Text c="dimmed" size="sm">Fixed signal chain</Text><Title order={3}>Nano Cortex</Title></div>
      <Badge color="orange" variant="outline">Amp editing hardware verified</Badge>
    </Group>
    <SimpleGrid cols={{ base: 2, sm: 4, lg: 8 }} spacing="xs">
      {state.slots.map((slot) => <Paper key={slot.role} p="sm" withBorder data-bypassed={slot.bypassed || undefined}>
        <Text c="dimmed" fw={700} size="xs" tt="uppercase">{roleNames[slot.role]}</Text>
        <Text fw={600} mt="xs">{slot.loaded_name ?? (slot.model_id == null ? "Assigned by device" : `Model ${slot.model_id}`)}</Text>
        <Text c={slot.bypassed ? "orange" : "dimmed"} size="xs">
          {slot.bypassed == null ? "state unavailable" : slot.bypassed ? "bypassed" : "on"}
        </Text>
      </Paper>)}
    </SimpleGrid>
    <Paper p="md" withBorder>
      <Text c="dimmed" fw={700} size="xs" tt="uppercase">Amp controls (raw 0-255)</Text>
      <SimpleGrid cols={{ base: 1, sm: 3, lg: 5 }} mt="sm">
        {(Object.keys(state.amp) as NanoAmpControl[]).map((control) => <Group align="flex-end" key={control} wrap="nowrap">
          <NumberInput aria-busy={busy === control} clampBehavior="strict" label={control[0].toUpperCase() + control.slice(1)} max={255} min={0} onChange={(value) => setDraft((current) => ({ ...current, [control]: typeof value === "number" ? value : null }))} value={draft[control] ?? ""} />
          <Button disabled={draft[control] == null || busy !== null} loading={busy === control} onClick={() => void apply(control)}>Apply</Button>
        </Group>)}
      </SimpleGrid>
      <Text c="dimmed" mt="sm" size="xs">Changes heard working state and saves nothing. Apply waits about six seconds for fresh device read-back.</Text>
      {error && <Alert color="red" mt="sm" title="Nano amp write failed">{error}</Alert>}
    </Paper>
  </Stack>;
}

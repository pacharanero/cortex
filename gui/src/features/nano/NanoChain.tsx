// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Badge, Button, Group, NumberInput, Paper, SimpleGrid, Stack, Switch, Text, Title } from "@mantine/core";
import { useEffect, useState } from "react";
import type { NanoAmpControl, NanoBypassTarget, NanoCurrentState, NanoSlotRole } from "../../shared/ipc/types";

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

interface NanoChainProps {
  state: NanoCurrentState;
  onSetAmp: (control: NanoAmpControl, value: number) => Promise<void>;
  onSetBypass: (target: NanoBypassTarget, bypassed: boolean) => Promise<void>;
}

export function NanoChain({ state, onSetAmp, onSetBypass }: NanoChainProps) {
  const [draft, setDraft] = useState(state.amp);
  const [busy, setBusy] = useState<string | null>(null);
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

  const toggleBypass = async (target: NanoBypassTarget, bypassed: boolean) => {
    setBusy(`bypass:${target}`); setError(null);
    try { await onSetBypass(target, bypassed); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  };

  return <Stack gap="md">
    <Group justify="space-between">
      <div><Text c="dimmed" size="sm">Fixed signal chain</Text><Title order={3}>Nano Cortex</Title></div>
      <Badge color="orange" variant="outline">Amp and bypass editing hardware verified</Badge>
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
      {error && <Alert color="red" mt="sm" title="Nano write failed">{error}</Alert>}
    </Paper>
  </Stack>;
}
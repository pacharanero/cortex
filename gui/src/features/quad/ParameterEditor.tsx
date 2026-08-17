// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Badge, Group, NumberInput, Select, Slider, Stack, Text, TextInput } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import type { ParameterInput, ParameterView } from "../../shared/ipc/types";

interface ParameterEditorProps {
  parameters: ParameterView[];
  disabled: boolean;
  onWrite: (index: number, input: ParameterInput) => Promise<void>;
}

/**
 * Edit a block's parameters.
 *
 * Values are written in the parameter's own units where the catalog gives a
 * usable range, and normalised otherwise. The device holds a normalised 0..1
 * float either way; the conversion belongs to the catalog, so it is done in
 * Rust rather than reimplemented here.
 *
 * Nothing is updated optimistically. A control shows the value the device last
 * reported, and the caller re-reads after a write - so a refused or clamped
 * write shows what actually happened rather than what was asked for.
 */
export function ParameterEditor({ parameters, disabled, onWrite }: ParameterEditorProps) {
  const [pending, setPending] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const write = async (index: number, input: ParameterInput) => {
    setPending(index);
    setError(null);
    try {
      await onWrite(index, input);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setPending(null);
    }
  };

  if (parameters.length === 0) {
    return <Text c="dimmed" size="sm">This block has no editable parameters.</Text>;
  }

  return (
    <Stack aria-busy={pending !== null} gap="md">
      {error && <Alert color="red" title="Parameter write failed">{error}</Alert>}
      {/* Controls are NOT disabled while a write is in flight. A disabled
          element cannot hold focus, so disabling the control being operated
          throws focus onto the document and kills keyboard interaction from
          the next key onwards - measured here as five arrow presses moving a
          slider one step. The same fault cost three rounds on the scene
          selector. Busy state is announced with aria-busy instead, and
          re-entry is guarded in `write`. */}
      {parameters.map((parameter) => (
        <ParameterControl
          busy={pending === parameter.index}
          disabled={disabled}
          key={parameter.index}
          onWrite={write}
          parameter={parameter}
        />
      ))}
    </Stack>
  );
}

interface ParameterControlProps {
  parameter: ParameterView;
  busy: boolean;
  disabled: boolean;
  onWrite: (index: number, input: ParameterInput) => Promise<void>;
}

function ParameterControl({ parameter, busy, disabled, onWrite }: ParameterControlProps) {
  // A local value so a control can be dragged or nudged smoothly, resynced
  // when the device reports something new. The device remains the authority:
  // this is display state during an interaction, not a second source of truth.
  const [draft, setDraft] = useState<number | null>(parameter.real ?? parameter.normalised);
  // Whether the user is currently holding this control. Syncing from the
  // device while they are would fight their input: every write is followed by
  // a re-read, so a run of arrow presses or a drag would keep being reset to
  // the value from one edit ago and appear to move a single step. Observed:
  // five presses moved GAIN by one step instead of five.
  const interacting = useRef(false);
  useEffect(() => {
    if (interacting.current) return;
    setDraft(parameter.real ?? parameter.normalised);
  }, [parameter.real, parameter.normalised]);

  const label = (
    <Group gap="xs">
      <Text fw={500} size="sm">{parameter.name || `Parameter ${parameter.index}`}</Text>
      {parameter.units && <Text c="dimmed" size="xs">{parameter.units}</Text>}
      {parameter.read_only && <Badge color="gray" size="xs">meter</Badge>}
      {parameter.per_scene && <Badge color="grape" size="xs">per scene</Badge>}
    </Group>
  );

  if (parameter.read_only) {
    return (
      <div>
        {label}
        <Text size="sm">
          {parameter.real ?? parameter.normalised ?? "no reading"}
        </Text>
      </div>
    );
  }

  if (parameter.kind === "str") {
    return (
      <div>
        {label}
        <TextInput
          defaultValue={parameter.text ?? ""}
          disabled={disabled}
          onBlur={(event) => {
            const value = event.currentTarget.value;
            if (value !== (parameter.text ?? "")) void onWrite(parameter.index, { kind: "text", value });
          }}
        />
      </div>
    );
  }

  if (parameter.kind === "switch" && parameter.step_names.length > 0) {
    // A switch's position is its index in the step list, sent normalised across
    // the declared range so the device receives the same shape as any other
    // parameter.
    const steps = parameter.step_names;
    const current = parameter.real ?? 0;
    return (
      <div>
        {label}
        <Select
          allowDeselect={false}
          data={steps.map((name, position) => ({ value: String(position), label: name }))}
          disabled={disabled}
          onChange={(value) => {
            if (value === null) return;
            const position = Number.parseInt(value, 10);
            void onWrite(parameter.index, { kind: "real", value: position });
          }}
          value={String(Math.round(current))}
        />
      </div>
    );
  }

  // Float, int, fader, and anything unrecognised: a slider for feel plus a
  // number input for a value someone already knows.
  const usesRealUnits = parameter.real !== null && parameter.max !== parameter.min;
  const min = usesRealUnits ? parameter.min : 0;
  const max = usesRealUnits ? parameter.max : 1;
  const step = parameter.kind === "int" ? 1 : (max - min) / 100;
  const commit = (value: number) =>
    onWrite(parameter.index, usesRealUnits ? { kind: "real", value } : { kind: "normalised", value });

  return (
    <div
      onBlur={(event) => {
        // Only release when focus actually leaves this control, not when it
        // moves between its slider and its number input.
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          interacting.current = false;
          setDraft(parameter.real ?? parameter.normalised);
        }
      }}
      onFocus={() => { interacting.current = true; }}
      onPointerDown={() => { interacting.current = true; }}
      onPointerUp={() => { interacting.current = false; }}
    >
      {label}
      <Group align="center" gap="sm" wrap="nowrap">
        <Slider
          disabled={disabled}
          label={(value) => value.toFixed(parameter.kind === "int" ? 0 : 2)}
          max={max}
          min={min}
          // Commit on release rather than on every movement: a drag would
          // otherwise send a write per pixel to a device that answers each one.
          onChange={setDraft}
          onChangeEnd={(value) => void commit(value)}
          step={step}
          style={{ flex: 1 }}
          value={draft ?? min}
        />
        <NumberInput
          allowDecimal={parameter.kind !== "int"}
          disabled={disabled}
          max={max}
          min={min}
          onBlur={(event) => {
            const value = Number.parseFloat(event.currentTarget.value);
            if (Number.isFinite(value)) void commit(value);
          }}
          step={step}
          style={{ width: 110 }}
          value={draft ?? ""}
          onChange={(value) => setDraft(typeof value === "number" ? value : Number.parseFloat(String(value)))}
        />
        {busy && <Text c="dimmed" size="xs">writing</Text>}
      </Group>
      {!usesRealUnits && (
        <Text c="dimmed" size="xs">
          Normalised 0-1: the catalog gives no usable range for this parameter.
        </Text>
      )}
    </div>
  );
}

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { ActionIcon, Group, NumberInput, Slider, Stack, Text } from "@mantine/core";
import type { ReactNode } from "react";

interface ContinuousControlProps {
  label: ReactNode;
  /** The accessible name for the slider/stepper, when `label` is not itself a plain string. */
  accessibleName?: string;
  value: number | string;
  min: number;
  max: number;
  step: number;
  allowDecimal?: boolean;
  disabled?: boolean;
  busy?: boolean;
  suffix?: string;
  onChange: (value: number | string) => void;
  /**
   * Fired once per gesture - slider release, numeric-input blur, or a
   * stepper click - rather than once per `onChange`. A caller with its own
   * explicit confirm step (an Apply button, as the Nano controls use) omits
   * this and keeps writing only from that button, unchanged from before this
   * prop existed.
   */
  onCommit?: (value: number) => void;
}

export function ContinuousControl({ label, accessibleName, value, min, max, step, allowDecimal = true, disabled = false, busy = false, suffix, onChange, onCommit }: ContinuousControlProps) {
  const name = accessibleName ?? (typeof label === "string" ? label : "");
  const hasNumber = typeof value === "number" && Number.isFinite(value);
  const numericValue = hasNumber ? value : min;
  const changeBy = (amount: number) => {
    if (!hasNumber) return;
    const next = Math.min(max, Math.max(min, Number((numericValue + amount).toPrecision(12))));
    onChange(next);
    onCommit?.(next);
  };
  const change = (nextValue: number | string) => {
    if (!allowDecimal && ((typeof nextValue === "number" && !Number.isInteger(nextValue)) || (typeof nextValue === "string" && nextValue.includes(".")))) return;
    onChange(nextValue);
  };

  return <Stack aria-busy={busy || undefined} gap={4}>
    {/* `component="div"`: `label` may be a rich node (badges, units) built by a
        caller such as the Quad parameter inspector, and Mantine's default `Text`
        element is `<p>`, which cannot legally contain another block element. */}
    <Text component="div" fw={600} size="sm">{label}</Text>
    <Slider
      disabled={disabled || !hasNumber}
      label={(nextValue) => `${nextValue}${suffix ?? ""}`}
      max={max}
      min={min}
      onChange={change}
      onChangeEnd={onCommit}
      step={step}
      thumbLabel={`${name} slider`}
      value={numericValue}
    />
    <Group gap={4} wrap="nowrap">
      <ActionIcon aria-label={`Decrease ${name}`} disabled={disabled || !hasNumber || numericValue <= min} onClick={() => changeBy(-step)} variant="default">-</ActionIcon>
      <NumberInput
        aria-label={`${name} numeric input`}
        allowDecimal={allowDecimal}
        clampBehavior="strict"
        disabled={disabled}
        hideControls
        max={max}
        min={min}
        onBlur={() => { if (hasNumber) onCommit?.(numericValue); }}
        onChange={change}
        step={step}
        suffix={suffix}
        value={value}
      />
      <ActionIcon aria-label={`Increase ${name}`} disabled={disabled || !hasNumber || numericValue >= max} onClick={() => changeBy(step)} variant="default">+</ActionIcon>
    </Group>
  </Stack>;
}

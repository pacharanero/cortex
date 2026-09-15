// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { ActionIcon, Group, NumberInput, Slider, Stack, Text } from "@mantine/core";

interface ContinuousControlProps {
  label: string;
  value: number | string;
  min: number;
  max: number;
  step: number;
  allowDecimal?: boolean;
  disabled?: boolean;
  busy?: boolean;
  suffix?: string;
  onChange: (value: number | string) => void;
}

export function ContinuousControl({ label, value, min, max, step, allowDecimal = true, disabled = false, busy = false, suffix, onChange }: ContinuousControlProps) {
  const hasNumber = typeof value === "number" && Number.isFinite(value);
  const numericValue = hasNumber ? value : min;
  const changeBy = (amount: number) => {
    if (!hasNumber) return;
    onChange(Math.min(max, Math.max(min, Number((numericValue + amount).toPrecision(12)))));
  };
  const change = (nextValue: number | string) => {
    if (!allowDecimal && ((typeof nextValue === "number" && !Number.isInteger(nextValue)) || (typeof nextValue === "string" && nextValue.includes(".")))) return;
    onChange(nextValue);
  };

  return <Stack aria-busy={busy || undefined} gap={4}>
    <Text fw={600} size="sm">{label}</Text>
    <Slider
      disabled={disabled || !hasNumber}
      label={(nextValue) => `${nextValue}${suffix ?? ""}`}
      max={max}
      min={min}
      onChange={change}
      step={step}
      thumbLabel={`${label} slider`}
      value={numericValue}
    />
    <Group gap={4} wrap="nowrap">
      <ActionIcon aria-label={`Decrease ${label}`} disabled={disabled || !hasNumber || numericValue <= min} onClick={() => changeBy(-step)} variant="default">-</ActionIcon>
      <NumberInput
        aria-label={`${label} numeric input`}
        allowDecimal={allowDecimal}
        clampBehavior="strict"
        disabled={disabled}
        hideControls
        max={max}
        min={min}
        onChange={change}
        step={step}
        suffix={suffix}
        value={value}
      />
      <ActionIcon aria-label={`Increase ${label}`} disabled={disabled || !hasNumber || numericValue >= max} onClick={() => changeBy(step)} variant="default">+</ActionIcon>
    </Group>
  </Stack>;
}

// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Badge } from "@mantine/core";
import type { CapabilityLabel, CapabilityStatus } from "./ipc/types";

/**
 * Text for every [`CapabilityStatus`], so evidence is conveyed by words
 * rather than colour alone (GUI-004.2, GUI-006.1). The wording is presentation
 * only: which status an operation actually has is decided entirely by the
 * Rust-owned matrix behind `cortexApi.capabilities()`, never here.
 */
const STATUS_TEXT: Record<CapabilityStatus, string> = {
  "confirmed-readable": "Hardware-verified (read)",
  "confirmed-writable": "Hardware-verified",
  inferred: "Inferred, not directly tested",
  unsupported: "Unsupported",
  unverified: "Not yet hardware-verified",
};

const STATUS_COLOR: Record<CapabilityStatus, string> = {
  "confirmed-readable": "teal",
  "confirmed-writable": "teal",
  inferred: "blue",
  unsupported: "gray",
  unverified: "yellow",
};

/**
 * Look up one operation's status in the fetched label list. An operation
 * absent from `labels` - whether the fetch has not resolved yet or the
 * backend genuinely omitted it - renders `unverified`, never confirmed by
 * omission.
 */
export function capabilityStatus(labels: CapabilityLabel[], operation: string): CapabilityStatus {
  return labels.find((label) => label.operation === operation)?.status ?? "unverified";
}

interface CapabilityBadgeProps {
  labels: CapabilityLabel[];
  operation: string;
  subject: string;
}

/** Renders one operation's evidence label as text, decorated with colour. */
export function CapabilityBadge({ labels, operation, subject }: CapabilityBadgeProps) {
  const status = capabilityStatus(labels, operation);
  return (
    <Badge color={STATUS_COLOR[status]} size="xs" variant="light">
      {subject}: {STATUS_TEXT[status]}
    </Badge>
  );
}

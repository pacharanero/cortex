// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, Group, Radio, Stack, Text } from "@mantine/core";
import { useState } from "react";
import type { SceneSnapshot } from "../../shared/ipc/types";

interface SceneSelectorProps {
  scenes: SceneSnapshot[];
  activeScene: number;
  disabled: boolean;
  onSwitch: (scene: number) => Promise<void>;
}

/**
 * Choose the active scene.
 *
 * Built as a radio group rather than a row of buttons because "exactly one of
 * eight is current" is what a radio group means, and a screen reader gets the
 * set size, the position in it, and the current selection without any extra
 * markup. The scene colour is decorative here: the letter and label carry the
 * same information as text, so nothing depends on colour alone.
 *
 * Switching scenes is non-persistent - it changes what the unit is playing and
 * saves nothing - so it needs no confirmation, but it is a real audible change
 * and is announced.
 */
export function SceneSelector({ scenes, activeScene, disabled, onSwitch }: SceneSelectorProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");

  const describe = (scene: SceneSnapshot) =>
    scene.label ? `${scene.letter} - ${scene.label}` : `${scene.letter} - unlabelled`;

  const change = async (value: string) => {
    // The radio value is a string; the API takes the zero-based index, so the
    // conversion happens once, here, and never turns into a letter.
    const scene = Number.parseInt(value, 10);
    const target = scenes.find((candidate) => candidate.index === scene);
    if (!target || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSwitch(scene);
      setAnnouncement(`Scene ${describe(target)} active`);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setError(message);
      // Say what did not happen, rather than leaving the last success standing.
      setAnnouncement(`Scene ${target.letter} was refused: ${message}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Stack gap="xs">
      <Radio.Group
        description="Changes what the unit plays now. Nothing is saved."
        label="Active scene"
        onChange={(value) => void change(value)}
        value={String(activeScene)}
      >
        <Group gap="sm" mt="xs" wrap="wrap">
          {scenes.map((scene) => (
            <Radio
              disabled={disabled || busy}
              key={scene.index}
              label={describe(scene)}
              // Mantine's default `sm` radio is 20px, under the 24x24 CSS px
              // that WCAG 2.2 AA (2.5.8 Target Size) asks for. `md` is 24.
              size="md"
              value={String(scene.index)}
            />
          ))}
        </Group>
      </Radio.Group>

      {error && <Alert color="red" title="Scene switch failed">{error}</Alert>}

      {/* Device-originated and command-completion changes are announced here so
          the switch is perceivable without watching the radio group. */}
      <Text aria-live="polite" className="visually-hidden" role="status">
        {announcement}
      </Text>
    </Stack>
  );
}

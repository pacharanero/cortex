// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Alert, ColorInput, Group, Radio, Stack, Text, TextInput } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import { CapabilityBadge } from "../../shared/CapabilityBadge";
import type { CapabilityLabel, SceneSnapshot } from "../../shared/ipc/types";

interface SceneSelectorProps {
  scenes: SceneSnapshot[];
  activeScene: number;
  /** Physical-session generation the current props were read under (`LiveSnapshot.generation`). */
  generation: number;
  /** Monotonic state revision within that generation (`LiveSnapshot.revision`). */
  revision: number;
  disabled: boolean;
  onSwitch: (scene: number) => Promise<void>;
  onRename: (scene: number, label: string | null) => Promise<void>;
  onRecolour: (scene: number, color: number) => Promise<void>;
  /** Evidence labels for `switch_scene`/`set_scene_label`/`set_scene_color`. */
  capabilities?: CapabilityLabel[];
}

/** `0xAARRGGBB` from the device to the `#rrggbb` an input wants. */
function toHex(color: number | null): string {
  if (color === null) return "#ffffff";
  return `#${(color & 0x00ffffff).toString(16).padStart(6, "0")}`;
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
 *
 * The device can also change scene on its own - a footswitch press, or
 * another client - and that has to reach a screen reader too, without moving
 * focus and without echoing the announcement `change()` already made for a
 * switch this control itself requested. `generation`/`revision` (as in
 * DES-SNAPSHOT) let a device-report effect tell a genuinely newer same-session
 * report apart from an unrelated poll, a metadata-only push, a stale reply
 * that settled late, or a reconnect starting a new generation - none of which
 * name an actual scene transition.
 */
export function SceneSelector({ scenes, activeScene, generation, revision, disabled, onSwitch, onRename, onRecolour, capabilities = [] }: SceneSelectorProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const groupRef = useRef<HTMLDivElement>(null);
  // Set when the user is driving with the keyboard, so focus is only pulled
  // back to the selected radio for them - never while they are using a mouse.
  const followFocus = useRef(false);
  // The last report this component has seen, so the device-report effect can
  // tell a newer report from a stale or unrelated one. Starts `null` so the
  // effect stays silent on mount rather than announcing the initial scene.
  const lastReport = useRef<{ generation: number; revision: number; activeScene: number } | null>(null);
  // Set by `change()` just before it awaits `onSwitch`, so the read-back that
  // follows a successful local switch does not also fire the device-report
  // announcement `change()` already made itself.
  const ownTransition = useRef(false);

  const describe = (scene: SceneSnapshot) =>
    scene.label ? `${scene.letter} - ${scene.label}` : `${scene.letter} - unlabelled`;

  // Announce a scene the *device* reports, never the one requested: a report
  // is only a transition worth announcing if it is strictly newer within the
  // same generation the previous report belonged to, and it actually changed
  // `active_scene` - guarding out unchanged polls, metadata-only revisions,
  // a stale/replayed report, and a reconnect's replacement generation.
  useEffect(() => {
    const previous = lastReport.current;
    lastReport.current = { generation, revision, activeScene };
    if (!previous) return;
    if (generation !== previous.generation) return;
    if (revision <= previous.revision) return;
    if (activeScene === previous.activeScene) return;
    if (ownTransition.current) {
      ownTransition.current = false;
      return;
    }
    const scene = scenes.find((candidate) => candidate.index === activeScene);
    if (!scene) return;
    setAnnouncement(`Scene ${describe(scene)} active`);
    // Deliberately no focus change: a device-originated transition must not
    // steal focus from whatever control the user is actually driving.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [generation, revision, activeScene]);

  const focusScene = (index: number) =>
    groupRef.current
      ?.querySelector<HTMLInputElement>(`input[type=radio][value="${index}"]`)
      ?.focus();

  // Focus has to be restored *after* React commits, not straight after the
  // await: re-rendering the group replaces the input nodes, so focusing from
  // inside the handler lands on a node that is about to be discarded. WebKitGTK
  // then has focus nowhere, and the next arrow press goes to the document -
  // which is exactly the "works once, then stops" behaviour observed on real
  // hardware. An effect keyed on the scene runs after the commit, so the node
  // it focuses is the one actually on screen.
  useEffect(() => {
    if (!followFocus.current) return;
    followFocus.current = false;
    focusScene(activeScene);
  }, [activeScene]);

  const change = async (value: string) => {
    // The radio value is a string; the API takes the zero-based index, so the
    // conversion happens once, here, and never turns into a letter.
    const scene = Number.parseInt(value, 10);
    const target = scenes.find((candidate) => candidate.index === scene);
    if (!target || busy) return;
    setBusy(true);
    setError(null);
    // Set before the await: the read-back this switch causes will change
    // `activeScene`/`revision` props once it lands, and the device-report
    // effect above must recognise that transition as this control's own
    // rather than announcing it a second time.
    ownTransition.current = true;
    try {
      await onSwitch(scene);
      setAnnouncement(`Scene ${describe(target)} active`);
    } catch (reason) {
      // The switch was refused, so no read-back is coming to consume the
      // flag - clear it now, or a later genuine device report would be
      // silently swallowed as if it were this failed attempt's echo.
      ownTransition.current = false;
      const message = reason instanceof Error ? reason.message : String(reason);
      setError(message);
      // Say what did not happen, rather than leaving the last success standing.
      setAnnouncement(`Scene ${target.letter} was refused: ${message}`);
    } finally {
      setBusy(false);
    }
  };

  // Arrow-key navigation is implemented here rather than inherited from the
  // browser's native radio-group behaviour, because that behaviour is not
  // universal: it works in Chromium but does nothing in the WebKitGTK webview
  // Tauri uses on Linux, which is the app's only shipping target today
  // (hardware-checked 2026-08-16). Handling the keys ourselves also makes the
  // behaviour identical on every engine instead of engine-defined, so this is
  // not merely a workaround. Follows the WAI-ARIA radio group pattern:
  // arrows move and select with wrap-around, Home and End jump to the ends.
  const keyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (disabled || busy || scenes.length === 0) return;
    const current = scenes.findIndex((scene) => scene.index === activeScene);
    const from = current === -1 ? 0 : current;
    let next: number;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        next = (from + 1) % scenes.length;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        next = (from - 1 + scenes.length) % scenes.length;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = scenes.length - 1;
        break;
      default:
        return;
    }
    // Claim the key even where the engine would have acted, so one press can
    // never move two places.
    event.preventDefault();
    followFocus.current = true;
    void change(String(scenes[next].index));
  };

  return (
    <Stack gap="xs">
      <Group gap="xs">
        <Text c="dimmed" fw={700} size="xs" tt="uppercase">Scene switching</Text>
        <CapabilityBadge labels={capabilities} operation="switch_scene" subject="Scene switching" />
      </Group>
      <Radio.Group
        description="Changes what the unit plays now. Nothing is saved."
        label="Active scene"
        onChange={(value) => void change(value)}
        value={String(activeScene)}
      >
        {/* The key and click handlers sit on a plain element rather than on
            Mantine's Group, because they have to reach the DOM: relying on a
            component library to forward onClick/onKeyDown/ref is a silent
            failure when it does not, and that is exactly what happened here.

            A click has to leave focus inside the group, or the arrow keys look
            broken immediately afterwards - which is what real hardware showed.
            Mantine renders the label as a sibling of the input (`for=`), not a
            wrapper, so clicking the text does not reliably focus the input; and
            focusing it here would not survive the re-render the switch causes.
            So a click marks the same after-commit focus path the keys use, and
            also focuses directly for the case where the scene does not change
            and no re-render follows. Pointer and keyboard have to leave the
            control in the same state, or they are not equivalent. */}
        <div
          aria-busy={busy}
          onClick={(event) => {
            const radio = (event.target as HTMLElement)
              .closest<HTMLElement>("[class*='Radio-root']")
              ?.querySelector<HTMLInputElement>("input[type=radio]");
            if (!radio) return;
            followFocus.current = true;
            radio.focus();
          }}
          onKeyDown={keyDown}
          ref={groupRef}
        >
          <Group gap="sm" mt="xs" wrap="wrap">
            {scenes.map((scene) => (
              <Radio
                // Deliberately NOT disabled while a switch is in flight. A
                // disabled input cannot hold focus, so disabling the control
                // being operated strands focus on the document and silently
                // kills keyboard navigation and screen-reader context - which
                // is precisely the "arrow keys stop working" fault seen on
                // hardware. Re-entry is prevented in `change` instead, and the
                // in-flight state is announced with aria-busy.
                disabled={disabled}
                key={scene.index}
                label={describe(scene)}
                // Mantine's default `sm` radio is 20px, under the 24x24 CSS px
                // that WCAG 2.2 AA (2.5.8 Target Size) asks for. `md` is 24.
                size="md"
                value={String(scene.index)}
              />
            ))}
          </Group>
        </div>
      </Radio.Group>

      {error && <Alert color="red" title="Scene switch failed">{error}</Alert>}

      <SceneDetails
        capabilities={capabilities}
        disabled={disabled}
        onRecolour={onRecolour}
        onRename={onRename}
        scene={scenes.find((candidate) => candidate.index === activeScene) ?? null}
      />

      {/* Device-originated and command-completion changes are announced here so
          the switch is perceivable without watching the radio group. */}
      <Text aria-live="polite" className="visually-hidden" role="status">
        {announcement}
      </Text>
    </Stack>
  );
}

interface SceneDetailsProps {
  scene: SceneSnapshot | null;
  disabled: boolean;
  onRename: (scene: number, label: string | null) => Promise<void>;
  onRecolour: (scene: number, color: number) => Promise<void>;
  capabilities: CapabilityLabel[];
}

/**
 * Rename and recolour the active scene.
 *
 * A full colour picker rather than the unit's own eight-colour palette,
 * because the hardware genuinely accepts arbitrary RGB: CorOS 4.0.1 stored and
 * read back off-palette `0xFF808080` exactly, and stepping through the scenes
 * on a real unit showed the reported colours on the physical LEDs
 * (2026-08-16). Reproducing a fixed palette would be a self-imposed limit.
 *
 * Both edits are non-persistent: they change the working copy and save
 * nothing.
 */
function SceneDetails({ scene, disabled, onRename, onRecolour, capabilities }: SceneDetailsProps) {
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  if (!scene) return null;

  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    setFailure(null);
    try {
      await work();
    } catch (reason) {
      setFailure(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Stack gap="xs">
      <Group gap="xs">
        <CapabilityBadge labels={capabilities} operation="set_scene_label" subject="Scene name" />
        <CapabilityBadge labels={capabilities} operation="set_scene_color" subject="Scene colour" />
      </Group>
      <Group align="flex-end" gap="sm" wrap="wrap">
        <TextInput
          // Keyed by scene so switching scenes reloads the field rather than
          // carrying the previous scene's text into it.
          key={`label-${scene.index}`}
          defaultValue={scene.label ?? ""}
          description="Blank clears the label"
          // NOT disabled while busy: a disabled input cannot hold focus, and
          // dropping focus mid-edit is the fault this file already records.
          disabled={disabled}
          label={`Scene ${scene.letter} name`}
          onBlur={(event) => {
            const value = event.currentTarget.value;
            if (value !== (scene.label ?? "")) void run(() => onRename(scene.index, value || null));
          }}
          style={{ flex: 1, minWidth: 180 }}
        />
        <ColorInput
          key={`colour-${scene.index}`}
          disabled={disabled}
          format="hex"
          label={`Scene ${scene.letter} colour`}
          onChangeEnd={(value) => {
            const parsed = Number.parseInt(value.replace("#", ""), 16);
            if (Number.isFinite(parsed)) void run(() => onRecolour(scene.index, parsed));
          }}
          style={{ width: 200 }}
          value={toHex(scene.color)}
          withEyeDropper={false}
        />
        {busy && <Text c="dimmed" size="xs">writing</Text>}
      </Group>
      {failure && <Alert color="red" title="Scene edit failed">{failure}</Alert>}
    </Stack>
  );
}

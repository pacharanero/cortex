# GUI First Draft

The first GUI draft is a Quad Cortex-specific, read-only interactive shell. It is intentionally a browser-runnable demo until the backend can safely route through the held `cortex session start` daemon without opening a second HID connection.

It adapts the mockable IPC-boundary architecture of `rixrix/deskop-nano-cortex` (Apache-2.0), while independently implementing the Quad-specific model and Mantine presentation. See `NOTICE` and `THIRD-PARTY-NOTICES.md`.

Run `s/gui-dev` after `npm install` in `gui/`.

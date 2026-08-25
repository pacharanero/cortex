# `s/`

Small scripts naming the repeated processes of working on this repository. Each is a thin wrapper round a real tool; the script is the canonical way to invoke that process, so a fresh clone finds the right entry point by listing one directory.

Run them from anywhere in the repo.

## `s/test`

Run the Rust and frontend build/test gate, including the traceability and documentation-quality fixture suites. Run it with `s/lint` before committing; CI additionally checks the Windows host boundary and Tauri integration build.

- `s/test` - everything

## `s/lint`

Run the lint half of the local gate: Rust formatting and clippy on both feature paths, frontend checks, Markdown and REUSE lint, rendered documentation links, nav completeness, spelling, device-data protection, version synchronization, and traceability validation. Run it with `s/test` before committing.

- `s/lint`

## `s/markdownlint`

Lint the repository's Markdown against `.markdownlint.jsonc`. Run by `s/lint` and by CI; run it directly while writing docs or specs.

Markdown is a first-class output here - the docs site, the zone specs, and the protocol reference are all Markdown - so it gets the same gate as the code. Most findings are cosmetic, but two classes are real defects: `MD056` catches an unescaped `|` inside a table cell, which silently truncates that row in the rendered docs, and `MD040` catches a fenced block with no language.

The linter runs through `npx` at a pinned version, so a local run and CI check the same rules. Config divergences from the house-style template each carry their reason inline in `.markdownlint.jsonc`.

- `s/markdownlint` - lint every tracked Markdown file
- `s/markdownlint --fix` - apply the auto-fixable findings
- `s/markdownlint docs/*.md` - lint a subset

## Documentation Quality

These checks run from `s/lint`, with their isolated fixtures run by `s/test` and CI:

- `s/check-docs-nav` - require every `docs/*.md` page to appear in `mkdocs.yml` or `.nav-exceptions`, and reject nav entries or exceptions naming missing pages
- `s/linkcheck` - build with Zensical, then validate the rendered site's internal links, image resources and anchors
- `s/spellcheck` - run the codespell version pinned in `requirements.txt` against `docs/`

Install and activate the documentation environment first:

```sh
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
source .venv/bin/activate
```

## `s/check-traceability`

Validate every existing Rust `@see` header against its target document and node identifier. Each covered file must link to at least one living zone `spec.md` or `design.md`; progress-ledger links may supplement that target but cannot replace it. Run by `s/lint` and CI, with parser behavior covered by `tests/check-traceability.sh` in `s/test` and CI.

- `s/check-traceability`

## `s/install-hooks`

Install this repository's tracked Git hooks for your checkout. The `pre-commit` hook runs `s/lint`.

Opt-in per checkout, and deliberately not installed by any other script: a clone should never silently gain a commit-time side effect. Sets `core.hooksPath` rather than copying into `.git/hooks`, which Git does not track and which goes stale the moment the tracked hook changes.

- `s/install-hooks` - start using the tracked hooks
- `s/install-hooks -u` - stop using them
- `git commit --no-verify` - skip the hook for one commit

## `s/install`

Install the `cortex` CLI and `cortex-mcp` server from this checkout onto your `PATH`.

Needed because the obvious command does not work here: the workspace root has no `[package]`, so `cargo install --path .` fails. The binary lives in `crates/cortex-cli`.

- `s/install` - install `cortex` and `cortex-mcp`
- `s/install --cli-only` - install only `cortex`
- `s/install --force` - reinstall over an existing copy
- `s/install --debug` - faster build, slower runtime
- `s/install --mcp` - compatibility alias; MCP is installed by default

Also preflights the udev rule and prints the fix if it is missing.

## `s/lint-no-device-data`

Fail if anything shaped like real device data - a serial number, MAC address, or firmware checksum - has crept into a tracked file. Run by `s/lint` and by CI.

Documentation examples are captured from a physical unit, so this is easy to do by accident and impossible to undo once published. Works by allow-list rather than deny-list, so the values being kept out never have to be committed.

- `s/lint-no-device-data`

## `s/progress`

Where the project is up to, counted from `spec/roadmap.md` - the single progress record. Counted rather than written, so the summary cannot drift from the thing it summarises.

- `s/progress` - per-section totals
- `s/progress --wip` - what is in progress
- `s/progress --next` - what is planned

## `s/docs`

Serve the documentation site locally with hot reload, on the first free port in 8000-8030, opening a browser.

One-off setup:

```sh
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
```

- `s/docs`
- `s/docs --dev-addr localhost:9000` - override the auto-picked port

## `s/docs-cli-reference`

Regenerate `docs/cli-reference.md` from the CLI's own `--help`, so the reference cannot drift from the real command surface. Run after adding or changing a command, and commit the result.

- `s/docs-cli-reference`

## `s/version++`

Bump the version, commit, and land the release commit on `main`. **This pushes.**

The landing *is* the release: the auto-tag workflow creates the tag once the commit reaches `main`. There is deliberately no `s/release` - a second script that also tagged was a source of divergence between repos.

Runs the full `s/lint` gate before touching the version, so a failure leaves no half-bumped tree.

- `s/version++` - patch bump (0.0.1 -> 0.0.2)
- `s/version++ minor` / `s/version++ major`
- `s/version++ --pr` - open a release PR instead of pushing to `main`

Every crate takes `version.workspace = true`, so the single version in the root `Cargo.toml` is the only one to bump. When the Tauri app lands, its `package.json` and `tauri.conf.json` have to move in the same commit.

## `s/release-preview`

Build the non-publishing Linux x86_64 release preview using the cargo-dist version declared in `Cargo.toml`. It validates the release plan, produces one archive with `cortex`, `cortex-mcp`, licences/notices, and the udev rule, writes a portable `SHA256SUMS`, then verifies both archive contents and checksum. It never tags, hosts, or publishes.

- `s/release-preview`

## `s/usb-trace`

Record the USB traffic to and from the Quad Cortex, using `usbmon` on the Linux host.

This is how to answer "what does the official client actually do". With the unit passed through to the Windows VM running Cortex Control, the host kernel still sees every transfer - QEMU's passthrough goes out through the host's own USB stack - so nothing needs installing inside Windows. It works equally well on our own client, which is the cheaper use: it shows what is on the wire rather than what our RX path believes arrived.

Named `usb-trace` rather than `usb-capture` because "capture" already means a Neural Capture in this domain, and `trace` is the project's existing word for protocol observation (`CORTEX_TRACE`).

- `s/usb-trace` - capture until Ctrl-C
- `s/usb-trace --seconds 30` - capture a fixed window
- `s/usb-trace --output FILE` - choose the file

One-off setup, which the script's preflight will prescribe if missing:

```sh
sudo pacman -S wireshark-cli
sudo modprobe usbmon
```

Writes to `traces/`, which is gitignored. **Captures must not be committed**: they hold Neural DSP's strings verbatim alongside the unit's serial number and the owner's preset names.

## `s/usb-decode`

Read a capture as Cortex Control messages rather than raw USB frames.

Drives `tshark` and pipes it into `cortex decode-trace`, which reassembles using the crate's own framing, trailer and gzip code - so the decoder cannot disagree with what the client actually does.

- `s/usb-decode traces/foo.pcapng` - decode a capture
- `s/usb-decode --live` - decode as it happens
- `s/usb-decode --quiet` - counts only

`--live` is the development monitor. Run it in a second terminal and every message to and from the unit appears as it goes past, whoever sent it - our client, Cortex Control in the VM, or the unit's own front panel. Wireshark's GUI can watch the same traffic but knows nothing of our framing, so it shows 129-byte blobs where this shows messages.

It was a capture of our *own* client that found the writer starvation in `session.rs`, which had been costing up to 100 seconds a handshake and had been misattributed to the device for a full day. Reach for this earlier than feels necessary.

## `s/hardware-smoke`

Run deterministic end-to-end CLI checks against a real Quad Cortex. It reads every write back through a separate command, saves only within one explicitly disposable scratch bank, moves the created fixture to the next scratch slot and back, deletes it, and restores the named starting slot even after a failed check or Ctrl-C.

- `s/hardware-smoke --scratch-bank 31 --restore-slot 1A --discard-working-copy`
- `s/hardware-smoke --scratch-bank 31 --restore-slot 1A --discard-working-copy --setlist "/media/p4/Presets/My Presets"`

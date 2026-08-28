---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["dx", "tooling", "scripts", "editorconfig", "lint", "test"]
spec: spec.md
---

# 500 DX Tooling - Design

> Design for the repo scripts and lint/format/test config. The local gate is explicit about the checks CI adds; the scripts run from any working directory.

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [house-style scripts.md](https://github.com/marcus-pacharanero/house-style/blob/main/scripts.md) - the `s/` script conventions.
- [600-ci-release design](../600-ci-release/design.md) - the CI workflow this zone mirrors locally.
- Owned source: `s/test`, `s/lint`, `.editorconfig`.

## [DES-SCRIPTS] The `s/` scripts

### Behaviour

The `s/` scripts are executable bash files at the repo root in `s/`. Each carries an SPDX header, a one-line description, `set -euo pipefail`, and a `cd` to the repo root via `git rev-parse --show-toplevel`. They are the canonical entry points so a maintainer or agent does not have to remember the exact incantations.

### Design choice: the canonical local test path

`s/test` runs the repository's build and test gate:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo test --all --no-default-features
(cd gui && npm run check)
tests/check-traceability.sh
tests/check-docs-nav.sh
tests/linkcheck.sh
tests/spellcheck.sh
tests/install-release.sh
```

CI additionally runs no-default workspace clippy, real-device-data and version/traceability lint, RustSec, Zizmor, REUSE, Windows host-boundary checks, the Tauri build boundary and platform setup. The split local contract is `s/test` plus `s/lint`; release commits run both through `s/version++` before changing any manifest.

### Design choice: `reuse` is optional in `s/lint`

`s/lint` runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `reuse lint`. But `reuse` is a Python tool (`pip install reuse`) that may not be installed. The script degrades gracefully:

```sh
if command -v reuse >/dev/null 2>&1; then
    reuse lint
else
    echo "s/lint: 'reuse' not installed; skipping SPDX lint (pip install reuse)" >&2
fi
```

The message goes to stderr so it does not corrupt output. The script does not fail when `reuse` is missing so a contributor can run the Rust checks, but maintainers must install REUSE before committing and CI always enforces it.

### Design choice: `cd` to repo root

Every `s/` script starts with `cd "$(git rev-parse --show-toplevel)"` so it runs from any working directory. This is the house-style rule (tauri-gui.md: "`s/gui-dev` is the canonical entry point. Do not make contributors remember `cd gui && npm run tauri dev`."). A maintainer in a subdirectory running `s/test` gets the same result as one at the root.

### Alternatives considered

- **A `Makefile` or `justfile`.** Rejected: the house style is `s/` scripts. They are discoverable, tab-completable, and do not require a tool beyond bash.
- **A Rust task runner (`xtask`).** Rejected for the scripts: they are thin shell wrappers over `cargo` and `reuse`; a Rust binary is overkill. An `xtask` may be worth it if the scripts grow complex (e.g. `s/version++` with TOML parsing), but not yet.
- **Fail hard when `reuse` is missing.** Rejected: it would block a contributor who has not installed Python tooling. The CI gate catches it; the local gate is a convenience.

## [DES-EDITORCONFIG] `.editorconfig`

### Behaviour

`.editorconfig` at the repo root enforces editor-agnostic style: UTF-8, LF line endings, 4-space indent (2 for `*.md`/`*.yaml`/`*.yml`/`*.json`/`*.toml`, 4 for `*.rs`), final newline, trailing-whitespace trim. `root = true` so no parent `.editorconfig` is consulted.

### Design choice: `root = true`

The repo is a git workspace root. A parent `.editorconfig` (e.g. in `~/`) should not override the repo's style. `root = true` stops the lookup.

### Design choice: 2-space indent for prose/config, 4-space for Rust

Rust is 4-space (the `cargo fmt` default). Markdown, YAML, JSON, TOML are 2-space (the house-style convention for config and prose). This matches the existing specs and `Cargo.toml`.

### Alternatives considered

- **4-space everywhere.** Rejected: 2-space for YAML/JSON/TOML is the house-style convention and matches the existing files.
- **No `.editorconfig`.** Rejected: editors without it guess the style; the file makes the style explicit and uniform.

## [DES-FUTURE] Scripts and remaining work

### `s/gui-dev`

Implemented now that `gui/` exists (zone 400). Runs the Tauri dev server from any working directory:

```sh
#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
cd gui
npm run tauri dev
```

This is the house-style rule (tauri-gui.md): the script is the canonical entry point, not `cd gui && npm run tauri dev`.

### `s/version++`

Implemented for the Rust workspace, npm package/lock, Tauri configuration and release commit flow. It requires a clean `main`, runs `s/test` and `s/lint` before changing any version, and leaves the tree untouched if either gate fails. `s/check-versions` enforces equality outside release runs.

A thin bash script works for `Cargo.toml` (toml-edit) and `gui/package.json` (jq). If the parsing gets complex, this is the one script that might justify an `xtask` Rust binary.

### Markdown lint

A `.markdownlint.json` enforcing the house-style prose rules (no hard-wrap, heading style, no emphasis as heading, etc.) will run in `s/lint` and CI. The rules will match house-style docs.md and the existing spec files (which use long-line paragraphs, not hard-wrapped at 80).

### `s/install-hooks` and `.githooks/pre-commit`

`s/install-hooks` runs `git config core.hooksPath .githooks` so the `.githooks/` directory is the hook source. `.githooks/pre-commit` runs a fast subset (fmt + clippy, not the full test suite) and refuses the commit on failure. `s/test` remains the full pre-push gate.

The pre-commit hook should be opt-in (a script the maintainer runs), not forced on every clone. Forcing hooks on contributors who do not expect them is hostile.

## [DES-LIMITS] Known Limitations

- **Local CI parity is deliberately split.** The complete local gate is `s/test` plus `s/lint`; CI additionally owns platform setup, Windows cross-checks, the Tauri integration build, RustSec, Zizmor and mandatory REUSE availability.
- **Release hosting remains externally gated.** `s/version++` prepares and lands the release commit, while post-merge CI and the protected `release` environment retain tag and publication authority.

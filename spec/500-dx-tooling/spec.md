---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["dx", "tooling", "lint", "format", "test", "scripts", "editorconfig"]
---

# 500 DX Tooling - Spec

> The repo scripts and lint/format/test configuration that make the local workflow match CI. `s/test` and `s/lint` are the canonical gates; `.editorconfig` and `cargo fmt`/`clippy` keep the style uniform. Owns the developer-experience surface that a maintainer or agent runs before every commit.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [AGENTS.md](../../AGENTS.md) - the workflow section (the `s/` scripts) and the "Before Every Commit" gate.
- [house-style scripts.md](https://github.com/marcus-pacharanero/house-style/blob/main/scripts.md) - the `s/` script conventions.
- [house-style testing.md](https://github.com/marcus-pacharanero/house-style/blob/main/testing.md) - the testing conventions.
- [house-style ci.md](https://github.com/marcus-pacharanero/house-style/blob/main/ci.md) - the CI conventions this zone mirrors locally.
- [600-ci-release spec](../600-ci-release/spec.md) - the CI workflow that `s/test` mirrors.
- Owned source: `s/test`, `s/lint`, `.editorconfig`.

## Problem Statement

A maintainer or agent running `s/test` locally should get the same gate CI runs: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all`, and `reuse lint`. If the local gate is green, CI is green. The `s/` scripts are the canonical entry point so no one has to remember the exact incantations.

This zone owns the scripts, the `.editorconfig`, and the lint/format config. The CI workflow itself is owned by zone 600; this zone mirrors it locally. The GUI dev script (`s/gui-dev`) and the version-bump script (`s/version++`) are planned; they land when `gui/` exists and when the release pipeline is wired.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| `s/test` runs fmt + clippy + tests | Implemented | `s/test` runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all` |
| `s/lint` runs fmt + clippy + REUSE | Implemented | `s/lint` runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `reuse lint` (with a fallback message if `reuse` is not installed) |
| `.editorconfig` enforces UTF-8, LF, 4-space indent (2 for md/yaml/json), final newline | Implemented | `.editorconfig` at repo root |
| `cargo fmt` and `cargo clippy` run in CI | Implemented | `.github/workflows/ci.yml` (owned by zone 600) |
| `s/gui-dev` and `s/version++` | Planned | Not yet implemented; `s/gui-dev` lands when `gui/` exists, `s/version++` lands with the release pipeline |
| Markdown lint | Planned | Not yet wired |

## User Stories

### Primary Users

Maintainers and AI coding agents running the local gate before a commit.

### Stories

**As a** maintainer
**I want** `s/test` to run the same gate CI runs
**So that** a green local run means a green CI run.

**As an** agent
**I want** `s/lint` to catch SPDX header and style issues before I commit
**So that** I do not push a `reuse lint` failure.

**As a** maintainer
**I want** `s/gui-dev` to run the Tauri dev server from any working directory
**So that** I do not have to remember `cd gui && npm run tauri dev`.

**As a** maint
**I want** `s/version++` to bump the version across `Cargo.toml`, `gui/package.json`, `tauri.conf.json` in one commit
**So that** the surfaces do not drift onto separate version clocks.

## Requirements

### Functional Requirements

#### Implemented

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | `s/test` runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all`. It mirrors `.github/workflows/ci.yml` so a green local run means a green CI run. | Must Have |
| FR-2 | `s/lint` runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `reuse lint` (with a stderr fallback message if `reuse` is not installed). | Must Have |
| FR-3 | `.editorconfig` enforces UTF-8 charset, LF line endings, 4-space indent (2 for `*.md`/`*.yaml`/`*.yml`/`*.json`/`*.toml`, 4 for `*.rs`), final newline, and trailing-whitespace trim. | Must Have |
| FR-4 | The `s/` scripts carry an SPDX header (`SPDX-FileCopyrightText` / `SPDX-License-Identifier`) and a one-line description of what they do. | Must Have |
| FR-5 | The `s/` scripts `set -euo pipefail` and `cd` to the repo root via `git rev-parse --show-toplevel`, so they run from any working directory. | Must Have |

#### Planned

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-10 | `s/gui-dev` runs the Tauri dev server from any working directory (lands when `gui/` exists, per house-style tauri-gui.md). | Must Have |
| FR-11 | `s/version++` bumps the canonical version across `Cargo.toml`, `gui/package.json`, `tauri.conf.json` in one release commit (lands with the release pipeline, per house-style tauri-gui.md / distribution.md). | Must Have |
| FR-12 | Markdown lint (e.g. `markdownlint` or equivalent) runs in `s/lint` and CI, enforcing prose style (line wrapping, heading style, etc.). | Should Have |
| FR-13 | `s/install-hooks` installs the `.githooks/` directory as the git hooks path. | Should Have |
| FR-14 | `.githooks/pre-commit` runs `s/lint` (or a fast subset) and refuses the commit on failure. | Should Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | `s/test` and `s/lint` are executable (`chmod +x`) and have a `#!/usr/bin/env bash` shebang. | Review-enforced |
| NFR-2 | The `s/` scripts do not depend on the current working directory; they `cd` to the repo root first. | Review-enforced |
| NFR-3 | `s/lint` degrades gracefully when `reuse` is not installed (stderr message, not a hard failure). | Implemented |
| NFR-4 | `.editorconfig` is `root = true` so no parent `.editorconfig` is consulted. | Implemented |
| NFR-5 | The local gate (`s/test`) mirrors CI exactly; if CI adds a step, the local script adds it in the same PR. | Review-enforced |

## Acceptance Criteria

- [x] `s/test` runs fmt + clippy + tests and exits non-zero on any failure.
- [x] `s/lint` runs fmt + clippy + REUSE and exits non-zero on any failure (except the `reuse`-not-installed fallback).
- [x] `.editorconfig` is present at the repo root with the correct rules.
- [x] The `s/` scripts run from any working directory.
- [x] The `s/` scripts carry SPDX headers.
- [ ] `s/gui-dev` runs the Tauri dev server (when `gui/` exists).
- [ ] `s/version++` bumps the version across all surfaces in one commit (when the release pipeline is wired).
- [ ] Markdown lint runs in `s/lint` and CI.
- [ ] `s/install-hooks` and `.githooks/pre-commit` are wired.

## Non-Goals

- **The CI workflow itself.** Owned by zone 600. This zone mirrors it locally.
- **The release pipeline.** Owned by zone 600 (`cargo-dist`, crates.io publish, auto-tag).
- **The GUI.** Owned by zone 400 (deferred). `s/gui-dev` lands when `gui/` exists.
- **Test content.** Tests live alongside the source they test; this zone owns the runner, not the test suites.

## Dependencies

- **`cargo`** (fmt, clippy, test) - the Rust toolchain.
- **`reuse`** (FSFE REUSE tool) - the SPDX header lint. Optional; `s/lint` degrades gracefully.
- **`git`** - the `s/` scripts use `git rev-parse --show-toplevel` to find the repo root.
- **Zone 600 (CI)** - the workflow this zone mirrors locally.
- **Zone 400 (GUI)** - the `s/gui-dev` script lands when `gui/` exists.

## Future

- **`s/version++` details.** The script will parse the current version from `Cargo.toml`, bump it (major/minor/patch via an arg), write it back to `Cargo.toml` (and `gui/package.json`, `tauri.conf.json` once they exist), and create a release commit. The exact bump semantics (and whether it opens a PR) will follow house-style distribution.md.
- **Markdown lint config.** A `.markdownlint.json` or equivalent enforcing the house-style prose rules (no hard-wrap, heading style, etc.). Will run in `s/lint` and CI.
- **Pre-commit hook scope.** `.githooks/pre-commit` should run a fast subset (fmt + clippy, not the full test suite) so the commit is not blocked on a long test run. `s/test` remains the full pre-push gate.

## Glossary

| Term | Definition |
| --- | --- |
| `s/` scripts | Repo scripts (`s/test`, `s/lint`, `s/gui-dev`, `s/version++`) that are the canonical entry points for the local workflow |
| Local gate | `s/test` + `s/lint`; mirrors CI so a green local run means a green CI run |
| REUSE lint | The FSFE REUSE tool that checks SPDX headers are present and correct on every file |
| `.editorconfig` | Editor-agnostic config enforcing charset, line endings, indent style, and final newline |
---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["dx", "tooling", "tasks", "roadmap"]
spec: spec.md
design: design.md
---

# 500 DX Tooling - Tasks

> Implementation tasks for the DX tooling. Phase 1 (core scripts + editorconfig) is done; phase 2 (GUI + version scripts) is blocked on their zones; phase 3 (lint + hooks) is the polish.

## Phase 1 - Core scripts (done)

### 1.1 `s/test`

- [x] Executable bash script with SPDX header and one-line description.
- [x] `set -euo pipefail`, `cd "$(git rev-parse --show-toplevel)"`.
- [x] `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all`.

### 1.2 `s/lint`

- [x] Executable bash script with SPDX header and one-line description.
- [x] `set -euo pipefail`, `cd "$(git rev-parse --show-toplevel)"`.
- [x] `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`.
- [x] `reuse lint` with a graceful fallback to a stderr message when `reuse` is not installed.

### 1.3 `.editorconfig`

- [x] `root = true`.
- [x] UTF-8, LF, final newline, trailing-whitespace trim.
- [x] 4-space indent; 2-space for `*.md`/`*.yaml`/`*.yml`/`*.json`/`*.toml`; 4-space for `*.rs`.

### 1.4 Parity with CI

- [x] `cargo fmt` and `cargo clippy` run in both `s/test` and `.github/workflows/ci.yml`.
- [ ] `s/test` runs `cargo test --all --no-default-features` (the leaf-crate no-default-features path CI runs). Currently local runs all-features only.

## Phase 2 - GUI and version scripts (planned, blocked)

### 2.1 `s/gui-dev` (blocked on zone 400)

- [ ] Executable bash script with SPDX header.
- [ ] `set -euo pipefail`, `cd "$(git rev-parse --show-toplevel)"`, `cd gui`, `npm run tauri dev`.
- [ ] Documented in AGENTS.md workflow section.

### 2.2 `s/version++` (blocked on zone 600 release pipeline)

- [ ] Executable bash script with SPDX header.
- [ ] Parse current version from `Cargo.toml` (workspace `[workspace.package] version`).
- [ ] Bump (major/minor/patch via arg) and write back to `Cargo.toml`.
- [ ] (When `gui/` exists) bump `gui/package.json` and `tauri.conf.json` in the same commit.
- [ ] Create a release commit with a conventional message.
- [ ] Follow house-style distribution.md for the exact semantics.

## Phase 3 - Lint and hooks (planned, next)

### 3.1 Markdown lint

- [ ] Add `.markdownlint.json` enforcing house-style prose rules (no hard-wrap, heading style, no emphasis-as-heading).
- [ ] Run markdownlint in `s/lint` and in CI (`.github/workflows/ci.yml`, owned by zone 600).
- [ ] Confirm the rules match the existing spec files (long-line paragraphs, not hard-wrapped).

### 3.2 Pre-commit hook

- [ ] Create `.githooks/pre-commit` running a fast subset (`cargo fmt --check` + `cargo clippy`, not the full test suite).
- [ ] Refuse the commit on failure.
- [ ] Create `s/install-hooks` running `git config core.hooksPath .githooks`.
- [ ] Document the opt-in in AGENTS.md (do not force hooks on every clone).

### 3.3 `s/test` no-default-features parity

- [ ] Add `cargo test --all --no-default-features` to `s/test` to match CI's leaf-crate no-default-features path.

## Phase 4 - Verification (planned)

### 4.1 Local-CI parity audit

- [ ] Audit `s/test` against `.github/workflows/ci.yml` step-by-step; confirm every CI step runs locally.
- [ ] Add a check that fails if CI adds a step the local script does not (or document the exception).

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1-1.4 | Implemented `s/test`, `s/lint`, `.editorconfig` | `s/test`, `s/lint`, `.editorconfig` | [x] | [x] |
| 2026-08-01 | 1.4 | Wrote spec/design/tasks for this zone | `spec/500-dx-tooling/{spec,design,tasks}.md` | [x] | [x] |
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

> The repo scripts and lint/format/test configuration. `s/test` and `s/lint` are the canonical local gates, including no-default workspace clippy/tests, Markdown lint, and traceability validation, and are available as an opt-in pre-commit hook; CI additionally runs Windows cross-checks and platform setup that are not duplicated locally.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [AGENTS.md](../../AGENTS.md) - the workflow section (the `s/` scripts) and the "Before Every Commit" gate.
- [house-style scripts.md](https://github.com/marcus-pacharanero/house-style/blob/main/scripts.md) - the `s/` script conventions.
- [house-style testing.md](https://github.com/marcus-pacharanero/house-style/blob/main/testing.md) - the testing conventions.
- [house-style ci.md](https://github.com/marcus-pacharanero/house-style/blob/main/ci.md) - the CI conventions this zone mirrors locally.
- [600-ci-release spec](../600-ci-release/spec.md) - the CI workflow that `s/test` mirrors.
- Owned source: `s/`, `.editorconfig`.

## Problem Statement

A maintainer or agent runs `s/test` for Rust formatting, all-feature clippy, default-feature and no-default workspace tests, frontend checks, and traceability-checker fixtures, then `s/lint` for formatting, both clippy feature paths, frontend checks, Markdown and REUSE lint, real-device-data protection, version synchronization, and live traceability validation. `s/install-hooks` makes `s/lint` a pre-commit hook for maintainers who want it, without imposing one on every clone. CI remains broader in the Windows host/MCP cross-checks and Tauri integration build, which need platform setup not guaranteed locally. Local green no longer skips the no-default workspace clippy/test paths that used to be remote-only.

This zone owns the scripts, the `.editorconfig`, and the lint/format config. The CI workflow itself is owned by zone 600; this zone mirrors it locally. `s/gui-dev` and `s/version++` exist. The version script synchronizes the Rust workspace, npm lock/package metadata and Tauri configuration in one release commit.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| `s/test` runs fmt + clippy + tests | Implemented | `s/test` runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all`, `cargo test --all --no-default-features`, frontend checks, `tests/check-traceability.sh`, `tests/check-docs-nav.sh`, `tests/linkcheck.sh` and `tests/spellcheck.sh` |
| `s/lint` runs fmt + clippy + no-HID check + REUSE + repository policy lints | Implemented | `s/lint` runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo clippy --all-targets --no-default-features -- -D warnings`, `reuse lint` when available, `s/lint-no-device-data`, `s/check-versions`, and `s/check-traceability` |
| Docs-site quality lints (nav orphans, internal links, spelling) | Implemented | `s/check-docs-nav` and `s/linkcheck` are dependency-free and resolve `mkdocs.yml`'s nav and every `docs/` internal link/anchor against the file tree; `s/spellcheck` runs pinned `codespell==2.4.1`. All three run from `s/lint` and dedicated CI steps, with fixture coverage in `tests/check-docs-nav.sh`, `tests/linkcheck.sh` and `tests/spellcheck.sh` |
| `.editorconfig` enforces UTF-8, LF, 4-space indent (2 for md/yaml/json), final newline | Implemented | `.editorconfig` at repo root |
| `cargo fmt` and `cargo clippy` run in CI | Implemented | `.github/workflows/ci.yml` (owned by zone 600) |
| `s/gui-dev` | Implemented | Runs the Tauri dev server from the repository-independent entry point |
| `s/version++` | Implemented | Release script synchronizes Cargo, npm and Tauri versions, runs the Rust and frontend gates, then lands one release commit |
| No-default workspace clippy/tests run locally, not only in CI | Implemented | `s/lint` runs `cargo clippy --all-targets --no-default-features -- -D warnings`; `s/test` runs `cargo test --all --no-default-features` |
| Markdown lint | Implemented | `s/markdownlint` runs `markdownlint-cli2@0.23.2` against `.markdownlint.jsonc`, from `s/lint` and a dedicated CI step |
| Traceability lint | Implemented | `s/check-traceability` resolves existing Rust `@see` paths and node IDs, requires a living spec/design target, and has isolated fixture coverage in `tests/check-traceability.sh` |
| Tracked Git hooks | Implemented | `s/install-hooks` sets `core.hooksPath=.githooks`; `.githooks/pre-commit` runs `s/lint` |

## User Stories

### Primary Users

Maintainers and AI coding agents running the local gate before a commit.

### Stories

**As a** maintainer
**I want** the local gate's deliberate differences from CI to be explicit
**So that** I know which checks still run only remotely.

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
| FR-1 | `s/test` runs `cargo fmt --all --check`, all-feature clippy, `cargo test --all` and `cargo test --all --no-default-features`. | Must Have |
| FR-2 | `s/lint` runs formatting, all-feature clippy, no-default-features workspace clippy, `s/markdownlint`, `s/check-docs-nav`, `s/linkcheck`, `s/spellcheck`, `reuse lint` when available, `s/lint-no-device-data`, `s/check-versions`, and `s/check-traceability`. | Must Have |
| FR-3 | `.editorconfig` enforces UTF-8 charset, LF line endings, 4-space indent (2 for `*.md`/`*.yaml`/`*.yml`/`*.json`/`*.toml`, 4 for `*.rs`), final newline, and trailing-whitespace trim. | Must Have |
| FR-4 | The `s/` scripts carry an SPDX header (`SPDX-FileCopyrightText` / `SPDX-License-Identifier`) and a one-line description of what they do. | Must Have |
| FR-5 | The `s/` scripts `set -euo pipefail` and `cd` to the repo root via `git rev-parse --show-toplevel`, so they run from any working directory. | Must Have |
| FR-10 | `s/gui-dev` runs the Tauri dev server from any working directory. | Must Have |
| FR-12 | `s/markdownlint` runs `markdownlint-cli2` (pinned, via `npx`) against `.markdownlint.jsonc` in both `s/lint` and CI. The config is not purely stylistic: `MD056` and `MD040` catch defects that silently degrade the rendered docs. | Should Have |
| FR-13 | `s/install-hooks` sets `core.hooksPath` to the tracked `.githooks/` directory, and reverses that with `-u`. | Should Have |
| FR-14 | `.githooks/pre-commit` runs `s/lint` and refuses the commit on failure. | Should Have |
| FR-15 | `s/check-traceability` validates complete `@see` syntax, target existence, literal structural node resolution, and at least one living spec/design target for every Rust file that already carries a header. Its parser behavior is covered in isolated temporary Git repositories. | Should Have |
| FR-16 | `s/check-docs-nav` fails if a `docs/` page is not reachable from `mkdocs.yml`'s nav and is not a named exception (with a reason) in `.nav-exceptions`; it also fails if a named exception no longer exists. | Should Have |
| FR-17 | `s/linkcheck` fails if a `docs/` internal Markdown link does not resolve to a file, or if a `#fragment` does not match a target heading's computed or explicit (`{#id}`) slug. External links are not checked. | Should Have |
| FR-18 | `s/spellcheck` runs pinned `codespell` against `docs/`, installing that pinned version on demand. | Should Have |

#### Planned

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-11 | `s/version++` synchronises the canonical version across Cargo, npm package/lock and Tauri configuration before the release commit. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | `s/test` and `s/lint` are executable (`chmod +x`) and have a `#!/usr/bin/env bash` shebang. | Review-enforced |
| NFR-2 | The `s/` scripts do not depend on the current working directory; they `cd` to the repo root first. | Review-enforced |
| NFR-3 | `s/lint` degrades gracefully when `reuse` is not installed (stderr message, not a hard failure). | Implemented |
| NFR-4 | `.editorconfig` is `root = true` so no parent `.editorconfig` is consulted. | Implemented |
| NFR-5 | Local and CI gates document their differences. No-default workspace clippy/tests now run locally; host-platform cross-checks (the Windows cross-compile) remain CI-only because they need a cross toolchain not guaranteed to be present locally. | Implemented |

## Acceptance Criteria

- [x] `s/test` runs fmt + clippy + tests and exits non-zero on any failure.
- [x] `s/lint` runs fmt + clippy + REUSE and exits non-zero on any failure (except the `reuse`-not-installed fallback).
- [x] `.editorconfig` is present at the repo root with the correct rules.
- [x] The `s/` scripts run from any working directory.
- [x] The `s/` scripts carry SPDX headers.
- [x] `s/gui-dev` runs the Tauri dev server.
- [x] `s/version++` bumps the version across all current surfaces in one commit.
- [x] `s/test` and `s/lint` run no-default-features workspace clippy and tests locally, matching CI's no-default job.
- [x] Markdown lint runs in `s/lint` and CI.
- [x] Docs nav-orphan, internal-link and spell checks run in `s/lint` and CI, with fixture coverage in `tests/check-docs-nav.sh`, `tests/linkcheck.sh` and `tests/spellcheck.sh`.
- [x] Traceability lint runs in `s/lint` and CI, with its parser behavior exercised by `s/test` and CI.
- [x] `s/install-hooks` and `.githooks/pre-commit` are wired.

## Non-Goals

- **The CI workflow itself.** Owned by zone 600. This zone mirrors it locally.
- **The release pipeline.** Owned by zone 600 (`cargo-dist`, crates.io publish, auto-tag).
- **The GUI.** Owned by zone 400; this zone owns only its repeated development/release scripts.
- **Test content.** Tests live alongside the source they test; this zone owns the runner, not the test suites.

## Dependencies

- **`cargo`** (fmt, clippy, test) - the Rust toolchain.
- **`reuse`** (FSFE REUSE tool) - required by the maintainer pre-commit gate; `s/lint` degrades for contributors but CI always enforces it.
- **`git`** - the `s/` scripts use `git rev-parse --show-toplevel` to find the repo root.
- **Zone 600 (CI)** - the workflow this zone mirrors locally.
- **Zone 400 (GUI)** - supplies the manifests `s/gui-dev` runs and `s/version++` must keep in sync.

## Future

- **`s/version++` GUI synchronization.** Implemented for `gui/package.json`, `gui/package-lock.json` and `gui/src-tauri/tauri.conf.json`; a future CI drift check can verify they match outside release runs.

## Glossary

| Term | Definition |
| --- | --- |
| `s/` scripts | Repo scripts (`s/test`, `s/lint`, `s/check-traceability`, `s/check-docs-nav`, `s/linkcheck`, `s/spellcheck`, `s/gui-dev`, `s/version++`) that are the canonical entry points for the local workflow |
| `.nav-exceptions` | Optional repo-root file naming `docs/` pages deliberately excluded from `mkdocs.yml`'s nav, one `<path><TAB><reason>` per line; read by `s/check-docs-nav` |
| Local gate | `s/test` + `s/lint`; a documented local subset of CI with additional device-data lint |
| REUSE lint | The FSFE REUSE tool that checks SPDX headers are present and correct on every file |
| `.editorconfig` | Editor-agnostic config enforcing charset, line endings, indent style, and final newline |

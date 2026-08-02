---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["ci", "release", "github-actions", "dependabot", "cargo-dist", "crates-io"]
---

# 600 CI / Release - Spec

> The GitHub Actions CI workflow, Dependabot config, and the planned release pipeline (auto-tag, crates.io publish, `cargo-dist`). Owns the externally-visible automation: a green CI run is the gate to merge, and a release tag is the gate to publish.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [AGENTS.md](../../AGENTS.md) - the "Before Every Commit" gate and the approval-required actions.
- [house-style ci.md](https://github.com/marcus-pacharanero/house-style/blob/main/ci.md) - the CI conventions (action SHA pinning, no-secrets-in-repo, etc.).
- [house-style distribution.md](https://github.com/marcus-pacharanero/house-style/blob/main/distribution.md) - the release/distribution conventions.
- [house-style dependencies.md](https://github.com/marcus-pacharanero/house-style/blob/main/dependencies.md) - the dependency-pinning conventions.
- [500-dx-tooling spec](../500-dx-tooling/spec.md) - the local gate (`s/test`/`s/lint`) that mirrors this zone's CI workflow.
- [900-project-governance spec](../900-project-governance/spec.md) - the license/REUSE lint this zone enforces in CI.
- Owned source: `.github/workflows/ci.yml`, `.github/dependabot.yml`.

## Problem Statement

CI runs the full gate on every push and PR: formatting, clippy (all-features and no-default-features, both with `-D warnings`), tests (all-features and no-default-features), and the REUSE license lint. A green CI run is the gate to merge. The workflow uses pinned action SHAs with `# vX.Y.Z` comments (house-style), cached dependencies, and the minimal permissions (`contents: read`).

Release is not yet wired. The plan: an auto-tag workflow (version bump on `main` -> tag -> release cascade), a crates.io publish workflow (gated on the tag, requiring approval per AGENTS.md), and a `cargo-dist` release pipeline for distributable binaries (the `cortex` CLI and, eventually, the Tauri GUI). All release actions are externally visible and require explicit approval before first use.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| CI runs `cargo fmt --all --check` | Implemented | `.github/workflows/ci.yml` "Formatting" step |
| CI runs `cargo clippy --all-targets --all-features -- -D warnings` | Implemented | `.github/workflows/ci.yml` "Clippy (all features)" step |
| CI runs `cargo clippy --all-targets --no-default-features -- -D warnings` | Implemented | `.github/workflows/ci.yml` "Clippy (no default features)" step |
| CI runs `cargo test --all` (all-features) | Implemented | `.github/workflows/ci.yml` "Tests (all features)" step |
| CI runs `cargo test --all --no-default-features` | Implemented | `.github/workflows/ci.yml` "Tests (leaf engine, no default features)" step |
| CI runs the REUSE license lint | Implemented | `.github/workflows/ci.yml` `reuse` job via `fsfe/reuse-action` |
| Actions are pinned to SHA with `# vX.Y.Z` comments | Implemented | `actions/checkout@3d3c42e...# v7.0.1`, `dtolnay/rust-toolchain@e97e2d8...# v1`, `Swatinem/rust-cache@c193711...# v2.9.1`, `fsfe/reuse-action@676e2d5...# v6.0.0` |
| Dependabot: cargo + github-actions, weekly, cooldown, grouping | Implemented | `.github/dependabot.yml` |
| Auto-tag workflow | Planned | Not implemented |
| crates.io publish workflow | Planned | Not implemented (requires approval per AGENTS.md) |
| `cargo-dist` release pipeline | Planned | Not implemented |

## User Stories

### Primary Users

Maintainers merging PRs, and the downstream consumers who install the crate or the `cortex` binary.

### Stories

**As a** maintainer
**I want** CI to run the full gate on every PR
**So that** I can merge knowing fmt + clippy + tests + REUSE are green.

**As a** downstream consumer
**I want** a release tag to produce a published crate on crates.io and a distributable `cortex` binary
**So that** I can `cargo install cortex-cli` or download a prebuilt binary.

**As a** maintainer
**I want** Dependabot to group routine dependency updates weekly with a cooldown
**So that** I am not spammed with per-version PRs but also not blindsided by a major bump.

**As a** maintainer
**I want** action SHAs pinned with version comments
**So that** a supply-chain compromise of a moving tag does not silently change my CI.

## Requirements

### Functional Requirements

#### Implemented

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | CI runs on `push` to `main`, on `pull_request`, and on `workflow_dispatch`. | Must Have |
| FR-2 | CI runs `cargo fmt --all --check` (fail on any unformatted file). | Must Have |
| FR-3 | CI runs `cargo clippy --all-targets --all-features -- -D warnings` and `cargo clippy --all-targets --no-default-features -- -D warnings` (both paths). | Must Have |
| FR-4 | CI runs `cargo test --all` (all-features) and `cargo test --all --no-default-features` (leaf engine only). | Must Have |
| FR-5 | CI runs the REUSE license lint (`fsfe/reuse-action`) as a separate job. | Must Have |
| FR-6 | Actions are pinned to specific SHA hashes with a `# vX.Y.Z` semver comment. | Must Have |
| FR-7 | CI uses `permissions: contents: read` (least privilege). | Must Have |
| FR-8 | CI installs `protoc` (for `prost-build`) via `protobuf-compiler`. | Must Have |
| FR-9 | CI caches the cargo registry/target via `Swatinem/rust-cache`. | Must Have |
| FR-10 | Dependabot watches `cargo` (workspace + each crate) and `github-actions`, weekly, with a cooldown and routine-update grouping. | Must Have |

#### Planned

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-20 | Auto-tag workflow: a version bump on `main` (via `s/version++`) produces a `vX.Y.Z` tag, which triggers the release cascade. | Must Have |
| FR-21 | crates.io publish workflow: gated on the release tag, publishes `cortex-rs` (and later `cortex-cli`) to crates.io. **Requires approval before first use** (AGENTS.md). | Must Have |
| FR-22 | `cargo-dist` release pipeline: gated on the release tag, builds distributable `cortex` binaries for the supported targets and attaches them to the GitHub release. | Must Have |
| FR-23 | The release tag triggers a GitHub Release with changelog notes. | Should Have |
| FR-24 | Dependabot updates for `github-actions` are grouped and pinned to SHAs with version comments (matching the existing convention). | Should Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | No secrets are committed to the repo; crates.io publishing uses a secret stored in GitHub. | Review-enforced |
| NFR-2 | Action SHAs are pinned; Dependabot opens PRs to bump them (with the version comment updated). | Review-enforced |
| NFR-3 | CI runs on `ubuntu-latest` (the project is Linux-first); no macOS/Windows matrix until the GUI lands. | Implemented |
| NFR-4 | The CI workflow and the local gate (`s/test`, zone 500) run the same steps; a green local run means a green CI run. | Review-enforced |
| NFR-5 | Release actions are externally visible and require explicit approval before first use (AGENTS.md). | Process-enforced |

## Acceptance Criteria

- [x] CI runs fmt + clippy (both feature paths) + tests (both feature paths) + REUSE on every push/PR.
- [x] Actions are pinned to SHAs with `# vX.Y.Z` comments.
- [x] Dependabot watches cargo + github-actions, weekly, with cooldown and grouping.
- [x] CI uses `permissions: contents: read`.
- [x] CI installs `protoc` and caches cargo.
- [ ] Auto-tag workflow produces a `vX.Y.Z` tag on a version bump.
- [ ] crates.io publish workflow publishes on the release tag (requires approval before first use).
- [ ] `cargo-dist` produces distributable `cortex` binaries on the release tag.
- [ ] The release tag produces a GitHub Release with changelog notes.

## Non-Goals

- **The local gate.** Owned by zone 500 (`s/test`, `s/lint`). This zone mirrors it in CI.
- **The GUI CI.** The GUI is deferred (zone 400); frontend lint/typecheck and Tauri build CI land when `gui/` exists.
- **A macOS/Windows matrix.** The project is Linux-first; the USB HID transport is the focus. A matrix may be worth it for the GUI eventually, not for the crate.
- **Hardware smoke in CI.** CI has no hardware; the hardware smoke runbook is manual (AGENTS.md). This zone does not attempt to connect to a real Quad Cortex.

## Dependencies

- **GitHub Actions**: `actions/checkout@v7.0.1`, `dtolnay/rust-toolchain@v1`, `Swatinem/rust-cache@v2.9.1`, `fsfe/reuse-action@v6.0.0` (all SHA-pinned).
- **Dependabot**: cargo + github-actions ecosystems.
- **`protoc`**: installed via `protobuf-compiler` for `prost-build`.
- **Zone 500 (DX tooling)**: the local gate that mirrors this zone's CI workflow.
- **Zone 900 (governance)**: the license/REUSE config this zone lints.
- **house-style distribution.md**: the release conventions for `cargo-dist` and crates.io.

## Future

- **Auto-tag details.** The trigger could be a push to `main` that changes the `Cargo.toml` version, or an explicit `s/version++` commit message. The tag format is `vX.Y.Z` (matching `cargo` and `cargo-dist` conventions).
- **crates.io first publish.** Requires `cargo login` with a token, `cargo publish --dry-run` in CI, and the maintainer's approval (AGENTS.md). The crate name is `cortex-rs`; the CLI binary crate is `cortex-cli`.
- **`cargo-dist`.** Produces distributable binaries for the `cortex` CLI (and later the Tauri GUI as an `.AppImage`/`.deb`). The targets are Linux-first (`x86_64-unknown-linux-gnu`, maybe `aarch64`); macOS/Windows are future.
- **Changelog generation.** `cargo-dist` can generate changelogs from commit history; alternatively, a `CHANGELOG.md` kept by hand follows house-style docs.md.

## Glossary

| Term | Definition |
| --- | --- |
| SHA pin | A GitHub Action pinned to a specific commit SHA with a `# vX.Y.Z` comment, so a moving tag cannot silently change CI |
| Dependabot cooldown | A delay before Dependabot opens a PR for a new release, to avoid churn on rapid-release deps |
| Auto-tag | A workflow that produces a `vX.Y.Z` git tag when the version in `Cargo.toml` bumps |
| Release cascade | tag -> crates.io publish -> `cargo-dist` binary build -> GitHub Release |
| `cargo-dist` | A tool that produces distributable Rust binaries from a release tag |
| Leaf engine, no default features | The `cargo build --no-default-features -p cortex-rs` path that confirms the crate builds without `hidapi` |
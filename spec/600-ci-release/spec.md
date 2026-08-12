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

> The Rust CI and documentation deployment workflows, Dependabot config, implemented auto-tagging, and planned crates.io/cargo-dist release pipeline.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [AGENTS.md](../../AGENTS.md) - the "Before Every Commit" gate and the approval-required actions.
- [house-style ci.md](https://github.com/marcus-pacharanero/house-style/blob/main/ci.md) - the CI conventions (action SHA pinning, no-secrets-in-repo, etc.).
- [house-style distribution.md](https://github.com/marcus-pacharanero/house-style/blob/main/distribution.md) - the release/distribution conventions.
- [house-style dependencies.md](https://github.com/marcus-pacharanero/house-style/blob/main/dependencies.md) - the dependency-pinning conventions.
- [500-dx-tooling spec](../500-dx-tooling/spec.md) - the local gate (`s/test`/`s/lint`) that mirrors this zone's CI workflow.
- [900-project-governance spec](../900-project-governance/spec.md) - the license/REUSE lint this zone enforces in CI.
- Owned source: `.github/workflows/`, `.github/dependabot.yml`.

## Problem Statement

CI runs formatting, clippy on all-feature and no-default workspace configurations, default-feature and no-default workspace tests, real-device-data lint, Windows host/MCP cross-checks, and REUSE. Documentation has a separate path-filtered Zensical Pages workflow. Actions are SHA-pinned with version comments and use minimal permissions.

Release is only partly wired. `s/version++` and the auto-tag workflow exist; crates.io publishing, `cargo-dist`, GitHub Release generation and GUI bundles do not. The first binary release is deliberately Linux x86_64 and installs the `cortex` and `cortex-mcp` pair; other host platforms remain unsupported until their daemon boundary and hardware behaviour are verified. All release actions are externally visible and require explicit approval before first use.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| CI runs `cargo fmt --all --check` | Implemented | `.github/workflows/ci.yml` "Formatting" step |
| CI runs `cargo clippy --all-targets --all-features -- -D warnings` | Implemented | `.github/workflows/ci.yml` "Clippy (all features)" step |
| CI runs `cargo clippy --all-targets --no-default-features -- -D warnings` | Implemented | `.github/workflows/ci.yml` "Clippy (no default features)" step |
| CI runs `cargo test --all` (workspace default features) | Implemented | `.github/workflows/ci.yml` default-feature test step |
| CI runs `cargo test --all --no-default-features` | Implemented | `.github/workflows/ci.yml` no-default workspace test step |
| CI runs the REUSE license lint | Implemented | `.github/workflows/ci.yml` `reuse` job via `fsfe/reuse-action` |
| CI type-checks and builds both GUI frontend modes (`npm run check`: fixture + Tauri) | Implemented | `.github/workflows/ci.yml` "Check fixture and Tauri frontends" step |
| CI builds the full Tauri backend as a debug, unbundled boundary check | Implemented | `.github/workflows/ci.yml` "Tauri build boundary (debug, no bundle)" step: `npm run tauri --prefix gui -- build --debug --no-bundle --ci` |
| Actions are pinned to SHA with `# vX.Y.Z` comments | Implemented | `actions/checkout@3d3c42e...# v7.0.1`, `dtolnay/rust-toolchain@e97e2d8...# v1`, `Swatinem/rust-cache@c193711...# v2.9.1`, `fsfe/reuse-action@676e2d5...# v6.0.0` |
| Dependabot: Cargo, npm, pip and GitHub Actions, weekly, cooldown, grouping | Implemented | `.github/dependabot.yml` |
| Zensical Pages deployment | Implemented and deployed | `.github/workflows/docs.yml` |
| Auto-tag workflow | Implemented | `.github/workflows/auto-tag.yml` |
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
| FR-4 | CI runs `cargo test --all` with workspace default features and `cargo test --all --no-default-features`. | Must Have |
| FR-5 | CI runs the REUSE license lint (`fsfe/reuse-action`) as a separate job. | Must Have |
| FR-6 | Actions are pinned to specific SHA hashes with a `# vX.Y.Z` semver comment. | Must Have |
| FR-7 | CI uses `permissions: contents: read` (least privilege). | Must Have |
| FR-8 | CI installs `protoc`, hidapi/udev prerequisites, and Tauri Linux system dependencies. | Must Have |
| FR-9 | CI caches the cargo registry/target via `Swatinem/rust-cache`. | Must Have |
| FR-10 | Dependabot watches Cargo at the workspace root, npm under `gui`, pip at the root, and GitHub Actions, weekly with cooldown and routine-update grouping. | Must Have |
| FR-11 | CI rejects real device identifiers and cross-checks `cortex-host` and `cortex-mcp` for `x86_64-pc-windows-gnu`. | Must Have |
| FR-12 | The path-filtered docs workflow builds Zensical and deploys through GitHub Pages artifacts. | Must Have |
| FR-13 | CI type-checks and builds both explicit GUI frontend modes (`npm run check --prefix gui`: fixture and Tauri). | Must Have |
| FR-14 | CI builds the full Tauri Rust backend as a debug, unbundled boundary check (`npm run tauri --prefix gui -- build --debug --no-bundle --ci`), catching `tauri.conf.json`/icon/`beforeBuildCommand` integration failures that `cargo test`/`cargo clippy` alone do not exercise. | Must Have |
| FR-20 | Auto-tag workflow is implemented: a version bump on `main` creates `vX.Y.Z` and directly invokes future release workflows rather than relying on tag-event recursion. Its first live release remains unevidenced. | Must Have |

#### Planned

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-21 | crates.io publish workflow: gated on the release tag, publishes `cortex-rs` (and later `cortex-cli`) to crates.io. **Requires approval before first use** (AGENTS.md). | Must Have |
| FR-22 | `cargo-dist` release pipeline: invoked from `auto-tag.yml` through `workflow_call`, builds Linux x86_64 `cortex` and `cortex-mcp` artifacts, and attaches them to the GitHub release. | Must Have |
| FR-23 | The auto-tag workflow invokes the release workflow, which creates a GitHub Release for that tag with changelog notes. | Should Have |
| FR-24 | Dependabot updates for `github-actions` are grouped and pinned to SHAs with version comments (matching the existing convention). | Should Have |
| FR-25 | Every binary release publishes one authoritative `SHA256SUMS` covering its artifacts. | Must Have |
| FR-26 | A docs-root `install.sh` fetches the latest supported Linux archive, verifies it against `SHA256SUMS`, installs both binaries without requiring Rust or `protoc`, and refreshes or prescribes shell completions. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | No secrets are committed to the repo; crates.io publishing uses a secret stored in GitHub. | Review-enforced |
| NFR-2 | Action SHAs are pinned; Dependabot opens PRs to bump them (with the version comment updated). | Review-enforced |
| NFR-3 | Jobs run on Linux today, with a Windows host-boundary cross-compile. Native Windows/macOS and hardware verification remain future gates. | Implemented |
| NFR-4 | The local gate documents its subset of CI; the remaining remote-only platform (Windows) cross-checks are not implied by local green. | Review-enforced |
| NFR-5 | Release actions are externally visible and require explicit approval before first use (AGENTS.md). | Process-enforced |

## Acceptance Criteria

- [x] CI runs fmt + clippy (both feature paths) + tests (both feature paths) + REUSE on every push/PR.
- [x] Actions are pinned to SHAs with `# vX.Y.Z` comments.
- [x] Dependabot watches Cargo, npm, pip and GitHub Actions, weekly, with cooldown and grouping.
- [x] CI uses `permissions: contents: read`.
- [x] CI installs Rust native prerequisites, rejects real device data, cross-checks the Windows host boundary and caches Cargo.
- [x] Auto-tag workflow is implemented for a `vX.Y.Z` tag on a version bump; no first live tag is claimed.
- [x] Documentation builds and deploys through the artifact-based Pages workflow.
- [x] CI type-checks/builds both GUI frontend modes and builds the full Tauri backend as a debug, unbundled boundary check.
- [ ] crates.io publish workflow publishes on the release tag (requires approval before first use).
- [ ] `cargo-dist` produces distributable Linux x86_64 `cortex` and `cortex-mcp` binaries on the release tag.
- [ ] The release tag produces a GitHub Release with changelog notes.
- [ ] The release publishes `SHA256SUMS`, and the public installer refuses an artifact that does not match it.

## Non-Goals

- **The local gate.** Owned by zone 500 (`s/test`, `s/lint`). This zone mirrors it in CI.
- **GUI build matrices and bundle/installer tests.** Frontend typecheck/build and a single-target debug, unbundled Tauri build boundary are in CI (Linux only); a real bundle (`.deb`/AppImage) is not built or tested here, and native Windows/macOS Tauri builds remain out of scope until those hosts are supported.
- **A macOS/Windows matrix.** The project is Linux-first today. Native Windows and macOS CI become required when their local IPC, process lifecycle, packaging and hardware paths are implemented.
- **Hardware smoke in CI.** CI has no hardware; the hardware smoke runbook is manual (AGENTS.md). This zone does not attempt to connect to a real Quad Cortex.

## Dependencies

- **GitHub Actions**: `actions/checkout@v7.0.1`, `dtolnay/rust-toolchain@v1`, `Swatinem/rust-cache@v2.9.1`, `fsfe/reuse-action@v6.0.0` (all SHA-pinned).
- **Dependabot**: Cargo, npm, pip and GitHub Actions ecosystems.
- **`protoc`**: installed via `protobuf-compiler` for `prost-build`.
- **Zone 500 (DX tooling)**: the local gate that mirrors this zone's CI workflow.
- **Zone 900 (governance)**: the license/REUSE config this zone lints.
- **house-style distribution.md**: the release conventions for `cargo-dist` and crates.io.

## Future

- **Auto-tag details.** The trigger could be a push to `main` that changes the `Cargo.toml` version, or an explicit `s/version++` commit message. The tag format is `vX.Y.Z` (matching `cargo` and `cargo-dist` conventions).
- **crates.io first publish.** Requires `cargo login` with a token, `cargo publish --dry-run` in CI, and the maintainer's approval (AGENTS.md). The crate name is `cortex-rs`; the CLI binary crate is `cortex-cli`.
- **`cargo-dist`.** Produces distributable `cortex` and `cortex-mcp` binaries (and later the Tauri GUI through its own bundler). The first target is `x86_64-unknown-linux-gnu`; Linux aarch64, macOS and Windows follow only after their host and hardware paths are verified.
- **Changelog generation.** Add the house-style `git-cliff` configuration so `s/version++` regenerates `CHANGELOG.md` before the release commit is tagged.

## Glossary

| Term | Definition |
| --- | --- |
| SHA pin | A GitHub Action pinned to a specific commit SHA with a `# vX.Y.Z` comment, so a moving tag cannot silently change CI |
| Dependabot cooldown | A delay before Dependabot opens a PR for a new release, to avoid churn on rapid-release deps |
| Auto-tag | A workflow that produces a `vX.Y.Z` git tag when the version in `Cargo.toml` bumps |
| Release cascade | tag -> crates.io publish -> `cargo-dist` binary build -> GitHub Release |
| `cargo-dist` | A tool that produces distributable Rust binaries from a release tag |
| Leaf engine, no default features | The `cargo build --no-default-features -p cortex-rs` path that confirms the crate builds without `hidapi` |

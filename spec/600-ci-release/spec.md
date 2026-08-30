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

> The Rust CI and documentation deployment workflows, Dependabot config, implemented auto-tagging, Linux binary releases and native GUI tester-preview packages, and planned crates.io publication.

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

CI runs formatting, clippy on all-feature and no-default workspace configurations, default-feature and no-default workspace tests, RustSec audit, Zizmor workflow analysis, real-device-data lint, released-installer fixtures, version and traceability checks, Windows host/MCP cross-checks, and REUSE. Documentation has a separate path-filtered Zensical Pages workflow. Actions are SHA-pinned with version comments and use minimal permissions.

Release is partly wired. `s/version++`, post-gate auto-tagging, the Linux x86_64 cargo-dist build, draft-staged GitHub Release hosting, and native Linux `.deb`, Apple Silicon `.dmg` and Windows x86_64 NSIS GUI builders exist; crates.io publishing does not. The CLI/MCP archive remains the hardware-verified Linux channel. The unsigned Windows and ad-hoc-signed macOS GUI packages remain provisional until their native daemon and USB paths pass hardware smoke. The host job names the protected `release` environment, so publication requires explicit human approval after every artifact is built and verified.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| CI runs `cargo fmt --all --check` | Implemented | `.github/workflows/ci.yml` "Formatting" step |
| CI runs `cargo clippy --all-targets --all-features -- -D warnings` | Implemented | `.github/workflows/ci.yml` "Clippy (all features)" step |
| CI runs `cargo clippy --all-targets --no-default-features -- -D warnings` | Implemented | `.github/workflows/ci.yml` "Clippy (no default features)" step |
| CI runs `cargo test --all` (workspace default features) | Implemented | `.github/workflows/ci.yml` default-feature test step |
| CI runs `cargo test --all --no-default-features` | Implemented | `.github/workflows/ci.yml` no-default workspace test step |
| CI runs the REUSE license lint | Implemented | `.github/workflows/ci.yml` `reuse` job via `fsfe/reuse-action` |
| CI runs `cargo audit` | Implemented | Separate blocking `Security audit` job installs pinned `cargo-audit 0.22.2` through checksum-verifying `taiki-e/install-action` |
| CI runs Zizmor | Implemented | Separate blocking `GitHub Actions security` job installs pinned Zizmor 1.29.0 and runs `zizmor --strict-collection .` with the read-only workflow token |
| CI tests the released installer transaction | Implemented | `tests/install-release.sh` accepts a compatible fixture and proves glibc/library failures do not replace either installed binary |
| CI type-checks and builds both GUI frontend modes (`npm run check`: fixture + Tauri) | Implemented | `.github/workflows/ci.yml` "Check fixture and Tauri frontends" step |
| CI builds the full Tauri backend as a debug, unbundled boundary check | Implemented | `.github/workflows/ci.yml` "Tauri build boundary (debug, no bundle)" step: `npm run tauri --prefix gui -- build --debug --no-bundle --ci` |
| CI validates Rust `@see` traceability links and the checker itself | Implemented | `.github/workflows/ci.yml` "Traceability links" step runs `tests/check-traceability.sh` and `s/check-traceability` |
| Actions are pinned to SHA with `# vX.Y.Z` comments | Implemented | `actions/checkout@3d3c42e...# v7.0.1`, `dtolnay/rust-toolchain@e97e2d8...# v1`, `Swatinem/rust-cache@c193711...# v2.9.1`, `fsfe/reuse-action@676e2d5...# v6.0.0` |
| Dependabot: Cargo, npm, pip and GitHub Actions, weekly, cooldown, grouping | Implemented | `.github/dependabot.yml` |
| Zensical Pages deployment | Implemented and deployed | `.github/workflows/docs.yml` |
| Auto-tag workflow | Implemented | `.github/workflows/auto-tag.yml` |
| crates.io publish workflow | Planned | Not implemented (requires approval per AGENTS.md) |
| `cargo-dist` release pipeline | Implemented and live-verified | `.github/workflows/release.yml` validates the cargo-dist plan and builds the Linux x86_64 archive; release `v0.2.0` published that archive and `SHA256SUMS` after protected approval |
| `git-cliff` generates `CHANGELOG.md` in the version-bump flow; the release workflow consumes it | Implemented (CLI-004.13) | `cliff.toml`; `s/version++` pins/verifies `git-cliff 2.13.1` before invoking it; `.github/workflows/release.yml` `host` job extracts the tagged `## [x.y.z]` section for `gh release create --notes-file`, falling back to `--generate-notes` when that section is absent |
| Release publication is gated and recoverable | Implemented; protected gate live-verified, draft transaction remote run pending | CI run `33183332396` created `v0.2.0` only after every blocking job passed; run `33186261368` rebuilt the tag, waited for protected approval, and published the release. The native package revision stages every asset on a replaceable private draft, publishes only after the authoritative checksum is uploaded, and refuses to modify any published release. |
| Native GUI tester-preview builders | Implemented, remote run pending | `.github/workflows/release.yml` runs host/CLI tests then builds Linux x86_64 `.deb`, macOS arm64 `.dmg` and Windows x86_64 NSIS `.exe` packages with `s/gui-bundle`; the Linux `.deb` has been built and its payload inspected locally |

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
| FR-15 | CI runs the traceability checker's isolated fixture suite and then requires every tracked Rust file to carry a module-level `//! @see` header whose target path, literal node IDs and living-spec requirement all resolve. | Should Have |
| FR-16 | CI runs `cargo audit` as a separate blocking job using pinned `cargo-audit 0.22.2`. | Must Have |
| FR-17 | CI runs `zizmor --strict-collection .` as a separate blocking job using pinned Zizmor 1.29.0 and a read-only workflow token. | Must Have |
| FR-18 | The released installer verifies glibc 2.34+, every staged dynamic dependency and staged binary execution before replacing either installed binary; fixture tests pin transactional refusal. | Must Have |
| FR-19 | Auto-tag runs only after all CI jobs succeed on a `main` push; it fails before tagging unless `release` has a required reviewer; retries require an existing tag to resolve to the exact commit and continue into a protected, tag-serialized release-hosting job. Hosting stages the complete asset set on a private draft, replaces an interrupted draft as one transaction because native packages are not reproducible, publishes only after checksum upload, and refuses to modify an already published release. An explicit release-workflow dispatch accepts an existing tag for recovery before publication. | Must Have |
| FR-20 | Auto-tag workflow is implemented: a version bump on `main` creates `vX.Y.Z` and directly invokes release workflows rather than relying on tag-event recursion. | Must Have |
| FR-22 | The cargo-dist release workflow is invoked from auto-tag through `workflow_call`, builds Linux x86_64 `cortex` and `cortex-mcp`, and attaches the combined archive to the GitHub Release. | Must Have |
| FR-23 | The auto-tag workflow invokes the release workflow, which creates a GitHub Release for that tag with changelog notes. | Should Have |
| FR-25 | Every binary release publishes one authoritative `SHA256SUMS` covering its artifacts. | Must Have |
| FR-26 | A docs-root `install.sh` fetches the latest supported Linux archive, verifies it against `SHA256SUMS`, installs both binaries without requiring Rust or `protoc`, and refreshes or prescribes shell completions. | Must Have |
| FR-27 | Pull requests affecting the GUI, host or packaging run native Linux x86_64, macOS arm64 and Windows x86_64 jobs. Each job executes host/CLI tests, stages a target-qualified `cortex` helper, builds exactly one Tauri package, and verifies its native architecture, helper/resources and applicable package lifecycle or signature before uploading it for inspection. | Must Have |
| FR-28 | A tagged release publishes the Linux CLI/MCP archive plus `.deb`, `.dmg` and NSIS `.exe` GUI packages behind the protected release environment, with one regenerated `SHA256SUMS` covering every published artifact. Every GUI asset filename ends in `-preview` and the release notes identify those packages as provisional. | Must Have |
| FR-29 | Windows and macOS GUI artifacts remain labelled provisional and unsigned/ad-hoc signed until each host passes a documented real-device smoke. Windows provides a native and QEMU/KVM package, IPC, USB, GUI lifecycle, upgrade and uninstall runbook; native Windows remains the final host evidence, and the equivalent macOS runbook/evidence remains outstanding. | Must Have |

#### Planned

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-21 | crates.io publish workflow: gated on the release tag, publishes `cortex-rs` (and later `cortex-cli`) to crates.io. **Requires approval before first use** (AGENTS.md). | Must Have |
| FR-24 | Dependabot updates for `github-actions` are grouped and pinned to SHAs with version comments (matching the existing convention). | Should Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | No secrets are committed to the repo; crates.io publishing uses a secret stored in GitHub. | Review-enforced |
| NFR-2 | Action SHAs are pinned; Dependabot opens PRs to bump them (with the version comment updated). | Review-enforced |
| NFR-3 | The main code-quality job remains Linux-based; release previews add native Linux, macOS arm64 and Windows x86_64 package/test jobs. Windows/macOS hardware verification remains manual and outstanding. | Implemented; remote matrix passed in run `33317851191` |
| NFR-4 | The local gate documents its subset of CI; the remaining remote-only platform (Windows) cross-checks are not implied by local green. | Review-enforced |
| NFR-5 | Release actions are externally visible and require explicit approval before first use (AGENTS.md). | Process-enforced |

## Acceptance Criteria

- [x] CI runs fmt + clippy (both feature paths) + tests (both feature paths) + REUSE on every push/PR.
- [x] Actions are pinned to SHAs with `# vX.Y.Z` comments.
- [x] Dependabot watches Cargo, npm, pip and GitHub Actions, weekly, with cooldown and grouping.
- [x] CI uses `permissions: contents: read`.
- [x] CI installs Rust native prerequisites, rejects real device data, cross-checks the Windows host boundary and caches Cargo.
- [x] Auto-tag workflow created `v0.2.0` from the gated release commit on its first live version bump.
- [x] Documentation builds and deploys through the artifact-based Pages workflow.
- [x] CI type-checks/builds both GUI frontend modes and builds the full Tauri backend as a debug, unbundled boundary check.
- [x] CI tests and runs the Rust `@see` traceability gate.
- [x] CI runs blocking RustSec and Zizmor jobs with pinned, checksum-verified scanner binaries.
- [x] Auto-tag is downstream of every blocking CI job, and publication uses the protected `release` environment with safe tag/release retry behavior.
- [x] Release packaging enforces the glibc 2.34/`libudev.so.1` contract, and installer fixtures prove incompatible staged binaries do not replace an existing installation.
- [ ] crates.io publish workflow publishes on the release tag (requires approval before first use).
- [x] `cargo-dist` produces distributable Linux x86_64 `cortex` and `cortex-mcp` binaries on the release tag.
- [x] The release tag produces a GitHub Release with the exact git-cliff-generated changelog section.
- [x] The release publishes `SHA256SUMS`, installer fixtures prove that a mismatched artifact is refused, and the exact public installer passed the CLI-004.10 clean Ubuntu 22.04 smoke against v0.2.0.
- [x] Release-preview run `33317851191` passed native Linux, macOS arm64 and Windows x86_64 host tests, package builds and package-specific verification without hardware.

## Non-Goals

- **The local gate.** Owned by zone 500 (`s/test`, `s/lint`). This zone mirrors it in CI.
- **Signed or notarized GUI installers.** The tester previews deliberately establish reproducible package and host boundaries before credentials or support claims. macOS uses ad-hoc signing and Windows is unsigned.
- **Hardware smoke in CI.** CI has no hardware; the hardware smoke runbook is manual (AGENTS.md). This zone does not attempt to connect to a real Quad Cortex.

## Dependencies

- **GitHub Actions**: `actions/checkout@v7.0.1`, `dtolnay/rust-toolchain@v1`, `Swatinem/rust-cache@v2.9.2`, `taiki-e/install-action@v2.86.8`, `fsfe/reuse-action@v6.0.0` (all SHA-pinned).
- **Security scanners**: `cargo-audit 0.22.2` and Zizmor 1.29.0, installed from verified prebuilt releases by `taiki-e/install-action`.
- **Dependabot**: Cargo, npm, pip and GitHub Actions ecosystems.
- **`protoc`**: installed via `protobuf-compiler` for `prost-build`.
- **Zone 500 (DX tooling)**: the local gate that mirrors this zone's CI workflow.
- **Zone 900 (governance)**: the license/REUSE config this zone lints.
- **house-style distribution.md**: the release conventions for `cargo-dist` and crates.io.

## Future

- **crates.io first publish.** Requires `cargo login` with a token, `cargo publish --dry-run` in CI, and the maintainer's approval (AGENTS.md). The crate name is `cortex-rs`; the CLI binary crate is `cortex-cli`.
- **`cargo-dist`.** Produces the distributable Linux `cortex` and `cortex-mcp` archive. Tauri's bundler separately produces the native GUI packages because it owns application metadata, icons, sidecars and operating-system package formats.

- **Changelog generation.** Implemented (CLI-004.13): `cliff.toml` plus the pinned/verified `git-cliff` invocation in `s/version++`, and the tagged-section extraction in `.github/workflows/release.yml`'s `host` job.

## Glossary

| Term | Definition |
| --- | --- |
| SHA pin | A GitHub Action pinned to a specific commit SHA with a `# vX.Y.Z` comment, so a moving tag cannot silently change CI |
| Dependabot cooldown | A delay before Dependabot opens a PR for a new release, to avoid churn on rapid-release deps |
| Auto-tag | A workflow that produces a `vX.Y.Z` git tag when the version in `Cargo.toml` bumps |
| Release cascade | blocking `main` CI -> tag -> binary build -> protected GitHub Release; crates.io remains a separately approved future channel |
| `cargo-dist` | A tool that produces distributable Rust binaries from a release tag |
| Leaf engine, no default features | The `cargo build --no-default-features -p cortex-rs` path that confirms the crate builds without `hidapi` |

---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["ci", "release", "tasks", "roadmap"]
spec: spec.md
design: design.md
---

# 600 CI / Release - Tasks

> Implementation tasks for CI and the release pipeline. Phase 1 (CI + Dependabot) is done; phase 2 (release pipeline) is planned and requires approval before first use (AGENTS.md).

## Phase 1 - CI and Dependabot (done)

### 1.1 CI workflow

- [x] `.github/workflows/ci.yml` runs on `push` to `main`, `pull_request`, `workflow_dispatch`.
- [x] `permissions: contents: read`.
- [x] `test` job: `actions/checkout` (SHA-pinned, `persist-credentials: false`), `dtolnay/rust-toolchain` (stable + rustfmt + clippy), `Swatinem/rust-cache`.
- [x] Install `protoc` via `protobuf-compiler`.
- [x] `cargo fmt --all --check`.
- [x] `cargo clippy --all-targets --all-features -- -D warnings`.
- [x] `cargo clippy --all-targets --no-default-features -- -D warnings`.
- [x] `cargo test --all`.
- [x] `cargo test --all --no-default-features`.
- [x] `reuse` job: `fsfe/reuse-action` (SHA-pinned).

### 1.2 Dependabot config

- [x] `.github/dependabot.yml` version 2.
- [x] `cargo` ecosystem: workspace root + each crate directory; weekly Monday; cooldown (7/3/7/14 days); routine minor+patch grouping.
- [x] `github-actions` ecosystem: root; weekly Monday; 7-day cooldown; routine minor+patch grouping.

### 1.3 SHA pinning convention

- [x] All actions pinned to specific commit SHAs with `# vX.Y.Z` comments.
- [x] `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1`.
- [x] `dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9 # v1`.
- [x] `Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4 # v2.9.1`.
- [x] `fsfe/reuse-action@676e2d560c9a403aa252096d99fcab3e1132b0f5 # v6.0.0`.

## Phase 2 - Release pipeline (planned, requires approval before first use)

### 2.1 Auto-tag workflow

- [ ] Detect a version bump on `main` (parse `Cargo.toml` workspace version, compare to the latest tag).
- [ ] Create a `vX.Y.Z` tag on the bump commit.
- [ ] (Alternative) Trigger on the `s/version++` commit message format.
- [ ] Confirm the tag format matches `cargo-dist` and `cargo` conventions.

### 2.2 crates.io publish workflow

- [ ] Gate on the `vX.Y.Z` tag.
- [ ] Run `cargo publish --dry-run -p cortex-rs` in CI.
- [ ] Run `cargo publish -p cortex-rs` (using `CARGO_REGISTRY_TOKEN` secret).
- [ ] (When the CLI is released independently) publish `cortex-cli`.
- [ ] **Requires maintainer approval before first use** (AGENTS.md).
- [ ] Confirm the crate metadata (`description`, `repository`, `homepage`, `license`, `keywords`, `categories`) is complete in `Cargo.toml`.

### 2.3 `cargo-dist` release pipeline

- [ ] Add `cargo-dist` config (`dist-workspace.toml` or `[workspace.metadata.dist]`).
- [ ] Gate on the `vX.Y.Z` tag.
- [ ] Build `cortex` binary for `x86_64-unknown-linux-gnu` (and `aarch64-unknown-linux-gnu` if feasible).
- [ ] Attach tarballs to the GitHub Release.
- [ ] (When the GUI exists) produce `.AppImage`/`.deb` for the Tauri app.

### 2.4 GitHub Release

- [ ] Create a GitHub Release from the tag.
- [ ] Generate changelog notes (from `cargo-dist` or a `CHANGELOG.md`).
- [ ] Link the `cargo-dist` artifacts and the crates.io page in the release body.

## Phase 3 - Polish (planned)

### 3.1 Changelog

- [ ] Decide: `cargo-dist` auto-generated changelog vs. hand-maintained `CHANGELOG.md` (house-style docs.md).
- [ ] If hand-maintained, add `CHANGELOG.md` and wire it into the release notes.

### 3.2 Crate metadata

- [ ] Add `keywords`, `categories` to `cortex-rs/Cargo.toml` for crates.io discoverability.
- [ ] Confirm `description`, `repository`, `homepage`, `license` are complete and accurate.
- [ ] Add a `categories` allowlist entry if needed (e.g. `hardware-support`, `multimedia::audio`).

### 3.3 Frontend CI (blocked on zone 400)

- [ ] When `gui/` exists, add frontend lint + typecheck (`npm run check`) to CI.
- [ ] Add a Tauri build step (or a `cargo-dist` job) for the GUI artifacts.

## Phase 4 - Verification (planned)

### 4.1 Local-CI parity

- [ ] Audit `s/test` (zone 500) against `.github/workflows/ci.yml` step-by-step.
- [ ] Add `cargo test --all --no-default-features` to `s/test` to match CI.

### 4.2 Release dry-run

- [ ] Before the first real release, run the auto-tag + crates.io dry-run + `cargo-dist` against a throwaway tag to confirm the cascade end-to-end.
- [ ] Confirm the crates.io dry-run succeeds with the final crate metadata.

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1-1.3 | Implemented CI workflow + Dependabot config with SHA-pinned actions | `.github/workflows/ci.yml`, `.github/dependabot.yml` | [x] | [x] |
| 2026-08-01 | 1.3 | Wrote spec/design/tasks for this zone | `spec/600-ci-release/{spec,design,tasks}.md` | [x] | [x] |
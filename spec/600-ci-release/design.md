---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["ci", "release", "github-actions", "dependabot", "cargo-dist", "crates-io", "sha-pinning"]
spec: spec.md
---

# 600 CI / Release - Design

> Design for the CI workflow and the planned release pipeline. The interesting parts are the SHA-pinning convention (supply-chain hardening), the dual-feature-path clippy/test matrix (leaf-crate discipline), and the release cascade (auto-tag -> crates.io -> `cargo-dist`).

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [house-style ci.md](https://github.com/marcus-pacharanero/house-style/blob/main/ci.md) - the CI conventions.
- [house-style distribution.md](https://github.com/marcus-pacharanero/house-style/blob/main/distribution.md) - the release conventions.
- [500-dx-tooling design](../500-dx-tooling/design.md) - the local gate that mirrors this zone.
- Owned source: `.github/workflows/ci.yml`, `.github/dependabot.yml`.

## [DES-CI] The CI workflow

### Behaviour

`.github/workflows/ci.yml` runs on `push` to `main`, on `pull_request`, and on `workflow_dispatch`. It has two jobs: `test` (fmt, clippy, tests) and `reuse` (license lint). Permissions are `contents: read`.

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:
permissions:
  contents: read
env:
  CARGO_TERM_COLOR: always
jobs:
  test:
    name: fmt, clippy, test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9 # v1
        with:
          toolchain: stable
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4 # v2.9.1
      - name: Install protoc (for prost-build)
        run: sudo apt-get update && sudo apt-get install -y protobuf-compiler
      - name: Formatting
        run: cargo fmt --all --check
      - name: Clippy (all features)
        run: cargo clippy --all-targets --all-features -- -D warnings
      - name: Clippy (no default features - leaf protocol surface only)
        run: cargo clippy --all-targets --no-default-features -- -D warnings
      - name: Tests (all features)
        run: cargo test --all
      - name: Tests (leaf engine, no default features)
        run: cargo test --all --no-default-features
  reuse:
    name: REUSE licence lint
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: fsfe/reuse-action@676e2d560c9a403aa252096d99fcab3e1132b0f5 # v6.0.0
```

### Design choice: SHA pins with version comments

Every action is pinned to a specific commit SHA with a `# vX.Y.Z` comment. This is the house-style rule (ci.md / AGENTS.md): a moving tag (`@v7` is actually `@<latest>` and can move) is a supply-chain risk; a SHA cannot. The version comment lets a human read the version and lets `pin-github-action` validate the pin.

| Action | SHA | Version |
| --- | --- | --- |
| `actions/checkout` | `3d3c42e5aac5ba805825da76410c181273ba90b1` | `# v7.0.1` |
| `dtolnay/rust-toolchain` | `e97e2d8cc328f1b50210efc529dca0028893a2d9` | `# v1` |
| `Swatinem/rust-cache` | `c19371144df3bb44fab255c43d04cbc2ab54d1c4` | `# v2.9.1` |
| `fsfe/reuse-action` | `676e2d560c9a403aa252096d99fcab3e1132b0f5` | `# v6.0.0` |

Dependabot opens PRs to bump these SHAs (the `github-actions` ecosystem in `dependabot.yml`), and the version comment is updated in the same PR.

### Design choice: dual feature paths (all-features + no-default-features)

The crate is a leaf with `default-features = false` building the protocol/domain surface (no `hidapi`). CI runs clippy and tests on **both** paths:

- `--all-features`: the full crate including the `hid` feature (transport, hidapi).
- `--no-default-features`: the leaf protocol/domain surface only.

This enforces the leaf-crate discipline (AGENTS.md / 001-overview NFR-4): the crate must compile and pass clippy/tests without `hidapi` in the dependency graph. A regression that pulls `hidapi` into the no-default-features path fails CI.

### Design choice: `persist-credentials: false` on checkout

`actions/checkout` writes the `GITHUB_TOKEN` into `.git/config` by default. `persist-credentials: false` prevents that, so a later step cannot accidentally push using the token. The release pipeline will use a separate token with explicit write permissions.

### Design choice: REUSE as a separate job

The REUSE license lint is a separate job (`reuse`) rather than a step in `test`. This lets it run in parallel and keeps the license check visible in the CI status as a distinct signal. It also means a REUSE failure does not block the fmt/clippy/test signal (and vice versa).

### Design choice: `protoc` installed via `apt`

`prost-build` needs `protoc` at build time. CI installs it via `sudo apt-get install -y protobuf-compiler`. The alternative is `arrows-circle/protoc-action` or a prebuilt binary, but the `apt` package is the simplest and most stable on `ubuntu-latest`.

### Alternatives considered

- **A matrix job (all-features + no-default-features as two matrix entries).** Rejected: the two paths are sequential steps in one job, not parallel matrix entries. The path is fast enough that the serial run is fine, and a single job is simpler to read.
- **macOS/Windows in the matrix.** Rejected: the project is Linux-first; the USB HID transport is the focus. A matrix may be worth it for the GUI eventually, not for the crate.
- **`cargo-tarpaulin` or coverage.** Not yet; coverage is a future nicety, not a gate.

## [DES-DEPENDABOT] Dependabot config

### Behaviour

`.github/dependabot.yml` watches two ecosystems:

- **cargo**: workspace root + each crate directory (`/`, `/crates/cortex-rs`, `/crates/cortex-cli`, `/crates/cortex-mcp`). Weekly, Monday, with a cooldown (7 days default, 3 days for patch, 7 for minor, 14 for major). Routine minor+patch updates are grouped into one PR.
- **github-actions**: root `/`. Weekly, Monday, 7-day cooldown. Routine minor+patch updates grouped.

### Design choice: cooldown

The cooldown (`default-days: 7`, with shorter windows for patch/minor) prevents Dependabot from opening a PR the day a new version is released, which catches the case where a release is yanked or a follow-up patch ships a day later. The 14-day major cooldown gives the maintainer time to assess a breaking change.

### Design choice: grouping

Routine minor and patch updates are grouped (`applies-to: version-updates`) so Dependabot opens one PR for "bump all deps a minor/patch" rather than N PRs. Major updates are not grouped (they need individual review).

### Design choice: per-crate `directories` for cargo

Dependabot's cargo ecosystem needs each `Cargo.toml` listed. The workspace root `/` covers the workspace manifest; the per-crate directories cover the crate manifests. This ensures Dependabot sees all dependency declarations.

### Alternatives considered

- **Daily updates.** Rejected: weekly is enough for a project of this size; daily is noise.
- **No grouping.** Rejected: one PR per dep is spam; grouping the routine ones keeps the signal high.
- **Group major updates too.** Rejected: a major bump needs individual review and a dedicated changelog note.

## [DES-RELEASE] Release pipeline (partial)

### Behaviour

The release pipeline is partly wired. `s/version++` and auto-tag exist; the remaining plan follows house-style distribution.md:

1. **`s/version++`** (zone 500) bumps the canonical Rust workspace version and synchronizes the npm and Tauri manifests in one release commit.
2. **Auto-tag workflow** detects the version bump on `main` and creates a `vX.Y.Z` tag. Implemented.
3. **crates.io publish workflow** is gated on the `vX.Y.Z` tag. It runs `cargo publish --dry-run` in CI, then `cargo publish` for `cortex-rs` (and later `cortex-cli`). Requires approval before first use (AGENTS.md).
4. **`cargo-dist`** is invoked directly by `auto-tag.yml` through `workflow_call`. The first supported target is `x86_64-unknown-linux-gnu`, packaging both `cortex` and `cortex-mcp`; the release also publishes licences/notices, the udev rule and one authoritative `SHA256SUMS`.
5. **GitHub Release** is created from the tag with changelog notes (from `cargo-dist` or a `CHANGELOG.md`).
6. **Public installer** at the docs-site root resolves the latest release, verifies the selected archive against `SHA256SUMS`, installs both binaries and refreshes or prescribes completions.

### Design choice: workflow-call release cascade

`s/version++` creates the commit; the auto-tag workflow creates the tag and directly invokes the release workflow through `workflow_call`. The tag is the permanent release identity, but its `GITHUB_TOKEN` creation event is not relied upon to trigger another workflow because GitHub suppresses that recursion.

### Design choice: approval before first publish

Per AGENTS.md, publishing to crates.io, cutting a release tag, and any externally visible action require explicit approval. The crates.io publish workflow will not run on the first tag without the maintainer enabling it. The workflow uses a secret (`CARGO_REGISTRY_TOKEN`) stored in GitHub.

### Design choice: `cargo-dist` for binaries

`cargo-dist` produces distributable binaries from a release tag. The product surface is the pair: `cortex` owns the held USB session and `cortex-mcp` gives local agent harnesses a bounded stdio adapter to it. The first preview is Linux x86_64 only, matching the operational Unix IPC adapter and hardware evidence. Linux aarch64 follows validation. The host boundary now has a Windows named-pipe seam, but Windows waits for that adapter, detached-process lifecycle and hardware testing. The Tauri GUI will use Tauri's bundler once its backend is connected.

### Alternatives considered

- **Manual `cargo publish` and `gh release create`.** Rejected for the long term: it is error-prone and not reproducible. Fine for the very first release; the workflow is the steady-state target.
- **A separate `s/release` script.** Rejected (house-style distribution.md): the tag is the trigger; there is no separate release script. `s/version++` is the only local step.
- **Releasing the crate and the binary independently.** Rejected for now: they move with the same canonical version (`s/version++`).

## [DES-LIMITS] Known Limitations

- **Release pipeline is incomplete.** Auto-tag exists; crates.io publish, `cargo-dist`, GitHub Release generation and GUI bundles are not implemented.
- **No `CHANGELOG.md`.** Changelog generation is undecided (cargo-dist auto vs. hand-maintained per house-style docs.md).
- **Linux-only CI.** No macOS/Windows matrix; the project is Linux-first and the USB HID transport is the focus.
- **No hardware smoke in CI.** CI has no hardware; the hardware smoke runbook is manual.
- **No frontend CI.** The GUI exists, but frontend lint/typecheck and Tauri build jobs are not wired into CI.
- **`s/test` does not run the no-default-features test path.** CI runs `cargo test --all --no-default-features`; the local `s/test` currently runs all-features only (zone 500 gap).

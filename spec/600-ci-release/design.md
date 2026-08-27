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
- Owned source: `.github/workflows/{ci,docs,auto-tag}.yml`, `.github/dependabot.yml`.

## [DES-CI] The CI workflow

### Behaviour

`.github/workflows/ci.yml` runs on `push` to `main`, `pull_request`, and `workflow_dispatch`. The test job installs protobuf, hidapi/udev and Tauri Linux prerequisites; rejects real device data; runs both feature configurations; and cross-checks host/MCP code for Windows. REUSE is a separate job. The YAML below is schematic; the workflow file is authoritative.

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
      - name: Install protoc and Linux native dependencies
        run: sudo apt-get update && sudo apt-get install -y <see workflow>
      - name: Formatting
        run: cargo fmt --all --check
      - name: Clippy (all features)
        run: cargo clippy --all-targets --all-features -- -D warnings
      - name: Clippy (no default features - leaf protocol surface only)
        run: cargo clippy --all-targets --no-default-features -- -D warnings
      - name: Tests (workspace default features)
        run: cargo test --all
      - name: Tests (workspace, no default features)
        run: cargo test --all --no-default-features
      - name: Tauri build boundary (debug, no bundle)
        run: npm run tauri --prefix gui -- build --debug --no-bundle --ci
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

### Design choice: dual feature configurations

The crate is a leaf with `default-features = false` building the protocol/domain surface (no `hidapi`). CI runs clippy and tests on **both** paths:

- clippy with `--all-features`: the complete workspace including HID.
- clippy/tests with `--no-default-features`: a workspace-wide feature cross-check. The focused leaf guarantee remains `cargo check --no-default-features -p cortex-rs`.

This enforces the leaf-crate discipline (AGENTS.md / 001-overview NFR-4): the crate must compile and pass clippy/tests without `hidapi` in the dependency graph. A regression that pulls `hidapi` into the no-default-features path fails CI.

### Design choice: `persist-credentials: false` on checkout

`actions/checkout` writes the `GITHUB_TOKEN` into `.git/config` by default. `persist-credentials: false` prevents that, so a later step cannot accidentally push using the token. The release pipeline will use a separate token with explicit write permissions.

### Design choice: REUSE as a separate job

The REUSE license lint is a separate job (`reuse`) rather than a step in `test`. This lets it run in parallel and keeps the license check visible in the CI status as a distinct signal. It also means a REUSE failure does not block the fmt/clippy/test signal (and vice versa).

### Design choice: `protoc` installed via `apt`

`prost-build` needs `protoc`; hidapi needs udev development files; and the Tauri workspace member needs WebKitGTK and related Linux libraries. CI installs the complete explicit package set with apt.

### Design choice: a Tauri build boundary, debug and unbundled

`cargo test --all` and `cargo clippy --all-targets --all-features` already compile `gui/src-tauri` as a workspace member, but neither one drives it through the Tauri CLI, so neither exercises `tauri-build`'s parse of `tauri.conf.json`, the declared icon files, or the `beforeBuildCommand` (`npm run build:tauri`) that produces `frontendDist`. A config typo, a missing icon, or a frontend build that the plain `vite build` step in "Check fixture and Tauri frontends" does not reach would only surface on a maintainer's machine or in a real release build. The added step runs `npm run tauri --prefix gui -- build --debug --no-bundle --ci`: `--debug` skips release-mode optimisation (this is a compile/config boundary, not a performance or distributable-artifact check), and `--no-bundle` skips packaging into a `.deb`/AppImage, which `bundle.active: false` in `tauri.conf.json` already opts out of pending CLI-004.4. `--ci` suppresses interactive prompts. This is the "Tauri build boundary" from ENG-002.2; producing a real installable bundle remains part of the future `cargo-dist`/GUI-bundle release pipeline, not this CI gate.

### Alternatives considered

- **A matrix job (all-features + no-default-features as two matrix entries).** Rejected: the two paths are sequential steps in one job, not parallel matrix entries. The path is fast enough that the serial run is fine, and a single job is simpler to read.
- **macOS/Windows in the matrix.** Rejected: the project is Linux-first; the USB HID transport is the focus. A matrix may be worth it for the GUI eventually, not for the crate.
- **`cargo-tarpaulin` or coverage.** Not yet; coverage is a future nicety, not a gate.

## [DES-DEPENDABOT] Dependabot config

### Behaviour

`.github/dependabot.yml` watches four ecosystems:

- **cargo**: workspace root. Cargo discovers member manifests from the workspace. Weekly with cooldown and routine grouping.
- **npm**: `/gui`, weekly with cooldown and routine grouping.
- **pip**: repository root for documentation tooling, weekly with cooldown and routine grouping.
- **github-actions**: root `/`. Weekly, Monday, 7-day cooldown. Routine minor+patch updates grouped.

### Design choice: cooldown

The cooldown (`default-days: 7`, with shorter windows for patch/minor) prevents Dependabot from opening a PR the day a new version is released, which catches the case where a release is yanked or a follow-up patch ships a day later. The 14-day major cooldown gives the maintainer time to assess a breaking change.

### Design choice: grouping

Routine minor and patch updates are grouped (`applies-to: version-updates`) so Dependabot opens one PR for "bump all deps a minor/patch" rather than N PRs. Major updates are not grouped (they need individual review).

### Design choice: workspace-root Cargo discovery

One Cargo entry at `/` covers the workspace and member manifests; duplicate per-crate entries would generate overlapping updates.

### Alternatives considered

- **Daily updates.** Rejected: weekly is enough for a project of this size; daily is noise.
- **No grouping.** Rejected: one PR per dep is spam; grouping the routine ones keeps the signal high.
- **Group major updates too.** Rejected: a major bump needs individual review and a dedicated changelog note.

## [DES-RELEASE] Release pipeline (partial)

### Behaviour

The release pipeline is partly wired. `s/version++`, auto-tagging, the Linux preview, and GitHub Release hosting exist; crates.io publishing, GUI bundles, and a live cascade smoke remain. The implementation follows house-style distribution.md:

1. **`s/version++`** (zone 500) bumps the canonical Rust workspace version, synchronizes the npm and Tauri manifests, and regenerates `CHANGELOG.md` with a pinned git-cliff version in one release commit.
2. **Auto-tag workflow** detects the version bump on `main` and creates a `vX.Y.Z` tag. Implemented.
3. **Planned crates.io publish workflow** will run dry-run and publish selected crates only after explicit approval.
4. **Partially implemented `cargo-dist` workflow** is invoked directly by `auto-tag.yml` through `workflow_call`. The first supported target is `x86_64-unknown-linux-gnu`, packaging both binaries plus licences/notices, the canonical two-product `70-neural-dsp-cortex.rules` and `SHA256SUMS`. The rule granting Nano access is not a Nano runtime support claim. Preview builds pass; live publication remains unexercised.
5. **GitHub Release** hosting is implemented. It publishes the tag's exact git-cliff section when present and falls back to GitHub-generated notes before the first generated changelog; the live cascade remains unevidenced.
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

- **Release pipeline is incomplete.** Auto-tagging, the Linux preview, and GitHub Release hosting exist; crates.io publishing, GUI bundles, and a live auto-tag-to-release smoke remain.
- **No committed `CHANGELOG.md` yet.** The pinned git-cliff flow is implemented, but the file is first generated by the next `s/version++` release commit.
- **Linux-native CI.** There is no native macOS/Windows matrix, but host and MCP crates are cross-checked for Windows.
- **No hardware smoke in CI.** CI has no hardware; the hardware smoke runbook is manual.
- **Frontend CI is Linux-only.** CI installs the locked npm tree and type-checks/builds both explicit fixture and Tauri frontend modes. Native Windows/macOS Tauri and hardware paths remain deferred until those hosts are supported.

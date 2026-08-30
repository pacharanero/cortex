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

> Design for the CI workflow and release pipeline. The interesting parts are the SHA-pinning convention (supply-chain hardening), the dual-feature-path clippy/test matrix (leaf-crate discipline), the native Tauri tester-preview matrix, and the release cascade.

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [house-style ci.md](https://github.com/marcus-pacharanero/house-style/blob/main/ci.md) - the CI conventions.
- [house-style distribution.md](https://github.com/marcus-pacharanero/house-style/blob/main/distribution.md) - the release conventions.
- [500-dx-tooling design](../500-dx-tooling/design.md) - the local gate that mirrors this zone.
- Owned source: `.github/workflows/{ci,docs,auto-tag}.yml`, `.github/dependabot.yml`.

## [DES-CI] The CI workflow

### Behaviour

`.github/workflows/ci.yml` runs on `push` to `main`, `pull_request`, and `workflow_dispatch`. The test job installs protobuf, hidapi/udev and Tauri Linux prerequisites; rejects real device data; runs released-installer fixtures and both feature configurations; and cross-checks host/MCP code for Windows. REUSE, RustSec audit and Zizmor are separate blocking jobs. A final reusable-workflow job exists only for pushes to `main`, depends on all four gates, and calls auto-tag after they pass. The YAML below is schematic; the workflow file is authoritative.

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
  audit:
    name: Security audit
    # pinned taiki-e/install-action -> cargo-audit 0.22.2 -> cargo audit
  workflow-security:
    name: GitHub Actions security
    # pinned taiki-e/install-action -> Zizmor 1.29.0 -> strict collection
  tag-release:
    needs: [test, audit, workflow-security, reuse]
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    uses: ./.github/workflows/auto-tag.yml
```

### Design choice: SHA pins with version comments

Every action is pinned to a specific commit SHA with a `# vX.Y.Z` comment. This is the house-style rule (ci.md / AGENTS.md): a moving tag (`@v7` is actually `@<latest>` and can move) is a supply-chain risk; a SHA cannot. The version comment lets a human read the version and lets `pin-github-action` validate the pin.

| Action | SHA | Version |
| --- | --- | --- |
| `actions/checkout` | `3d3c42e5aac5ba805825da76410c181273ba90b1` | `# v7.0.1` |
| `dtolnay/rust-toolchain` | `6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772` | `# v1` |
| `Swatinem/rust-cache` | `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` | `# v2.9.2` |
| `taiki-e/install-action` | `fcf5432d9f50d67e37ee6e29bdb7a224ff67b4a7` | `# v2.86.8` |
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

### Design choice: security scanners as separate blocking jobs

`cargo audit` and `zizmor --strict-collection .` are distinct jobs so a dependency advisory and an unsafe workflow each produce one unambiguous required-check signal. Both prebuilt scanner binaries are version-pinned and installed through the SHA-pinned `taiki-e/install-action` with `fallback: none`; installation cannot silently switch to a source build or a different downloader. Zizmor receives only the workflow's read-only token for online ref validation. Auto-tag is a reusable workflow called by the final main-only CI job rather than a privileged `workflow_run` trigger, which Zizmor correctly treats as fundamentally dangerous.

### Design choice: `protoc` installed via `apt`

`prost-build` needs `protoc`; hidapi needs udev development files; and the Tauri workspace member needs WebKitGTK and related Linux libraries. CI installs the complete explicit package set with apt.

### Design choice: a Tauri build boundary, debug and unbundled

`cargo test --all` and `cargo clippy --all-targets --all-features` already compile `gui/src-tauri` as a workspace member, but neither one drives it through the Tauri CLI, so neither exercises `tauri-build`'s parse of `tauri.conf.json`, the declared icon files, or the `beforeBuildCommand` (`npm run build:tauri`) that produces `frontendDist`. A config typo, a missing icon, or a frontend build that the plain `vite build` step in "Check fixture and Tauri frontends" does not reach would otherwise surface only in the native package matrix. The core test job runs `npm run tauri --prefix gui -- build --debug --no-bundle --ci`: `--debug` skips release-mode optimisation (this is a compile/config boundary, not a performance or distributable-artifact check), and `--no-bundle` keeps this fast boundary separate from the real package builds in the native GUI matrix. `--ci` suppresses interactive prompts. This is the "Tauri build boundary" from ENG-002.2.

### Alternatives considered

- **A matrix job (all-features + no-default-features as two matrix entries).** Rejected: the two paths are sequential steps in one job, not parallel matrix entries. The path is fast enough that the serial run is fine, and a single job is simpler to read.
- **macOS/Windows in the core crate matrix.** Rejected: the project is Linux-first and the USB HID transport remains hardware-verified only there. Native macOS and Windows runners belong to the separate GUI package matrix, where they exercise their host IPC and packaging boundaries without overstating hardware support.
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

The release pipeline is partly wired. `s/version++`, post-gate auto-tagging and the Linux archive are live; protected draft-staged GitHub Release hosting and the native GUI package matrix are implemented with their first remote run pending, while crates.io publishing remains future work. The implementation follows house-style distribution.md:

1. **`s/version++`** (zone 500) runs both full local gates, bumps the canonical Rust workspace version, synchronizes the npm and Tauri manifests, and regenerates `CHANGELOG.md` with a pinned git-cliff version in one release commit.
2. **Auto-tag workflow** is called only after test, RustSec, Zizmor and REUSE pass on a `main` push; it detects the version bump and creates a `vX.Y.Z` tag. Implemented.
3. **Planned crates.io publish workflow** will run dry-run and publish selected crates only after explicit approval.
4. **`cargo-dist` workflow** is invoked directly by `auto-tag.yml` through `workflow_call`. The first supported target is `x86_64-unknown-linux-gnu`, packaging both binaries plus licences/notices, the canonical two-product `70-neural-dsp-cortex.rules` and `SHA256SUMS`. Before packaging, `s/check-release-runtime` rejects binaries requiring glibc newer than 2.34 or a `cortex` binary that does not declare `libudev.so.1`. The rule granting Nano access is not a Nano runtime support claim.
5. **Native GUI matrix** runs on Ubuntu 22.04 x86_64, macOS 15 arm64 and Windows 2022 x86_64. Each runner installs the same locked frontend tree and pinned `protoc`, executes the host/CLI tests natively, stages `cortex-<target-triple>[.exe]`, and asks Tauri to produce exactly one `.deb`, `.dmg` or NSIS `.exe`. Linux inspects architecture, helper, udev rule and post-install activation; macOS mounts the DMG and verifies its helper/resources and ad-hoc signature; Windows performs a silent current-user install, runs the helper, launches the GUI and verifies hook-driven uninstall. The ordinary development config keeps bundling disabled; the release overlay alone supplies icons, package metadata, resources and `externalBin`. `docs/gui/windows-smoke.md` then separates QEMU/KVM tester evidence from the final native Windows host pass; the equivalent macOS device smoke is still outstanding.
6. **GitHub Release** hosting is implemented behind the protected `release` environment. Auto-tag reads the environment first and refuses to mint the tag unless a required-reviewer rule exists. It publishes the tag's exact git-cliff section when present and otherwise asks GitHub's generate-notes API for a notes file. A tag-scoped concurrency group serializes retries. The host creates a private draft, renames every native GUI asset with a `-preview` suffix, prepends the provisional-host warning to release notes, uploads every payload, uploads the regenerated `SHA256SUMS` last, and publishes only after all uploads succeed. An interrupted retry deletes and replaces only that private draft because native installers are not reproducible; an already published release is immutable and makes the workflow fail rather than add, replace or retain stale assets.
7. **Public installer** at the docs-site root resolves the latest release and verifies the selected archive against `SHA256SUMS`. It then checks host glibc, runs `ldd` over both staged binaries, executes staged CLI version output and starts staged MCP with EOF. Only after every check passes does it stage the complete replacement in the destination, retain the prior files and replace them; a final rename failure restores the prior set before returning failure. This script remains the Linux CLI/MCP installer rather than guessing how to automate native GUI trust prompts.

The first live cascade on 2026-08-28 created annotated tag `v0.2.0` at release commit `c2a7612` after all main gates passed. Its host job stopped before release creation because `upload-artifact` had preserved the `target/release-preview` hierarchy while the consumer expected flattened files. PR #36 corrected that contract and added a protected exact-tag dispatch; recovery run `33186261368` rebuilt the tag on Ubuntu 22.04, waited for required approval, published the release, and produced a checksum-valid archive with the expected contents.

### Design choice: workflow-call release cascade

`s/version++` creates the commit; the main-push CI job runs every blocking check, then calls auto-tag as a reusable workflow. Auto-tag creates the tag and directly invokes the release workflow through a second `workflow_call`. The tag is the permanent release identity, but its `GITHUB_TOKEN` creation event is not relied upon to trigger another workflow because GitHub suppresses that recursion. Avoiding `workflow_run` also prevents a privileged workflow from checking out an attacker-controlled run.

### Design choice: approval before first publish

Per AGENTS.md, publishing to crates.io, cutting a release tag, and any externally visible action require explicit approval. Binary hosting names the protected `release` environment and waits for its required reviewer after the read-only archive build. The crates.io publish workflow will not run on the first tag without the maintainer enabling it. The workflow uses a secret (`CARGO_REGISTRY_TOKEN`) stored in GitHub.

### Design choice: `cargo-dist` for binaries

`cargo-dist` produces distributable binaries from a release tag. The product surface is the pair: `cortex` owns the held USB session and `cortex-mcp` gives local agent harnesses a bounded stdio adapter to it. The first supported archive remains Linux x86_64, matching the hardware evidence. Tauri's bundler handles the GUI separately: its `externalBin` contract renames the staged target-qualified helper to the ordinary sibling `cortex` executable that the Rust backend already discovers. The Windows named-pipe and detached-process boundaries are implemented, and native no-hardware tests are configured with their first remote run pending; Windows and macOS packages remain provisional until real-device smoke closes that gap.

### Alternatives considered

- **Manual `cargo publish` and `gh release create`.** Rejected for the long term: it is error-prone and not reproducible. Fine for the very first release; the workflow is the steady-state target.
- **A separate `s/release` script.** Rejected (house-style distribution.md): the tag is the trigger; there is no separate release script. `s/version++` is the only local step.
- **Releasing the crate and the binary independently.** Rejected for now: they move with the same canonical version (`s/version++`).

## [DES-LIMITS] Known Limitations

- **Release pipeline is incomplete.** Post-gate auto-tagging, the Linux archive, native GUI package matrix, protected draft-staged GitHub Release hosting and pre-publication retry repair are implemented. The release workflow accepts an existing tag on explicit dispatch so a workflow defect can be fixed before replacing an interrupted private draft through the same protected hosting job; it will not repair or mutate a published release. The native matrix still needs its first remote pass; crates.io publishing remains future work.
- **Native does not mean hardware-verified.** Once run, macOS and Windows runners prove compilation, local IPC/process behavior and package production without a connected Cortex. They do not establish USB permissions, HID behavior or device compatibility.
- **No hardware smoke in CI.** CI has no hardware; the hardware smoke runbook is manual.
- **Frontend quality checks remain Linux-centralized.** The full frontend unit/type/build suite runs in the main Linux job; native release jobs rebuild the locked frontend through Tauri and focus on host tests plus packaging rather than triplicating every browser-fixture test.

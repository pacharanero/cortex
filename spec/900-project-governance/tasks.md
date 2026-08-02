---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["governance", "license", "tasks", "roadmap"]
spec: spec.md
design: design.md
---

# 900 Project Governance - Tasks

> Implementation tasks for licensing, attribution, and legal hygiene. Phase 1 (the core posture) is done; phase 2 is the future polish (dual licensing, company name, CLA decision).

## Phase 1 - Core posture (done)

### 1.1 License files

- [x] `LICENSE` with the full AGPL-3.0 text.
- [x] `LICENSES/AGPL-3.0-or-later.txt` present.
- [x] `LICENSES/MIT.txt` present (for the vendored .proto).
- [x] `Cargo.toml` `license = "AGPL-3.0-or-later"`.

### 1.2 SPDX headers

- [x] Every `.rs` file has `SPDX-FileCopyrightText: 2026 Dr Marcus Baw` / `SPDX-License-Identifier: AGPL-3.0-or-later`.
- [x] Every `.toml`, `.yml`, `.sh` file has the same header (with the language's comment prefix).
- [x] Vendored `.proto` files keep their own MIT SPDX header (`SPDX-FileCopyrightText: 2026 Stokes` / `SPDX-License-Identifier: MIT`).

### 1.3 `REUSE.toml`

- [x] Bulk annotations for `**/*.md`, `**/*.txt`, `**/*.json`, `**/Cargo.lock`, `LICENSE`, `.gitignore`, `.editorconfig`, etc.
- [x] `precedence = "aggregate"` so the vendored .proto's own MIT header is not overridden.
- [x] `reuse lint` passes.

### 1.4 Attribution

- [x] `NOTICE`: project copyright, AGPL+CC-BY-SA decision, attribution for `pyquadcortex` (MIT, vendored .proto) and `deskop-nano-cortex` (Apache-2.0, architectural precedent).
- [x] `THIRD-PARTY-NOTICES.md`: full attribution + license texts for `pyquadcortex` (MIT), `deskop-nano-cortex` (Apache-2.0), `qc-stomp-tools` (MIT), and the four reference-only unlicensed repos.
- [x] Prior-art licensing table in `AGENTS.md` is the source of truth; `THIRD-PARTY-NOTICES.md` mirrors it.

### 1.5 Trademark and unaffiliation

- [x] Trademark/unaffiliation notice in `README.md` (Unofficial blockquote).
- [x] Trademark/unaffiliation notice in `AGENTS.md` (Legal hygiene).
- [x] Trademark/unaffiliation notice in `NOTICE` (Trademark and unaffiliation notice section).
- [x] Reverse-engineering legal basis cited (UK CDPA s50B / s296A, EU Software Directive Art 6) in `README.md` and `NOTICE`.

### 1.6 REUSE enforcement

- [x] `reuse lint` runs in CI (zone 600 `reuse` job).
- [x] `reuse lint` runs locally in `s/lint` (zone 500, with a graceful fallback if `reuse` is not installed).

## Phase 2 - Future polish (planned, requires approval)

### 2.1 Copyright holder

- [ ] Confirm the copyright holder with Marcus: keep "2026 Dr Marcus Baw", or add a company name (AGENTS.md: mixed-domain work without a default company).
- [ ] If the holder changes, update every SPDX header and `REUSE.toml` in one commit.

### 2.2 Dual licensing

- [ ] If a closed derivative needs to exist, add a `DUAL-LICENSE.md` and the dual-license boilerplate.
- [ ] Requires approval (AGENTS.md).

### 2.3 `qc-stomp-tools` incorporation (conditional)

- [ ] Only if we target on-device builds: adapt `qc-stomp-tools` (MIT) code with attribution.
- [ ] Carry the upstream copyright and add a `NOTICE`/`THIRD-PARTY-NOTICES.md` entry.

### 2.4 CLA decision

- [ ] Decide whether a contributor licence agreement is needed (current stance: not in scope; the AGPL header is the inbound-outbound grant).

## Phase 3 - Verification (planned)

### 3.1 REUSE audit

- [ ] Run `reuse lint` and confirm zero warnings.
- [ ] Audit the prior-art table in `AGENTS.md` against `THIRD-PARTY-NOTICES.md` for drift.

### 3.2 Unlicensed-content audit

- [ ] Confirm no content from the four reference-only unlicensed repos is committed into this repo's tree.
- [ ] Confirm findings from those repos are cited in our own words in `quad-cortex-linux-editor-and-protocol.md` and link out.

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1-1.6 | Established the licensing, attribution, and REUSE posture | `LICENSE`, `LICENSES/`, `NOTICE`, `THIRD-PARTY-NOTICES.md`, `REUSE.toml`, SPDX headers, `AGENTS.md`, `README.md` | [x] | [x] |
| 2026-08-01 | 1.6 | Wrote spec/design/tasks for this zone | `spec/900-project-governance/{spec,design,tasks}.md` | [x] | [x] |
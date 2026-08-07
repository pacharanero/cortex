---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["governance", "license", "agpl", "spdx", "reuse", "notice", "trademark", "unaffiliation"]
---

# 900 Project Governance - Spec

> Owns the licensing, attribution, trademark/unaffiliation, and legal-hygiene posture for the repo. AGPL-3.0-or-later for code, CC-BY-SA-4.0 for written content, SPDX headers on every source file, `REUSE.toml` for header-less files, and `NOTICE`/`THIRD-PARTY-NOTICES.md` for attribution of the MIT- and Apache-2.0-licensed prior art. This is the cross-cutting zone that keeps the project legally clean and reusable.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [AGENTS.md](../../AGENTS.md) - the project license decision, prior-art licensing table, and the approval-required actions (editing `NOTICE`/`THIRD-PARTY-NOTICES.md`).
- [house-style licensing.md](https://github.com/marcus-pacharanero/house-style/blob/main/licensing.md) - the licensing conventions (AGPL code, CC-BY-SA content, SPDX headers, REUSE).
- [Legal and attribution guide](../../docs/legal.md) - the reverse-engineering-for-interoperability legal basis and prior-art policy.
- [600-ci-release spec](../600-ci-release/spec.md) - the CI job that enforces the REUSE license lint.
- [500-dx-tooling spec](../500-dx-tooling/spec.md) - the local `s/lint` that runs `reuse lint`.
- Owned source: `AGENTS.md`, `LICENSE`, `LICENSES/`, `NOTICE`, `THIRD-PARTY-NOTICES.md`, `REUSE.toml`, the SPDX header on every source file.

## Problem Statement

This is an unofficial, reverse-engineered interoperability client for hardware whose vendor ships no Linux editor. That posture carries legal-hygiene obligations the project must satisfy from day one:

1. **A deliberate license choice.** AGPL-3.0-or-later for code (not available for proprietary subsumption), CC-BY-SA-4.0 for written content. The MIT- and Apache-2.0-licensed prior art we port from remains under its own terms; attribution is recorded, not relicensed.
2. **Attribution of prior art.** `NOTICE` and `THIRD-PARTY-NOTICES.md` record the copyright and licence of projects whose work is incorporated or adapted (`pyquadcortex` MIT and `deskop-nano-cortex` Apache-2.0), licensed projects retained only as references (`qc-stomp-tools` and the indirect `nano-cortex-web-editor` BLE provenance), and the four studied repositories without a clear repository-wide licence.
3. **Trademark and unaffiliation.** "Neural DSP", "Quad Cortex", "Nano Cortex", and "Cortex Control" are trademarks of Neural DSP Technologies. The project is unaffiliated and says so in `README.md`, `AGENTS.md`, and `NOTICE`.
4. **SPDX headers on every source file.** `SPDX-FileCopyrightText` and `SPDX-License-Identifier` on every `.rs`, `.toml`, `.yml`, `.sh`, etc. `REUSE.toml` covers the header-less files (markdown, JSON, lockfiles, `.gitignore`, `.editorconfig`).
5. **REUSE compliance.** `reuse lint` passes in CI and locally; the repo is REUSE-compliant.
6. **Legal-hygiene norms for reverse engineering.** Do not redistribute Neural DSP binaries, firmware, or artwork. Do not publish raw captures containing their strings. Keep the recovered schema limited to what interoperability requires. State clearly that the work is unofficial. Prefer the USB route over the device-rooting route.

This zone owns the files that encode these obligations.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| Code license is AGPL-3.0-or-later | Implemented | `LICENSE` (full AGPL text), `Cargo.toml` `license = "AGPL-3.0-or-later"`, SPDX headers on source files |
| Written content license is CC-BY-SA-4.0 | Implemented | final matching Markdown annotation in `REUSE.toml` and `LICENSES/CC-BY-SA-4.0.txt` |
| `LICENSES/AGPL-3.0-or-later.txt` is present | Implemented | Full license text in `LICENSES/` |
| `LICENSES/CC-BY-SA-4.0.txt` is present | Implemented | Full written-content license text in `LICENSES/` |
| `LICENSES/MIT.txt` is present (for the vendored .proto) | Implemented | MIT license text in `LICENSES/` |
| `NOTICE` records attribution for `pyquadcortex` (MIT) | Implemented | `NOTICE` -> Recovered schema provenance |
| `NOTICE` records attribution for `deskop-nano-cortex` (Apache-2.0) | Implemented | `NOTICE` -> architectural precedent |
| `THIRD-PARTY-NOTICES.md` matches the current prior-art table | Implemented | Distinguishes incorporated/adapted, MIT reference, indirect Nano BLE provenance, mixed file-level notices, and no-licence reference projects |
| `REUSE.toml` covers header-less files | Implemented | `REUSE.toml` bulk annotations for `**/*.md`, `**/*.txt`, `**/*.json`, `**/Cargo.lock`, `LICENSE`, `.gitignore`, `.editorconfig`, etc. |
| Every file is REUSE-covered | Implemented | Source files use inline SPDX where supported; generated/headerless files use bulk annotations |
| Vendored `.proto` files keep their own MIT SPDX header | Implemented | `crates/cortex-rs/proto/*.proto` carry `SPDX-FileCopyrightText: 2026 Stokes` / `SPDX-License-Identifier: MIT` |
| Trademark and unaffiliation notice in `README.md` | Implemented | `README.md` -> Unofficial blockquote |
| Trademark and unaffiliation notice in `AGENTS.md` | Implemented | `AGENTS.md` -> Legal hygiene |
| Trademark and unaffiliation notice in `NOTICE` | Implemented | `NOTICE` -> Trademark and unaffiliation notice |
| `reuse lint` passes | Implemented | CI `reuse` job (zone 600); local `s/lint` (zone 500) |
| Reverse-engineering legal basis cited (UK CDPA s50B / s296A, EU Software Directive Art 6) | Implemented | `README.md`, `NOTICE` |

## User Stories

### Primary Users

Maintainers, downstream consumers, and anyone auditing the project's legal posture.

### Stories

**As a** downstream consumer
**I want** the license to be AGPL-3.0-or-later with clear attribution of the MIT/Apache prior art
**So that** I know my obligations and the project's provenance.

**As a** maintainer
**I want** `reuse lint` to pass in CI
**So that** every file has a verified SPDX header or a `REUSE.toml` annotation.

**As a** Neural DSP user
**I want** the README and NOTICE to state clearly that the project is unofficial and unaffiliated
**So that** there is no confusion about endorsement.

**As a** maintainer
**I want** the prior-art licensing table in AGENTS.md to be the source of truth for what may be taken from where
**So that** I do not accidentally commit material whose repository-wide licensing is unclear into this repo's tree.

## Requirements

### Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | The project license is **AGPL-3.0-or-later** for code and **CC-BY-SA-4.0** for written content. `LICENSES/` contains both canonical texts; `LICENSE` carries the project code license. | Must Have |
| FR-2 | Every source file carries an SPDX header: `SPDX-FileCopyrightText: 2026 Dr Marcus Baw` and `SPDX-License-Identifier: AGPL-3.0-or-later`, using the language's comment prefix. | Must Have |
| FR-3 | `REUSE.toml` first covers headerless assets/config as AGPL, then applies a final matching CC-BY-SA annotation to Markdown. Matching order is part of the contract. | Must Have |
| FR-4 | `NOTICE` records the project copyright, the AGPL+CC-BY-SA license decision, and the attribution for `pyquadcortex` (MIT, vendored .proto) and `deskop-nano-cortex` (Apache-2.0, architectural precedent). | Must Have |
| FR-5 | `THIRD-PARTY-NOTICES.md` records full attribution and applicable licence text or link for incorporated/adapted work (`pyquadcortex` MIT and `deskop-nano-cortex` Apache-2.0), the licensed projects currently retained as references (`qc-stomp-tools` and the indirect `nano-cortex-web-editor` BLE provenance), and the reference-only repositories without a clear repository-wide licence (`OpenCortex`, `qc-extras`, `quad-cortex-usb-re-notes`, `toneparse`). | Must Have |
| FR-6 | `LICENSES/MIT.txt` is present, covering the vendored `.proto` files' license. | Must Have |
| FR-7 | The vendored `.proto` files in `crates/cortex-rs/proto/` keep their own MIT SPDX header (`SPDX-FileCopyrightText: 2026 Stokes` / `SPDX-License-Identifier: MIT`) above the recovery note. | Must Have |
| FR-8 | `reuse lint` passes (CI-enforced in zone 600, locally in zone 500). | Must Have |
| FR-9 | A trademark and unaffiliation notice appears in `README.md`, `AGENTS.md`, and `NOTICE`: "Neural DSP", "Quad Cortex", "Nano Cortex", and "Cortex Control" are trademarks of Neural DSP Technologies; the project is unofficial and unaffiliated. | Must Have |
| FR-10 | The reverse-engineering legal basis is cited (UK CDPA s50B / s296A, EU Software Directive Art 6) in `README.md` and `NOTICE`. | Must Have |
| FR-11 | The prior-art licensing table in `AGENTS.md` is the source of truth for what may be taken from which repo, and is mirrored in `THIRD-PARTY-NOTICES.md`. | Must Have |
| FR-12 | Editing `NOTICE` or `THIRD-PARTY-NOTICES.md` attribution requires approval (AGENTS.md). | Must Have |
| FR-13 | No vendored content from the reference-only repositories is committed into this repo's tree. Findings from them are cited in our own words and linked out. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | `reuse lint` passes with no warnings. | CI-enforced |
| NFR-2 | The SPDX copyright holder line is consistent across the repo. | Review-enforced |
| NFR-3 | The trademark/unaffiliation notice is consistent across `README.md`, `AGENTS.md`, and `NOTICE`. | Review-enforced |
| NFR-4 | The prior-art table in `AGENTS.md` and the entries in `THIRD-PARTY-NOTICES.md` do not drift. | Review-enforced |

## Acceptance Criteria

- [x] `LICENSE` contains the full AGPL-3.0 text.
- [x] `LICENSES/AGPL-3.0-or-later.txt`, `LICENSES/CC-BY-SA-4.0.txt`, and `LICENSES/MIT.txt` are present.
- [x] Every file has inline SPDX metadata where supported or an applicable REUSE annotation.
- [x] `REUSE.toml` covers the header-less files.
- [x] The vendored `.proto` files keep their own MIT SPDX header.
- [x] `NOTICE` records attribution for `pyquadcortex` and `deskop-nano-cortex`.
- [x] `THIRD-PARTY-NOTICES.md` matches the current AGENTS/prior-art licensing table and distinguishes each project's actual use and licensing posture.
- [x] `reuse lint` passes in CI.
- [x] Trademark/unaffiliation notice in `README.md`, `AGENTS.md`, `NOTICE`.
- [x] Reverse-engineering legal basis cited in `README.md` and `NOTICE`.
- [x] No content from the reference-only repositories without a clear repository-wide licence is committed into this repo's tree.

## Non-Goals

- **Changing the license.** AGPL is a deliberate choice (AGENTS.md). Changing it requires approval.
- **Dual licensing infrastructure.** Available on request, but no dual-license boilerplate is in the repo yet.
- **A contributor licence agreement (CLA).** Not in scope; the AGPL header on every file is the inbound-outbound grant.

## Dependencies

- **`REUSE.toml`** - the FSFE REUSE bulk-annotation format.
- **`reuse` tool** - the FSFE REUSE linter (CI in zone 600, local in zone 500).
- **`LICENSES/`** - the SPDX license texts.
- **`AGENTS.md` prior-art table** - the source of truth for what may be taken from where.
- **house-style licensing.md** - the licensing conventions.

## Future

- **Dual licensing.** If a closed derivative needs to exist, a dual-license boilerplate and a `DUAL-LICENSE.md` may be added. Requires approval (AGENTS.md).
- **`qc-stomp-tools` incorporation.** It is studied but not incorporated. If on-device builds ever become justified, its MIT code may be adapted with upstream copyright and an approved notice update.
- **Human-rights clause.** The `README.md` licensing section notes "The project's own work is not to be used in weaponry, immigration enforcement, or other activities which infringe human rights." This is a statement of intent, not a license term; the AGPL is the enforceable grant.

## Glossary

| Term | Definition |
| --- | --- |
| AGPL-3.0-or-later | GNU Affero General Public License v3.0 or later; the code license for this project |
| CC-BY-SA-4.0 | Creative Commons Attribution-ShareAlike 4.0 International; the written-content license |
| SPDX header | `SPDX-FileCopyrightText` and `SPDX-License-Identifier` lines on a file, machine-readable by the REUSE tool |
| `REUSE.toml` | FSFE REUSE bulk-annotation file for files that cannot carry an inline SPDX header |
| `NOTICE` | The project copyright + attribution summary (SPDX-style) |
| `THIRD-PARTY-NOTICES.md` | Full attribution and license texts for all incorporated and reference-only prior art |
| Prior-art table | The table in `AGENTS.md` listing each vendored reference repo, its license, and what may be taken from it |
| Unaffiliation | The project is not affiliated with or endorsed by Neural DSP Technologies |
| Reverse engineering for interoperability | The legal basis (UK CDPA s50B / s296A, EU Software Directive Art 6) for reverse-engineering the Cortex Control protocol |
| Reference-only | Studied for understanding but not incorporated; no code/scripts/prose committed into this repo |

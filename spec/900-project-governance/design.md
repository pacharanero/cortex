---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["governance", "license", "agpl", "spdx", "reuse", "notice", "trademark"]
spec: spec.md
---

# 900 Project Governance - Design

> Design for the licensing, attribution, and legal-hygiene posture. The interesting parts are the AGPL choice (and why), the prior-art attribution discipline (what may be taken from which repo), and the REUSE compliance setup (SPDX headers + `REUSE.toml` for header-less files).

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [AGENTS.md](../../AGENTS.md) - the project license decision and the prior-art licensing table.
- [house-style licensing.md](https://github.com/marcus-pacharanero/house-style/blob/main/licensing.md) - the licensing conventions.
- [600-ci-release design](../600-ci-release/design.md) [DES-CI] - the CI job that enforces `reuse lint`.
- Owned source: `AGENTS.md`, `LICENSE`, `LICENSES/`, `NOTICE`, `THIRD-PARTY-NOTICES.md`, `REUSE.toml`, SPDX headers.

## [DES-LICENSE] The AGPL choice

### Behaviour

The project is **AGPL-3.0-or-later** for code, **CC-BY-SA-4.0** for written content. The full AGPL text is in `LICENSE`; the SPDX license text is in `LICENSES/AGPL-3.0-or-later.txt`. Every source file carries the AGPL SPDX license identifier in its header.

### Design choice: AGPL, not MIT/Apache

AGPL is a deliberate choice (AGENTS.md): the toolkit is not available for subsumption into proprietary products. A proprietary vendor (or anyone else) who wants to embed `cortex-rs` in a closed derivative must request a dual license. MIT or Apache would permit that without asking; AGPL does not. This protects the project's interoperability goal: the toolkit stays open.

The MIT- and Apache-2.0-licensed prior art we port in remains under its own terms. The vendored `.proto` files keep their MIT SPDX header (copyright (c) 2026 Stokes). The attribution is recorded in `NOTICE` and `THIRD-PARTY-NOTICES.md`; the project's own code is AGPL on top.

### Design choice: CC-BY-SA-4.0 for written content

Markdown, docs, and specs are CC-BY-SA-4.0 (share-alike). This keeps the written content open and prevents a proprietary fork of the docs without the corresponding source. The `REUSE.toml` annotations for `**/*.md` carry the AGPL license identifier (because `REUSE.toml` is one license per file, and AGPL is a superset of the attribution/share-alike intent); the `NOTICE` and `README.md` state the CC-BY-SA-4.0 intent for prose.

### Design choice: "or later"

`AGPL-3.0-or-later` allows the project to move to a future AGPL version (v4+) if one is released, without re-licensing every file. This is the FSF-recommended form.

### Alternatives considered

- **MIT or Apache-2.0.** Rejected: permits proprietary subsumption without asking. The interoperability goal needs the toolkit to stay open.
- **GPL-3.0 (not AGPL).** Rejected: network use without distribution does not trigger the GPL source-share obligation. The AGPL closes the "ASP loophole" - a service that uses `cortex-rs` over the network must share its source.
- **A custom license.** Rejected: standard licenses are reviewable and trusted; a custom license is a red flag for downstream consumers.

## [DES-PRIOR-ART] Prior-art attribution

### Behaviour

The prior-art licensing table in `AGENTS.md` is the source of truth for what may be taken from which repo. It is mirrored in `THIRD-PARTY-NOTICES.md` with full license texts.

| Repo | License | Use |
| --- | --- | --- |
| `pyquadcortex` (stokes-audio) | MIT | **Port freely with attribution.** Recovered `.proto` vendored; framing, write-STALL, trailer envelope derived. |
| `deskop-nano-cortex` (rixrix) | Apache-2.0 | **Adapt with attribution.** Architectural precedent for the Tauri app. No code copied yet. |
| `qc-stomp-tools` (VanIseghemThomas) | MIT | Adapt with attribution. On-device ioctls; relevant only if we target on-device builds. |
| `OpenCortex` (VanIseghemThomas) | None declared | **Reference only.** Do not copy. |
| `qc-extras` (roelj) | None declared | **Reference only.** Do not copy. |
| `quad-cortex-usb-re-notes` (hsaastamoinen) | None declared | **Reference only.** Do not copy. |
| `toneparse` (vian21) | None declared | **Reference only.** Do not copy. |

### Design choice: MIT/Apache compatible with AGPL

The MIT and Apache-2.0 licenses are compatible with AGPL-3.0-or-later. Material ported from `pyquadcortex` (MIT), adapted from `deskop-nano-cortex` (Apache-2.0), or adapted from `qc-stomp-tools` (MIT) can be incorporated into an AGPL project, provided the upstream copyright and a NOTICE entry are retained. The vendored `.proto` files keep their own MIT SPDX header; the project's own code is AGPL on top.

### Design choice: reference-only repos are never committed

The four unlicensed repos (`OpenCortex`, `qc-extras`, `quad-cortex-usb-re-notes`, `toneparse`) have no license file (GitHub reports `license: null`), so all rights are reserved. No code, scripts, or prose from them is committed into this repo's tree. Findings are cited in our own words in `quad-cortex-linux-editor-and-protocol.md` (at the parent workspace root) and link out. This is the AGENTS.md rule: "Do not commit vendored reference-repo content into this repo's tree."

### Design choice: `NOTICE` is the summary; `THIRD-PARTY-NOTICES.md` is the full text

`NOTICE` is the SPDX-style summary: project copyright, license decision, and a one-line attribution per incorporated project. `THIRD-PARTY-NOTICES.md` is the full text: project description, repository link, license, copyright, use-in-cortex-rs, and the license text (for MIT) or a link (for Apache-2.0). This split follows the standard open-source attribution convention.

### Alternatives considered

- **Relicense the vendored .proto as AGPL.** Rejected: the MIT license's distribution terms require retaining the upstream copyright and MIT notice. Relicensing would violate those terms. The .proto files keep their MIT header.
- **Copy reference-only repo content with a citation.** Rejected: no license means no permission. Citation is not a license.
- **A single `NOTICE` file with everything.** Rejected: `THIRD-PARTY-NOTICES.md` is the conventional place for full license texts; `NOTICE` is the summary. Splitting them keeps `NOTICE` scannable.

## [DES-REUSE] REUSE compliance

### Behaviour

Every source file carries an SPDX header. `REUSE.toml` covers the header-less files. `reuse lint` passes in CI and locally.

### Design choice: SPDX header on every source file

Every `.rs`, `.toml`, `.yml`, `.sh` file starts with:

```rust
// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later
```

(or the language's comment prefix). This is machine-readable by the REUSE tool and human-readable. The copyright holder is "2026 Dr Marcus Baw"; a company name may be added later (AGENTS.md: "Confirm the copyright holder with Marcus before seeding headers").

### Design choice: `REUSE.toml` for header-less files

Files that cannot carry an inline header (`**/*.md`, `**/*.txt`, `**/*.json`, `**/Cargo.lock`, `LICENSE`, `.gitignore`, `.editorconfig`, etc.) are covered by `REUSE.toml`:

```toml
[[annotations]]
path = [
    "**/*.md",
    "**/*.txt",
    "**/*.json",
    "**/Cargo.lock",
    "LICENSE",
    "**/LICENSE",
    ".dockerignore",
    ".gitignore",
    "**/.gitignore",
    "**/.gitkeep",
    ".editorconfig",
]
precedence = "aggregate"
SPDX-FileCopyrightText = "2026 Dr Marcus Baw"
SPDX-License-Identifier = "AGPL-3.0-or-later"
```

`precedence = "aggregate"` means the bulk annotation is combined with any inline header (the vendored `.proto` files keep their own MIT header; the `REUSE.toml` annotation does not override it).

### Design choice: vendored `.proto` keeps its own MIT header

The recovered `.proto` files in `crates/cortex-rs/proto/` carry their own MIT SPDX header (`SPDX-FileCopyrightText: 2026 Stokes` / `SPDX-License-Identifier: MIT`) above the recovery note. This satisfies the MIT license's retention requirement. The `REUSE.toml` `aggregate` precedence does not override it.

### Alternatives considered

- **Inline headers on markdown.** Rejected: markdown has no standard comment syntax; an SPDX header in prose is noisy. `REUSE.toml` is the REUSE-recommended approach.
- **A single `LICENSE` file, no SPDX headers.** Rejected: the REUSE standard requires per-file attribution; `reuse lint` enforces it.
- **Override the vendored .proto header with AGPL.** Rejected: violates the MIT retention requirement.

## [DES-TRADEMARK] Trademark and unaffiliation

### Behaviour

The trademark and unaffiliation notice appears in `README.md`, `AGENTS.md`, and `NOTICE`. It states: "Neural DSP", "Quad Cortex", "Nano Cortex", and "Cortex Control" are trademarks of Neural DSP Technologies; the project is unofficial and unaffiliated.

### Design choice: consistent notice in three places

`README.md` (the first thing a user reads), `AGENTS.md` (the first thing an agent reads), and `NOTICE` (the SPDX-style attribution) all carry the notice. This makes the unaffiliation clear regardless of entry point.

### Design choice: cite the reverse-engineering legal basis

`README.md` and `NOTICE` cite the legal basis: UK CDPA s50B / s296A and EU Software Directive Article 6. This is the established case for reverse engineering for interoperability. The notice also states the practical norms: no redistribution of binaries/firmware/artwork, no raw captures with readable strings, schema limited to interoperability needs.

### Alternatives considered

- **A single trademark file.** Rejected: the notice must be visible where the user/agent lands, not in a file they might not open.
- **No legal-basis citation.** Rejected: the citation is the answer to "is this legal?" The norms are the answer to "what do you not do?" Both belong.

## [DES-LIMITS] Known Limitations

- **Copyright holder is an individual, not a company.** The SPDX header says "2026 Dr Marcus Baw". A company name may be added later (AGENTS.md); the change touches every file's header and `REUSE.toml`.
- **No dual-license boilerplate.** Available on request; no `DUAL-LICENSE.md` yet.
- **No CLA.** Not in scope; the AGPL header on every file is the inbound-outbound grant.
- **`THIRD-PARTY-NOTICES.md` does not carry the Apache-2.0 full text inline.** It links to the Apache license URL instead, following the standard convention (the full text is large). The MIT text is inlined because it is short.
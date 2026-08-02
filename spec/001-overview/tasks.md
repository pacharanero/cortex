---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["overview", "traceability"]
spec: spec.md
design: design.md
---

# cortex-rs - Overview Tasks

> Spec-tree establishment and cross-cutting tasks. Per-layer implementation tasks live in each zone's `tasks.md`.

## Phase 1 - Spec tree

### 1.1 Establish taxonomy and routing index

- [x] Define numbering ranges, traceability contract, and the authoritative routing index.
- [x] Create zone folders for all planned surfaces.

### 1.2 Write zone specs

- [x] 001-overview: full spec + design
- [x] 100-transport: spec + design
- [ ] 110-framing: spec + design
- [ ] 120-proto-schema: spec + design
- [ ] 130-domain-model: spec + design
- [ ] 140-session: spec + design (planned)
- [ ] 150-client: spec + design (planned)
- [ ] 200-cli: spec + design
- [ ] 300-mcp: spec + design (stub)
- [ ] 400-gui: spec + design (deferred)
- [ ] 500-dx-tooling: spec + design
- [ ] 600-ci-release: spec + design
- [ ] 900-project-governance: spec + design

### 1.3 Write roadmap

- [ ] roadmap.md with stable IDs (PROT-xxx, CLI-xxx, MCP-xxx, GUI-xxx)

## Phase 2 - Traceability

### 2.1 Add @see headers

- [ ] Add `@see` traceability headers to all owned source files.

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1, 1.2 | Created spec tree | spec/** | [x] | [x] |
// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `cortex-mcp` MCP server: an agentic surface over the Quad Cortex (and
//! Nano Cortex) for patch editing.
//!
//! **Greenfield.** No MCP server for any Neural DSP hardware exists. The
//! design that matters is the **safety surface**, not the tool list:
//!
//! - Read and recall are free; **saving is always explicitly confirmed**.
//! - Never write to the factory setlist; restrict saves to a designated
//!   scratch range of USER slots unless overridden.
//! - Back up the target slot (`read_preset`) before overwriting, and keep
//!   the blob.
//! - Surface the row-numbering trap (zero-based in the API, 1-4 on screen; a
//!   wrong-row edit succeeds silently) in tool descriptions.
//! - Single owning process for the USB interface.
//!
//! See AGENTS.md -> MCP safety surface for the full design. This file is the
//! scaffold; the tool surface is wired up as the crate matures.
//!
//! @see spec/300-mcp/spec.md
//! @see spec/300-mcp/design.md

fn main() -> anyhow::Result<()> {
    eprintln!("cortex-mcp: MCP server not yet implemented (scaffold)");
    eprintln!("cortex-mcp: see AGENTS.md -> MCP safety surface for the design");
    Ok(())
}

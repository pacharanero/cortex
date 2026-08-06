// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `cortex-mcp` MCP server: an agentic surface over the Quad Cortex for
//! patch editing. Nano Cortex support is planned after its transport is
//! established on hardware.
//!
//! The first milestone deliberately exposes no persistent save or delete
//! operation. It reuses `cortex session`, which remains the sole HID owner.
//!
//! @see spec/300-mcp/spec.md
//! @see spec/300-mcp/design.md

mod server;
mod transport;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    server::serve().await
}

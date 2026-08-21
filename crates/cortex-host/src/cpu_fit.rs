// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Read-only interpretation of the Quad Cortex CPU-load push.
//!
//! @see spec/roadmap.md [MCP-003.2]

use cortex_rs::view::{CpuLoad, Preset};

/// A grid cell with a device-reported share of DSP load.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CpuFitCell {
    /// Zero-based grid row.
    pub row: usize,
    /// Zero-based grid column.
    pub column: usize,
    /// The model in this cell when the live grid contains one.
    pub model: Option<String>,
    /// The device-reported CPU share for this cell.
    pub load: f32,
    /// The device says this cell runs on the second DSP core.
    pub on_core2: bool,
}

/// Aggregated load for one device-reported DSP core.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CpuFitCore {
    /// The human-facing core number: one or two.
    pub core: u8,
    /// Sum of the device-reported cell shares on this core.
    pub load: f32,
}

/// A conservative explanation of the latest CPU push and live grid.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CpuFitAnalysis {
    /// Total device CPU load, when supplied by the device.
    pub total: Option<f32>,
    /// Device-reported load aggregated by core.
    pub cores: Vec<CpuFitCore>,
    /// Loaded cells, sorted from highest to lowest reported share.
    pub cells: Vec<CpuFitCell>,
    /// Guidance that does not infer an undocumented row-to-core mapping.
    pub advice: Vec<String>,
}

impl CpuFitAnalysis {
    /// Interpret a CPU push against the current live grid.
    #[must_use]
    pub fn from_live(cpu: &CpuLoad, preset: &Preset) -> Self {
        let mut core_loads = [0.0_f32; 2];
        let mut cells = Vec::new();
        for (row, chain) in cpu.chains.iter().enumerate() {
            for (column, reported) in chain.iter().enumerate() {
                let core = usize::from(reported.on_core2);
                core_loads[core] += reported.load;
                let model = preset
                    .blocks
                    .iter()
                    .find(|block| block.row == row && block.column == column)
                    .and_then(|block| block.name.clone());
                cells.push(CpuFitCell {
                    row,
                    column,
                    model,
                    load: reported.load,
                    on_core2: reported.on_core2,
                });
            }
        }
        cells.sort_by(|left, right| right.load.total_cmp(&left.load));

        let cores = vec![
            CpuFitCore {
                core: 1,
                load: core_loads[0],
            },
            CpuFitCore {
                core: 2,
                load: core_loads[1],
            },
        ];
        let mut advice = vec![
            "The Quad reports two DSP cores. Grid rows are signal-chain lanes, not fixed core assignments; use each cell's on_core2 flag as the device-reported mapping.".to_string(),
            "A cross-row move changes routing. Only rows 0 and 2 can branch, and the device may adjust split/rejoin points; read back the grid and audition the result after every move.".to_string(),
        ];
        if (core_loads[0] - core_loads[1]).abs() > f32::EPSILON {
            let (busy, spare) = if core_loads[0] > core_loads[1] {
                (1, 2)
            } else {
                (2, 1)
            };
            advice.push(format!(
                "Core {busy} has the higher reported load than core {spare}. If a new block is refused, a cross-row parallel route may help, but the device does not document a stable row-to-core allocation. Make one change, read back the grid and CPU mapping, audition it, then retry placement with verification."
            ));
        }
        Self {
            total: cpu.total,
            cores,
            cells,
            advice,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_device_core_flags_without_equating_rows_to_cores() {
        let cpu = CpuLoad {
            total: Some(57.0),
            chains: vec![
                vec![
                    cortex_rs::view::CpuColumn {
                        load: 20.0,
                        on_core2: false,
                    },
                    cortex_rs::view::CpuColumn {
                        load: 10.0,
                        on_core2: true,
                    },
                ],
                vec![cortex_rs::view::CpuColumn {
                    load: 27.0,
                    on_core2: true,
                }],
            ],
        };
        let preset = Preset {
            slot: "(live grid)".to_string(),
            setlist: "(live grid)".to_string(),
            name: String::new(),
            chains: 4,
            rows: Vec::new(),
            scenes: Vec::new(),
            blocks: Vec::new(),
        };

        let analysis = CpuFitAnalysis::from_live(&cpu, &preset);

        assert_eq!(
            analysis.cores[0],
            CpuFitCore {
                core: 1,
                load: 20.0
            }
        );
        assert_eq!(
            analysis.cores[1],
            CpuFitCore {
                core: 2,
                load: 37.0
            }
        );
        assert!(analysis.cells[0].on_core2);
        assert!(
            analysis
                .advice
                .iter()
                .any(|advice| advice.contains("not fixed core assignments"))
        );
    }
}

//! The design document's walkthrough boards, as data — same role as the TS
//! `boards.ts`. The tests assert the exact figures the doc quotes, so a port
//! that drifts from the reference fails loudly.

use crate::defs::Dir::{E, N, S, W};
use crate::defs::MachineId as M;
use crate::sim::{FilterCfg, Placement};

fn p(x: i32, y: i32, m: M, d: crate::defs::Dir) -> Placement {
    Placement::new(x, y, m, Some(d))
}
fn fixed(x: i32, y: i32, m: M) -> Placement {
    Placement::new(x, y, m, None)
}

/// Act I, Round 1 — one drill, one furnace, one vault. Quota 20.
pub fn round_1() -> (i32, i32, Vec<Placement>) {
    (8, 5, vec![
        p(0, 2, M::Drill, E),
        p(1, 2, M::Belt, E), p(2, 2, M::Belt, E),
        p(3, 2, M::Furnace, E),
        p(4, 2, M::Belt, E), p(5, 2, M::Belt, E), p(6, 2, M::Belt, E),
        fixed(7, 2, M::Vault),
    ])
}

/// Act I, Round 4 (Efficiency Audit) — two lanes, Heat Sink, Overclocker on
/// the Fabricator (the bottleneck), Polisher before the vault. Quota 200.
pub fn round_4() -> (i32, i32, Vec<Placement>) {
    (8, 5, vec![
        p(0, 1, M::Drill, E), p(1, 1, M::Belt, E), p(2, 1, M::Furnace, E),
        p(3, 1, M::Belt, E), p(4, 1, M::Belt, S),
        fixed(2, 2, M::Heatsink),
        p(0, 3, M::Drill, E), p(1, 3, M::Belt, E), p(2, 3, M::Furnace, E),
        p(3, 3, M::Belt, E), p(4, 3, M::Belt, N),
        p(4, 2, M::Merger, E),
        fixed(5, 1, M::Overclock),
        p(5, 2, M::Fab, E),
        p(6, 2, M::Polisher, E),
        fixed(7, 2, M::Vault),
    ])
}

/// A closed 8-tile ring, fed from outside, with no exit: exists to prove the
/// loop ROTATES rather than deadlocks.
pub fn loop_rig() -> (i32, i32, Vec<Placement>) {
    (5, 4, vec![
        p(0, 1, M::Drill, E),
        p(1, 1, M::Belt, E), p(2, 1, M::Belt, E),
        p(3, 1, M::Polisher, S),
        p(3, 2, M::Belt, W), p(2, 2, M::Polisher, W), p(1, 2, M::Belt, N),
    ])
}

/// The gated polish loop: the build the whole design rests on. Items enter at
/// quality 0, gain +2 per lap, eject at the `quality >= 5` gate on lap three.
pub fn gated_loop() -> (i32, i32, Vec<Placement>) {
    let mut cells = vec![
        p(0, 1, M::Drill, E),
        p(1, 1, M::Belt, E),
        // the ring
        p(2, 1, M::Belt, E), p(3, 1, M::Polisher, E), p(4, 1, M::Belt, S),
        p(4, 2, M::Belt, S), p(4, 3, M::Belt, W), p(3, 3, M::Polisher, W),
        p(2, 3, M::Belt, N),
        // the exit
        p(1, 2, M::Belt, W),
        fixed(0, 2, M::Vault),
    ];
    let mut filter = Placement::new(2, 2, M::Filter, Some(N));
    filter.d2 = Some(W);
    filter.cfg = Some(FilterCfg { min_quality: Some(5), item_type: None });
    cells.push(filter);
    (5, 4, cells)
}

/// A Splitter feeding two vaults, for checking round-robin fairness.
pub fn split_rig() -> (i32, i32, Vec<Placement>) {
    let mut splitter = Placement::new(1, 1, M::Splitter, Some(E));
    splitter.d2 = Some(N);
    (4, 3, vec![
        p(0, 1, M::Drill, E),
        splitter,
        fixed(2, 1, M::Vault),
        fixed(1, 0, M::Vault),
    ])
}

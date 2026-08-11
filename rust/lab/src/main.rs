//! overflow-lab — headless playtesting harness.
//!
//! This is the groundwork for the balance workflow the game wants: bots play
//! thousands of complete seeded runs, and the outcome distribution (how many
//! rounds runs survive, where they die, what payouts look like) becomes the
//! data you tune quotas and card pools against. Target a run-length
//! distribution, change a number in `defs.rs`, re-run, diff.
//!
//!   cargo run --release -p overflow-lab -- --runs 2000 --seed 1
//!   cargo run --release -p overflow-lab -- --runs 2000 --jsonl /tmp/runs.jsonl
//!   cargo run --release -p overflow-lab -- --bench
//!
//! The only bot so far is LaneBot, a deliberately naive baseline: it builds
//! parallel drill→furnace lanes and slots Polishers in front of the vault.
//! It knows nothing about loops, filters, or assemblers — so the round where
//! LaneBot dies is the round the design *forces* players off pure widening.
//! Smarter bots raise that ceiling; the gap between bots measures how much
//! depth the mechanics actually buy.

use overflow_core::cards::Offer;
use overflow_core::defs::{Dir, DirectiveId, MachineId};
use overflow_core::run::{Game, GamePhase, BOARD_H, BOARD_W, QUOTAS};
use overflow_core::sim::{Placement, Sim};
use std::time::Instant;

// ── LaneBot ──────────────────────────────────────────────────────────────────

/// Lane rows in build order: centre first (shortest transit), spreading out.
const LANES: [i32; 7] = [3, 2, 4, 1, 5, 0, 6];

struct LaneBot {
    started: Vec<i32>,
}

impl LaneBot {
    fn new() -> Self {
        Self { started: Vec::new() }
    }

    /// Belt path for a lane: east along the row, then down/up column 8 to the
    /// vault row, sharing the spine with other lanes.
    fn belt_plan(y: i32) -> Vec<(i32, i32, Dir)> {
        let mut plan: Vec<(i32, i32, Dir)> = Vec::new();
        for x in [1, 2, 3] {
            plan.push((x, y, Dir::E));
        }
        for x in [5, 6, 7] {
            plan.push((x, y, Dir::E));
        }
        if y == 3 {
            plan.push((8, 3, Dir::E));
        } else {
            let toward = if y < 3 { Dir::S } else { Dir::N };
            plan.push((8, y, toward));
            let range: Vec<i32> = if y < 3 { (y + 1..3).collect() } else { (4..y).rev().collect() };
            for yy in range {
                plan.push((8, yy, toward));
            }
            plan.push((8, 3, Dir::E));
        }
        plan
    }

    fn occupied(g: &Game, x: i32, y: i32) -> bool {
        g.board.iter().any(|p| p.x == x && p.y == y)
    }

    fn build(&mut self, g: &mut Game) {
        // Slot Polishers inline on the vault approach whenever we're dealt one.
        while let Some(i) = g.hand.iter().position(|c| c.machine == MachineId::Polisher) {
            let spot = [(7, 3), (6, 3), (8, 2), (8, 4)]
                .into_iter()
                .find(|&(x, y)| !Self::occupied(g, x, y));
            let Some((x, y)) = spot else { break };
            let d = if y == 3 { Dir::E } else if y < 3 { Dir::S } else { Dir::N };
            if g.play_card(i, x, y, Some(d), None, None).is_err() {
                break;
            }
        }

        // Heat Sinks go beside an existing Furnace.
        while let Some(i) = g.hand.iter().position(|c| c.machine == MachineId::Heatsink) {
            let furnaces: Vec<(i32, i32)> = g
                .board
                .iter()
                .filter(|p| p.m == MachineId::Furnace)
                .map(|p| (p.x, p.y))
                .collect();
            let spot = furnaces.iter().find_map(|&(fx, fy)| {
                [(fx, fy + 1), (fx, fy - 1)]
                    .into_iter()
                    .find(|&(x, y)| x >= 0 && y >= 0 && x < BOARD_W && y < BOARD_H && !Self::occupied(g, x, y))
            });
            let Some((x, y)) = spot else { break };
            if g.play_card(i, x, y, None, None, None).is_err() {
                break;
            }
        }

        // Open new lanes while we hold a Drill and a Furnace for one.
        while let Some(&lane) = LANES.iter().find(|l| !self.started.contains(l)) {
            let Some(di) = g.hand.iter().position(|c| c.machine == MachineId::Drill) else { break };
            if g.play_card(di, 0, lane, Some(Dir::E), None, None).is_err() {
                break;
            }
            self.started.push(lane);
            if let Some(fi) = g.hand.iter().position(|c| c.machine == MachineId::Furnace) {
                let _ = g.play_card(fi, 4, lane, Some(Dir::E), None, None);
            }
            for (x, y, d) in Self::belt_plan(lane) {
                if !Self::occupied(g, x, y) {
                    let _ = g.buy_belt(x, y, d);
                }
            }
        }
    }

    /// Shop strategy: lane-relevant directives first (they compound), then
    /// preferred machines, rerolling a few times while flush to fish.
    fn shop(&self, g: &mut Game) {
        const PREF: [MachineId; 5] = [
            MachineId::Polisher, MachineId::Heatsink, MachineId::Drill,
            MachineId::Furnace, MachineId::Overclock,
        ];
        let mut rerolls = 0;
        loop {
            let mut bought = false;
            while let Some(i) = g.offers.iter().position(|o| {
                matches!(
                    *o,
                    Offer::Directive(DirectiveId::Flywheel | DirectiveId::Superheater)
                )
            }) {
                if g.shop_buy(i).is_err() {
                    break;
                }
                bought = true;
            }
            for want in PREF {
                while let Some(i) = g
                    .offers
                    .iter()
                    .position(|o| matches!(*o, Offer::Machine(c) if c.machine == want))
                {
                    if g.shop_buy(i).is_err() {
                        break;
                    }
                    bought = true;
                }
            }
            if !bought {
                if rerolls < 4 && g.credits > g.reroll_price() * 4 && g.shop_reroll().is_ok() {
                    rerolls += 1;
                    continue;
                }
                break;
            }
        }
        g.shop_done().unwrap();
    }
}

// ── batch runner ─────────────────────────────────────────────────────────────

struct RunRecord {
    seed: u32,
    rounds_cleared: usize,
    total_delivered: i64,
    machines: usize,
}

fn play_one(seed: u32) -> RunRecord {
    let mut g = Game::new(seed);
    let mut bot = LaneBot::new();
    loop {
        match g.phase {
            GamePhase::Build => {
                bot.build(&mut g);
                g.run_shift().expect("bot placed something illegal");
            }
            GamePhase::Shop => {
                bot.shop(&mut g);
            }
            GamePhase::Over { .. } => break,
        }
    }
    RunRecord {
        seed,
        rounds_cleared: g.history.iter().filter(|o| o.cleared).count(),
        total_delivered: g.history.iter().map(|o| o.result.payout).sum(),
        machines: g.board.len(),
    }
}

fn batch(runs: u32, seed0: u32, jsonl: Option<&str>) {
    let t0 = Instant::now();
    let records: Vec<RunRecord> = (0..runs).map(|i| play_one(seed0.wrapping_add(i))).collect();
    let dt = t0.elapsed();

    if let Some(path) = jsonl {
        let mut out = String::new();
        for r in &records {
            out.push_str(&format!(
                "{{\"seed\":{},\"rounds_cleared\":{},\"total_delivered\":{},\"machines\":{}}}\n",
                r.seed, r.rounds_cleared, r.total_delivered, r.machines
            ));
        }
        std::fs::write(path, out).expect("write jsonl");
        eprintln!("wrote {} records to {path}", records.len());
    }

    // Survival histogram: how many runs died at each round.
    let mut died_at = [0u32; 13]; // index = rounds cleared; 12 = won
    for r in &records {
        died_at[r.rounds_cleared] += 1;
    }
    let mut sorted: Vec<usize> = records.iter().map(|r| r.rounds_cleared).collect();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let mean = sorted.iter().sum::<usize>() as f64 / sorted.len() as f64;

    println!("LaneBot × {runs} runs ({:.2}s, {:.1} runs/sec)", dt.as_secs_f64(), runs as f64 / dt.as_secs_f64());
    println!("rounds cleared: median {median}, mean {mean:.2}");
    println!();
    println!("  cleared │ runs   │ quota that killed them");
    println!("  ────────┼────────┼───────────────────────");
    for (i, &n) in died_at.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let bar = "█".repeat(((n as f64 / runs as f64) * 40.0).ceil() as usize);
        let quota = if i < 12 { QUOTAS[i].to_string() } else { "— (won)".into() };
        println!("  {i:>7} │ {n:>6} │ {quota:>8}  {bar}");
    }
}

// ── throughput bench ─────────────────────────────────────────────────────────

fn bench() {
    println!("packed drill→belt boards, 60 warm-up ticks then 200 timed:");
    for n in [40i32, 100, 200, 320, 500] {
        let mut cells: Vec<Placement> = Vec::new();
        for y in 0..n {
            cells.push(Placement::new(0, y, MachineId::Drill, Some(Dir::E)));
            for x in 1..n - 1 {
                cells.push(Placement::new(x, y, MachineId::Belt, Some(Dir::E)));
            }
            cells.push(Placement::new(n - 1, y, MachineId::Vault, None));
        }
        let mut s = Sim::new(n, n, &cells, 1).unwrap();
        for _ in 0..60 {
            s.step();
        }
        let t0 = Instant::now();
        const PASSES: u32 = 200;
        for _ in 0..PASSES {
            s.step();
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / PASSES as f64;
        println!(
            "  {:>7} tiles | {:>7} items in flight | {:>8.3} ms/tick | {:>10.0} ticks/sec",
            n * n,
            s.items_in_flight(),
            ms,
            1000.0 / ms
        );
    }
}

// ── entry ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));

    if args.iter().any(|a| a == "--bench") {
        bench();
        return;
    }
    let runs: u32 = get("--runs").map(|s| s.parse().expect("--runs N")).unwrap_or(1000);
    let seed: u32 = get("--seed").map(|s| s.parse().expect("--seed N")).unwrap_or(1);
    let jsonl = get("--jsonl").map(String::as_str);
    batch(runs, seed, jsonl);
}

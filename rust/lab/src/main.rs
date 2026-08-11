//! overflow-lab — headless playtesting harness.
//!
//! Bots play thousands of complete seeded runs; the outcome distribution and
//! the per-round payout/quota ratios are the balance instruments. Target a
//! distribution, change a number in `defs.rs`/`run.rs`, re-run, diff.
//!
//!   cargo run --release -p overflow-lab -- --runs 2000 --seed 1
//!   cargo run --release -p overflow-lab -- --bench
//!
//! DockBot is the naive baseline for the consignment model: it feeds bought
//! ore through splitter columns into a bank of furnaces and ships ingots.
//! It avoids dirty lots entirely (it owns no filters), ignores the market,
//! and never builds tier 2 — so wherever DockBot dies is the floor the
//! quota curve leans on.

use overflow_core::cards::Offer;
use overflow_core::defs::{ContractId, Dir, DirectiveId, ItemType, MachineId};
use overflow_core::run::{Game, GamePhase, BOARD_H, BOARD_W, QUOTAS};
use overflow_core::sim::{Placement, Sim};
use std::time::Instant;

// ── DockBot ──────────────────────────────────────────────────────────────────

const VY: i32 = BOARD_H / 2; // vault row
const SPINE: i32 = BOARD_W - 2;

/// One furnace lane: the splitter that feeds it, the furnace tile, its row.
struct Lane {
    row: i32,
    split_dir: Dir, // which way the distribution column continues
}

/// Lanes in build order: three off each bay's distribution column.
fn lanes() -> Vec<Lane> {
    vec![
        Lane { row: VY - 3, split_dir: Dir::S },
        Lane { row: VY - 2, split_dir: Dir::S },
        Lane { row: VY - 1, split_dir: Dir::S },
        Lane { row: VY + 3, split_dir: Dir::N },
        Lane { row: VY + 2, split_dir: Dir::N },
        Lane { row: VY + 1, split_dir: Dir::N },
    ]
}

struct DockBot {
    built: usize, // lanes opened so far
}

impl DockBot {
    fn new() -> Self {
        Self { built: 0 }
    }

    fn occupied(g: &Game, x: i32, y: i32) -> bool {
        g.board.iter().any(|p| Game::cells_of(p).contains(&(x, y)))
    }

    fn build(&mut self, g: &mut Game) {
        // The fab in the starting kit is dead weight for a widening bot.
        if let Some(i) = g.hand.iter().position(|c| c.machine == MachineId::Fab) {
            let _ = g.sell_blueprint(i);
        }

        // Trunk: bay feed belts and the overflow row straight to the vault
        // (unsmelted ore still pays pennies rather than clogging).
        let mut trunk: Vec<(i32, i32, Dir)> = vec![(1, VY - 3, Dir::E), (1, VY + 3, Dir::E)];
        for x in 3..SPINE {
            trunk.push((x, VY, Dir::E));
        }
        trunk.push((SPINE, VY, Dir::E));
        for (x, y, d) in trunk {
            if !Self::occupied(g, x, y) {
                let _ = g.buy_belt(x, y, d);
            }
        }

        // Open lanes while we hold furnaces: splitter on the column, furnace
        // beside it, output row east, spine to the vault row.
        while self.built < lanes().len() {
            let Some(fi) = g.hand.iter().position(|c| c.machine == MachineId::Furnace) else {
                break;
            };
            let lane = &lanes()[self.built];
            if g.play_card(fi, 3, lane.row, Some(Dir::E), None, None).is_err() {
                break;
            }
            self.built += 1;
        }

        // (Re)lay lane plumbing for every opened lane — idempotent, so runs
        // broken by a thin wallet complete as soon as credits allow.
        for lane in lanes().iter().take(self.built) {
            if !Self::occupied(g, 2, lane.row) && g.buy_splitter(2, lane.row, lane.split_dir).is_ok() {
                // point the eject edge east into the furnace (buy_splitter
                // defaults d2 to clockwise-of-d, which is west for a
                // south-running column)
                while g
                    .board
                    .iter()
                    .find(|p| p.x == 2 && p.y == lane.row)
                    .and_then(|p| p.d2)
                    != Some(Dir::E)
                {
                    g.rotate_d2(2, lane.row).unwrap();
                }
            }
            for x in 5..SPINE {
                if !Self::occupied(g, x, lane.row) {
                    let _ = g.buy_belt(x, lane.row, Dir::E);
                }
            }
            let toward = if lane.row < VY { Dir::S } else { Dir::N };
            if !Self::occupied(g, SPINE, lane.row) {
                let _ = g.buy_belt(SPINE, lane.row, toward);
            }
            let range: Vec<i32> = if lane.row < VY {
                (lane.row + 1..VY).collect()
            } else {
                (VY + 1..lane.row).rev().collect()
            };
            for yy in range {
                if !Self::occupied(g, SPINE, yy) {
                    let _ = g.buy_belt(SPINE, yy, toward);
                }
            }
            // splitter column terminals: dump leftovers onto the overflow row
            let term = if lane.row < VY { VY - 1 } else { VY + 1 };
            for yy in [term] {
                if !Self::occupied(g, 2, yy) {
                    let _ = g.buy_belt(2, yy, if lane.row < VY { Dir::S } else { Dir::N });
                }
            }
            if !Self::occupied(g, 2, VY) {
                let _ = g.buy_belt(2, VY, Dir::E);
            }
        }
    }

    /// Shop: fuel first (clean ore lots split across the bays), then more
    /// furnaces, then whatever compounds. Dirty lots are poison without
    /// filters; the bot knows its limits.
    /// The supply window: buy clean ore cards while flush, then allocate the
    /// whole supply hand across the bays, alternating.
    fn supply(&self, g: &mut Game) {
        let mut rerolls = 0;
        loop {
            let pick = g.lot_offers.iter().position(|l| {
                l.entries.iter().any(|e| e.0 == ItemType::Ore)
                    && !l.entries.iter().any(|e| e.0 == ItemType::Slag)
                    && g.credits >= l.price + 10
            });
            match pick {
                Some(i) => {
                    if g.buy_lot(i).is_err() {
                        break;
                    }
                }
                None => {
                    if rerolls < 2 && g.credits > g.reroll_price() * 6 && g.shop_reroll().is_ok() {
                        rerolls += 1;
                        continue;
                    }
                    break;
                }
            }
        }
        // allocate only PURE ORE cards (a furnace line jams on anything
        // else), alternating bays; discard the rest — the bot knows its
        // limits.
        let mut bay = 0;
        let i = 0;
        while i < g.supply_hand.len() {
            let pure_ore =
                g.supply_hand[i].runs.iter().all(|r| r.0 == overflow_core::defs::ItemType::Ore);
            if !pure_ore {
                g.supply_hand.remove(i);
                continue;
            }
            if g.allocate(i, bay).is_err() {
                bay = (bay + 1) % g.bay_slots.len();
                if g.allocate(i, bay).is_err() {
                    break; // both bays full
                }
            }
            bay = (bay + 1) % g.bay_slots.len();
        }
        g.supply_done().unwrap();
    }

    fn shop(&self, g: &mut Game) {
        // machines and compounding buys
        let mut rerolls = 0;
        loop {
            let mut bought = false;
            if self.built < lanes().len() || g.hand.iter().any(|c| c.machine == MachineId::Furnace)
            {
                while let Some(i) = g
                    .offers
                    .iter()
                    .position(|o| matches!(*o, Offer::Machine(c) if c.machine == MachineId::Furnace))
                {
                    if g.credits < g.offer_price(g.offers[i]) + 20 || g.shop_buy(i).is_err() {
                        break;
                    }
                    bought = true;
                }
            }
            while let Some(i) = g.offers.iter().position(|o| {
                matches!(*o, Offer::Directive(DirectiveId::Superheater | DirectiveId::Flywheel))
            }) {
                if g.credits < g.offer_price(g.offers[i]) + 30 || g.shop_buy(i).is_err() {
                    break;
                }
                bought = true;
            }
            // the contract shelf: input-granting deals compound for a hauler
            while let Some(i) = g.contract_offers.iter().position(|c| {
                matches!(c, ContractId::BulkManifests | ContractId::OreRetainer | ContractId::NightShifts)
            }) {
                if g.buy_contract(i).is_err() {
                    break;
                }
                bought = true;
            }
            if !bought {
                if rerolls < 3 && g.credits > g.reroll_price() * 5 && g.shop_reroll().is_ok() {
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
    /// Total payout of each ROUND attempted (summed over its shifts).
    round_payouts: Vec<i64>,
}

fn play_one(seed: u32) -> RunRecord {
    let mut g = Game::new(seed);
    let mut bot = DockBot::new();
    loop {
        match g.phase {
            GamePhase::Build => {
                bot.build(&mut g);
                g.run_shift().expect("bot made an illegal move");
            }
            GamePhase::Shop => {
                bot.shop(&mut g);
            }
            GamePhase::Supply => {
                bot.supply(&mut g);
            }
            GamePhase::Over { .. } => break,
        }
    }
    // aggregate per-shift history into per-round payouts
    let mut round_payouts: Vec<i64> = Vec::new();
    for o in &g.history {
        if round_payouts.len() <= o.round {
            round_payouts.resize(o.round + 1, 0);
        }
        round_payouts[o.round] += o.result.payout;
    }
    RunRecord {
        seed,
        rounds_cleared: g.history.iter().filter(|o| o.cleared).count(),
        total_delivered: g.history.iter().map(|o| o.result.payout).sum(),
        machines: g.board.len(),
        round_payouts,
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

    let mut died_at = [0u32; 13];
    for r in &records {
        died_at[r.rounds_cleared] += 1;
    }
    let mut sorted: Vec<usize> = records.iter().map(|r| r.rounds_cleared).collect();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let mean = sorted.iter().sum::<usize>() as f64 / sorted.len() as f64;

    println!(
        "DockBot × {runs} runs ({:.2}s, {:.1} runs/sec)",
        dt.as_secs_f64(),
        runs as f64 / dt.as_secs_f64()
    );
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

    // The overshoot instrument: round payout (all shifts) vs quota.
    println!();
    println!("  round │ reached │ mean payout │ quota │ ratio");
    println!("  ──────┼─────────┼─────────────┼───────┼──────");
    for (r, &quota) in QUOTAS.iter().enumerate() {
        let pays: Vec<i64> =
            records.iter().filter_map(|rec| rec.round_payouts.get(r).copied()).collect();
        if pays.is_empty() {
            break;
        }
        let mean_pay = pays.iter().sum::<i64>() as f64 / pays.len() as f64;
        println!(
            "  {:>5} │ {:>7} │ {:>11.0} │ {:>5} │ {:>5.2}",
            r + 1,
            pays.len(),
            mean_pay,
            quota,
            mean_pay / quota as f64
        );
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

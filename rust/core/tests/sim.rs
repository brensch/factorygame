//! The design doc's claims, as assertions, plus deck/run coverage. Ported
//! from the retired TS reference suite (git history: `sim/src/sim.test.ts`);
//! the 19 pinned numbers survived the crossing and now live only here.

use overflow_core::boards::*;
use overflow_core::cards::Offer;
use overflow_core::defs::{item_value, Dir, ItemType, MachineId, QUALITY_CAP};
use overflow_core::run::{GamePhase, QUOTAS, SHOP_SIZE};
use overflow_core::sim::{run_board, Placement, Sim};

const SEED: u32 = 0xc0ffee;

#[test]
fn quality_multiplies_base_value() {
    assert_eq!(item_value(ItemType::Ingot, 0), 4.0);
    assert_eq!(item_value(ItemType::Gear, 2), 24.0);
    assert_eq!(item_value(ItemType::Engine, 10), 224.0);
}

// ── Round 1 — the design doc's first board ──────────────────────────────────

#[test]
fn round_1_delivers_13_ingots_for_52_credits() {
    let (w, h, cells) = round_1();
    let r = run_board(w, h, &cells, 60, SEED).unwrap();
    assert_eq!(r.count(ItemType::Ingot), 13);
    assert_eq!(r.payout, 52);
}

#[test]
fn round_1_first_ingot_lands_at_tick_12() {
    let (w, h, cells) = round_1();
    let r = run_board(w, h, &cells, 60, SEED).unwrap();
    assert_eq!(r.delivered[0].tick, 12);
}

#[test]
fn round_1_two_items_stranded_and_zero_jams() {
    let (w, h, cells) = round_1();
    let r = run_board(w, h, &cells, 60, SEED).unwrap();
    assert_eq!(r.in_flight, 2);
    assert_eq!(r.jam_ticks, 0);
}

// ── Round 4 — Efficiency Audit ──────────────────────────────────────────────

#[test]
fn round_4_delivers_10_quality_2_gears_for_240() {
    let (w, h, cells) = round_4();
    let r = run_board(w, h, &cells, 60, SEED).unwrap();
    assert_eq!(r.count(ItemType::Gear), 10);
    assert_eq!(r.payout, 240);
    assert!(r.delivered.iter().all(|d| d.quality == 2));
}

// ── belt loops ──────────────────────────────────────────────────────────────

#[test]
fn full_closed_ring_rotates_instead_of_deadlocking() {
    let (w, h, cells) = loop_rig();
    let mut s = Sim::new(w, h, &cells, SEED).unwrap();
    for _ in 0..60 {
        s.step();
    }
    let ring = [(1, 1), (2, 1), (3, 1), (3, 2), (2, 2), (1, 2)];
    for (x, y) in ring {
        assert_eq!(s.peek(x, y).unwrap().quality, QUALITY_CAP);
    }
}

#[test]
fn quality_climbs_while_the_ring_turns() {
    let (w, h, cells) = loop_rig();
    let mut s = Sim::new(w, h, &cells, SEED).unwrap();
    for _ in 0..20 {
        s.step();
    }
    let early = s.peek(2, 1).map(|i| i.quality).unwrap_or(0);
    for _ in 0..15 {
        s.step();
    }
    let later = s.peek(2, 1).map(|i| i.quality).unwrap_or(0);
    assert!(later > early);
}

// ── Filter — the gate that makes loops useful ───────────────────────────────

#[test]
fn gated_loop_ejects_at_exactly_the_gate() {
    let (w, h, cells) = gated_loop();
    let r = run_board(w, h, &cells, 60, SEED).unwrap();
    assert_eq!(r.count(ItemType::Ore), 7);
    assert!(r.delivered.iter().all(|d| d.quality == 6));
    assert_eq!(r.payout, 18);
}

// ── Splitter ────────────────────────────────────────────────────────────────

#[test]
fn splitter_round_robins_fairly() {
    let (w, h, cells) = split_rig();
    let mut s = Sim::new(w, h, &cells, SEED).unwrap();
    let (mut east, mut north) = (0, 0);
    for _ in 0..60 {
        s.step();
        for m in &s.moves {
            if m.to == 6 {  // (2,1) on a w=4 board
                east += 1;
            }
            if m.to == 1 {
                north += 1;
            }
        }
    }
    assert_eq!(east, north);
    assert!(east + north > 10);
}

// ── invariants ──────────────────────────────────────────────────────────────

#[test]
fn deterministic_under_a_fixed_seed() {
    let (w, h, cells) = round_4();
    let a = run_board(w, h, &cells, 60, 1234).unwrap();
    let b = run_board(w, h, &cells, 60, 1234).unwrap();
    assert_eq!(a.payout, b.payout);
    assert_eq!(a.delivered.len(), b.delivered.len());
}

#[test]
fn order_independent_under_placement_shuffle() {
    let (w, h, cells) = round_4();
    let mut shuffled = cells.clone();
    // fixed permutation, so this test is itself deterministic
    for i in (1..shuffled.len()).rev() {
        let j = (i * 7 + 3) % (i + 1);
        shuffled.swap(i, j);
    }
    let base = run_board(w, h, &cells, 60, SEED).unwrap();
    let perm = run_board(w, h, &shuffled, 60, SEED).unwrap();
    assert_eq!(perm.payout, base.payout);
    assert_eq!(perm.count(ItemType::Gear), base.count(ItemType::Gear));
}

#[test]
fn heat_sink_only_tag_does_not_leak_onto_a_drill() {
    let cells = vec![
        Placement::new(0, 0, MachineId::Drill, Some(Dir::E)),
        Placement::new(1, 0, MachineId::Vault, None),
        Placement::new(0, 1, MachineId::Heatsink, None),
    ];
    let r = run_board(3, 3, &cells, 60, SEED).unwrap();
    assert!(r.delivered.iter().all(|d| d.quality == 0));
}

#[test]
fn rejects_two_machines_on_one_tile() {
    let cells = vec![
        Placement::new(1, 1, MachineId::Drill, Some(Dir::E)),
        Placement::new(1, 1, MachineId::Belt, Some(Dir::E)),
    ];
    assert!(Sim::new(3, 3, &cells, SEED).err().unwrap().contains("two machines"));
}

#[test]
fn rejects_out_of_bounds_placement() {
    let cells = vec![Placement::new(5, 0, MachineId::Drill, Some(Dir::E))];
    assert!(Sim::new(3, 3, &cells, SEED).err().unwrap().contains("out of bounds"));
}

#[test]
fn junction_crosses_two_lanes_without_mixing() {
    // Ore runs west→east, sap runs north→south, sharing one junction tile.
    let cells = vec![
        Placement::new(0, 2, MachineId::Drill, Some(Dir::E)),
        Placement::new(1, 2, MachineId::Belt, Some(Dir::E)),
        Placement::new(2, 2, MachineId::Junction, None),
        Placement::new(3, 2, MachineId::Belt, Some(Dir::E)),
        Placement::new(4, 2, MachineId::Vault, None),
        Placement::new(2, 0, MachineId::Tap, Some(Dir::S)),
        Placement::new(2, 1, MachineId::Belt, Some(Dir::S)),
        Placement::new(2, 3, MachineId::Belt, Some(Dir::S)),
        Placement::new(2, 4, MachineId::Vault, None),
    ];
    let r = run_board(5, 5, &cells, 60, 1).unwrap();
    assert!(r.count(ItemType::Ore) >= 12, "ore crossed: {}", r.count(ItemType::Ore));
    assert!(r.count(ItemType::Sap) >= 7, "sap crossed: {}", r.count(ItemType::Sap));
}

// ── run structure: the hand and the shop ─────────────────────────────────────

use overflow_core::defs::{ContractId, DirectiveId};
use overflow_core::rng::Rng;
use overflow_core::run::{roll_lot, Game, SHIFTS_PER_ROUND};
#[allow(unused_imports)]
use overflow_core::run::HAND_MAX;

/// The consignment starter build: a furnace beside each bay, a lane along
/// the bay's row, and a shared spine into the vault at (17,9).
fn build_starter(g: &mut Game) {
    for (row, toward) in [(6, Dir::S), (12, Dir::N)] {
        let f = g.hand.iter().position(|c| c.machine == MachineId::Furnace).unwrap();
        g.play_card(f, 1, row, Some(Dir::E), None, None).unwrap();
        for x in 2..=15 {
            g.buy_belt(x, row, Dir::E).unwrap();
        }
        g.buy_belt(16, row, toward).unwrap();
        let range: Vec<i32> = if row < 9 { (row + 1..9).collect() } else { (10..row).rev().collect() };
        for yy in range {
            g.buy_belt(16, yy, toward).unwrap();
        }
    }
    if !g.board.iter().any(|p| p.x == 16 && p.y == 9) {
        g.buy_belt(16, 9, Dir::E).unwrap();
    }
}

#[test]
fn a_full_scripted_round_1_processes_the_starter_consignment() {
    let mut g = Game::new(42);
    assert_eq!(g.credits, 75);
    assert_eq!(g.hand.len(), 4); // three furnaces and a fab
    assert_eq!(g.bays().len(), 2);
    let queued: u32 = g.bay_queues.iter().flatten().map(|e| e.1).sum();
    assert_eq!(queued, 120, "the starter ore consignment waits at the docks");

    build_starter(&mut g);

    // shifts sum toward the quota; the factory stays warm in between
    let mut shifts = 0;
    while g.phase == GamePhase::Build {
        g.run_shift().unwrap();
        shifts += 1;
        assert!(shifts <= SHIFTS_PER_ROUND);
    }
    assert_eq!(g.phase, GamePhase::Shop, "the starter build must clear round 1");
    assert!(shifts >= 2, "one shift should NOT clear round 1 (got {shifts})");

    // the shop: machines + a directive + a contract, and three shipments
    assert_eq!(g.offers.len(), SHOP_SIZE);
    assert!(g.offers.iter().any(|o| matches!(o, Offer::Directive(_))));
    assert!(g.offers.iter().any(|o| matches!(o, Offer::Contract(_))));
    assert_eq!(g.lot_offers.len(), 3);

    // buy a shipment to bay 0: queue grows, credits shrink
    let price = g.lot_offers[0].price;
    let before = g.credits;
    g.buy_lot(0, 0).unwrap();
    assert_eq!(g.credits, before - price);
    assert!(!g.bay_queues[0].is_empty());

    g.shop_done().unwrap();
    assert_eq!(g.round, 1);
    assert_eq!(g.phase, GamePhase::Build);
}

#[test]
fn an_idle_factory_burns_all_three_shifts_then_dies() {
    let mut g = Game::new(9);
    for _ in 0..SHIFTS_PER_ROUND - 1 {
        g.run_shift().unwrap();
        assert_eq!(g.phase, GamePhase::Build, "shifts remain");
    }
    g.run_shift().unwrap();
    assert_eq!(g.phase, GamePhase::Over { won: false });
    assert!(g.run_shift().is_err(), "no shifts left to run");
}

#[test]
fn retry_rewinds_the_whole_round() {
    let mut g = Game::new(9);
    let credits0 = g.credits;
    let queue0 = g.bay_queues.clone();
    g.buy_belt(5, 5, Dir::E).unwrap(); // spend something
    for _ in 0..SHIFTS_PER_ROUND {
        g.run_shift().unwrap();
    }
    assert_eq!(g.phase, GamePhase::Over { won: false });
    g.retry_round().unwrap();
    assert_eq!(g.phase, GamePhase::Build);
    assert_eq!(g.credits, credits0, "spent credits come back");
    assert_eq!(g.bay_queues, queue0, "queues rewound");
    assert!(!g.board.iter().any(|p| p.x == 5 && p.y == 5), "the belt is gone");
}

#[test]
fn the_factory_stays_warm_between_shifts() {
    let mut g = Game::new(42);
    build_starter(&mut g);
    g.run_shift().unwrap();
    assert!(!g.carry.is_empty(), "material still in the pipes after shift 1");
    let queued: u32 = g.bay_queues.iter().flatten().map(|e| e.1).sum();
    assert!(queued < 120, "the bays streamed some of their queues");
    assert!(queued > 0, "40 ticks cannot drain 120 items from two bays");
}

#[test]
fn bays_and_vault_are_bolted_down() {
    let mut g = Game::new(1);
    assert!(g.rotate(0, 6).is_err(), "bay");
    assert!(g.sell(0, 6).is_err(), "bay");
    assert!(g.move_by(&[(0, 6)], 1, 0).is_err(), "bay");
    assert!(g.sell(17, 9).is_err(), "vault");
}

#[test]
fn selling_a_blueprint_recovers_half_its_current_price() {
    let mut g = Game::new(5);
    let i = g.hand.iter().position(|c| c.machine == MachineId::Furnace).unwrap();
    let before = g.credits;
    g.sell_blueprint(i).unwrap();
    // round 0: mult = 175/35 = 5 → furnace priced 25, sells for 12
    assert_eq!(g.credits, before + 12);
}

#[test]
fn shop_prices_track_the_quota_curve() {
    use overflow_core::run::priced;
    assert_eq!(priced(3, 0), (3.0f64 * QUOTAS[1] as f64 / 35.0).round() as u32);
    assert_eq!(priced(3, 4), (3.0f64 * QUOTAS[5] as f64 / 35.0).round() as u32);
}

#[test]
fn directives_buff_their_tag_and_stack() {
    let line = || {
        let mut g = Game::new(42);
        build_starter(&mut g);
        g
    };
    let base = line().project().unwrap().payout;
    let mut g = line();
    g.directives.push(DirectiveId::Superheater); // furnaces are HEAT
    let buffed = g.project().unwrap().payout;
    assert!(buffed > base, "superheater speeds the furnaces: {base} -> {buffed}");
    g.directives.push(DirectiveId::Superheater);
    assert!(g.project().unwrap().payout > buffed, "directives stack");
    // an off-tag directive does nothing for this board
    let mut g = line();
    g.directives.push(DirectiveId::Enrichment);
    assert_eq!(g.project().unwrap().payout, base);
}

// ── the consignment layer ────────────────────────────────────────────────────

#[test]
fn lots_scale_with_round_and_contracts_bias_them() {
    let mut rng = Rng::new(7);
    let early = roll_lot(0, &[], &mut rng);
    let mut rng = Rng::new(7);
    let late = roll_lot(6, &[], &mut rng);
    assert_eq!(early.name, late.name, "same template under the same seed");
    assert!(
        late.entries[0].1 > early.entries[0].1 * 2,
        "quantities scale with the round: {} -> {}",
        early.entries[0].1,
        late.entries[0].1
    );

    // Tar Sands: ore lots gain ore and pick up slag
    let mut rng = Rng::new(1);
    let mut clean = None;
    let mut dirty = None;
    for _ in 0..40 {
        let l = roll_lot(0, &[], &mut rng);
        if l.name == "Bulk Ore" {
            clean = Some(l);
            break;
        }
    }
    let mut rng = Rng::new(1);
    for _ in 0..40 {
        let l = roll_lot(0, &[ContractId::TarSands], &mut rng);
        if l.name == "Bulk Ore" {
            dirty = Some(l);
            break;
        }
    }
    let (clean, dirty) = (clean.unwrap(), dirty.unwrap());
    assert!(dirty.entries[0].1 > clean.entries[0].1, "more ore under Tar Sands");
    assert!(
        dirty.entries.iter().any(|e| e.0 == ItemType::Slag),
        "…and slag mixed in: {dirty:?}"
    );
}

#[test]
fn sap_wilts_but_resin_does_not_and_sweet_tooth_stops_it() {
    // a lone belt ring holds a sap item while ticks pass
    let cells = vec![
        Placement::new(0, 0, MachineId::Belt, Some(Dir::E)),
        Placement::new(1, 0, MachineId::Belt, Some(Dir::W)),
    ];
    let wilted = {
        let mut s = Sim::new(3, 3, &cells, 1).unwrap();
        s.seed_items(&[overflow_core::sim::SeedItem { x: 0, y: 0, buffered: false, ty: ItemType::Sap, quality: 5 }]);
        for _ in 0..40 {
            s.step();
        }
        s.peek(0, 0).or(s.peek(1, 0)).unwrap().quality
    };
    assert!(wilted <= 1, "sap should wilt hard over 40 ticks: q{wilted}");

    let kept = {
        let mut s = Sim::new(3, 3, &cells, 1).unwrap();
        s.sap_decay = false; // the Sweet Tooth contract
        s.seed_items(&[overflow_core::sim::SeedItem { x: 0, y: 0, buffered: false, ty: ItemType::Sap, quality: 5 }]);
        for _ in 0..40 {
            s.step();
        }
        s.peek(0, 0).or(s.peek(1, 0)).unwrap().quality
    };
    assert_eq!(kept, 5);
}

#[test]
fn crystal_cracks_in_junctions_unless_contracted() {
    let cells = vec![
        Placement::new(0, 1, MachineId::Belt, Some(Dir::E)),
        Placement::new(1, 1, MachineId::Junction, None),
        Placement::new(2, 1, MachineId::Belt, Some(Dir::E)),
        Placement::new(3, 1, MachineId::Vault, None),
    ];
    let run = |crack: bool| {
        let mut s = Sim::new(5, 3, &cells, 1).unwrap();
        s.crystal_crack = crack;
        s.seed_items(&[overflow_core::sim::SeedItem { x: 0, y: 1, buffered: false, ty: ItemType::Crystal, quality: 4 }]);
        for _ in 0..10 {
            s.step();
        }
        s.delivered[0].quality
    };
    assert_eq!(run(true), 3, "one junction pass, one quality point");
    assert_eq!(run(false), 4, "Gentle Hands: no crack");
}

#[test]
fn flux_catalyzes_a_batch() {
    let cells = vec![
        Placement::new(0, 0, MachineId::Furnace, Some(Dir::E)),
        Placement::new(1, 0, MachineId::Vault, None),
    ];
    let run = |with_flux: bool, bonus: i32| {
        let mut s = Sim::new(3, 3, &cells, 1).unwrap();
        s.flux_bonus = bonus;
        let mut seeds = vec![overflow_core::sim::SeedItem { x: 0, y: 0, buffered: true, ty: ItemType::Ore, quality: 0 }];
        if with_flux {
            seeds.push(overflow_core::sim::SeedItem { x: 0, y: 0, buffered: true, ty: ItemType::Flux, quality: 0 });
        }
        s.seed_items(&seeds);
        for _ in 0..10 {
            s.step();
        }
        s.delivered[0].quality
    };
    assert_eq!(run(false, 2), 0);
    assert_eq!(run(true, 2), 2, "flux consumed for +2 quality");
    assert_eq!(run(true, 3), 3, "Flux Injector: +3");
}

#[test]
fn the_chute_swallows_and_pays_nothing() {
    let cells = vec![
        Placement::new(0, 0, MachineId::Belt, Some(Dir::E)),
        Placement::new(1, 0, MachineId::Chute, None),
    ];
    let mut s = Sim::new(3, 3, &cells, 1).unwrap();
    s.seed_items(&[overflow_core::sim::SeedItem { x: 0, y: 0, buffered: false, ty: ItemType::Slag, quality: 0 }]);
    for _ in 0..5 {
        s.step();
    }
    assert_eq!(s.items_in_flight(), 0, "swallowed");
    assert!(s.delivered.is_empty(), "unpaid");
}

#[test]
fn bays_stream_their_queue_in_order() {
    let cells = vec![
        Placement::new(0, 0, MachineId::Bay, Some(Dir::E)),
        Placement::new(1, 0, MachineId::Belt, Some(Dir::E)),
        Placement::new(2, 0, MachineId::Vault, None),
    ];
    let mut s = Sim::new(4, 3, &cells, 1).unwrap();
    s.seed_items(&[
        overflow_core::sim::SeedItem { x: 0, y: 0, buffered: true, ty: ItemType::Ore, quality: 0 },
        overflow_core::sim::SeedItem { x: 0, y: 0, buffered: true, ty: ItemType::Flux, quality: 0 },
    ]);
    for _ in 0..10 {
        s.step();
    }
    assert_eq!(s.delivered.len(), 2);
    assert_eq!(s.delivered[0].ty, ItemType::Ore, "queue order preserved");
    assert_eq!(s.delivered[1].ty, ItemType::Flux);
}

#[test]
fn contract_value_hooks_pay_out() {
    let cells = vec![
        Placement::new(0, 0, MachineId::Belt, Some(Dir::E)),
        Placement::new(1, 0, MachineId::Vault, None),
    ];
    let run = |gear_mult: Option<f64>, purist: bool, q: i32| {
        let mut s = Sim::new(3, 3, &cells, 1).unwrap();
        if let Some(m) = gear_mult {
            s.set_demand(ItemType::Gear, m);
        }
        s.purist = purist;
        s.seed_items(&[overflow_core::sim::SeedItem { x: 0, y: 0, buffered: false, ty: ItemType::Gear, quality: q }]);
        for _ in 0..5 {
            s.step();
        }
        s.delivered[0].value
    };
    assert_eq!(run(None, false, 0), 16.0);
    assert_eq!(run(Some(1.5), false, 0), 24.0, "Gear Syndicate");
    assert_eq!(run(None, true, 6), 16.0 * 2.5 * 1.5, "Purist on a q6 gear");
}

//! The design doc's claims, as assertions, plus deck/run coverage. Ported
//! from the retired TS reference suite (git history: `sim/src/sim.test.ts`);
//! the 19 pinned numbers survived the crossing and now live only here.

use overflow_core::boards::*;
use overflow_core::cards::Offer;
use overflow_core::defs::{item_value, Dir, ItemType, MachineId, QUALITY_CAP};
use overflow_core::run::{Game, GamePhase, HAND_MAX, QUOTAS, SHOP_SIZE};
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

/// The whole starting kit, built compactly by the vault: two drill+furnace
/// lanes sharing the spine into (17,9). ~112 payout at 60 ticks.
fn build_starting_lanes(g: &mut Game) {
    for (i, y) in [9, 8].into_iter().enumerate() {
        let d = g.hand.iter().position(|c| c.machine == MachineId::Drill).unwrap();
        g.play_card(d, 13, y, Some(Dir::E), None, None).unwrap();
        let f = g.hand.iter().position(|c| c.machine == MachineId::Furnace).unwrap();
        g.play_card(f, 14, y, Some(Dir::E), None, None).unwrap();
        g.buy_belt(15, y, Dir::E).unwrap();
        g.buy_belt(16, y, if i == 0 { Dir::E } else { Dir::S }).unwrap();
    }
}


#[test]
fn a_full_scripted_round_1_clears_quota_and_opens_the_shop() {
    let mut g = Game::new(42);
    assert_eq!(g.credits, 40);
    assert_eq!(g.hand.len(), 4); // 2 Drills + 2 Furnaces, the starting kit

    build_starting_lanes(&mut g);
    // placement is free (paid at the shop); only the 4 belts cost credits
    assert_eq!(g.credits, 40 - 4);

    let projected = g.project().unwrap().payout;
    assert!(projected >= QUOTAS[0], "projection {projected} should clear {}", QUOTAS[0]);

    let outcome = g.run_shift().unwrap();
    assert!(outcome.cleared);
    assert_eq!(g.phase, GamePhase::Shop);
    assert_eq!(g.offers.len(), SHOP_SIZE);

    // buy a machine at its current (round-scaled) price, then leave
    g.credits += 1_000; // affordability isn't under test; prices are
    let credits_before = g.credits;
    let idx = g.offers.iter().position(|o| matches!(o, Offer::Machine(_))).unwrap();
    let price = g.offer_price(g.offers[idx]);
    g.shop_buy(idx).unwrap();
    assert_eq!(g.credits, credits_before - price);
    assert_eq!(g.hand.len(), 1); // whole kit placed + the purchase

    g.shop_done().unwrap();
    assert_eq!(g.round, 1);
    assert_eq!(g.phase, GamePhase::Build);
    assert_eq!(g.hand.len(), 1); // the hand persists between rounds
}

#[test]
fn an_empty_board_ends_the_run() {
    let mut g = Game::new(9);
    let outcome = g.run_shift().unwrap();
    assert!(!outcome.cleared);
    assert_eq!(g.phase, GamePhase::Over { won: false });
}

#[test]
fn removing_a_machine_returns_its_blueprint_to_the_hand() {
    let mut g = Game::new(5);
    let n_before = g.hand.len();
    let i = g.hand.iter().position(|c| c.machine == MachineId::Drill).unwrap();
    g.play_card(i, 0, 0, Some(Dir::E), None, None).unwrap();
    assert_eq!(g.hand.len(), n_before - 1);
    g.sell(0, 0).unwrap();
    assert_eq!(g.hand.len(), n_before); // back in the hand, not a discard pile
    assert!(g.hand.iter().filter(|c| c.machine == MachineId::Drill).count() == 2);
}

#[test]
fn belts_refund_credits_but_machines_do_not() {
    let mut g = Game::new(5);
    g.buy_belt(0, 0, Dir::E).unwrap();
    let before = g.credits;
    g.sell(0, 0).unwrap();
    assert_eq!(g.credits, before + 1); // infrastructure refunds in credits
    let i = g.hand.iter().position(|c| c.machine == MachineId::Drill).unwrap();
    g.play_card(i, 0, 0, Some(Dir::E), None, None).unwrap();
    let before = g.credits;
    g.sell(0, 0).unwrap();
    assert_eq!(g.credits, before); // machines come back as blueprints instead
}

#[test]
fn selling_a_blueprint_recovers_half_its_current_price() {
    let mut g = Game::new(5);
    let i = g.hand.iter().position(|c| c.machine == MachineId::Furnace).unwrap();
    let before = g.credits;
    g.sell_blueprint(i).unwrap();
    // round 0: mult = 115/35 ≈ 3.29 → furnace priced 16, sells for 8
    assert_eq!(g.credits, before + 8);
    assert_eq!(g.hand.len(), 3);
}

#[test]
fn the_hand_caps_at_ten_blueprints() {
    let mut g = Game::new(42);
    build_starting_lanes(&mut g);
    g.run_shift().unwrap();
    g.credits = 100_000; // not testing affordability here
    let machine_slot = |g: &Game| g.offers.iter().position(|o| matches!(o, Offer::Machine(_)));
    while g.hand.len() < HAND_MAX {
        match machine_slot(&g) {
            Some(i) => g.shop_buy(i).unwrap(),
            None => g.shop_reroll().unwrap(),
        }
    }
    while machine_slot(&g).is_none() {
        g.shop_reroll().unwrap();
    }
    let i = machine_slot(&g).unwrap();
    assert!(g.shop_buy(i).is_err(), "buying a machine past the cap must be refused");
    g.shop_done().unwrap();
    // ...and pulling a machine off the board is refused too while full
    assert!(g.sell(13, 9).is_err());
    g.sell_blueprint(0).unwrap();
    g.sell(13, 9).unwrap(); // room again
}

#[test]
fn rerolls_escalate_within_a_shop() {
    let mut g = Game::new(42);
    build_starting_lanes(&mut g);
    g.run_shift().unwrap();
    let first = g.reroll_price();
    let before = g.credits;
    g.shop_reroll().unwrap();
    assert_eq!(g.credits, before - first);
    assert_eq!(g.offers.len(), SHOP_SIZE);
    assert_eq!(g.reroll_price(), first * 2, "second reroll costs double");
}

#[test]
fn shop_prices_track_the_quota_curve() {
    use overflow_core::run::priced;
    // prices follow the quota curve everywhere
    assert_eq!(priced(3, 0), (3.0f64 * QUOTAS[1] as f64 / 35.0).round() as u32);
    assert_eq!(priced(3, 4), (3.0f64 * QUOTAS[5] as f64 / 35.0).round() as u32);
}

#[test]
fn directives_buff_their_tag_and_stack() {
    // A drill+furnace lane, then Flywheel (KINETIC): the drill speeds up.
    let line = || {
        let mut g = Game::new(42);
        let drill = g.hand.iter().position(|c| c.machine == MachineId::Drill).unwrap();
        g.play_card(drill, 13, 9, Some(Dir::E), None, None).unwrap();
        let f = g.hand.iter().position(|c| c.machine == MachineId::Furnace).unwrap();
        g.play_card(f, 14, 9, Some(Dir::E), None, None).unwrap();
        g.buy_belt(15, 9, Dir::E).unwrap();
        g.buy_belt(16, 9, Dir::E).unwrap();
        g
    };
    let base = line().project().unwrap().payout;

    let mut g = line();
    g.directives.push(overflow_core::defs::DirectiveId::Flywheel);
    let buffed = g.project().unwrap().payout;
    assert!(buffed > base, "flywheel must speed the kinetic drill: {base} -> {buffed}");

    g.directives.push(overflow_core::defs::DirectiveId::Flywheel);
    let stacked = g.project().unwrap().payout;
    assert!(stacked > buffed, "directives stack: {buffed} -> {stacked}");

    // An off-tag directive does nothing for this board.
    let mut g = line();
    g.directives.push(overflow_core::defs::DirectiveId::Enrichment);
    assert_eq!(g.project().unwrap().payout, base);
}

#[test]
fn retry_rewinds_a_failed_round() {
    let mut g = Game::new(9);
    let outcome = g.run_shift().unwrap(); // empty board: instant failure
    assert!(!outcome.cleared);
    assert_eq!(g.phase, GamePhase::Over { won: false });
    g.retry_round().unwrap();
    assert_eq!(g.phase, GamePhase::Build);
    assert_eq!(g.round, 0);
    assert_eq!(g.hand.len(), 4, "hand untouched by the failed attempt");
}

//! The design doc's claims, as assertions — a port of the TS test suite plus
//! deck/run coverage. If a number here disagrees with `sim/src/sim.test.ts`,
//! one of the two implementations is wrong and the docs say which.

use overflow_core::boards::*;
use overflow_core::deck::Deck;
use overflow_core::defs::{item_value, Dir, ItemType, MachineId, QUALITY_CAP};
use overflow_core::rng::Rng;
use overflow_core::run::{Game, GamePhase, QUOTAS};
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

// ── deck ────────────────────────────────────────────────────────────────────

#[test]
fn deck_draws_are_deterministic_per_seed() {
    let mut r1 = Rng::new(7);
    let mut r2 = Rng::new(7);
    let mut d1 = Deck::starting(&mut r1);
    let mut d2 = Deck::starting(&mut r2);
    assert_eq!(d1.draw(4, &mut r1), d2.draw(4, &mut r2));
}

#[test]
fn deck_recycles_discard_but_not_consumed_cards() {
    let mut rng = Rng::new(1);
    let mut d = Deck::starting(&mut rng); // 4 cards
    let hand = d.draw(4, &mut rng);
    assert_eq!(hand.len(), 4);
    assert_eq!(d.size(), 0);
    // play two (consumed — never returned), discard two
    d.to_discard(hand[2]);
    d.to_discard(hand[3]);
    let next = d.draw(4, &mut rng);
    assert_eq!(next.len(), 2); // only the discarded pair cycles back
}

// ── run structure ───────────────────────────────────────────────────────────

#[test]
fn a_full_scripted_round_1_clears_quota() {
    let mut g = Game::new(42);
    assert_eq!(g.credits, 15);
    assert_eq!(g.hand.len(), 4);

    // hand is 2 Drills + 2 Furnaces in some order; find one of each
    let drill = g.hand.iter().position(|c| c.machine == MachineId::Drill).unwrap();
    g.play_card(drill, 0, 3, Some(Dir::E), None, None).unwrap();
    let furnace = g.hand.iter().position(|c| c.machine == MachineId::Furnace).unwrap();
    g.play_card(furnace, 4, 3, Some(Dir::E), None, None).unwrap();
    for x in [1, 2, 3] {
        g.buy_belt(x, 3, Dir::E).unwrap();
    }
    for x in [5, 6, 7, 8] {
        g.buy_belt(x, 3, Dir::E).unwrap();
    }

    let projected = g.project().unwrap().payout;
    assert!(projected >= QUOTAS[0], "projection {projected} should clear 20");

    let outcome = g.run_shift().unwrap();
    assert!(outcome.cleared);
    assert_eq!(g.phase, GamePhase::Reward);
    assert_eq!(g.offers.len(), 3);

    g.pick_reward(Some(0)).unwrap();
    assert_eq!(g.round, 1);
    assert_eq!(g.phase, GamePhase::Build);
    // 2 unplayed + 1 reward recycle back; the 2 placed machines are consumed,
    // so the deck genuinely shrank — that scarcity is the design.
    assert_eq!(g.hand.len(), 3);
}

#[test]
fn an_empty_board_ends_the_run() {
    let mut g = Game::new(9);
    let outcome = g.run_shift().unwrap();
    assert!(!outcome.cleared);
    assert_eq!(g.phase, GamePhase::Over { won: false });
}

#[test]
fn selling_returns_the_card_to_the_deck() {
    let mut g = Game::new(5);
    let n_before = g.deck.size() + g.hand.len();
    let i = g.hand.iter().position(|c| c.machine == MachineId::Drill).unwrap();
    g.play_card(i, 0, 0, Some(Dir::E), None, None).unwrap();
    assert_eq!(g.deck.size() + g.hand.len(), n_before - 1); // consumed
    g.sell(0, 0).unwrap();
    assert_eq!(g.deck.size() + g.hand.len(), n_before); // back in circulation
}

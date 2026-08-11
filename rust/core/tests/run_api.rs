//! The board-editing API a frontend needs during the build phase: rotation,
//! secondary-edge rotation, filter gates, and the steppable shift sim.

use overflow_core::defs::{Dir, MachineId, QUALITY_CAP};
use overflow_core::run::{is_audit, shift_len, Game};
use overflow_core::sim::FilterCfg;

/// A fresh game, fast-forwarded past the supply window to the build floor.
fn fresh(seed: u32) -> Game {
    let mut g = Game::new(seed);
    g.supply_done().unwrap();
    g
}

/// Place whatever card is at hand slot 0 at (x, y) facing east.
/// (Slot 0 is a Furnace: a 2×1 body covering (x,y) and (x+1,y).)
fn play_first(g: &mut Game, x: i32, y: i32) {
    g.play_card(0, x, y, Some(Dir::E), None, None).unwrap();
}

#[test]
fn rotate_cycles_clockwise_through_all_four_edges() {
    let mut g = fresh(1);
    play_first(&mut g, 0, 0);
    for want in [Dir::S, Dir::W, Dir::N, Dir::E] {
        g.rotate(0, 0).unwrap();
        assert_eq!(g.board.iter().find(|p| p.x == 0 && p.y == 0).unwrap().d, Some(want));
    }
}

#[test]
fn rotate_refuses_the_vault_and_empty_tiles() {
    let mut g = fresh(1);
    assert!(g.rotate(17, 9).is_err()); // the vault
    assert!(g.rotate(4, 4).is_err()); // empty
}

#[test]
fn rotate_d2_turns_only_the_secondary_edge() {
    let mut g = fresh(1);
    // A filter placed by hand (belts aside, any placement with d2 works).
    let mut p = overflow_core::sim::Placement::new(2, 2, MachineId::Filter, Some(Dir::E));
    p.d2 = Some(Dir::N);
    p.cfg = Some(FilterCfg { min_quality: Some(5), item_type: None });
    g.board.push(p);
    g.rotate_d2(2, 2).unwrap();
    let p = g.board.iter().find(|p| p.x == 2 && p.y == 2).unwrap();
    assert_eq!(p.d2, Some(Dir::E));
    assert_eq!(p.d, Some(Dir::E)); // untouched
}

#[test]
fn filter_gate_sets_and_clamps() {
    let mut g = fresh(1);
    let mut p = overflow_core::sim::Placement::new(2, 2, MachineId::Filter, Some(Dir::E));
    p.d2 = Some(Dir::N);
    g.board.push(p);
    g.set_filter_gate(2, 2, 7).unwrap();
    let gate = |g: &Game| {
        g.board.iter().find(|p| p.x == 2 && p.y == 2).unwrap().cfg.unwrap().min_quality
    };
    assert_eq!(gate(&g), Some(7));
    g.set_filter_gate(2, 2, 99).unwrap();
    assert_eq!(gate(&g), Some(QUALITY_CAP));
    g.set_filter_gate(2, 2, -3).unwrap();
    assert_eq!(gate(&g), Some(0));
}

#[test]
fn filter_gate_refuses_non_filters() {
    let mut g = fresh(1);
    play_first(&mut g, 0, 0);
    assert!(g.set_filter_gate(0, 0, 5).is_err());
}

#[test]
fn stepped_shift_sim_matches_the_committed_shift_exactly() {
    let mut g = Game::new(42);
    // slot the dealt supply cards at bay A so the line has material
    while !g.supply_hand.is_empty() {
        if g.allocate(0, 0).is_err() {
            break;
        }
    }
    g.supply_done().unwrap();
    // furnace beside bay A, short line east then down the far column
    let furnace = g.hand.iter().position(|c| c.machine == MachineId::Furnace).unwrap();
    g.play_card(furnace, 1, 6, Some(Dir::E), None, None).unwrap(); // (1,6)+(2,6)
    for x in 3..=15 {
        g.buy_belt(x, 6, Dir::E).unwrap();
    }
    for y in [6, 7, 8] {
        g.buy_belt(16, y, Dir::S).unwrap();
    }
    g.buy_belt(16, 9, Dir::E).unwrap();

    // Animate: step the sim one tick at a time, as the renderer will.
    let mut animated = g.shift_sim().unwrap();
    let ticks = g.shift_ticks();
    for _ in 0..ticks {
        animated.step();
    }
    let seen = animated.result(ticks);

    // Commit: the game re-runs the same board, seed, queues and carry.
    let outcome = g.run_shift().unwrap();
    assert!(seen.payout > 0, "the bay-fed line must actually deliver");
    assert_eq!(outcome.result.payout, seen.payout);
    assert_eq!(outcome.result.in_flight, seen.in_flight);
    assert_eq!(outcome.result.jam_ticks, seen.jam_ticks);
}

#[test]
fn move_preserves_direction_and_config() {
    let mut g = fresh(1);
    let mut p = overflow_core::sim::Placement::new(2, 2, MachineId::Filter, Some(Dir::E));
    p.d2 = Some(Dir::N);
    p.cfg = Some(FilterCfg { min_quality: Some(7), item_type: None });
    g.board.push(p);
    g.move_by(&[(2, 2)], 3, 1).unwrap();
    let q = g.board.iter().find(|p| p.x == 5 && p.y == 3).unwrap();
    assert_eq!(q.d, Some(Dir::E));
    assert_eq!(q.d2, Some(Dir::N));
    assert_eq!(q.cfg.unwrap().min_quality, Some(7));
}

#[test]
fn group_move_is_rigid_even_through_its_own_footprint() {
    // A 2×1 furnace and its belt move one tile east — the furnace's new
    // cells overlap the belt's old tile. Legal: the piece moves at once.
    let mut g = fresh(1);
    play_first(&mut g, 0, 0); // furnace covers (0,0)+(1,0)
    g.buy_belt(2, 0, Dir::E).unwrap();
    g.move_by(&[(0, 0), (2, 0)], 1, 0).unwrap();
    assert!(g.board.iter().any(|p| p.x == 1 && p.y == 0 && p.m == MachineId::Furnace));
    assert!(g.board.iter().any(|p| p.x == 3 && p.y == 0 && p.m == MachineId::Belt));
}

#[test]
fn blocked_or_out_of_bounds_group_move_changes_nothing() {
    let mut g = fresh(1);
    play_first(&mut g, 0, 0); // furnace covers (0,0)+(1,0)
    g.buy_belt(3, 0, Dir::E).unwrap(); // a stationary obstacle

    // furnace east cell would land on the stationary belt: refused whole.
    assert!(g.move_by(&[(0, 0)], 2, 0).is_err());
    assert!(g.board.iter().any(|p| p.x == 0 && p.y == 0));

    // Off the west edge: refused.
    assert!(g.move_by(&[(0, 0)], -1, 0).is_err());
    assert!(g.board.iter().any(|p| p.x == 0 && p.y == 0));
}

#[test]
fn the_vault_stays_bolted_down() {
    let mut g = fresh(1);
    // Selecting only the vault: nothing movable.
    assert!(g.move_by(&[(17, 9)], 1, 0).is_err());
    // A selection sweeping over the vault moves everything else and leaves it.
    g.buy_belt(16, 9, Dir::E).unwrap();
    g.move_by(&[(16, 9), (17, 9)], 0, 1).unwrap();
    assert!(g.board.iter().any(|p| p.x == 17 && p.y == 9 && p.m == MachineId::Vault));
    assert!(g.board.iter().any(|p| p.x == 16 && p.y == 10 && p.m == MachineId::Belt));
}

#[test]
fn audits_fall_on_rounds_4_8_and_12() {
    let audits: Vec<usize> = (0..12).filter(|&r| is_audit(r)).collect();
    assert_eq!(audits, vec![3, 7, 11]);
    assert_eq!(shift_len(3), shift_len(0), "shifts are uniform now");
}

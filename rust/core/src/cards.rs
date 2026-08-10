//! Blueprint cards and the round shop.
//!
//! The design as of 2026-08-10 (v2 — replacing the deal-a-hand deck):
//!
//!   - Machines are **blueprint cards**. Your hand is a persistent inventory,
//!     capped at [`crate::run::HAND_MAX`]; you play a blueprint whenever you
//!     like — placement itself is free, because you paid when you bought it.
//!   - Clearing a shift opens the **shop**: a rack of offers from the
//!     unlocked pool. Buying costs the machine's price and puts the card in
//!     your hand. Rerolling the rack costs a flat fee. This is the credit
//!     sink that lets production growth compound with the quota curve.
//!   - Removing a placed machine returns its blueprint to your hand.
//!     Selling a blueprint from your hand refunds half its price.
//!   - Belts and Junctions are NOT cards. They're cheap infrastructure,
//!     always available, paid per tile.
//!
//! Open design questions live in NOTES.md; this module is the mechanism.

use crate::defs::{def, MachineId, CARD_POOL};
use crate::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub machine: MachineId,
}

impl Card {
    /// What the shop charges for it.
    pub fn cost(self) -> u32 {
        def(self.machine).cost
    }

    /// What selling it back from your hand recovers.
    pub fn sell_value(self) -> u32 {
        def(self.machine).cost / 2
    }
}

/// The Ferrous Dynamics starting kit: enough blueprints to build one smelting
/// line twice over, so the first shift is buildable before any shop exists.
pub fn starting_hand() -> Vec<Card> {
    [MachineId::Drill, MachineId::Drill, MachineId::Furnace, MachineId::Furnace]
        .into_iter()
        .map(|m| Card { machine: m })
        .collect()
}

/// One shop rack: `n` distinct offers from the unlocked pool.
pub fn shop_offers(unlocked: &[MachineId], n: usize, rng: &mut Rng) -> Vec<Card> {
    let mut pool: Vec<MachineId> = unlocked.to_vec();
    let mut out = Vec::with_capacity(n);
    while out.len() < n && !pool.is_empty() {
        let i = rng.below(pool.len());
        out.push(Card { machine: pool.swap_remove(i) });
    }
    out
}

/// What a fresh profile has unlocked. Meta-progression grows this; the lab
/// bots use it as-is.
pub fn default_unlocked() -> Vec<MachineId> {
    // Everything except the deep tier-3/4 machines, which unlock by milestone.
    CARD_POOL
        .iter()
        .copied()
        .filter(|m| {
            !matches!(
                m,
                MachineId::EngineWorks | MachineId::LensGrinder | MachineId::CircuitBench
            )
        })
        .collect()
}

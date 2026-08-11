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

use crate::defs::{def, directive, DirectiveId, MachineId, CARD_POOL, DIRECTIVE_POOL};
use crate::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub machine: MachineId,
}

impl Card {
    /// Base price, before the round multiplier.
    pub fn cost(self) -> u32 {
        def(self.machine).cost
    }
}

/// One slot on the shop rack: a machine blueprint (goes to the hand) or a
/// directive (applies to the run permanently on purchase).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    Machine(Card),
    Directive(DirectiveId),
}

impl Offer {
    /// Base price, before the round multiplier.
    pub fn base_cost(self) -> u32 {
        match self {
            Offer::Machine(c) => c.cost(),
            Offer::Directive(d) => directive(d).cost,
        }
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

/// One shop rack: `n - 1` distinct machine offers from the unlocked pool,
/// plus one directive. The directive slot is the route-commitment lever —
/// it competes with raw throughput for the same credits every single round.
pub fn shop_rack(unlocked: &[MachineId], n: usize, rng: &mut Rng) -> Vec<Offer> {
    let mut pool: Vec<MachineId> = unlocked.to_vec();
    let mut out = Vec::with_capacity(n);
    while out.len() + 1 < n && !pool.is_empty() {
        let i = rng.below(pool.len());
        out.push(Offer::Machine(Card { machine: pool.swap_remove(i) }));
    }
    out.push(Offer::Directive(DIRECTIVE_POOL[rng.below(DIRECTIVE_POOL.len())]));
    out
}

/// What a fresh profile has unlocked: the whole pool. The cross-chain
/// assemblers (Circuit Bench, Lens Grinder, Engine Works) used to be
/// meta-locked, which quietly forced every run down the metal chain — the
/// opposite of the "many viable routes" goal. Shop inflation gates them now.
pub fn default_unlocked() -> Vec<MachineId> {
    CARD_POOL.to_vec()
}

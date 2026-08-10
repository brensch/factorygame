//! Blueprint cards — the deckbuilding layer.
//!
//! The design shift from the first prototype: machines are no longer freely
//! bought from an always-available palette. They are **blueprint cards**:
//!
//!   - Your meta-progression unlocks cards into the **pool**.
//!   - A run starts with a small fixed **deck** drawn from that pool.
//!   - Each round deals a **hand** at random from the deck. You can only place
//!     what you were dealt — the chance element the design wants.
//!   - Playing a card places the machine (credits still pay for it) and
//!     **consumes** the card: it's a physical machine on the board now.
//!   - Selling a placed machine returns its card to the discard pile.
//!   - Round rewards offer 1 of 3 new cards, added to the discard — the classic
//!     deckbuilder loop, so a run's identity is the deck you sculpt.
//!   - Belts are NOT cards. They're 1-credit infrastructure, always available;
//!     a deck full of belt cards would be a deck of dead draws.
//!
//! Open design questions live in NOTES.md; this module is the mechanism.

use crate::defs::{def, MachineId, CARD_POOL};
use crate::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub machine: MachineId,
}

impl Card {
    pub fn cost(self) -> u32 {
        def(self.machine).cost
    }
}

#[derive(Clone, Debug)]
pub struct Deck {
    draw: Vec<Card>,
    discard: Vec<Card>,
}

impl Deck {
    /// The Ferrous Dynamics starting deck: enough to build one smelting line,
    /// with one spare of each so a bad first hand can't brick the run.
    pub fn starting(rng: &mut Rng) -> Deck {
        let mut d = Deck {
            draw: [MachineId::Drill, MachineId::Drill, MachineId::Furnace, MachineId::Furnace]
                .into_iter()
                .map(|m| Card { machine: m })
                .collect(),
            discard: Vec::new(),
        };
        rng.shuffle(&mut d.draw);
        d
    }

    pub fn size(&self) -> usize {
        self.draw.len() + self.discard.len()
    }

    /// Draw up to `n` cards. When the draw pile empties, the discard is
    /// shuffled in — so consumed (placed) machines are genuinely gone, but
    /// unplayed and refunded cards keep cycling.
    pub fn draw(&mut self, n: usize, rng: &mut Rng) -> Vec<Card> {
        let mut hand = Vec::with_capacity(n);
        for _ in 0..n {
            if self.draw.is_empty() {
                if self.discard.is_empty() {
                    break;
                }
                std::mem::swap(&mut self.draw, &mut self.discard);
                rng.shuffle(&mut self.draw);
            }
            hand.push(self.draw.pop().unwrap());
        }
        hand
    }

    /// A returned card (unplayed at end of round, or a sold machine).
    pub fn to_discard(&mut self, c: Card) {
        self.discard.push(c);
    }

    /// Round reward: `n` distinct offers from the unlocked pool.
    pub fn offers(unlocked: &[MachineId], n: usize, rng: &mut Rng) -> Vec<Card> {
        let mut pool: Vec<MachineId> = unlocked.to_vec();
        let mut out = Vec::with_capacity(n);
        while out.len() < n && !pool.is_empty() {
            let i = rng.below(pool.len());
            out.push(Card { machine: pool.swap_remove(i) });
        }
        out
    }
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

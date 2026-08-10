//! Run structure: rounds, quotas, credits, the hand, the shop, the
//! build/shift loop.
//!
//! This is the layer a UI (browser/WASM now, Bevy later) and the lab bots both
//! drive. It owns no rendering and no policy — it exposes legal actions and
//! applies them.

use crate::cards::{default_unlocked, shop_offers, starting_hand, Card};
use crate::defs::{def, Dir, MachineId, QUALITY_CAP};
use crate::rng::Rng;
use crate::sim::{FilterCfg, Placement, ShiftResult, Sim};

pub const BOARD_W: i32 = 10;
pub const BOARD_H: i32 = 7;
pub const QUOTAS: [i64; 12] = [20, 45, 90, 200, 400, 700, 1200, 2400, 4500, 8000, 14000, 30000];
/// Blueprints the hand can hold. Buying past this is refused, and so is
/// pulling a machine off the board when there's no room for its card.
pub const HAND_MAX: usize = 10;
/// Offers on one shop rack.
pub const SHOP_SIZE: usize = 5;
/// Flat cost to reroll the rack.
pub const REROLL_COST: u32 = 5;
pub const STARTING_CREDITS: u32 = 15;

/// Rounds flagged as audits (zero-based). Mechanically, only the final one
/// does anything yet — it halves the shift — the rest are flagged in the UI
/// and NOTES.md is honest about that.
pub const AUDIT_ROUNDS: [usize; 3] = [3, 7, 11];

pub fn is_audit(round: usize) -> bool {
    AUDIT_ROUNDS.contains(&round)
}

pub fn shift_len(round: usize) -> u32 {
    if round == 11 { 30 } else { 60 } // final audit halves the shift
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamePhase {
    /// Placing machines and belts; hand is live.
    Build,
    /// The between-rounds shop: buy blueprints, reroll, continue.
    Shop,
    /// Run ended: cleared all rounds, or missed quota.
    Over { won: bool },
}

#[derive(Clone, Debug)]
pub struct RoundOutcome {
    pub round: usize,
    pub result: ShiftResult,
    pub quota: i64,
    pub cleared: bool,
}

pub struct Game {
    pub round: usize,
    pub credits: u32,
    pub phase: GamePhase,
    pub board: Vec<Placement>,
    /// Persistent blueprint inventory, capped at [`HAND_MAX`].
    pub hand: Vec<Card>,
    /// The shop rack while `phase == Shop`.
    pub offers: Vec<Card>,
    pub unlocked: Vec<MachineId>,
    pub history: Vec<RoundOutcome>,
    rng: Rng,
    seed: u32,
}

impl Game {
    pub fn new(seed: u32) -> Game {
        let rng = Rng::new(seed);
        // The Vault starts bolted to the east edge, as in the design doc.
        let board = vec![Placement::new(BOARD_W - 1, 3, MachineId::Vault, None)];
        Game {
            round: 0,
            credits: STARTING_CREDITS,
            phase: GamePhase::Build,
            board,
            hand: starting_hand(),
            offers: Vec::new(),
            unlocked: default_unlocked(),
            history: Vec::new(),
            rng,
            seed,
        }
    }

    pub fn quota(&self) -> i64 {
        QUOTAS[self.round]
    }

    fn occupied(&self, x: i32, y: i32) -> bool {
        self.board.iter().any(|p| p.x == x && p.y == y)
    }

    fn in_bounds(x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < BOARD_W && y < BOARD_H
    }

    fn buy_infra(&mut self, x: i32, y: i32, m: MachineId, d: Option<Dir>) -> Result<(), String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        let cost = def(m).cost;
        if self.credits < cost {
            return Err(format!("cannot afford {}", def(m).name));
        }
        if !Self::in_bounds(x, y) || self.occupied(x, y) {
            return Err(format!("tile {},{} unavailable", x, y));
        }
        self.credits -= cost;
        let mut p = Placement::new(x, y, m, d);
        p.d2 = None;
        self.board.push(p);
        Ok(())
    }

    /// Lay a belt — infrastructure, not a card. 1 credit.
    pub fn buy_belt(&mut self, x: i32, y: i32, d: Dir) -> Result<(), String> {
        self.buy_infra(x, y, MachineId::Belt, Some(d))
    }

    /// Place a junction — the crossing tile. Infrastructure, no orientation.
    pub fn buy_junction(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.buy_infra(x, y, MachineId::Junction, None)
    }

    /// Play a blueprint from the hand: place its machine. Free — the card
    /// was paid for at the shop.
    pub fn play_card(
        &mut self,
        hand_idx: usize,
        x: i32,
        y: i32,
        d: Option<Dir>,
        d2: Option<Dir>,
        cfg: Option<FilterCfg>,
    ) -> Result<(), String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        let card = *self.hand.get(hand_idx).ok_or("no such card in hand")?;
        if !Self::in_bounds(x, y) || self.occupied(x, y) {
            return Err(format!("tile {},{} unavailable", x, y));
        }
        self.hand.remove(hand_idx);
        let mut p = Placement::new(x, y, card.machine, d);
        p.d2 = d2;
        p.cfg = cfg;
        self.board.push(p);
        Ok(())
    }

    /// Remove a placed machine. Infrastructure (belts, junctions) refunds its
    /// credits; a real machine goes back to the hand as its blueprint.
    pub fn sell(&mut self, x: i32, y: i32) -> Result<(), String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        let i = self
            .board
            .iter()
            .position(|p| p.x == x && p.y == y && p.m != MachineId::Vault)
            .ok_or("nothing sellable there")?;
        let m = self.board[i].m;
        if matches!(m, MachineId::Belt | MachineId::Junction) {
            self.board.remove(i);
            self.credits += def(m).cost;
        } else {
            if self.hand.len() >= HAND_MAX {
                return Err("hand is full — sell a blueprint first".into());
            }
            self.board.remove(i);
            self.hand.push(Card { machine: m });
        }
        Ok(())
    }

    /// Sell a blueprint out of the hand for half its shop price.
    pub fn sell_blueprint(&mut self, hand_idx: usize) -> Result<(), String> {
        if matches!(self.phase, GamePhase::Over { .. }) {
            return Err("run is over".into());
        }
        let card = *self.hand.get(hand_idx).ok_or("no such card in hand")?;
        self.hand.remove(hand_idx);
        self.credits += card.sell_value();
        Ok(())
    }

    fn board_mut(&mut self, x: i32, y: i32) -> Result<&mut Placement, String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        self.board
            .iter_mut()
            .find(|p| p.x == x && p.y == y && p.m != MachineId::Vault)
            .ok_or_else(|| format!("nothing editable at {x},{y}"))
    }

    /// Rotate a placed machine's output edge clockwise.
    pub fn rotate(&mut self, x: i32, y: i32) -> Result<(), String> {
        let p = self.board_mut(x, y)?;
        match p.d {
            Some(d) => {
                p.d = Some(d.turn_cw());
                Ok(())
            }
            None => Err("machine has no output edge".into()),
        }
    }

    /// Rotate the secondary edge (Filter eject, Splitter second output).
    pub fn rotate_d2(&mut self, x: i32, y: i32) -> Result<(), String> {
        let p = self.board_mut(x, y)?;
        match p.d2 {
            Some(d) => {
                p.d2 = Some(d.turn_cw());
                Ok(())
            }
            None => Err("machine has no secondary edge".into()),
        }
    }

    /// Set a Filter's quality gate.
    pub fn set_filter_gate(&mut self, x: i32, y: i32, min_quality: i32) -> Result<(), String> {
        let p = self.board_mut(x, y)?;
        if p.m != MachineId::Filter {
            return Err("not a filter".into());
        }
        let mut cfg = p.cfg.unwrap_or_default();
        cfg.min_quality = Some(min_quality.clamp(0, QUALITY_CAP));
        p.cfg = Some(cfg);
        Ok(())
    }

    /// Move every machine on `tiles` by (dx, dy), as one rigid piece.
    /// All-or-nothing: if any destination is off the board or occupied by a
    /// machine that isn't itself moving, nothing moves. The Vault is bolted
    /// down — selections simply flow around it. Free, like all build edits.
    pub fn move_by(&mut self, tiles: &[(i32, i32)], dx: i32, dy: i32) -> Result<(), String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        let moving: Vec<usize> = self
            .board
            .iter()
            .enumerate()
            .filter(|(_, p)| p.m != MachineId::Vault && tiles.contains(&(p.x, p.y)))
            .map(|(i, _)| i)
            .collect();
        if moving.is_empty() {
            return Err("nothing movable selected".into());
        }
        for &i in &moving {
            let (nx, ny) = (self.board[i].x + dx, self.board[i].y + dy);
            if !Self::in_bounds(nx, ny) {
                return Err(format!("move would leave the board at {nx},{ny}"));
            }
            let blocked = self
                .board
                .iter()
                .enumerate()
                .any(|(j, q)| !moving.contains(&j) && q.x == nx && q.y == ny);
            if blocked {
                return Err(format!("tile {nx},{ny} is occupied"));
            }
        }
        for i in moving {
            self.board[i].x += dx;
            self.board[i].y += dy;
        }
        Ok(())
    }

    /// The shift as a steppable sim — same board, same seed as `run_shift`,
    /// so a renderer can animate tick by tick and the committed result is
    /// guaranteed identical.
    pub fn shift_sim(&self) -> Result<Sim, String> {
        Sim::new(BOARD_W, BOARD_H, &self.board, self.shift_seed())
    }

    /// Dry-run the shift without committing — the projection panel.
    pub fn project(&self) -> Result<ShiftResult, String> {
        Ok(self.shift_sim()?.run(shift_len(self.round)))
    }

    /// Per-round shift seed: derived from the run seed so a run is fully
    /// reproducible, but each round's Duplicator rolls differ.
    fn shift_seed(&self) -> u32 {
        self.seed
            .wrapping_mul(0x9e37_79b9)
            .wrapping_add(self.round as u32)
    }

    /// Run the shift for real and advance the round state machine.
    pub fn run_shift(&mut self) -> Result<&RoundOutcome, String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        let mut sim = self.shift_sim()?;
        let result = sim.run(shift_len(self.round));
        let quota = self.quota();
        let cleared = result.payout >= quota;
        self.history.push(RoundOutcome { round: self.round, result, quota, cleared });

        if !cleared {
            self.phase = GamePhase::Over { won: false };
        } else {
            let surplus = (self.history.last().unwrap().result.payout - quota) as u32;
            self.credits += surplus;
            if self.round + 1 >= QUOTAS.len() {
                self.phase = GamePhase::Over { won: true };
            } else {
                self.offers = shop_offers(&self.unlocked, SHOP_SIZE, &mut self.rng);
                self.phase = GamePhase::Shop;
            }
        }
        Ok(self.history.last().unwrap())
    }

    /// Buy the shop offer at `offer_idx` into the hand.
    pub fn shop_buy(&mut self, offer_idx: usize) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        let card = *self.offers.get(offer_idx).ok_or("no such offer")?;
        if self.credits < card.cost() {
            return Err(format!("cannot afford {}", def(card.machine).name));
        }
        if self.hand.len() >= HAND_MAX {
            return Err("hand is full — sell a blueprint first".into());
        }
        self.credits -= card.cost();
        self.offers.remove(offer_idx);
        self.hand.push(card);
        Ok(())
    }

    /// Swap the rack for a fresh one, for [`REROLL_COST`].
    pub fn shop_reroll(&mut self) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        if self.credits < REROLL_COST {
            return Err("cannot afford a reroll".into());
        }
        self.credits -= REROLL_COST;
        self.offers = shop_offers(&self.unlocked, SHOP_SIZE, &mut self.rng);
        Ok(())
    }

    /// Leave the shop and start the next round.
    pub fn shop_done(&mut self) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        self.offers.clear();
        self.round += 1;
        self.phase = GamePhase::Build;
        Ok(())
    }
}

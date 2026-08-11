//! Run structure: rounds, quotas, credits, the hand, the shop, the
//! build/shift loop.
//!
//! This is the layer a UI (browser/WASM now, Bevy later) and the lab bots both
//! drive. It owns no rendering and no policy — it exposes legal actions and
//! applies them.

use crate::cards::{default_unlocked, shop_rack, starting_hand, Card, Offer};
use crate::defs::{def, Dir, DirectiveId, ItemType, MachineId, Tag, ITEM_TYPES, QUALITY_CAP, TAGS};
use crate::rng::Rng;
use crate::sim::{FilterCfg, Placement, ShiftResult, Sim};

pub const BOARD_W: i32 = 18;
pub const BOARD_H: i32 = 18;
/// Refit 2026-08-10 against the compact-building LaneBot's measured payout
/// curve (112, 137, 151, 167, 234, 278 by round). Shape: ~0.75× of widening
/// capacity at round 1, tightening to ~1.05× by round 4, permanently above
/// pure widening from round 5 — multipliers, markets and doctrines carry
/// from there. Steps ~1.25–1.45×. The previous hand-authored curve measured
/// 2.2× free money at round 1 and a 0.48× cliff at round 4.
pub const QUOTAS: [i64; 12] = [85, 115, 145, 180, 260, 380, 550, 800, 1150, 1650, 2400, 3400];
/// Blueprints the hand can hold. Buying past this is refused, and so is
/// pulling a machine off the board when there's no room for its card.
pub const HAND_MAX: usize = 10;
/// Offers on one shop rack (the last slot is always a directive).
pub const SHOP_SIZE: usize = 5;
/// Base reroll price, before the round multiplier and escalation.
pub const REROLL_BASE: u32 = 2;
/// Enough to lay belts for BOTH starting lane kits — round 1's quota
/// demands the whole kit, not half of it.
pub const STARTING_CREDITS: u32 = 40;
/// The spot market pays this multiple for the in-demand item.
pub const MARKET_MULT: f64 = 2.0;
/// Audit inspections slow the inspected tag to this fraction.
pub const AUDIT_SPEED_PENALTY: f64 = 0.6;

/// Shop inflation: prices track the quota curve so a purchase stays a real
/// decision all run. The floor matters as much as the slope — early quotas
/// are trivially overshot with the starting kit, so early prices sitting at
/// ~1× base is what let a good first round buy the whole rack.
pub fn shop_price_mult(round: usize) -> f64 {
    let next = (round + 1).min(QUOTAS.len() - 1);
    (QUOTAS[next] as f64 / 35.0).max(3.0)
}

/// A base price scaled for the shop of the given round.
pub fn priced(base: u32, round: usize) -> u32 {
    (base as f64 * shop_price_mult(round)).round() as u32
}

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
    pub offers: Vec<Offer>,
    /// Directives owned this run — permanent, stacking, never placed.
    pub directives: Vec<DirectiveId>,
    /// The spot market for the round being (or about to be) built: this item
    /// pays [`MARKET_MULT`] × at the vault. Rolled when the shop opens, so
    /// purchases can chase it.
    pub market: ItemType,
    /// Audit rounds only: the tag under inspection, working at
    /// [`AUDIT_SPEED_PENALTY`] speed this round.
    pub audit_tag: Option<Tag>,
    pub unlocked: Vec<MachineId>,
    pub history: Vec<RoundOutcome>,
    /// Rerolls taken in the current shop; escalates the price.
    rerolls: u32,
    rng: Rng,
    seed: u32,
}

impl Game {
    pub fn new(seed: u32) -> Game {
        let rng = Rng::new(seed);
        // The Vault starts bolted to the east edge, mid-board.
        let board = vec![Placement::new(BOARD_W - 1, BOARD_H / 2, MachineId::Vault, None)];
        let mut g = Game {
            round: 0,
            credits: STARTING_CREDITS,
            phase: GamePhase::Build,
            board,
            hand: starting_hand(),
            offers: Vec::new(),
            directives: Vec::new(),
            market: ItemType::Ore, // rolled properly just below
            audit_tag: None,
            unlocked: default_unlocked(),
            history: Vec::new(),
            rerolls: 0,
            rng,
            seed,
        };
        g.roll_conditions(0);
        g
    }

    /// Roll the round's chance elements: what the market wants, and (on audit
    /// rounds) which tag the inspectors slow down. Stored, so a retry replays
    /// the same conditions.
    fn roll_conditions(&mut self, round: usize) {
        self.market = ITEM_TYPES[self.rng.below(ITEM_TYPES.len())];
        self.audit_tag = if is_audit(round) {
            Some(TAGS[self.rng.below(TAGS.len())])
        } else {
            None
        };
    }

    /// What the shop currently charges for an offer.
    pub fn offer_price(&self, o: Offer) -> u32 {
        priced(o.base_cost(), self.round)
    }

    /// What the next reroll costs: base × round inflation × how many rerolls
    /// this shop has already had. Fishing gets expensive fast.
    pub fn reroll_price(&self) -> u32 {
        priced(REROLL_BASE, self.round) * (self.rerolls + 1)
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

    fn buy_infra(&mut self, x: i32, y: i32, m: MachineId, d: Option<Dir>, d2: Option<Dir>) -> Result<(), String> {
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
        p.d2 = d2;
        self.board.push(p);
        Ok(())
    }

    /// Lay a belt — infrastructure, not a card. 1 credit.
    pub fn buy_belt(&mut self, x: i32, y: i32, d: Dir) -> Result<(), String> {
        self.buy_infra(x, y, MachineId::Belt, Some(d), None)
    }

    /// Place a junction — the crossing tile. Infrastructure, no orientation.
    pub fn buy_junction(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.buy_infra(x, y, MachineId::Junction, None, None)
    }

    /// Place a merger — accepts from every side, outputs one way.
    pub fn buy_merger(&mut self, x: i32, y: i32, d: Dir) -> Result<(), String> {
        self.buy_infra(x, y, MachineId::Merger, Some(d), None)
    }

    /// Place a splitter — alternates between `d` and the next edge clockwise.
    pub fn buy_splitter(&mut self, x: i32, y: i32, d: Dir) -> Result<(), String> {
        self.buy_infra(x, y, MachineId::Splitter, Some(d), Some(d.turn_cw()))
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

    /// Remove a placed machine. Infrastructure refunds its credits; a real
    /// machine goes back to the hand as its blueprint.
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
        if matches!(m, MachineId::Belt | MachineId::Junction | MachineId::Merger | MachineId::Splitter) {
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

    /// Sell a blueprint out of the hand for half its current shop price —
    /// blueprints appreciate with the market.
    pub fn sell_blueprint(&mut self, hand_idx: usize) -> Result<(), String> {
        if matches!(self.phase, GamePhase::Over { .. }) {
            return Err("run is over".into());
        }
        let card = *self.hand.get(hand_idx).ok_or("no such card in hand")?;
        self.hand.remove(hand_idx);
        self.credits += priced(card.cost(), self.round) / 2;
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

    /// The shift as a steppable sim — same board, seed, directives, market
    /// and audit as `run_shift`, so a renderer can animate tick by tick and
    /// the committed result is guaranteed identical.
    pub fn shift_sim(&self) -> Result<Sim, String> {
        let mut sim = Sim::new(BOARD_W, BOARD_H, &self.board, self.shift_seed())?;
        sim.apply_directives(&self.directives);
        sim.set_demand(self.market, MARKET_MULT);
        if let Some(tag) = self.audit_tag {
            sim.apply_tag_speed(tag, AUDIT_SPEED_PENALTY);
        }
        Ok(sim)
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
                self.offers = shop_rack(&self.unlocked, SHOP_SIZE, &mut self.rng);
                self.rerolls = 0;
                // Next round's conditions are known in the shop, so purchases
                // can chase the market and brace for the inspection.
                self.roll_conditions(self.round + 1);
                self.phase = GamePhase::Shop;
            }
        }
        Ok(self.history.last().unwrap())
    }

    /// A missed quota isn't the end while you're iterating: rewind to the
    /// build phase of the same round and try again. Board, hand, credits and
    /// directives are exactly as you left them.
    pub fn retry_round(&mut self) -> Result<(), String> {
        if self.phase != (GamePhase::Over { won: false }) {
            return Err("nothing to retry".into());
        }
        self.phase = GamePhase::Build;
        Ok(())
    }

    /// Buy the shop offer at `offer_idx`: a machine goes to the hand, a
    /// directive applies to the run immediately and permanently.
    pub fn shop_buy(&mut self, offer_idx: usize) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        let offer = *self.offers.get(offer_idx).ok_or("no such offer")?;
        let price = self.offer_price(offer);
        if self.credits < price {
            return Err("cannot afford that".into());
        }
        if let Offer::Machine(_) = offer {
            if self.hand.len() >= HAND_MAX {
                return Err("hand is full — sell a blueprint first".into());
            }
        }
        self.credits -= price;
        self.offers.remove(offer_idx);
        match offer {
            Offer::Machine(c) => self.hand.push(c),
            Offer::Directive(d) => self.directives.push(d),
        }
        Ok(())
    }

    /// Swap the rack for a fresh one. Each reroll in the same shop costs more.
    pub fn shop_reroll(&mut self) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        let price = self.reroll_price();
        if self.credits < price {
            return Err("cannot afford a reroll".into());
        }
        self.credits -= price;
        self.rerolls += 1;
        self.offers = shop_rack(&self.unlocked, SHOP_SIZE, &mut self.rng);
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

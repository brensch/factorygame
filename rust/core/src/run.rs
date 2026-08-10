//! Run structure: rounds, quotas, credits, hands, the build/shift loop.
//!
//! This is the layer a UI (browser/WASM now, Bevy later) and the lab bots both
//! drive. It owns no rendering and no policy — it exposes legal actions and
//! applies them.

use crate::deck::{default_unlocked, Card, Deck};
use crate::defs::{def, Dir, Kind, MachineId, QUALITY_CAP};
use crate::rng::Rng;
use crate::sim::{FilterCfg, Placement, ShiftResult, Sim};

pub const BOARD_W: i32 = 10;
pub const BOARD_H: i32 = 7;
pub const QUOTAS: [i64; 12] = [20, 45, 90, 200, 400, 700, 1200, 2400, 4500, 8000, 14000, 30000];
pub const HAND_SIZE: usize = 4;
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
    /// Awaiting a reward pick (index into `offers`).
    Reward,
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
    pub deck: Deck,
    pub hand: Vec<Card>,
    pub offers: Vec<Card>,
    pub unlocked: Vec<MachineId>,
    pub history: Vec<RoundOutcome>,
    rng: Rng,
    seed: u32,
}

impl Game {
    pub fn new(seed: u32) -> Game {
        let mut rng = Rng::new(seed);
        let mut deck = Deck::starting(&mut rng);
        let hand = deck.draw(HAND_SIZE, &mut rng);
        // The Vault starts bolted to the east edge, as in the design doc.
        let board = vec![Placement::new(BOARD_W - 1, 3, MachineId::Vault, None)];
        Game {
            round: 0,
            credits: STARTING_CREDITS,
            phase: GamePhase::Build,
            board,
            deck,
            hand,
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

    /// Lay a belt — infrastructure, not a card. 1 credit.
    pub fn buy_belt(&mut self, x: i32, y: i32, d: Dir) -> Result<(), String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        let cost = def(MachineId::Belt).cost;
        if self.credits < cost {
            return Err("cannot afford belt".into());
        }
        if !Self::in_bounds(x, y) || self.occupied(x, y) {
            return Err(format!("tile {},{} unavailable", x, y));
        }
        self.credits -= cost;
        self.board.push(Placement::new(x, y, MachineId::Belt, Some(d)));
        Ok(())
    }

    /// Play a card from the hand: place its machine. Consumes the card.
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
        if self.credits < card.cost() {
            return Err(format!("cannot afford {}", def(card.machine).name));
        }
        if !Self::in_bounds(x, y) || self.occupied(x, y) {
            return Err(format!("tile {},{} unavailable", x, y));
        }
        self.credits -= card.cost();
        self.hand.remove(hand_idx);
        let mut p = Placement::new(x, y, card.machine, d);
        p.d2 = d2;
        p.cfg = cfg;
        self.board.push(p);
        Ok(())
    }

    /// Remove a placed machine: full credit refund, card back to the discard.
    pub fn sell(&mut self, x: i32, y: i32) -> Result<(), String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        let i = self
            .board
            .iter()
            .position(|p| p.x == x && p.y == y && p.m != MachineId::Vault)
            .ok_or("nothing sellable there")?;
        let p = self.board.remove(i);
        self.credits += def(p.m).cost;
        if def(p.m).kind != Kind::Logistics || p.m != MachineId::Belt {
            self.deck.to_discard(Card { machine: p.m });
        }
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
                self.offers = Deck::offers(&self.unlocked, 3, &mut self.rng);
                self.phase = GamePhase::Reward;
            }
        }
        Ok(self.history.last().unwrap())
    }

    /// Pick a reward card (or None to skip). Unplayed hand discards, a fresh
    /// hand is dealt, and the next round begins.
    pub fn pick_reward(&mut self, offer_idx: Option<usize>) -> Result<(), String> {
        if self.phase != GamePhase::Reward {
            return Err("no reward pending".into());
        }
        if let Some(i) = offer_idx {
            let c = *self.offers.get(i).ok_or("no such offer")?;
            self.deck.to_discard(c);
        }
        self.offers.clear();
        for c in self.hand.drain(..) {
            self.deck.to_discard(c);
        }
        self.round += 1;
        self.hand = self.deck.draw(HAND_SIZE, &mut self.rng);
        self.phase = GamePhase::Build;
        Ok(())
    }
}

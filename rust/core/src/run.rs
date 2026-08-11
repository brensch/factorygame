//! Run structure: rounds, quotas, credits, the hand, the shop, the
//! build/shift loop.
//!
//! This is the layer a UI (browser/WASM now, Bevy later) and the lab bots both
//! drive. It owns no rendering and no policy — it exposes legal actions and
//! applies them.

use crate::cards::{default_unlocked, shop_rack, starting_hand, Card, Offer};
use crate::defs::{
    def, ContractId, Dir, DirectiveId, ItemType, MachineId, Tag, ITEM_TYPES, QUALITY_CAP, TAGS,
};
use crate::rng::Rng;
use crate::sim::{FilterCfg, Placement, SeedItem, ShiftResult, Sim};

pub const BOARD_W: i32 = 18;
pub const BOARD_H: i32 = 18;
/// Placeholder pending re-measurement under the consignment model; the lab
/// refits this after every economy change.
pub const QUOTAS: [i64; 12] = [130, 175, 235, 310, 410, 540, 700, 900, 1170, 1500, 1930, 2500];

/// A round is played in shifts: deliveries sum toward the quota, the factory
/// stays warm in between, and clearing early converts spare shifts to bonus
/// credits.
pub const SHIFTS_PER_ROUND: u32 = 3;
pub const SHIFT_TICKS: u32 = 40;
/// Blueprints the hand can hold. Buying past this is refused, and so is
/// pulling a machine off the board when there's no room for its card.
pub const HAND_MAX: usize = 10;
/// Offers on one shop rack (the last slot is always a directive).
pub const SHOP_SIZE: usize = 5;
/// Base reroll price, before the round multiplier and escalation.
pub const REROLL_BASE: u32 = 2;
/// Enough to plumb the starting kit across the big board: trunk, two or
/// three furnace lanes, and the spine to the vault.
pub const STARTING_CREDITS: u32 = 75;
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

pub fn shift_len(_round: usize) -> u32 {
    SHIFT_TICKS
}

/// One shipment on offer: what's inside, at what quality, for what price.
#[derive(Clone, Debug, PartialEq)]
pub struct Lot {
    pub name: &'static str,
    pub entries: Vec<(ItemType, u32)>,
    pub quality: i32,
    pub price: u32,
}

/// The shipment catalogue. Quantities scale with the round so late factories
/// stay fed; contracts bias what arrives. Public so the lab and tests can
/// sample the distribution directly.
pub fn roll_lot(round: usize, contracts: &[ContractId], rng: &mut Rng) -> Lot {
    let scale = 1.0 + round as f64 * 0.5;
    let q = |n: u32| (n as f64 * scale).round() as u32;
    let mut lot = match rng.below(6) {
        0 => Lot { name: "Bulk Ore", entries: vec![(ItemType::Ore, q(60))], quality: 0, price: 0 },
        1 => Lot {
            name: "Dirty Ore",
            entries: vec![(ItemType::Ore, q(80)), (ItemType::Slag, q(20))],
            quality: 0,
            price: 0,
        },
        2 => Lot { name: "Fresh Sap", entries: vec![(ItemType::Sap, q(40))], quality: 2, price: 0 },
        3 => Lot { name: "Crystal Case", entries: vec![(ItemType::Crystal, q(18))], quality: 2, price: 0 },
        4 => Lot {
            name: "Mixed Manifest",
            entries: vec![(ItemType::Ore, q(35)), (ItemType::Sap, q(20))],
            quality: 1,
            price: 0,
        },
        _ => Lot { name: "Flux Drum", entries: vec![(ItemType::Flux, q(10))], quality: 0, price: 0 },
    };
    if contracts.contains(&ContractId::TarSands) {
        for e in lot.entries.iter_mut() {
            if e.0 == ItemType::Ore {
                let extra = (e.1 as f64 * 0.6) as u32;
                e.1 += extra;
                lot.entries.push((ItemType::Slag, (extra as f64 * 0.33) as u32));
                break;
            }
        }
    }
    if contracts.contains(&ContractId::BulkManifests) {
        for e in lot.entries.iter_mut() {
            e.1 = (e.1 as f64 * 1.3).round() as u32;
        }
    }
    // Price: ~35% of the raw value of the contents.
    let raw: f64 = lot
        .entries
        .iter()
        .map(|&(t, n)| crate::defs::item_value(t, lot.quality) * n as f64)
        .sum();
    lot.price = (raw * 0.35).round().max(1.0) as u32;
    lot
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
    /// Contracts owned this run — the joker layer, biasing input and output.
    pub contracts: Vec<ContractId>,
    /// Shipments on offer while the shop is open.
    pub lot_offers: Vec<Lot>,
    /// Each bay's queue: (type, count, quality) runs, streamed in order.
    pub bay_queues: Vec<Vec<(ItemType, u32, i32)>>,
    /// Items still on the board between shifts — the warm factory.
    pub carry: Vec<SeedItem>,
    /// Shifts spent on the current round, of [`SHIFTS_PER_ROUND`].
    pub shifts_used: u32,
    /// Deliveries banked toward the current quota across this round's shifts.
    pub round_delivered: i64,
    snapshot: Option<Snapshot>,
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

/// Everything a retry rewinds: the state of the world at round start.
#[derive(Clone)]
struct Snapshot {
    credits: u32,
    board: Vec<Placement>,
    hand: Vec<Card>,
    bay_queues: Vec<Vec<(ItemType, u32, i32)>>,
    carry: Vec<SeedItem>,
}

impl Game {
    pub fn new(seed: u32) -> Game {
        let rng = Rng::new(seed);
        // Vault east, two loading bays west: flow has a direction.
        let board = vec![
            Placement::new(BOARD_W - 1, BOARD_H / 2, MachineId::Vault, None),
            Placement::new(0, BOARD_H / 2 - 3, MachineId::Bay, Some(Dir::E)),
            Placement::new(0, BOARD_H / 2 + 3, MachineId::Bay, Some(Dir::E)),
        ];
        // Head office sends a starter consignment, split across the docks.
        let bay_queues = vec![
            vec![(ItemType::Ore, 70, 0)],
            vec![(ItemType::Ore, 50, 0)],
        ];
        let mut g = Game {
            round: 0,
            credits: STARTING_CREDITS,
            phase: GamePhase::Build,
            board,
            hand: starting_hand(),
            offers: Vec::new(),
            directives: Vec::new(),
            contracts: Vec::new(),
            lot_offers: Vec::new(),
            bay_queues,
            carry: Vec::new(),
            shifts_used: 0,
            round_delivered: 0,
            snapshot: None,
            market: ItemType::Ore, // rolled properly just below
            audit_tag: None,
            unlocked: default_unlocked(),
            history: Vec::new(),
            rerolls: 0,
            rng,
            seed,
        };
        g.roll_conditions(0);
        g.take_snapshot();
        g
    }

    /// The bays, in board order — queue index i belongs to the i-th bay.
    pub fn bays(&self) -> Vec<(i32, i32)> {
        self.board.iter().filter(|p| p.m == MachineId::Bay).map(|p| (p.x, p.y)).collect()
    }

    fn take_snapshot(&mut self) {
        self.snapshot = Some(Snapshot {
            credits: self.credits,
            board: self.board.clone(),
            hand: self.hand.clone(),
            bay_queues: self.bay_queues.clone(),
            carry: self.carry.clone(),
        });
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

    /// Place a scrap chute — swallows anything, pays nothing. Slag disposal.
    pub fn buy_chute(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.buy_infra(x, y, MachineId::Chute, None, None)
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
            .position(|p| {
                p.x == x && p.y == y && !matches!(p.m, MachineId::Vault | MachineId::Bay)
            })
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
            .find(|p| p.x == x && p.y == y && !matches!(p.m, MachineId::Vault | MachineId::Bay))
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

    /// Set (or clear) a Filter's item-type gate — the sorting-spine primitive.
    pub fn set_filter_type(&mut self, x: i32, y: i32, ty: Option<ItemType>) -> Result<(), String> {
        let p = self.board_mut(x, y)?;
        if p.m != MachineId::Filter {
            return Err("not a filter".into());
        }
        let mut cfg = p.cfg.unwrap_or_default();
        cfg.item_type = ty;
        p.cfg = Some(cfg);
        Ok(())
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
            .filter(|(_, p)| {
                !matches!(p.m, MachineId::Vault | MachineId::Bay) && tiles.contains(&(p.x, p.y))
            })
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

    /// How long this run's shifts are (contracts can stretch them).
    pub fn shift_ticks(&self) -> u32 {
        SHIFT_TICKS + if self.contracts.contains(&ContractId::NightShifts) { 8 } else { 0 }
    }

    /// The shift as a steppable sim — same board, seed, carry, queues,
    /// directives, contracts, market and audit as `run_shift`, so a renderer
    /// can animate tick by tick and the committed result is identical.
    pub fn shift_sim(&self) -> Result<Sim, String> {
        let mut sim = Sim::new(BOARD_W, BOARD_H, &self.board, self.shift_seed())?;
        // the warm factory: whatever was in the pipes is still in the pipes
        sim.seed_items(&self.carry);
        // bay queues stream from the docks, in order
        for ((x, y), queue) in self.bays().into_iter().zip(&self.bay_queues) {
            let mut seeds = Vec::new();
            for &(ty, count, quality) in queue {
                for _ in 0..count {
                    seeds.push(SeedItem { x, y, buffered: true, ty, quality });
                }
            }
            sim.seed_items(&seeds);
        }
        sim.apply_directives(&self.directives);
        sim.set_demand(self.market, MARKET_MULT);
        if self.contracts.contains(&ContractId::GearSyndicate) {
            sim.set_demand(ItemType::Gear, 1.5);
        }
        sim.purist = self.contracts.contains(&ContractId::PuristClause);
        sim.sap_decay = !self.contracts.contains(&ContractId::SweetTooth);
        sim.crystal_crack = !self.contracts.contains(&ContractId::GentleHands);
        if self.contracts.contains(&ContractId::FluxInjector) {
            sim.flux_bonus = 3;
        }
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
            .wrapping_add((self.round as u32) * 8 + self.shifts_used)
    }

    /// Run ONE shift of the round. Deliveries bank toward the quota; the
    /// board's material state carries into the next shift. The round ends
    /// when the quota is met (spare shifts pay a bonus) or the shifts are
    /// spent (the run ends, retry available).
    pub fn run_shift(&mut self) -> Result<&RoundOutcome, String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        if self.shifts_used >= SHIFTS_PER_ROUND {
            return Err("no shifts left this round".into());
        }
        let mut sim = self.shift_sim()?;
        let result = sim.run(self.shift_ticks());

        // Capture the warm state: bay leftovers go back to their queues,
        // everything else carries as loose material.
        let bays = self.bays();
        let mut carry = Vec::new();
        for q in self.bay_queues.iter_mut() {
            q.clear();
        }
        for seed in sim.export_state() {
            match bays.iter().position(|&(bx, by)| (bx, by) == (seed.x, seed.y)) {
                Some(b) if seed.buffered => {
                    // compress runs of identical items back into queue form
                    match self.bay_queues[b].last_mut() {
                        Some(e) if e.0 == seed.ty && e.2 == seed.quality => e.1 += 1,
                        _ => self.bay_queues[b].push((seed.ty, 1, seed.quality)),
                    }
                }
                _ => carry.push(seed),
            }
        }
        self.carry = carry;

        self.shifts_used += 1;
        self.round_delivered += result.payout;
        let quota = self.quota();
        let cleared = self.round_delivered >= quota;
        self.history.push(RoundOutcome { round: self.round, result, quota, cleared });

        if cleared {
            let surplus = (self.round_delivered - quota) as u32;
            let spare = SHIFTS_PER_ROUND - self.shifts_used;
            self.credits += surplus + spare * (quota as u32 / 10);
            // round_delivered stays visible through the shop; shop_done resets
            if self.round + 1 >= QUOTAS.len() {
                self.phase = GamePhase::Over { won: true };
            } else {
                self.offers = shop_rack(&self.unlocked, SHOP_SIZE, &mut self.rng);
                self.lot_offers = (0..3)
                    .map(|_| roll_lot(self.round + 1, &self.contracts, &mut self.rng))
                    .collect();
                self.rerolls = 0;
                self.roll_conditions(self.round + 1);
                self.phase = GamePhase::Shop;
            }
        } else if self.shifts_used >= SHIFTS_PER_ROUND {
            self.phase = GamePhase::Over { won: false };
        }
        // else: still Build — rearrange and run the next shift
        Ok(self.history.last().unwrap())
    }

    /// A missed quota isn't the end while you're iterating: rewind the whole
    /// round — board, hand, credits, queues, warm material — to how it stood
    /// when the round began, and try again.
    pub fn retry_round(&mut self) -> Result<(), String> {
        if self.phase != (GamePhase::Over { won: false }) {
            return Err("nothing to retry".into());
        }
        let snap = self.snapshot.clone().ok_or("no snapshot to restore")?;
        self.credits = snap.credits;
        self.board = snap.board;
        self.hand = snap.hand;
        self.bay_queues = snap.bay_queues;
        self.carry = snap.carry;
        self.shifts_used = 0;
        self.round_delivered = 0;
        self.phase = GamePhase::Build;
        Ok(())
    }

    /// Buy the shipment at `lot_idx`, queueing it at bay `bay_idx`. The bay
    /// choice IS the input decision: what arrives where, in what order.
    pub fn buy_lot(&mut self, lot_idx: usize, bay_idx: usize) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        let lot = self.lot_offers.get(lot_idx).ok_or("no such shipment")?.clone();
        if bay_idx >= self.bay_queues.len() {
            return Err("no such bay".into());
        }
        if self.credits < lot.price {
            return Err("cannot afford that shipment".into());
        }
        self.credits -= lot.price;
        self.lot_offers.remove(lot_idx);
        for (ty, count) in lot.entries {
            self.bay_queues[bay_idx].push((ty, count, lot.quality));
        }
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
            Offer::Contract(c) => self.contracts.push(c),
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
        self.lot_offers =
            (0..3).map(|_| roll_lot(self.round + 1, &self.contracts, &mut self.rng)).collect();
        Ok(())
    }

    /// Leave the shop and start the next round.
    pub fn shop_done(&mut self) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        self.offers.clear();
        self.lot_offers.clear();
        self.round += 1;
        self.shifts_used = 0;
        self.round_delivered = 0;
        self.phase = GamePhase::Build;
        self.take_snapshot();
        Ok(())
    }
}

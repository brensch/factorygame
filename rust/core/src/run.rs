//! Run structure: rounds, quotas, credits, the hand, the shop, the
//! build/shift loop.
//!
//! This is the layer a UI (browser/WASM now, Bevy later) and the lab bots both
//! drive. It owns no rendering and no policy — it exposes legal actions and
//! applies them.

use crate::cards::{default_unlocked, shop_rack, starting_hand, Card, Offer};
use crate::defs::{
    contract, def, shape_cells, ContractId, ContractKind, Dir, DirectiveId, ItemType, MachineId,
    Tag, CONTRACT_POOL, ITEM_TYPES, QUALITY_CAP, TAGS,
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
/// How many consignments a bay can hold queued at once.
pub const BAY_SLOTS: usize = 3;
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

/// The basic deal table: what a Supply Line sends. Ore-heavy, no exotics.
pub fn roll_basic_lot(round: usize, contracts: &[ContractId], rng: &mut Rng) -> Lot {
    let scale = 1.0 + round as f64 * 0.5;
    let q = |n: u32| (n as f64 * scale).round() as u32;
    let mut lot = match rng.below(3) {
        0 => Lot { name: "Bulk Ore", entries: vec![(ItemType::Ore, q(55))], quality: 0, price: 0 },
        1 => Lot {
            name: "Mixed Manifest",
            entries: vec![(ItemType::Ore, q(35)), (ItemType::Sap, q(18))],
            quality: 1,
            price: 0,
        },
        _ => Lot { name: "Fresh Sap", entries: vec![(ItemType::Sap, q(35))], quality: 2, price: 0 },
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
    lot
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamePhase {
    /// Placing machines and belts; hand is live.
    Build,
    /// The between-rounds shop: contracts up top, equipment below.
    Shop,
    /// The start-of-round supply window: buy consignments, slot them at bays.
    Supply,
    /// Run ended: cleared all rounds, or missed quota.
    Over { won: bool },
}

/// A consignment sitting in a bay slot: a name and runs of (type, count,
/// quality), streamed in order.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotLot {
    pub name: String,
    pub runs: Vec<(ItemType, u32, i32)>,
}

/// An owned contract: the deal, plus term-tracking when it isn't ongoing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContractInst {
    pub id: ContractId,
    /// Rounds remaining before a term contract lapses. None = ongoing.
    pub rounds_left: Option<u32>,
    /// Progress toward a term contract's delivery requirement.
    pub progress: u32,
}

impl ContractInst {
    pub fn new(id: ContractId) -> Self {
        let rounds_left = match contract(id).kind {
            ContractKind::Ongoing => None,
            ContractKind::Term { rounds, .. } => Some(rounds),
        };
        Self { id, rounds_left, progress: 0 }
    }
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
    /// Contracts owned this run — the joker layer, biasing input and output,
    /// granting free consignments, or demanding deliveries on a deadline.
    pub contracts: Vec<ContractInst>,
    /// Contracts on offer while the shop is open (their own shelf, up top).
    pub contract_offers: Vec<ContractId>,
    /// Shipments on offer while the supply window is open.
    pub lot_offers: Vec<Lot>,
    /// Unallocated supply cards: bought or dealt, waiting to be slotted.
    pub supply_hand: Vec<SlotLot>,
    /// Each bay's slots: up to [`BAY_SLOTS`] allocated cards, streamed in
    /// order and CONSUMED by running a shift.
    pub bay_slots: Vec<Vec<SlotLot>>,
    /// Items that physically streamed into a bay but weren't processed —
    /// they persist (warm), but their card is already spent.
    pub bay_hoppers: Vec<Vec<(ItemType, u32, i32)>>,
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
    supply_hand: Vec<SlotLot>,
    bay_slots: Vec<Vec<SlotLot>>,
    bay_hoppers: Vec<Vec<(ItemType, u32, i32)>>,
    carry: Vec<SeedItem>,
    contracts: Vec<ContractInst>,
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
        let bay_slots = vec![Vec::new(), Vec::new()];
        let mut g = Game {
            round: 0,
            credits: STARTING_CREDITS,
            phase: GamePhase::Supply,
            board,
            hand: starting_hand(),
            offers: Vec::new(),
            directives: Vec::new(),
            contracts: vec![ContractInst::new(ContractId::SupplyLine)],
            contract_offers: Vec::new(),
            lot_offers: Vec::new(),
            supply_hand: Vec::new(),
            bay_slots,
            bay_hoppers: vec![Vec::new(), Vec::new()],
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
        g.deal_supply();
        g.lot_offers = (0..3).map(|_| roll_lot(0, &[], &mut g.rng)).collect();
        g
    }

    /// The contract-driven deal: every round, owned contracts put supply
    /// cards straight into your hand. This is the primary input faucet.
    fn deal_supply(&mut self) {
        let ids = self.contract_ids();
        if self.has_contract(ContractId::SupplyLine) {
            for _ in 0..2 {
                // round 1 teaches: pure ore, no routing traps. Mixed lots
                // (sap in an ore line jams a naive build) start at round 2.
                let lot = if self.round == 0 {
                    Lot { name: "Bulk Ore", entries: vec![(ItemType::Ore, 55)], quality: 0, price: 0 }
                } else {
                    roll_basic_lot(self.round, &ids, &mut self.rng)
                };
                self.supply_hand.push(SlotLot {
                    name: lot.name.into(),
                    runs: lot.entries.iter().map(|&(t, n)| (t, n, lot.quality)).collect(),
                });
            }
        }
        if self.has_contract(ContractId::OreRetainer) {
            self.supply_hand.push(SlotLot {
                name: "Retainer Ore".into(),
                runs: vec![(ItemType::Ore, 30, 0)],
            });
        }
        if self.has_contract(ContractId::Prospector) && self.rng.next_f64() < 0.5 {
            self.supply_hand.push(SlotLot {
                name: "Prospector Crystal".into(),
                runs: vec![(ItemType::Crystal, 10, 2)],
            });
        }
    }

    /// Allocate a supply card from the hand into a bay's next free slot.
    pub fn allocate(&mut self, supply_idx: usize, bay_idx: usize) -> Result<(), String> {
        if !matches!(self.phase, GamePhase::Build | GamePhase::Supply) {
            return Err("cannot allocate now".into());
        }
        if supply_idx >= self.supply_hand.len() {
            return Err("no such supply card".into());
        }
        if bay_idx >= self.bay_slots.len() {
            return Err("no such bay".into());
        }
        if self.bay_slots[bay_idx].len() >= BAY_SLOTS {
            return Err("that bay's slots are full".into());
        }
        let lot = self.supply_hand.remove(supply_idx);
        self.bay_slots[bay_idx].push(lot);
        Ok(())
    }

    /// Pull a slotted card back into the supply hand.
    pub fn unslot(&mut self, bay_idx: usize, slot_idx: usize) -> Result<(), String> {
        if !matches!(self.phase, GamePhase::Build | GamePhase::Supply) {
            return Err("cannot rearrange now".into());
        }
        let slots =
            self.bay_slots.get_mut(bay_idx).ok_or("no such bay")?;
        if slot_idx >= slots.len() {
            return Err("no such slot".into());
        }
        let lot = slots.remove(slot_idx);
        self.supply_hand.push(lot);
        Ok(())
    }

    /// Move a slotted card up one place in its bay's streaming order.
    pub fn slot_up(&mut self, bay_idx: usize, slot_idx: usize) -> Result<(), String> {
        if !matches!(self.phase, GamePhase::Build | GamePhase::Supply) {
            return Err("cannot rearrange now".into());
        }
        let slots = self.bay_slots.get_mut(bay_idx).ok_or("no such bay")?;
        if slot_idx == 0 || slot_idx >= slots.len() {
            return Err("cannot move that".into());
        }
        slots.swap(slot_idx, slot_idx - 1);
        Ok(())
    }

    /// Does the run hold a contract of this kind (any instance)?
    pub fn has_contract(&self, id: ContractId) -> bool {
        self.contracts.iter().any(|c| c.id == id)
    }

    fn contract_ids(&self) -> Vec<ContractId> {
        self.contracts.iter().map(|c| c.id).collect()
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
            supply_hand: self.supply_hand.clone(),
            bay_slots: self.bay_slots.clone(),
            bay_hoppers: self.bay_hoppers.clone(),
            carry: self.carry.clone(),
            contracts: self.contracts.clone(),
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

    /// Every cell a placement covers, shape-expanded.
    pub fn cells_of(p: &Placement) -> Vec<(i32, i32)> {
        shape_cells(p.m, p.x, p.y, p.d.unwrap_or(Dir::E))
    }

    fn occupied(&self, x: i32, y: i32) -> bool {
        self.board.iter().any(|p| Self::cells_of(p).contains(&(x, y)))
    }

    /// Index of the placement covering (x, y), if any.
    fn placement_at(&self, x: i32, y: i32) -> Option<usize> {
        self.board.iter().position(|p| Self::cells_of(p).contains(&(x, y)))
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
        let probe = Placement::new(x, y, card.machine, d);
        for (cx, cy) in Self::cells_of(&probe) {
            if !Self::in_bounds(cx, cy) || self.occupied(cx, cy) {
                return Err(format!("tile {cx},{cy} unavailable"));
            }
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
            .placement_at(x, y)
            .filter(|&i| !matches!(self.board[i].m, MachineId::Vault | MachineId::Bay))
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
        let i = self
            .placement_at(x, y)
            .filter(|&i| !matches!(self.board[i].m, MachineId::Vault | MachineId::Bay))
            .ok_or_else(|| format!("nothing editable at {x},{y}"))?;
        Ok(&mut self.board[i])
    }

    /// Rotate a placed machine clockwise: its output edge, and for shaped
    /// machines the whole body — refused if the rotated body doesn't fit.
    pub fn rotate(&mut self, x: i32, y: i32) -> Result<(), String> {
        if self.phase != GamePhase::Build {
            return Err("not in build phase".into());
        }
        let i = self
            .placement_at(x, y)
            .filter(|&i| !matches!(self.board[i].m, MachineId::Vault | MachineId::Bay))
            .ok_or_else(|| format!("nothing editable at {x},{y}"))?;
        let Some(d) = self.board[i].d else {
            return Err("machine has no output edge".into());
        };
        let mut probe = self.board[i];
        probe.d = Some(d.turn_cw());
        for (cx, cy) in Self::cells_of(&probe) {
            if !Self::in_bounds(cx, cy) {
                return Err("no room to rotate".into());
            }
            if let Some(j) = self.placement_at(cx, cy) {
                if j != i {
                    return Err("no room to rotate".into());
                }
            }
        }
        self.board[i].d = probe.d;
        Ok(())
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
                !matches!(p.m, MachineId::Vault | MachineId::Bay)
                    && Self::cells_of(p).iter().any(|c| tiles.contains(c))
            })
            .map(|(i, _)| i)
            .collect();
        if moving.is_empty() {
            return Err("nothing movable selected".into());
        }
        let stationary: Vec<(i32, i32)> = self
            .board
            .iter()
            .enumerate()
            .filter(|(j, _)| !moving.contains(j))
            .flat_map(|(_, q)| Self::cells_of(q))
            .collect();
        for &i in &moving {
            for (cx, cy) in Self::cells_of(&self.board[i]) {
                let (nx, ny) = (cx + dx, cy + dy);
                if !Self::in_bounds(nx, ny) {
                    return Err(format!("move would leave the board at {nx},{ny}"));
                }
                if stationary.contains(&(nx, ny)) {
                    return Err(format!("tile {nx},{ny} is occupied"));
                }
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
        SHIFT_TICKS + if self.has_contract(ContractId::NightShifts) { 8 } else { 0 }
    }

    /// The shift as a steppable sim — same board, seed, carry, queues,
    /// directives, contracts, market and audit as `run_shift`, so a renderer
    /// can animate tick by tick and the committed result is identical.
    pub fn shift_sim(&self) -> Result<Sim, String> {
        let mut sim = Sim::new(BOARD_W, BOARD_H, &self.board, self.shift_seed())?;
        // the warm factory: whatever was in the pipes is still in the pipes
        sim.seed_items(&self.carry);
        // each bay streams its hopper (already-delivered material) first,
        // then this round's slotted cards, in slot order
        for (b, (x, y)) in self.bays().into_iter().enumerate() {
            let mut seeds = Vec::new();
            for &(ty, count, quality) in &self.bay_hoppers[b] {
                for _ in 0..count {
                    seeds.push(SeedItem { x, y, buffered: true, ty, quality });
                }
            }
            for lot in &self.bay_slots[b] {
                for &(ty, count, quality) in &lot.runs {
                    for _ in 0..count {
                        seeds.push(SeedItem { x, y, buffered: true, ty, quality });
                    }
                }
            }
            sim.seed_items(&seeds);
        }
        sim.apply_directives(&self.directives);
        sim.set_demand(self.market, MARKET_MULT);
        if self.has_contract(ContractId::GearSyndicate) {
            sim.set_demand(ItemType::Gear, 1.5);
        }
        sim.purist = self.has_contract(ContractId::PuristClause);
        sim.sap_decay = !self.has_contract(ContractId::SweetTooth);
        sim.crystal_crack = !self.has_contract(ContractId::GentleHands);
        if self.has_contract(ContractId::FluxInjector) {
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

        // The cards are spent: slotted consignments dissolved into the
        // stream when the shift ran. Whatever physically remains in a bay
        // becomes hopper material (warm, but card-less).
        let bays = self.bays();
        let mut carry = Vec::new();
        let mut hoppers: Vec<Vec<(ItemType, u32, i32)>> = vec![Vec::new(); bays.len()];
        for seed in sim.export_state() {
            match bays.iter().position(|&(bx, by)| (bx, by) == (seed.x, seed.y)) {
                Some(b) if seed.buffered => match hoppers[b].last_mut() {
                    Some(e) if e.0 == seed.ty && e.2 == seed.quality => e.1 += 1,
                    _ => hoppers[b].push((seed.ty, 1, seed.quality)),
                },
                _ => carry.push(seed),
            }
        }
        self.carry = carry;
        self.bay_hoppers = hoppers;
        for slots in self.bay_slots.iter_mut() {
            slots.clear();
        }

        // Term contracts count this shift's deliveries toward their targets.
        for c in self.contracts.iter_mut() {
            if let ContractKind::Term { deliver, .. } = contract(c.id).kind {
                c.progress += result.delivered.iter().filter(|d| d.ty == deliver).count() as u32;
            }
        }

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
                self.contract_offers = (0..2)
                    .map(|_| CONTRACT_POOL[self.rng.below(CONTRACT_POOL.len())])
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
        self.supply_hand = snap.supply_hand;
        self.bay_slots = snap.bay_slots;
        self.bay_hoppers = snap.bay_hoppers;
        self.carry = snap.carry;
        self.contracts = snap.contracts;
        self.shifts_used = 0;
        self.round_delivered = 0;
        self.phase = GamePhase::Build;
        Ok(())
    }

    /// Buy the shipment at `lot_idx` as a supply CARD into the hand — the
    /// one-per-round purchase window. Allocation happens on the floor.
    pub fn buy_lot(&mut self, lot_idx: usize) -> Result<(), String> {
        if self.phase != GamePhase::Supply {
            return Err("the supply window is closed".into());
        }
        let lot = self.lot_offers.get(lot_idx).ok_or("no such shipment")?.clone();
        if self.credits < lot.price {
            return Err("cannot afford that shipment".into());
        }
        self.credits -= lot.price;
        self.lot_offers.remove(lot_idx);
        self.supply_hand.push(SlotLot {
            name: lot.name.into(),
            runs: lot.entries.iter().map(|&(ty, n)| (ty, n, lot.quality)).collect(),
        });
        Ok(())
    }

    /// Buy the contract on offer at `idx` — it takes effect immediately.
    pub fn buy_contract(&mut self, idx: usize) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        let id = *self.contract_offers.get(idx).ok_or("no such contract")?;
        let price = priced(contract(id).cost, self.round);
        if self.credits < price {
            return Err("cannot afford that contract".into());
        }
        self.credits -= price;
        self.contract_offers.remove(idx);
        self.contracts.push(ContractInst::new(id));
        Ok(())
    }

    /// Sell an owned contract back for half its current price. Term deals
    /// can be dumped before they lapse; ongoing ones give up their boost.
    pub fn sell_contract(&mut self, idx: usize) -> Result<(), String> {
        if matches!(self.phase, GamePhase::Over { .. }) {
            return Err("run is over".into());
        }
        let inst = *self.contracts.get(idx).ok_or("no such contract")?;
        self.contracts.remove(idx);
        self.credits += priced(contract(inst.id).cost, self.round) / 2;
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
            Offer::Contract(c) => self.contracts.push(ContractInst::new(c)),
        }
        Ok(())
    }

    /// Swap the current offers for fresh ones (works in the shop AND the
    /// supply window). Each reroll in the same visit costs more.
    pub fn shop_reroll(&mut self) -> Result<(), String> {
        if !matches!(self.phase, GamePhase::Shop | GamePhase::Supply) {
            return Err("nothing to reroll".into());
        }
        let price = self.reroll_price();
        if self.credits < price {
            return Err("cannot afford a reroll".into());
        }
        self.credits -= price;
        self.rerolls += 1;
        match self.phase {
            GamePhase::Shop => {
                self.offers = shop_rack(&self.unlocked, SHOP_SIZE, &mut self.rng);
                self.contract_offers = (0..2)
                    .map(|_| CONTRACT_POOL[self.rng.below(CONTRACT_POOL.len())])
                    .collect();
            }
            GamePhase::Supply => {
                let ids = self.contract_ids();
                self.lot_offers =
                    (0..3).map(|_| roll_lot(self.round, &ids, &mut self.rng)).collect();
            }
            _ => {}
        }
        Ok(())
    }

    /// Leave the shop: term contracts settle (fulfil → reward, lapse →
    /// gone), the round advances, and the supply window opens.
    pub fn shop_done(&mut self) -> Result<(), String> {
        if self.phase != GamePhase::Shop {
            return Err("the shop is closed".into());
        }
        self.offers.clear();
        self.contract_offers.clear();
        // settle term contracts at the round boundary
        let mut reward_total = 0u32;
        self.contracts.retain_mut(|c| {
            let ContractKind::Term { count, reward, .. } = contract(c.id).kind else {
                return true;
            };
            if c.progress >= count {
                reward_total += reward;
                return false; // fulfilled and paid
            }
            let left = c.rounds_left.unwrap_or(0);
            if left <= 1 {
                return false; // lapsed
            }
            c.rounds_left = Some(left - 1);
            true
        });
        self.credits += reward_total;

        self.round += 1;
        self.shifts_used = 0;
        self.round_delivered = 0;
        self.enter_supply();
        Ok(())
    }

    /// Open the supply window: the contracts deal this round's cards, and
    /// the one-time purchase rack is rolled.
    fn enter_supply(&mut self) {
        self.deal_supply();
        let ids = self.contract_ids();
        self.lot_offers = (0..3).map(|_| roll_lot(self.round, &ids, &mut self.rng)).collect();
        self.phase = GamePhase::Supply;
    }

    /// Close the supply window and take the floor.
    pub fn supply_done(&mut self) -> Result<(), String> {
        if self.phase != GamePhase::Supply {
            return Err("the supply window is closed".into());
        }
        self.lot_offers.clear();
        self.phase = GamePhase::Build;
        self.take_snapshot();
        Ok(())
    }
}

//! Machine and item definitions.
//!
//! Every balance number in the game lives in this module and nowhere else.
//! The simulation reads these and contains no constants of its own.

// ── items ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ItemType {
    Ore, Sap, Crystal,
    Ingot, Resin, Shard,
    Gear, Circuit, Lens,
    Engine, Core, Beacon,
    /// Catalyst: worthless to ship, but wired into any recipe machine it is
    /// consumed with the batch for bonus quality.
    Flux,
    /// Contaminant: worthless, clogs machines, wants filtering into a chute.
    Slag,
}

impl ItemType {
    pub fn base_value(self) -> f64 {
        use ItemType::*;
        match self {
            Ore | Sap | Crystal => 1.0,
            Ingot | Resin | Shard => 4.0,
            Gear | Circuit | Lens => 16.0,
            Engine | Core | Beacon => 64.0,
            Flux => 2.0,
            Slag => 0.0,
        }
    }
}

pub const ITEM_TYPES: [ItemType; 14] = [
    ItemType::Ore, ItemType::Sap, ItemType::Crystal,
    ItemType::Ingot, ItemType::Resin, ItemType::Shard,
    ItemType::Gear, ItemType::Circuit, ItemType::Lens,
    ItemType::Engine, ItemType::Core, ItemType::Beacon,
    ItemType::Flux, ItemType::Slag,
];

pub const TAGS: [Tag; 5] = [Tag::Heat, Tag::Kinetic, Tag::Volt, Tag::Precision, Tag::Organic];

pub const QUALITY_STEP: f64 = 0.25;
pub const QUALITY_CAP: i32 = 10; // raised to 20 by the Overengineered relic

/// Chance per pass-through that a Duplicator queues a clone.
pub const DUP_CLONE_CHANCE: f64 = 0.15;

pub fn item_value(t: ItemType, quality: i32) -> f64 {
    t.base_value() * (1.0 + QUALITY_STEP * quality as f64)
}

// ── geometry ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dir {
    N, S, E, W,
}

impl Dir {
    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::N => (0, -1),
            Dir::S => (0, 1),
            Dir::E => (1, 0),
            Dir::W => (-1, 0),
        }
    }
    pub const ALL: [Dir; 4] = [Dir::N, Dir::S, Dir::E, Dir::W];

    pub fn turn_cw(self) -> Dir {
        match self {
            Dir::N => Dir::E,
            Dir::E => Dir::S,
            Dir::S => Dir::W,
            Dir::W => Dir::N,
        }
    }
}

// ── machines ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tag {
    Heat, Kinetic, Volt, Precision, Organic,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Extractor, Processor, Assembler, Logistics, Modifier, Vault,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MachineId {
    Drill, Tap, Geode,
    Furnace, Retort, Lapidary, Compress,
    Fab, CircuitBench, LensGrinder, EngineWorks,
    Belt, Junction, Merger, Splitter, Buffer, Filter,
    Overclock, Polisher, Heatsink, Dup,
    Vault, Bay, Chute,
}

/// One port on a machine shape: which cell, which edge of that cell.
/// Defined in the shape's default (east-facing) orientation.
pub struct Port {
    pub dx: i32,
    pub dy: i32,
    pub edge: Dir,
}

/// A multi-cell machine body. Items may only enter through `ins` and the
/// single finished-goods edge is `out`. Cells are offsets from the anchor.
pub struct Shape {
    pub cells: &'static [(i32, i32)],
    pub ins: &'static [Port],
    pub out: Port,
}

/// Rotate a cell offset from default (E) orientation to `r`.
pub fn rot_cell(dx: i32, dy: i32, r: Dir) -> (i32, i32) {
    match r {
        Dir::E => (dx, dy),
        Dir::S => (-dy, dx),
        Dir::W => (-dx, -dy),
        Dir::N => (dy, -dx),
    }
}

/// Rotate an edge direction from default (E) orientation to `r`.
pub fn rot_edge(d: Dir, r: Dir) -> Dir {
    let steps = match r {
        Dir::E => 0,
        Dir::S => 1,
        Dir::W => 2,
        Dir::N => 3,
    };
    let mut out = d;
    for _ in 0..steps {
        out = out.turn_cw();
    }
    out
}

/// Absolute cells of a shape at (x, y) with orientation `r`, normalized so
/// the anchor stays the top-left of the rotated bounding box.
pub fn shape_cells(m: MachineId, x: i32, y: i32, r: Dir) -> Vec<(i32, i32)> {
    match def(m).shape {
        None => vec![(x, y)],
        Some(sh) => {
            let rot: Vec<(i32, i32)> =
                sh.cells.iter().map(|&(dx, dy)| rot_cell(dx, dy, r)).collect();
            let minx = rot.iter().map(|c| c.0).min().unwrap();
            let miny = rot.iter().map(|c| c.1).min().unwrap();
            rot.iter().map(|&(cx, cy)| (x + cx - minx, y + cy - miny)).collect()
        }
    }
}

/// An absolute port: (cell x, cell y, outward edge).
pub type AbsPort = (i32, i32, Dir);

/// Absolute ports of a shape at (x, y) with orientation `r`:
/// (input ports, output port) — 1×1 machines return empty/None here and use
/// their legacy any-edge behaviour.
pub fn shape_ports(m: MachineId, x: i32, y: i32, r: Dir) -> (Vec<AbsPort>, Option<AbsPort>) {
    let Some(sh) = def(m).shape else { return (Vec::new(), None) };
    let rot: Vec<(i32, i32)> = sh.cells.iter().map(|&(dx, dy)| rot_cell(dx, dy, r)).collect();
    let minx = rot.iter().map(|c| c.0).min().unwrap();
    let miny = rot.iter().map(|c| c.1).min().unwrap();
    let place = |p: &Port| {
        let (cx, cy) = rot_cell(p.dx, p.dy, r);
        (x + cx - minx, y + cy - miny, rot_edge(p.edge, r))
    };
    (sh.ins.iter().map(place).collect(), Some(place(&sh.out)))
}

pub struct Recipe {
    /// Item types consumed. Repeats mean "two of these".
    pub inputs: &'static [ItemType],
    pub output: ItemType,
    /// Cycle length in ticks at speed 1.0.
    pub ticks: f64,
}

#[derive(Clone, Copy)]
pub struct Aura {
    /// Multiplier applied to a neighbour's work rate. 4/3 == 0.75x cycle time.
    pub speed: f64,
    /// Quality added to a neighbour's output.
    pub quality_out: i32,
    /// Neighbour becomes jam-immune.
    pub no_jam: bool,
    /// If set, the aura only applies to neighbours carrying this tag.
    pub only_tag: Option<Tag>,
}

pub struct MachineDef {
    pub id: MachineId,
    pub name: &'static str,
    pub kind: Kind,
    pub cost: u32,
    pub tags: &'static [Tag],
    /// Extractor: produces item every `period` ticks, from nothing.
    pub produces: Option<ItemType>,
    pub period: f64,
    pub spawn_quality: i32,
    pub recipe: Option<Recipe>,
    /// Belt-like: holds exactly one item and passes it along.
    pub transport: bool,
    /// Multi-cell body with located ports; None = a plain 1×1 machine.
    pub shape: Option<&'static Shape>,
    /// Polisher: quality added to every item passing through.
    pub quality_bonus: i32,
    pub aura: Option<Aura>,
}

const BASE: MachineDef = MachineDef {
    id: MachineId::Belt,
    name: "",
    kind: Kind::Logistics,
    cost: 0,
    tags: &[],
    produces: None,
    period: 0.0,
    spawn_quality: 0,
    recipe: None,
    transport: false,
    shape: None,
    quality_bonus: 0,
    aura: None,
};

// ── the shapes: the factory gets a silhouette ────────────────────────────────
const SHAPE_FURNACE: Shape = Shape {
    cells: &[(0, 0), (1, 0)],
    ins: &[Port { dx: 0, dy: 0, edge: Dir::W }],
    out: Port { dx: 1, dy: 0, edge: Dir::E },
};
const SHAPE_LAPIDARY: Shape = Shape {
    cells: &[(0, 0), (0, 1)],
    ins: &[Port { dx: 0, dy: 0, edge: Dir::N }],
    out: Port { dx: 0, dy: 1, edge: Dir::S },
};
const SHAPE_COMPRESS: Shape = Shape {
    cells: &[(0, 0), (1, 0), (0, 1), (1, 1)],
    ins: &[Port { dx: 0, dy: 0, edge: Dir::W }, Port { dx: 0, dy: 1, edge: Dir::W }],
    out: Port { dx: 1, dy: 0, edge: Dir::E },
};
const SHAPE_FAB: Shape = Shape {
    cells: &[(0, 0), (1, 0), (0, 1), (1, 1)],
    ins: &[Port { dx: 0, dy: 0, edge: Dir::W }, Port { dx: 0, dy: 1, edge: Dir::W }],
    out: Port { dx: 1, dy: 1, edge: Dir::E },
};
const SHAPE_CIRCUIT: Shape = Shape {
    cells: &[(0, 0), (1, 0), (1, 1)],
    ins: &[Port { dx: 0, dy: 0, edge: Dir::W }, Port { dx: 1, dy: 1, edge: Dir::S }],
    out: Port { dx: 1, dy: 0, edge: Dir::E },
};
const SHAPE_LENS: Shape = Shape {
    cells: &[(0, 0), (0, 1), (1, 1)],
    ins: &[Port { dx: 0, dy: 0, edge: Dir::N }, Port { dx: 0, dy: 1, edge: Dir::W }],
    out: Port { dx: 1, dy: 1, edge: Dir::E },
};
const SHAPE_ENGINE: Shape = Shape {
    cells: &[(0, 0), (1, 0), (2, 0), (1, 1)],
    ins: &[Port { dx: 0, dy: 0, edge: Dir::W }, Port { dx: 2, dy: 0, edge: Dir::E }],
    out: Port { dx: 1, dy: 1, edge: Dir::S },
};

use ItemType as I;
use MachineId as M;
use Tag as T;

pub fn def(id: MachineId) -> &'static MachineDef {
    match id {
        // ── extractors ──────────────────────────────────────────────────────
        M::Drill => &MachineDef { id: M::Drill, name: "Drill", kind: Kind::Extractor, cost: 3,
            tags: &[T::Kinetic], produces: Some(I::Ore), period: 4.0, ..BASE },
        M::Tap => &MachineDef { id: M::Tap, name: "Sap Tap", kind: Kind::Extractor, cost: 4,
            tags: &[T::Organic], produces: Some(I::Sap), period: 6.0, spawn_quality: 1, ..BASE },
        M::Geode => &MachineDef { id: M::Geode, name: "Geode Cracker", kind: Kind::Extractor, cost: 8,
            tags: &[T::Precision], produces: Some(I::Crystal), period: 10.0, ..BASE },

        // ── processors ──────────────────────────────────────────────────────
        M::Furnace => &MachineDef { id: M::Furnace, name: "Furnace", kind: Kind::Processor, cost: 5,
            tags: &[T::Heat, T::Kinetic], shape: Some(&SHAPE_FURNACE),
            recipe: Some(Recipe { inputs: &[I::Ore], output: I::Ingot, ticks: 3.0 }), ..BASE },
        M::Retort => &MachineDef { id: M::Retort, name: "Retort", kind: Kind::Processor, cost: 5,
            tags: &[T::Heat, T::Organic], shape: Some(&SHAPE_FURNACE),
            recipe: Some(Recipe { inputs: &[I::Sap], output: I::Resin, ticks: 3.0 }), ..BASE },
        M::Lapidary => &MachineDef { id: M::Lapidary, name: "Lapidary", kind: Kind::Processor, cost: 9,
            tags: &[T::Precision], shape: Some(&SHAPE_LAPIDARY),
            recipe: Some(Recipe { inputs: &[I::Crystal], output: I::Shard, ticks: 5.0 }), ..BASE },
        M::Compress => &MachineDef { id: M::Compress, name: "Compressor", kind: Kind::Processor, cost: 14,
            tags: &[T::Kinetic], shape: Some(&SHAPE_COMPRESS),
            recipe: Some(Recipe { inputs: &[I::Ore, I::Ore, I::Ore, I::Ore], output: I::Ingot, ticks: 4.0 }), ..BASE },

        // ── assemblers ──────────────────────────────────────────────────────
        M::Fab => &MachineDef { id: M::Fab, name: "Fabricator", kind: Kind::Assembler, cost: 12,
            tags: &[T::Kinetic, T::Volt], shape: Some(&SHAPE_FAB),
            recipe: Some(Recipe { inputs: &[I::Ingot, I::Ingot], output: I::Gear, ticks: 5.0 }), ..BASE },
        M::CircuitBench => &MachineDef { id: M::CircuitBench, name: "Circuit Bench", kind: Kind::Assembler, cost: 16,
            tags: &[T::Volt, T::Precision], shape: Some(&SHAPE_CIRCUIT),
            recipe: Some(Recipe { inputs: &[I::Ingot, I::Shard], output: I::Circuit, ticks: 6.0 }), ..BASE },
        M::LensGrinder => &MachineDef { id: M::LensGrinder, name: "Lens Grinder", kind: Kind::Assembler, cost: 16,
            tags: &[T::Precision], shape: Some(&SHAPE_LENS),
            recipe: Some(Recipe { inputs: &[I::Shard, I::Resin], output: I::Lens, ticks: 6.0 }), ..BASE },
        M::EngineWorks => &MachineDef { id: M::EngineWorks, name: "Engine Works", kind: Kind::Assembler, cost: 30,
            tags: &[T::Kinetic, T::Volt], shape: Some(&SHAPE_ENGINE),
            recipe: Some(Recipe { inputs: &[I::Gear, I::Circuit], output: I::Engine, ticks: 8.0 }), ..BASE },

        // ── logistics ───────────────────────────────────────────────────────
        M::Belt => &MachineDef { id: M::Belt, name: "Belt", kind: Kind::Logistics, cost: 1,
            transport: true, ..BASE },
        // Mindustry-style crossing: items exit the way they entered, so two
        // lanes share the tile without mixing. Infrastructure, like belts.
        M::Junction => &MachineDef { id: M::Junction, name: "Junction", kind: Kind::Logistics, cost: 2,
            transport: true, ..BASE },
        M::Merger => &MachineDef { id: M::Merger, name: "Merger", kind: Kind::Logistics, cost: 4,
            transport: true, ..BASE },
        M::Splitter => &MachineDef { id: M::Splitter, name: "Splitter", kind: Kind::Logistics, cost: 4,
            transport: true, ..BASE },
        M::Buffer => &MachineDef { id: M::Buffer, name: "Buffer", kind: Kind::Logistics, cost: 6,
            transport: true, ..BASE },
        M::Filter => &MachineDef { id: M::Filter, name: "Filter", kind: Kind::Logistics, cost: 6,
            transport: true, ..BASE },

        // ── modifiers ───────────────────────────────────────────────────────
        M::Overclock => &MachineDef { id: M::Overclock, name: "Overclocker", kind: Kind::Modifier, cost: 10,
            tags: &[T::Volt],
            aura: Some(Aura { speed: 1.0 / 0.75, quality_out: 0, no_jam: false, only_tag: None }), ..BASE },
        M::Polisher => &MachineDef { id: M::Polisher, name: "Polisher", kind: Kind::Modifier, cost: 8,
            tags: &[T::Precision], transport: true, quality_bonus: 1, ..BASE },
        M::Heatsink => &MachineDef { id: M::Heatsink, name: "Heat Sink", kind: Kind::Modifier, cost: 9,
            tags: &[T::Heat],
            aura: Some(Aura { speed: 1.0, quality_out: 1, no_jam: true, only_tag: Some(T::Heat) }), ..BASE },
        M::Dup => &MachineDef { id: M::Dup, name: "Duplicator", kind: Kind::Modifier, cost: 20,
            tags: &[T::Volt], transport: true, ..BASE },

        // ── vault, docks, disposal ──────────────────────────────────────────
        M::Vault => &MachineDef { id: M::Vault, name: "Vault", kind: Kind::Vault, cost: 0, ..BASE },
        // The loading bay: streams its assigned shipment queue onto the board,
        // one item per tick. The only source of material in the game.
        M::Bay => &MachineDef { id: M::Bay, name: "Loading Bay", kind: Kind::Logistics, cost: 0, ..BASE },
        // The scrap chute: swallows anything, pays nothing. Where slag goes.
        M::Chute => &MachineDef { id: M::Chute, name: "Scrap Chute", kind: Kind::Logistics, cost: 2, ..BASE },
    }
}

/// Machines that can appear as blueprint cards. Routing primitives — belts,
/// junctions, mergers, splitters — are infrastructure, always available and
/// paid per tile; Vaults are part of the board.
pub const CARD_POOL: &[MachineId] = &[
    M::Furnace, M::Retort, M::Lapidary, M::Compress,
    M::Fab, M::CircuitBench, M::LensGrinder, M::EngineWorks,
    M::Buffer, M::Filter,
    M::Overclock, M::Polisher, M::Heatsink, M::Dup,
];

// ── directives ───────────────────────────────────────────────────────────────
// Permanent run-wide buffs, keyed to a machine tag. Bought in the shop, never
// placed, stack without limit. These are the route-commitment mechanic: every
// directive you own makes machines of its tag better, which makes the next
// shop's tagged offers more attractive than raw throughput.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DirectiveId {
    Superheater, Flywheel, Overvolt, FineTolerances, Enrichment,
}

pub struct DirectiveDef {
    pub id: DirectiveId,
    pub name: &'static str,
    pub tag: Tag,
    /// Work-rate multiplier for machines carrying the tag.
    pub speed: f64,
    /// Quality added to tagged machines' outputs.
    pub quality_out: i32,
    /// Quality added per pass on tagged transport (Polisher's stat).
    pub quality_transport: i32,
    /// Base shop price, before the round multiplier.
    pub cost: u32,
    pub blurb: &'static str,
}

pub const DIRECTIVE_POOL: [DirectiveId; 5] = [
    DirectiveId::Superheater, DirectiveId::Flywheel, DirectiveId::Overvolt,
    DirectiveId::FineTolerances, DirectiveId::Enrichment,
];

pub fn directive(id: DirectiveId) -> &'static DirectiveDef {
    use DirectiveId as D;
    match id {
        D::Superheater => &DirectiveDef { id: D::Superheater, name: "Superheater", tag: T::Heat,
            speed: 1.3, quality_out: 0, quality_transport: 0, cost: 12,
            blurb: "All HEAT machines work 30% faster, permanently. Stacks. The furnace-rush doctrine." },
        D::Flywheel => &DirectiveDef { id: D::Flywheel, name: "Flywheel", tag: T::Kinetic,
            speed: 1.3, quality_out: 0, quality_transport: 0, cost: 12,
            blurb: "All KINETIC machines work 30% faster, permanently. Stacks. Drills, presses and fabricators spin up." },
        D::Overvolt => &DirectiveDef { id: D::Overvolt, name: "Overvolt", tag: T::Volt,
            speed: 1.3, quality_out: 0, quality_transport: 0, cost: 12,
            blurb: "All VOLT machines work 30% faster, permanently. Stacks. The assembler-and-aura doctrine." },
        D::FineTolerances => &DirectiveDef { id: D::FineTolerances, name: "Fine Tolerances", tag: T::Precision,
            speed: 1.0, quality_out: 1, quality_transport: 1, cost: 15,
            blurb: "PRECISION machines gain +1 output quality, and Polishers polish +1 more per pass. Stacks. The loop doctrine." },
        D::Enrichment => &DirectiveDef { id: D::Enrichment, name: "Enrichment", tag: T::Organic,
            speed: 1.0, quality_out: 1, quality_transport: 0, cost: 15,
            blurb: "ORGANIC machines gain +1 output quality, permanently. Stacks. Sap runs rich." },
    }
}

// ── contracts ────────────────────────────────────────────────────────────────
// The joker layer: permanent run-wide deals that bias what comes in and what
// it pays going out. Bought in the shop, never placed, visible all run.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ContractId {
    TarSands, BulkManifests, SweetTooth, GentleHands,
    GearSyndicate, PuristClause, FluxInjector, NightShifts,
    OreRetainer, Prospector, GearFutures, ResinCall,
}

/// How a contract lives: forever, or as a term deal with a delivery
/// requirement — fulfil it within the term for the reward, or it lapses.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ContractKind {
    Ongoing,
    Term { rounds: u32, deliver: ItemType, count: u32, reward: u32 },
}

pub struct ContractDef {
    pub id: ContractId,
    pub name: &'static str,
    pub cost: u32,
    pub kind: ContractKind,
    pub blurb: &'static str,
}

pub const CONTRACT_POOL: [ContractId; 12] = [
    ContractId::TarSands, ContractId::BulkManifests, ContractId::SweetTooth,
    ContractId::GentleHands, ContractId::GearSyndicate, ContractId::PuristClause,
    ContractId::FluxInjector, ContractId::NightShifts,
    ContractId::OreRetainer, ContractId::Prospector, ContractId::GearFutures,
    ContractId::ResinCall,
];

pub fn contract(id: ContractId) -> &'static ContractDef {
    use ContractId as C;
    use ContractKind::*;
    match id {
        C::OreRetainer => &ContractDef { id: C::OreRetainer, name: "Ore Retainer", cost: 20, kind: Ongoing,
            blurb: "A standing order: a free 30-ore consignment arrives every round, if a bay slot is open." },
        C::Prospector => &ContractDef { id: C::Prospector, name: "Prospector's Luck", cost: 15, kind: Ongoing,
            blurb: "Every round there's a coin-flip chance a free crystal case turns up at the docks." },
        C::GearFutures => &ContractDef { id: C::GearFutures, name: "Gear Futures", cost: 10,
            kind: Term { rounds: 3, deliver: ItemType::Gear, count: 15, reward: 250 },
            blurb: "Deliver 15 gears within 3 rounds → 250 credits. Miss the window and it lapses, worthless." },
        C::ResinCall => &ContractDef { id: C::ResinCall, name: "Resin Call", cost: 8,
            kind: Term { rounds: 3, deliver: ItemType::Resin, count: 25, reward: 180 },
            blurb: "Deliver 25 resin within 3 rounds → 180 credits. Sap wilts; move fast." },
        C::TarSands => &ContractDef { id: C::TarSands, name: "Tar Sands Deal", cost: 14, kind: Ongoing,
            blurb: "Every ore lot you buy carries +60% more ore — and +20% slag mixed in. Volume has a smell." },
        C::BulkManifests => &ContractDef { id: C::BulkManifests, name: "Bulk Manifests", kind: Ongoing, cost: 18,
            blurb: "Every lot you buy is 30% larger. The paperwork rounds up." },
        C::SweetTooth => &ContractDef { id: C::SweetTooth, name: "Sweet Tooth", kind: Ongoing, cost: 12,
            blurb: "Sap never wilts on your floor. Take your time." },
        C::GentleHands => &ContractDef { id: C::GentleHands, name: "Gentle Hands", kind: Ongoing, cost: 12,
            blurb: "Crystal no longer cracks in mergers, splitters or junctions." },
        C::GearSyndicate => &ContractDef { id: C::GearSyndicate, name: "Gear Syndicate", kind: Ongoing, cost: 16,
            blurb: "The syndicate pays 1.5× for every gear delivered. Permanently. No questions." },
        C::PuristClause => &ContractDef { id: C::PuristClause, name: "Purist Clause", kind: Ongoing, cost: 16,
            blurb: "Deliveries at quality 6+ pay 1.5×. Craftsmanship, rewarded." },
        C::FluxInjector => &ContractDef { id: C::FluxInjector, name: "Flux Injector", kind: Ongoing, cost: 14,
            blurb: "Flux catalyzes +3 quality per batch instead of +2." },
        C::NightShifts => &ContractDef { id: C::NightShifts, name: "Night Shifts", kind: Ongoing, cost: 18,
            blurb: "Every shift runs 8 ticks longer. The union looks the other way." },
    }
}

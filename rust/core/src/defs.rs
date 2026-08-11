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
}

impl ItemType {
    pub fn base_value(self) -> f64 {
        use ItemType::*;
        match self {
            Ore | Sap | Crystal => 1.0,
            Ingot | Resin | Shard => 4.0,
            Gear | Circuit | Lens => 16.0,
            Engine | Core | Beacon => 64.0,
        }
    }
}

pub const ITEM_TYPES: [ItemType; 12] = [
    ItemType::Ore, ItemType::Sap, ItemType::Crystal,
    ItemType::Ingot, ItemType::Resin, ItemType::Shard,
    ItemType::Gear, ItemType::Circuit, ItemType::Lens,
    ItemType::Engine, ItemType::Core, ItemType::Beacon,
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
    Vault,
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
    quality_bonus: 0,
    aura: None,
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
            tags: &[T::Heat, T::Kinetic],
            recipe: Some(Recipe { inputs: &[I::Ore], output: I::Ingot, ticks: 3.0 }), ..BASE },
        M::Retort => &MachineDef { id: M::Retort, name: "Retort", kind: Kind::Processor, cost: 5,
            tags: &[T::Heat, T::Organic],
            recipe: Some(Recipe { inputs: &[I::Sap], output: I::Resin, ticks: 3.0 }), ..BASE },
        M::Lapidary => &MachineDef { id: M::Lapidary, name: "Lapidary", kind: Kind::Processor, cost: 9,
            tags: &[T::Precision],
            recipe: Some(Recipe { inputs: &[I::Crystal], output: I::Shard, ticks: 5.0 }), ..BASE },
        M::Compress => &MachineDef { id: M::Compress, name: "Compressor", kind: Kind::Processor, cost: 14,
            tags: &[T::Kinetic],
            recipe: Some(Recipe { inputs: &[I::Ore, I::Ore, I::Ore, I::Ore], output: I::Ingot, ticks: 4.0 }), ..BASE },

        // ── assemblers ──────────────────────────────────────────────────────
        M::Fab => &MachineDef { id: M::Fab, name: "Fabricator", kind: Kind::Assembler, cost: 12,
            tags: &[T::Kinetic, T::Volt],
            recipe: Some(Recipe { inputs: &[I::Ingot, I::Ingot], output: I::Gear, ticks: 5.0 }), ..BASE },
        M::CircuitBench => &MachineDef { id: M::CircuitBench, name: "Circuit Bench", kind: Kind::Assembler, cost: 16,
            tags: &[T::Volt, T::Precision],
            recipe: Some(Recipe { inputs: &[I::Ingot, I::Shard], output: I::Circuit, ticks: 6.0 }), ..BASE },
        M::LensGrinder => &MachineDef { id: M::LensGrinder, name: "Lens Grinder", kind: Kind::Assembler, cost: 16,
            tags: &[T::Precision],
            recipe: Some(Recipe { inputs: &[I::Shard, I::Resin], output: I::Lens, ticks: 6.0 }), ..BASE },
        M::EngineWorks => &MachineDef { id: M::EngineWorks, name: "Engine Works", kind: Kind::Assembler, cost: 30,
            tags: &[T::Kinetic, T::Volt],
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

        // ── vault ───────────────────────────────────────────────────────────
        M::Vault => &MachineDef { id: M::Vault, name: "Vault", kind: Kind::Vault, cost: 0, ..BASE },
    }
}

/// Machines that can appear as blueprint cards. Routing primitives — belts,
/// junctions, mergers, splitters — are infrastructure, always available and
/// paid per tile; Vaults are part of the board.
pub const CARD_POOL: &[MachineId] = &[
    M::Drill, M::Tap, M::Geode,
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

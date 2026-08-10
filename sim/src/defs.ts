/**
 * Machine and item definitions.
 *
 * Every balance number in the game lives in this file and nowhere else. The
 * simulation in `sim.ts` reads these and contains no constants of its own —
 * that separation is the point, and it is what lets balance be tuned without
 * touching engine code (and, later, be loaded from .tres/JSON in Godot).
 */

export type Dir = "N" | "S" | "E" | "W";
export type Tag = "HEAT" | "KINETIC" | "VOLT" | "PRECISION" | "ORGANIC";
export type Kind =
  | "extractor"
  | "processor"
  | "assembler"
  | "logistics"
  | "modifier"
  | "vault";

export const DIRV: Record<Dir, [number, number]> = {
  N: [0, -1],
  S: [0, 1],
  E: [1, 0],
  W: [-1, 0],
};

/** Base credit value by item type. Final value = base * (1 + 0.25 * quality). */
export const ITEM_VALUE: Record<string, number> = {
  ore: 1, sap: 1, crystal: 1,
  ingot: 4, resin: 4, shard: 4,
  gear: 16, circuit: 16, lens: 16,
  engine: 64, core: 64, beacon: 64,
};

export const QUALITY_STEP = 0.25;
export const QUALITY_CAP = 10; // raised to 20 by the Overengineered relic

export interface Recipe {
  /** Item types consumed. Repeats mean "two of these", e.g. ['ingot','ingot']. */
  inputs: string[];
  output: string;
  /** Cycle length in ticks at speed 1.0. */
  ticks: number;
}

export interface Aura {
  /** Multiplier applied to a neighbour's work rate. 1.333 == 0.75x cycle time. */
  speed?: number;
  /** Quality added to a neighbour's output. */
  qualityOut?: number;
  /** Neighbour becomes jam-immune. */
  noJam?: boolean;
  /** If set, the aura only applies to neighbours carrying this tag. */
  onlyTag?: Tag;
}

export interface MachineDef {
  id: string;
  name: string;
  kind: Kind;
  cost: number;
  tags: Tag[];

  /** Extractor: produces `produces` every `period` ticks, from nothing. */
  produces?: string;
  period?: number;
  spawnQuality?: number;

  /** Processor / assembler. */
  recipe?: Recipe;

  /** Belt-like: holds exactly one item and passes it along. */
  transport?: boolean;
  /** Polisher: quality added to every item passing through. */
  qualityBonus?: number;

  /** Modifier: never touches items, projects onto 4 orthogonal neighbours. */
  aura?: Aura;
}

const M = (d: MachineDef) => d;

export const DEFS: Record<string, MachineDef> = {
  // ── extractors ────────────────────────────────────────────────────────────
  drill: M({ id: "drill", name: "Drill", kind: "extractor", cost: 3,
    tags: ["KINETIC"], produces: "ore", period: 4 }),
  tap: M({ id: "tap", name: "Sap Tap", kind: "extractor", cost: 4,
    tags: ["ORGANIC"], produces: "sap", period: 6, spawnQuality: 1 }),
  geode: M({ id: "geode", name: "Geode Cracker", kind: "extractor", cost: 8,
    tags: ["PRECISION"], produces: "crystal", period: 10 }),

  // ── processors ────────────────────────────────────────────────────────────
  furnace: M({ id: "furnace", name: "Furnace", kind: "processor", cost: 5,
    tags: ["HEAT", "KINETIC"], recipe: { inputs: ["ore"], output: "ingot", ticks: 3 } }),
  retort: M({ id: "retort", name: "Retort", kind: "processor", cost: 5,
    tags: ["HEAT", "ORGANIC"], recipe: { inputs: ["sap"], output: "resin", ticks: 3 } }),
  lapidary: M({ id: "lapidary", name: "Lapidary", kind: "processor", cost: 9,
    tags: ["PRECISION"], recipe: { inputs: ["crystal"], output: "shard", ticks: 5 } }),
  compress: M({ id: "compress", name: "Compressor", kind: "processor", cost: 14,
    tags: ["KINETIC"], recipe: { inputs: ["ore", "ore", "ore", "ore"], output: "ingot", ticks: 4 } }),

  // ── assemblers ────────────────────────────────────────────────────────────
  fab: M({ id: "fab", name: "Fabricator", kind: "assembler", cost: 12,
    tags: ["KINETIC", "VOLT"], recipe: { inputs: ["ingot", "ingot"], output: "gear", ticks: 5 } }),
  circuit: M({ id: "circuit", name: "Circuit Bench", kind: "assembler", cost: 16,
    tags: ["VOLT", "PRECISION"], recipe: { inputs: ["ingot", "shard"], output: "circuit", ticks: 6 } }),
  lens: M({ id: "lens", name: "Lens Grinder", kind: "assembler", cost: 16,
    tags: ["PRECISION"], recipe: { inputs: ["shard", "resin"], output: "lens", ticks: 6 } }),
  engine: M({ id: "engine", name: "Engine Works", kind: "assembler", cost: 30,
    tags: ["KINETIC", "VOLT"], recipe: { inputs: ["gear", "circuit"], output: "engine", ticks: 8 } }),

  // ── logistics ─────────────────────────────────────────────────────────────
  belt: M({ id: "belt", name: "Belt", kind: "logistics", cost: 1, tags: [], transport: true }),
  merger: M({ id: "merger", name: "Merger", kind: "logistics", cost: 4, tags: [], transport: true }),
  splitter: M({ id: "splitter", name: "Splitter", kind: "logistics", cost: 4, tags: [], transport: true }),
  buffer: M({ id: "buffer", name: "Buffer", kind: "logistics", cost: 6, tags: [], transport: true }),
  filter: M({ id: "filter", name: "Filter", kind: "logistics", cost: 6, tags: [], transport: true }),

  // ── modifiers ─────────────────────────────────────────────────────────────
  overclock: M({ id: "overclock", name: "Overclocker", kind: "modifier", cost: 10,
    tags: ["VOLT"], aura: { speed: 1 / 0.75 } }),
  heatsink: M({ id: "heatsink", name: "Heat Sink", kind: "modifier", cost: 9,
    tags: ["HEAT"], aura: { qualityOut: 1, noJam: true, onlyTag: "HEAT" } }),
  polisher: M({ id: "polisher", name: "Polisher", kind: "modifier", cost: 8,
    tags: ["PRECISION"], transport: true, qualityBonus: 1 }),
  dup: M({ id: "dup", name: "Duplicator", kind: "modifier", cost: 20,
    tags: ["VOLT"], transport: true }),

  // ── vault ─────────────────────────────────────────────────────────────────
  vault: M({ id: "vault", name: "Vault", kind: "vault", cost: 0, tags: [] }),
};

export function itemValue(type: string, quality: number): number {
  return (ITEM_VALUE[type] ?? 0) * (1 + QUALITY_STEP * quality);
}

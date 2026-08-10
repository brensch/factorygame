/**
 * OVERFLOW — reference tick simulation.
 *
 * This is the whole game. Everything else is presentation.
 *
 * Deliberate properties, all of which the eventual Godot port must preserve:
 *   - Pure data. No rendering, no engine types, no wall-clock time.
 *   - Deterministic. Same board + same seed => same result, every time.
 *   - Order-independent. Tile iteration order never affects the outcome.
 *
 * The hard part is `transfer()`. See the comment there.
 */

import { DEFS, DIRV, itemValue, QUALITY_CAP, type Dir, type MachineDef } from "./defs";

export interface Item {
  type: string;
  quality: number;
}

/** A machine placed on the board. `d` is the output edge. */
export interface Placement {
  x: number;
  y: number;
  t: string;
  d?: Dir;
}

interface Tile {
  def: MachineDef | null;
  d: Dir | null;
  /** The finished item waiting on the output edge. At most one. */
  out: Item | null;
  /** Items consumed but not yet assembled. */
  inputs: Item[];
  /** Work accumulated toward the current cycle, in ticks. */
  progress: number;
  /** Aura-adjusted work rate. 1.0 is baseline. */
  speed: number;
  /** Aura-granted quality added to this machine's output. */
  qualityOut: number;
  jamImmune: boolean;
}

export interface Delivery {
  tick: number;
  type: string;
  quality: number;
  value: number;
}

export interface ShiftResult {
  ticks: number;
  delivered: Delivery[];
  /** Total credits scored. */
  payout: number;
  /** Items still on belts when the shift ended — these are forfeit. */
  inFlight: number;
  /** Tick counts where at least one machine was blocked on a full output. */
  jamTicks: number;
  byType: Record<string, number>;
}

const OPP: Record<Dir, Dir> = { N: "S", S: "N", E: "W", W: "E" };

export class Sim {
  readonly w: number;
  readonly h: number;
  private tiles: Tile[];
  private rngState: number;
  tick = 0;
  delivered: Delivery[] = [];
  jamTicks = 0;

  constructor(w: number, h: number, placements: Placement[], seed = 0xc0ffee) {
    this.w = w;
    this.h = h;
    this.rngState = seed >>> 0;
    this.tiles = Array.from({ length: w * h }, () => ({
      def: null, d: null, out: null, inputs: [],
      progress: 0, speed: 1, qualityOut: 0, jamImmune: false,
    }));

    for (const p of placements) {
      const def = DEFS[p.t];
      if (!def) throw new Error(`unknown machine '${p.t}' at ${p.x},${p.y}`);
      if (p.x < 0 || p.y < 0 || p.x >= w || p.y >= h)
        throw new Error(`placement out of bounds at ${p.x},${p.y}`);
      const t = this.tiles[this.idx(p.x, p.y)];
      if (t.def) throw new Error(`two machines on tile ${p.x},${p.y}`);
      t.def = def;
      t.d = p.d ?? null;
    }

    this.applyAuras(placements);
  }

  /** Test/debug accessor: the item sitting on a tile's output edge, if any. */
  peek(x: number, y: number): Item | null {
    return this.tiles[this.idx(x, y)].out;
  }

  private idx(x: number, y: number) { return y * this.w + x; }
  private xy(i: number): [number, number] { return [i % this.w, Math.floor(i / this.w)]; }

  /** Deterministic PRNG (mulberry32) so runs are exactly reproducible. */
  private rand(): number {
    this.rngState = (this.rngState + 0x6d2b79f5) >>> 0;
    let t = this.rngState;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  }

  /**
   * Auras are resolved once, at build time, into flat per-tile stats. Nothing
   * during the shift re-reads adjacency — so a tick costs the same on tick 1
   * and tick 60, and aura stacking is unambiguous (multiplicative for speed,
   * additive for quality).
   */
  private applyAuras(placements: Placement[]) {
    for (const p of placements) {
      const src = DEFS[p.t];
      if (!src?.aura) continue;
      const a = src.aura;
      for (const d of Object.keys(DIRV) as Dir[]) {
        const [dx, dy] = DIRV[d];
        const nx = p.x + dx, ny = p.y + dy;
        if (nx < 0 || ny < 0 || nx >= this.w || ny >= this.h) continue;
        const n = this.tiles[this.idx(nx, ny)];
        if (!n.def) continue;
        if (a.onlyTag && !n.def.tags.includes(a.onlyTag)) continue;
        if (a.speed) n.speed *= a.speed;
        if (a.qualityOut) n.qualityOut += a.qualityOut;
        if (a.noJam) n.jamImmune = true;
      }
    }
  }

  private target(i: number): number {
    const t = this.tiles[i];
    if (!t.d) return -1;
    const [x, y] = this.xy(i);
    const [dx, dy] = DIRV[t.d];
    const nx = x + dx, ny = y + dy;
    if (nx < 0 || ny < 0 || nx >= this.w || ny >= this.h) return -1;
    return this.idx(nx, ny);
  }

  /** Can this tile take `item` right now, given its current contents? */
  private canAccept(i: number, item: Item): boolean {
    const t = this.tiles[i];
    if (!t.def) return false;
    const def = t.def;
    if (def.kind === "vault") return true;
    if (def.transport) return t.out === null;
    if (def.recipe) {
      const need = def.recipe.inputs.filter((x) => x === item.type).length;
      if (need === 0) return false;
      const have = t.inputs.filter((x) => x.type === item.type).length;
      return have < need;
    }
    return false; // extractors and pure modifiers never accept
  }

  /** Structural compatibility, ignoring current occupancy. Used to build the flow graph. */
  private couldAccept(i: number, item: Item): boolean {
    const t = this.tiles[i];
    if (!t.def) return false;
    if (t.def.kind === "vault" || t.def.transport) return true;
    if (t.def.recipe) return t.def.recipe.inputs.includes(item.type);
    return false;
  }

  private put(i: number, item: Item) {
    const t = this.tiles[i]!;
    const def = t.def!;
    if (def.kind === "vault") {
      this.delivered.push({
        tick: this.tick, type: item.type, quality: item.quality,
        value: itemValue(item.type, item.quality),
      });
      return;
    }
    if (def.transport) {
      let q = item.quality;
      if (def.qualityBonus) q = Math.min(QUALITY_CAP, q + def.qualityBonus);
      t.out = { type: item.type, quality: q };
      // Duplicator: 15% chance to clone. The clone lands in the same tile's
      // slot only if empty next tick, so in practice it feeds the next tile —
      // modelled here as an immediate extra item pushed into the delivery path.
      if (def.id === "dup" && this.rand() < 0.15) t.inputs.push({ ...t.out });
      return;
    }
    t.inputs.push(item);
  }

  /**
   * Production pass: advance every machine's cycle, and move finished goods
   * onto the output edge.
   */
  private produce() {
    for (let i = 0; i < this.tiles.length; i++) {
      const t = this.tiles[i];
      const def = t.def;
      if (!def) continue;

      // Belt-like tiles with a queued duplicate release it when the slot frees.
      if (def.transport && t.out === null && t.inputs.length > 0) {
        t.out = t.inputs.shift()!;
        continue;
      }

      if (def.produces && def.period) {
        if (t.out !== null) {
          if (!t.jamImmune) this.jamTicks++;
          continue; // jammed: output edge still occupied
        }
        t.progress += t.speed;
        if (t.progress >= def.period) {
          t.progress -= def.period;
          t.out = { type: def.produces, quality: def.spawnQuality ?? 0 };
        }
        continue;
      }

      if (def.recipe) {
        const r = def.recipe;
        const ready = r.inputs.every((need) => {
          const want = r.inputs.filter((x) => x === need).length;
          return t.inputs.filter((x) => x.type === need).length >= want;
        });
        if (!ready) continue;
        if (t.out !== null) {
          if (!t.jamImmune) this.jamTicks++;
          continue;
        }
        t.progress += t.speed;
        if (t.progress >= r.ticks) {
          t.progress -= r.ticks;
          const consumed: Item[] = [];
          for (const need of r.inputs) {
            const k = t.inputs.findIndex((x) => x.type === need);
            consumed.push(t.inputs.splice(k, 1)[0]);
          }
          const meanQ = consumed.reduce((a, b) => a + b.quality, 0) / consumed.length;
          t.out = {
            type: r.output,
            quality: Math.min(QUALITY_CAP, Math.floor(meanQ) + t.qualityOut),
          };
        }
      }
    }
  }

  /**
   * Transfer pass — the one genuinely hard problem in the whole design.
   *
   * Naively moving every item into the tile ahead fails two ways: a full belt
   * line only advances at its head (items should move as a train), and a full
   * closed loop deadlocks (it should rotate).
   *
   * The fix is to move DOWNSTREAM TILES FIRST, so each target vacates before
   * its source tries to fill it. That order is a reverse topological sort of
   * the flow graph — which exists only if the graph is acyclic, and belt loops
   * are deliberately cyclic. So: find strongly connected components (Tarjan),
   * process the condensed DAG sinks-first, and resolve each multi-tile SCC —
   * a loop — as a simultaneous rotation. Belt loops then behave exactly like
   * straight belts, which is what makes the polish-loop build legal at all.
   */
  private transfer() {
    const n = this.tiles.length;
    const edge = new Int32Array(n).fill(-1);
    for (let i = 0; i < n; i++) {
      const t = this.tiles[i];
      if (!t.def || t.out === null) continue;
      const j = this.target(i);
      if (j < 0 || !this.couldAccept(j, t.out)) continue;
      edge[i] = j;
    }

    // ── Tarjan SCC over the (at most one out-edge per node) flow graph ──
    const index = new Int32Array(n).fill(-1);
    const low = new Int32Array(n).fill(0);
    const onStack = new Uint8Array(n);
    const stack: number[] = [];
    const comps: number[][] = [];
    let counter = 0;

    for (let s = 0; s < n; s++) {
      if (index[s] !== -1 || edge[s] === -1) continue;
      // iterative DFS — boards get large and recursion depth is a real risk
      const work: Array<[number, number]> = [[s, 0]];
      while (work.length) {
        const frame = work[work.length - 1];
        const v = frame[0];
        if (frame[1] === 0) {
          index[v] = low[v] = counter++;
          stack.push(v); onStack[v] = 1;
        }
        const wNode = frame[1] === 0 ? edge[v] : -1;
        frame[1] = 1;
        if (wNode !== -1 && index[wNode] === -1) {
          work.push([wNode, 0]);
          continue;
        }
        if (wNode !== -1 && onStack[wNode]) low[v] = Math.min(low[v], index[wNode]);
        work.pop();
        if (work.length) {
          const p = work[work.length - 1][0];
          low[p] = Math.min(low[p], low[v]);
        }
        if (low[v] === index[v]) {
          const comp: number[] = [];
          for (;;) {
            const u = stack.pop()!;
            onStack[u] = 0;
            comp.push(u);
            if (u === v) break;
          }
          comps.push(comp);
        }
      }
    }

    // Tarjan emits components in reverse topological order — sinks first,
    // which is exactly the order we want to move in.
    for (const comp of comps) {
      if (comp.length === 1) {
        const i = comp[0];
        const j = edge[i];
        const t = this.tiles[i];
        if (j < 0 || t.out === null) continue;
        if (this.canAccept(j, t.out)) {
          const item = t.out; t.out = null;
          this.put(j, item);
        } else if (!t.jamImmune && !t.def!.transport) {
          // Belts stalled behind a busy machine are normal backpressure, not a
          // jam. Only a machine that finished work it cannot hand off counts.
          this.jamTicks++;
        }
        continue;
      }

      // Multi-tile SCC: a closed loop. Every member's target is another member,
      // so they rotate simultaneously. Snapshot first, then commit — otherwise
      // the result depends on iteration order, which is exactly what we forbid.
      const snapshot = comp.map((i) => this.tiles[i].out);
      const movable = comp.every((i, k) => snapshot[k] !== null);
      if (!movable) {
        // Partially empty ring: the hole propagates backward around the loop.
        // Sort by tile index and mark destinations so nothing moves twice, so
        // the result does not depend on Tarjan's emission order.
        const order = [...comp].sort((a, b) => a - b);
        const moved = new Set<number>();
        for (let pass = 0; pass < order.length; pass++) {
          let changed = false;
          for (const i of order) {
            const t = this.tiles[i];
            const j = edge[i];
            if (j < 0 || t.out === null || moved.has(i)) continue;
            if (!this.canAccept(j, t.out)) continue;
            const item = t.out; t.out = null;
            this.put(j, item);
            moved.add(j);
            changed = true;
          }
          if (!changed) break;
        }
        continue;
      }
      for (const i of comp) this.tiles[i].out = null;
      comp.forEach((i, k) => this.put(edge[i], snapshot[k]!));
    }
  }

  step() {
    this.tick++;
    this.produce();
    this.transfer();
  }

  run(ticks = 60): ShiftResult {
    for (let i = 0; i < ticks; i++) this.step();
    let inFlight = 0;
    for (const t of this.tiles) {
      if (t.out) inFlight++;
      inFlight += t.inputs.length;
    }
    const byType: Record<string, number> = {};
    for (const d of this.delivered) byType[d.type] = (byType[d.type] ?? 0) + 1;
    return {
      ticks,
      delivered: this.delivered,
      payout: Math.round(this.delivered.reduce((a, b) => a + b.value, 0)),
      inFlight,
      jamTicks: this.jamTicks,
      byType,
    };
  }
}

export function runBoard(
  w: number, h: number, placements: Placement[], ticks = 60, seed = 0xc0ffee,
): ShiftResult {
  return new Sim(w, h, placements, seed).run(ticks);
}

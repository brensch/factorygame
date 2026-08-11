/**
 * OVERFLOW prototype — canvas shell over the Rust core compiled to wasm.
 *
 * This file owns presentation and input, nothing else. Every rule — the sim,
 * the card deck, quotas, credits, what's legal to place where — lives in
 * `rust/core`, reached through the JSON ABI in `rust/web`. The maps below are
 * cosmetic (colours and labels keyed by machine name); if a decision affects
 * the outcome of a run, it is not made here.
 */

// ── types mirroring the wire format (rust/web/src/lib.rs) ────────────────────
type Dir = "N" | "E" | "S" | "W";

interface Card { m: string; name: string; cost: number; kind: string }
type ShopOffer =
  | { type: "machine"; m: string; name: string; price: number; kind: string }
  | { type: "directive"; d: string; name: string; price: number; tag: string; blurb: string };
interface OwnedDirective { d: string; name: string; tag: string; n: number }
interface Pl {
  x: number; y: number; m: string; kind: string;
  d: Dir | null; d2: Dir | null; minQ: number | null;
}
interface Outcome {
  round: number; payout: number; quota: number;
  cleared: boolean; inFlight: number; jamTicks: number;
}
interface State {
  round: number; credits: number; quota: number; shiftLen: number;
  audit: boolean; phase: "build" | "shop" | "over"; won: boolean;
  qualityCap: number; boardW: number; boardH: number;
  board: Pl[]; hand: Card[]; offers: ShopOffer[];
  directives: OwnedDirective[];
  handMax: number; rerollPrice: number; priceMult: number;
  nextQuota: number | null; nextAudit: boolean;
  auras: { x: number; y: number }[];
  flows: Flow[];
  last: Outcome | null; err: string | null;
}
/** One output edge on the board, with the core's verdict on the connection. */
interface Flow {
  fx: number; fy: number; tx: number; ty: number;
  d: Dir; status: "ok" | "open" | "bad"; secondary: boolean;
}
interface Frame {
  tick: number; total: number; payout: number; done: boolean;
  items: { id: number; x: number; y: number; t: string; q: number }[];
  moves: { id: number; fx: number; fy: number }[];
  /** Items consumed on arrival this tick (machine inputs, vault deliveries):
   *  they animate their final hop, then vanish. */
  hops: { id: number; fx: number; fy: number; x: number; y: number; t: string; q: number }[];
}
interface CatAura { speed: number; q: number; noJam: boolean; onlyTag: string | null }
interface CatM {
  m: string; name: string; kind: string; cost: number; tags: string[];
  produces: string | null; period: number; spawnQ: number;
  recipe: { inputs: string[]; output: string; ticks: number } | null;
  transport: boolean; qualityBonus: number; aura: CatAura | null; blurb: string;
}
interface Catalog {
  qualityStep: number; qualityCap: number; dupChance: number;
  items: Record<string, number>; machines: CatM[];
}

// ── wasm boot ────────────────────────────────────────────────────────────────
const wasm = await WebAssembly.instantiate(
  await (await fetch("./game.wasm")).arrayBuffer(), {},
);
const core = wasm.instance.exports as {
  memory: WebAssembly.Memory;
  out_ptr(): number;
  catalog(): number;
  sel_clear(): number;
  sel_add(x: number, y: number): number;
  sel_move(dx: number, dy: number): number;
  boot(seed: number): number;
  state(): number;
  belt(x: number, y: number, d: number): number;
  junction(x: number, y: number): number;
  merger(x: number, y: number, d: number): number;
  splitter(x: number, y: number, d: number): number;
  retry(): number;
  play(i: number, x: number, y: number, d: number, d2: number, minQ: number): number;
  sell(x: number, y: number): number;
  sell_hand(i: number): number;
  rotate(x: number, y: number): number;
  rotate2(x: number, y: number): number;
  set_gate(x: number, y: number, q: number): number;
  shop_buy(i: number): number;
  shop_reroll(): number;
  shop_done(): number;
  project(): number;
  shift_start(): number;
  shift_step(): number;
  shift_finish(): number;
};

const decoder = new TextDecoder();
function read<T>(len: number): T {
  return JSON.parse(
    decoder.decode(new Uint8Array(core.memory.buffer, core.out_ptr(), len)),
  ) as T;
}

const DCODE: Record<Dir, number> = { N: 0, E: 1, S: 2, W: 3 };
const dc = (d: Dir | null) => (d === null ? -1 : DCODE[d]);

// The machine catalogue: every definition the game runs on, exported by the
// core so the UI can explain cards without owning any rules.
const CATALOG = read<Catalog>(core.catalog());
const MDEF = new Map(CATALOG.machines.map((m) => [m.m, m]));

// ── cosmetic maps — colours and labels only ──────────────────────────────────
const DIRV: Record<Dir, [number, number]> = { N: [0, -1], E: [1, 0], S: [0, 1], W: [-1, 0] };
const OPP: Record<Dir, Dir> = { N: "S", S: "N", E: "W", W: "E" };
const ORDER: Dir[] = ["N", "E", "S", "W"];
const turn = (d: Dir, n = 1): Dir => ORDER[(ORDER.indexOf(d) + n + 4) % 4];

// Connection verdicts, straight from the core's flow graph.
const FLOW_COLOR: Record<Flow["status"], string> = {
  ok: "#6fcf5f", open: "#5a6a7d", bad: "#ff5d5d",
};

const CAT: Record<string, string> = {
  extractor: "#f0a63a", processor: "#e8623c", assembler: "#8b7bf0",
  modifier: "#34c8b0", logistics: "#647890", vault: "#6fcf5f",
};
const SHORT: Record<string, string> = {
  drill: "DRL", tap: "TAP", geode: "GEO", furnace: "FUR", retort: "RET",
  lapidary: "LAP", compress: "CMP", fab: "FAB", circuit: "CIR", lens: "LNS",
  engine: "ENG", belt: "", merger: "MRG", splitter: "SPL", buffer: "BUF",
  filter: "FIL", overclock: "OCK", polisher: "POL", heatsink: "HSK",
  dup: "DUP", vault: "VLT",
};
const ITEM_COLOR: Record<string, string> = {
  ore: "#f0a63a", sap: "#6fcf5f", crystal: "#7fd8ff",
  ingot: "#e8623c", resin: "#a8d86a", shard: "#5fa8f5",
  gear: "#8b7bf0", circuit: "#c58bf0", lens: "#f08ad0",
  engine: "#6fcf5f", core: "#ffe08a", beacon: "#9fe8ff",
};

// ── client state: what the player is doing, never what the game is ───────────
type Infra = "belt" | "junction" | "merger" | "splitter";
type Tool = { kind: Infra } | { kind: "card"; idx: number };

const TAG_COLOR: Record<string, string> = {
  heat: "#e8623c", kinetic: "#f0a63a", volt: "#8b7bf0",
  precision: "#7fd8ff", organic: "#6fcf5f",
};

let G: State;
const ui = {
  tool: { kind: "belt" } as Tool,
  dir: "E" as Dir,
  selected: null as { x: number; y: number } | null,
  /** Tiles in the group selection, as "x,y" keys. Moves as one piece. */
  multiSel: new Set<string>(),
  animating: false,
  speed: 1,
  positions: new Map<number, { x: number; y: number; px: number; py: number; t: string; q: number }>(),
  alpha: 0,
  tick: 0,
  payout: 0,
};

const seed = () => (Math.random() * 4294967296) >>> 0;

/** Run a command against the core; the returned state is the new truth. */
function cmd(len: number): boolean {
  const s = read<State>(len);
  const ok = s.err === null;
  if (!ok) toast(s.err!);
  G = s;
  return ok;
}

const at = (x: number, y: number) => G.board.find((p) => p.x === x && p.y === y);

// ── canvas ───────────────────────────────────────────────────────────────────
const cv = document.getElementById("cv") as HTMLCanvasElement;
const ctx = cv.getContext("2d")!;
let TILE = 54;

function layout() {
  const stage = document.getElementById("stage")!;
  const r = stage.getBoundingClientRect();
  const [W, H] = [G.boardW, G.boardH];
  TILE = Math.max(26, Math.floor(Math.min((r.width - 24) / W, (r.height - 24) / H)));
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  cv.width = W * TILE * dpr;
  cv.height = H * TILE * dpr;
  cv.style.width = W * TILE + "px";
  cv.style.height = H * TILE + "px";
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
}

function rr(x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

function arrow(cx: number, cy: number, d: Dir, len: number, col: string) {
  const [dx, dy] = DIRV[d];
  const ax = cx + dx * len, ay = cy + dy * len;
  const px = -dy, py = dx;
  ctx.fillStyle = col;
  ctx.beginPath();
  ctx.moveTo(ax, ay);
  ctx.lineTo(ax - dx * 7 + px * 5, ay - dy * 7 + py * 5);
  ctx.lineTo(ax - dx * 7 - px * 5, ay - dy * 7 - py * 5);
  ctx.closePath();
  ctx.fill();
}

/** Midpoint of a tile's edge on side `d`, in canvas coordinates. */
function edgeMid(x: number, y: number, d: Dir): [number, number] {
  const [dx, dy] = DIRV[d];
  return [x * TILE + TILE / 2 + (dx * TILE) / 2, y * TILE + TILE / 2 + (dy * TILE) / 2];
}

/** Output port: a triangle straddling the tile edge, coloured by the core's
 *  verdict on the connection. Green flows, grey dangles, red never will. */
function portOut(x: number, y: number, d: Dir, status: Flow["status"], secondary: boolean) {
  const [mx, my] = edgeMid(x, y, d);
  const [dx, dy] = DIRV[d];
  const px = -dy, py = dx;
  const s = Math.max(5, TILE * 0.15) * (secondary ? 0.8 : 1);
  ctx.fillStyle = FLOW_COLOR[status];
  ctx.beginPath();
  ctx.moveTo(mx + dx * s, my + dy * s);
  ctx.lineTo(mx - dx * s * 0.4 + px * s * 0.9, my - dy * s * 0.4 + py * s * 0.9);
  ctx.lineTo(mx - dx * s * 0.4 - px * s * 0.9, my - dy * s * 0.4 - py * s * 0.9);
  ctx.closePath();
  ctx.fill();
  if (secondary) {
    ctx.strokeStyle = "#0b0e13";
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }
}

/** Input notch: a dim triangle just inside the edge an item arrives through. */
function portIn(x: number, y: number, travel: Dir) {
  const [mx, my] = edgeMid(x, y, OPP[travel]);
  const [dx, dy] = DIRV[travel];
  const px = -dy, py = dx;
  const s = Math.max(4, TILE * 0.11);
  ctx.fillStyle = "#9fb0c366";
  ctx.beginPath();
  ctx.moveTo(mx + dx * s * 1.2, my + dy * s * 1.2);
  ctx.lineTo(mx + px * s * 0.8, my + py * s * 0.8);
  ctx.lineTo(mx - px * s * 0.8, my - py * s * 0.8);
  ctx.closePath();
  ctx.fill();
}

function draw() {
  const [W, H] = [G.boardW, G.boardH];
  ctx.clearRect(0, 0, cv.width, cv.height);

  // floor
  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
    ctx.fillStyle = "#151a22";
    ctx.strokeStyle = "#222b36";
    ctx.lineWidth = 1;
    rr(x * TILE + 1.5, y * TILE + 1.5, TILE - 3, TILE - 3, 5);
    ctx.fill(); ctx.stroke();
  }

  // aura halos (targets computed by the core), under machines
  for (const a of G.auras) {
    ctx.strokeStyle = CAT.modifier;
    ctx.lineWidth = 1.5;
    ctx.setLineDash([4, 3]);
    rr(a.x * TILE + 3, a.y * TILE + 3, TILE - 6, TILE - 6, 6);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // index the flow graph by source and (for good connections) by target
  const outFlows = new Map<string, Flow[]>();
  const inFlows = new Map<string, Flow[]>();
  for (const f of G.flows) {
    const fk = `${f.fx},${f.fy}`;
    if (!outFlows.has(fk)) outFlows.set(fk, []);
    outFlows.get(fk)!.push(f);
    if (f.status === "ok") {
      const tk = `${f.tx},${f.ty}`;
      if (!inFlows.has(tk)) inFlows.set(tk, []);
      inFlows.get(tk)!.push(f);
    }
  }

  // machines
  for (const c of G.board) {
    const col = CAT[c.kind];
    const cx = c.x * TILE + TILE / 2, cy = c.y * TILE + TILE / 2;
    const k = `${c.x},${c.y}`;
    const outs = outFlows.get(k) ?? [];
    const ins = inFlows.get(k) ?? [];

    if (c.m === "junction") {
      // the crossing: both axes drawn edge to edge, with a hub so it reads
      // as over/under rather than a merge
      ctx.strokeStyle = "#3a4655";
      ctx.lineWidth = Math.max(6, TILE * 0.17);
      ctx.lineCap = "round";
      const [wx, wy] = edgeMid(c.x, c.y, "W"), [ex, ey] = edgeMid(c.x, c.y, "E");
      const [nx, ny] = edgeMid(c.x, c.y, "N"), [sx, sy] = edgeMid(c.x, c.y, "S");
      ctx.beginPath(); ctx.moveTo(wx, wy); ctx.lineTo(ex, ey); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(nx, ny); ctx.lineTo(sx, sy); ctx.stroke();
      ctx.fillStyle = "#232c38";
      ctx.strokeStyle = "#8ea1b8";
      ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.arc(cx, cy, Math.max(4, TILE * 0.12), 0, Math.PI * 2);
      ctx.fill(); ctx.stroke();
      for (const f of ins) portIn(c.x, c.y, f.d);
    } else if (c.m === "belt" || c.m === "merger") {
      // A belt is a path: from every edge that feeds it, through the centre,
      // out the arrow edge. Corners curve, merges fork — the routing is the
      // drawing.
      const out = outs[0];
      const entries = ins.map((f) => OPP[f.d]);
      if (!entries.length && out) entries.push(OPP[out.d]);
      ctx.strokeStyle = "#3a4655";
      ctx.lineWidth = Math.max(6, TILE * 0.17);
      ctx.lineCap = "round";
      for (const e of entries) {
        const [ex, ey] = edgeMid(c.x, c.y, e);
        const [ox, oy] = out ? edgeMid(c.x, c.y, out.d) : [cx, cy];
        ctx.beginPath();
        ctx.moveTo(ex, ey);
        ctx.quadraticCurveTo(cx, cy, ox, oy);
        ctx.stroke();
      }
      if (out) {
        arrow(cx, cy, out.d, TILE / 2 - 9, out.status === "bad" ? FLOW_COLOR.bad : "#8ea1b8");
        if (out.status !== "ok") portOut(c.x, c.y, out.d, out.status, false);
      }
      if (c.m === "merger") {
        ctx.fillStyle = "#8ea1b8";
        ctx.font = `700 ${Math.max(7, TILE * 0.18)}px ui-monospace,monospace`;
        ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText("MRG", cx, cy + TILE * 0.3);
      }
    } else {
      ctx.fillStyle = col;
      ctx.globalAlpha = 0.92;
      rr(c.x * TILE + 4, c.y * TILE + 4, TILE - 8, TILE - 8, 6);
      ctx.fill();
      ctx.globalAlpha = 1;

      if (ui.selected && ui.selected.x === c.x && ui.selected.y === c.y) {
        ctx.strokeStyle = "#fff"; ctx.lineWidth = 2;
        rr(c.x * TILE + 1.5, c.y * TILE + 1.5, TILE - 3, TILE - 3, 7);
        ctx.stroke();
      }

      ctx.fillStyle = "#0b0e13";
      ctx.font = `700 ${Math.max(8, TILE * 0.21)}px ui-monospace,monospace`;
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillText(SHORT[c.m] ?? "", cx, cy);

      if (c.m === "filter") {
        ctx.fillStyle = "#0b0e13";
        ctx.font = `700 ${Math.max(7, TILE * 0.16)}px ui-monospace,monospace`;
        ctx.fillText("≥" + (c.minQ ?? "?"), cx, cy + TILE * 0.26);
      }

      // ports: where it emits (coloured by the connection's verdict) and
      // where it's being fed from
      for (const f of outs) portOut(c.x, c.y, f.d, f.status, f.secondary);
      for (const f of ins) portIn(c.x, c.y, f.d);
    }
  }

  // group selection: dashed outline on every selected tile
  if (ui.multiSel.size) {
    ctx.strokeStyle = "#7fd8ff";
    ctx.lineWidth = 1.5;
    ctx.setLineDash([5, 3]);
    for (const k of ui.multiSel) {
      const [x, y] = k.split(",").map(Number);
      rr(x * TILE + 2, y * TILE + 2, TILE - 4, TILE - 4, 7);
      ctx.stroke();
    }
    ctx.setLineDash([]);
  }

  // items in flight
  for (const p of ui.positions.values()) {
    const x = p.px + (p.x - p.px) * ui.alpha;
    const y = p.py + (p.y - p.py) * ui.alpha;
    const cx = x * TILE + TILE / 2, cy = y * TILE + TILE / 2;
    const r = Math.max(3.5, TILE * 0.11);
    ctx.fillStyle = ITEM_COLOR[p.t] ?? "#fff";
    ctx.strokeStyle = "#0b0e13";
    ctx.lineWidth = 2;
    ctx.beginPath(); ctx.arc(cx, cy, r, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
    if (p.q > 0) {
      ctx.fillStyle = "#fff";
      ctx.font = `700 ${Math.max(7, TILE * 0.15)}px ui-monospace,monospace`;
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillText(String(p.q), cx, cy - r - Math.max(5, TILE * 0.13));
    }
  }

  // ghost of the pending placement under the cursor, arrow showing where
  // its output will point (R rotates before placing)
  if (G.phase === "build" && !ui.animating && hover && !at(hover.x, hover.y) && !drag) {
    const mKey = ui.tool.kind === "card" ? G.hand[ui.tool.idx]?.m : ui.tool.kind;
    const m = mKey ? MDEF.get(mKey) : undefined;
    if (m) {
      ctx.globalAlpha = 0.32;
      ctx.fillStyle = CAT[m.kind];
      rr(hover.x * TILE + 4, hover.y * TILE + 4, TILE - 8, TILE - 8, 6);
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.strokeStyle = "#ffffff55"; ctx.lineWidth = 1.5;
      rr(hover.x * TILE + 2, hover.y * TILE + 2, TILE - 4, TILE - 4, 7);
      ctx.stroke();
      if ((m.transport || m.produces || m.recipe) && m.m !== "junction") {
        const hcx = hover.x * TILE + TILE / 2, hcy = hover.y * TILE + TILE / 2;
        arrow(hcx, hcy, ui.dir, TILE / 2 - 6, "#ffffffaa");
      }
    }
  }

  // live preview of the belt run being dragged
  if (drag?.mode === "belt" && drag.path.length) {
    const path = drag.path;
    ctx.strokeStyle = "#8ea1b877";
    ctx.lineWidth = Math.max(5, TILE * 0.14);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.beginPath();
    path.forEach((p, i) => {
      const px = p.x * TILE + TILE / 2, py = p.y * TILE + TILE / 2;
      if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
    });
    ctx.stroke();
    const last = path[path.length - 1];
    const prev = path[path.length - 2];
    const d: Dir = prev
      ? (last.x > prev.x ? "E" : last.x < prev.x ? "W" : last.y > prev.y ? "S" : "N")
      : ui.dir;
    arrow(last.x * TILE + TILE / 2, last.y * TILE + TILE / 2, d, TILE / 2 - 8, "#ffffffaa");
  }

  // rubber-band rectangle
  if (drag?.mode === "band") {
    const x = Math.min(drag.x0, drag.x1) * TILE + 2;
    const y = Math.min(drag.y0, drag.y1) * TILE + 2;
    const w = (Math.abs(drag.x1 - drag.x0) + 1) * TILE - 4;
    const h = (Math.abs(drag.y1 - drag.y0) + 1) * TILE - 4;
    ctx.fillStyle = "#7fd8ff18";
    ctx.strokeStyle = "#7fd8ff";
    ctx.lineWidth = 1.5;
    ctx.setLineDash([5, 3]);
    rr(x, y, w, h, 7);
    ctx.fill();
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // the group mid-move: dim the originals, ghost the piece at its target,
  // tinted by whether it can land there
  if (drag?.mode === "move" && (drag.dx || drag.dy)) {
    const ok = moveValid(drag.tiles, drag.dx, drag.dy);
    ctx.fillStyle = "#0b0e13aa";
    for (const k of drag.tiles) {
      const [x, y] = k.split(",").map(Number);
      rr(x * TILE + 1.5, y * TILE + 1.5, TILE - 3, TILE - 3, 5);
      ctx.fill();
    }
    for (const k of drag.tiles) {
      const [x, y] = k.split(",").map(Number);
      const p = at(x, y);
      if (!p) continue;
      const gx = x + drag.dx, gy = y + drag.dy;
      ctx.globalAlpha = 0.55;
      ctx.fillStyle = p.m === "belt" || p.m === "merger" ? "#3a4655" : CAT[p.kind];
      rr(gx * TILE + 4, gy * TILE + 4, TILE - 8, TILE - 8, 6);
      ctx.fill();
      ctx.globalAlpha = 1;
      if (SHORT[p.m]) {
        ctx.fillStyle = "#0b0e13";
        ctx.font = `700 ${Math.max(8, TILE * 0.21)}px ui-monospace,monospace`;
        ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText(SHORT[p.m], gx * TILE + TILE / 2, gy * TILE + TILE / 2);
      }
      ctx.strokeStyle = ok ? FLOW_COLOR.ok : FLOW_COLOR.bad;
      ctx.lineWidth = 2;
      rr(gx * TILE + 2, gy * TILE + 2, TILE - 4, TILE - 4, 7);
      ctx.stroke();
    }
  }
}

// ── placement ────────────────────────────────────────────────────────────────
function placeAt(x: number, y: number, d: Dir) {
  if (ui.tool.kind === "belt") { cmd(core.belt(x, y, DCODE[d])); return; }
  if (ui.tool.kind === "junction") { cmd(core.junction(x, y)); return; }
  if (ui.tool.kind === "merger") { cmd(core.merger(x, y, DCODE[d])); return; }
  if (ui.tool.kind === "splitter") { cmd(core.splitter(x, y, DCODE[d])); return; }
  const card = G.hand[ui.tool.idx];
  if (!card) return;
  // Filters eject sideways by default; splitters split sideways. The core
  // validates — these are just sensible initial edges the player can rotate.
  const d2 = card.m === "filter" || card.m === "splitter" ? turn(d) : null;
  const minQ = card.m === "filter" ? 5 : -1;
  if (cmd(core.play(ui.tool.idx, x, y, DCODE[d], dc(d2), minQ))) {
    ui.tool = { kind: "belt" }; // the blueprint is placed; drop back to belts
  }
}

// ── side panel ───────────────────────────────────────────────────────────────
function paintHand() {
  const pal = document.getElementById("pal")!;
  pal.innerHTML = "";

  const infra = (kind: Infra, label: string, glyph: string, cost: number) => {
    const el = document.createElement("div");
    el.className = "pi" + (ui.tool.kind === kind ? " on" : "") + (G.credits >= cost ? "" : " no");
    el.innerHTML =
      `<div class="sw" style="background:${CAT.logistics}">${glyph}</div>` +
      `<div class="nm">${label}</div><div class="cs">${cost}c</div>`;
    el.onclick = () => { ui.tool = { kind }; ui.selected = null; paintAll(); };
    pal.appendChild(el);
  };
  infra("belt", "Belt", "▸", 1);
  infra("junction", "Junction", "✚", 2);
  infra("merger", "Merger", "⇒", 4);
  infra("splitter", "Splitter", "⇉", 4);

  G.hand.forEach((card, i) => {
    const el = document.createElement("div");
    const on = ui.tool.kind === "card" && ui.tool.idx === i;
    el.className = "pi" + (on ? " on" : "");
    el.innerHTML =
      `<div class="sw" style="background:${CAT[card.kind]}">${SHORT[card.m] || "▸"}</div>` +
      `<div class="nm">${card.name}</div><div class="cs">owned</div>`;
    el.onclick = () => { ui.tool = { kind: "card", idx: i }; ui.selected = null; paintAll(); };
    el.oncontextmenu = (e) => {
      e.preventDefault();
      const value = Math.floor(Math.round(card.cost * G.priceMult) / 2);
      if (cmd(core.sell_hand(i))) {
        if (ui.tool.kind === "card") ui.tool = { kind: "belt" };
        toast(`Sold ${card.name} blueprint for ${value}c`);
      }
      paintAll();
    };
    pal.appendChild(el);
  });

  (document.getElementById("deckInfo") as HTMLElement).textContent =
    `hand ${G.hand.length}/${G.handMax}`;
}

// ── the info panel: what a machine does, fed by the core's catalogue ─────────
const capName = (k: string) => k.charAt(0).toUpperCase() + k.slice(1);
const chip = (item: string, n = 1) =>
  `<span class="ichip"><i style="background:${ITEM_COLOR[item] ?? "#fff"}"></i>${n > 1 ? `${n}× ` : ""}${capName(item)}</span>`;

function grouped(inputs: string[]): [string, number][] {
  const counts = new Map<string, number>();
  for (const i of inputs) counts.set(i, (counts.get(i) ?? 0) + 1);
  return [...counts.entries()];
}

const producersOf = (item: string) =>
  CATALOG.machines.filter((m) => m.produces === item || m.recipe?.output === item);
const consumersOf = (item: string) =>
  CATALOG.machines.filter((m) => m.recipe?.inputs.includes(item));

function auraText(a: CatAura): string {
  const effects: string[] = [];
  if (a.speed !== 1) effects.push(`${Math.round((a.speed - 1) * 100)}% faster`);
  if (a.q) effects.push(`+${a.q} output quality`);
  if (a.noJam) effects.push("never jams");
  const target = a.onlyTag ? `adjacent ${a.onlyTag.toUpperCase()} machines` : "all adjacent machines";
  return `<b>Aura</b> — ${target}: ${effects.join(", ")}`;
}

/** One compact line of mechanics, for offer cards and the hand. */
function mechShort(key: string): string {
  const m = MDEF.get(key);
  if (!m) return "";
  if (m.recipe)
    return `${grouped(m.recipe.inputs).map(([i, n]) => (n > 1 ? `${n}×${capName(i)}` : capName(i))).join(" + ")} → ${capName(m.recipe.output)}`;
  if (m.produces) return `makes ${capName(m.produces)}`;
  if (m.aura) {
    const fx = m.aura.speed !== 1 ? `+${Math.round((m.aura.speed - 1) * 100)}% speed`
      : `+${m.aura.q} quality${m.aura.noJam ? ", no jams" : ""}`;
    return `${fx} aura${m.aura.onlyTag ? ` (${m.aura.onlyTag.toUpperCase()})` : ""}`;
  }
  if (m.qualityBonus) return `+${m.qualityBonus} quality pass-through`;
  switch (key) {
    case "filter": return "quality gate valve";
    case "splitter": return "splits a lane in two";
    case "merger": return "joins lanes into one";
    case "dup": return `${Math.round(CATALOG.dupChance * 100)}% chance to clone`;
    case "buffer": return "belt (for now)";
    default: return m.kind;
  }
}

function infoHTML(key: string): string {
  const m = MDEF.get(key);
  if (!m) return "";
  const rows: string[] = [];

  if (m.produces) {
    rows.push(`Makes ${chip(m.produces)} every ${m.period} ticks` +
      (m.spawnQ ? ` · starts at quality ${m.spawnQ}` : ""));
  }
  if (m.recipe) {
    rows.push(`${grouped(m.recipe.inputs).map(([i, n]) => chip(i, n)).join(" + ")} → ${chip(m.recipe.output)} · ${m.recipe.ticks} ticks`);
  }
  if (m.qualityBonus) rows.push(`<b>+${m.qualityBonus} quality</b> to every item passing through`);
  if (m.aura) rows.push(auraText(m.aura));

  // the chain around it: where its inputs come from, where its output goes
  if (m.recipe) {
    for (const [i] of grouped(m.recipe.inputs)) {
      const from = producersOf(i).map((p) => p.name);
      if (from.length) rows.push(`${chip(i)} comes from ${from.join(", ")}`);
    }
  }
  const out = m.produces ?? m.recipe?.output;
  if (out) {
    const base = CATALOG.items[out];
    const eaters = consumersOf(out).map((c) => c.name);
    rows.push(`${chip(out)} is worth <b>${base}c</b> +${Math.round(CATALOG.qualityStep * 100)}%/quality at the Vault` +
      (eaters.length ? ` · feeds ${eaters.join(", ")}` : ""));
  }

  // synergies via tags
  if (m.tags.length) {
    const boosters = CATALOG.machines
      .filter((b) => b.aura?.onlyTag && m.tags.includes(b.aura.onlyTag) && b.m !== key)
      .map((b) => b.name);
    rows.push(`<span class="itags">${m.tags.map((t) => t.toUpperCase()).join(" · ")}</span>` +
      (boosters.length ? ` — boosted by ${boosters.join(", ")}` : ""));
  }
  if (m.aura?.onlyTag) {
    const targets = CATALOG.machines
      .filter((t) => t.tags.includes(m.aura!.onlyTag!) && (t.produces || t.recipe))
      .map((t) => t.name);
    if (targets.length) rows.push(`Boosts: ${targets.join(", ")}`);
  }

  return `<p class="iblurb">${m.blurb}</p>` + rows.map((r) => `<div class="irow">${r}</div>`).join("");
}

function paintInfo() {
  const title = document.getElementById("infoTitle")!;
  const body = document.getElementById("infoBody")!;
  // Priority: a selected placed machine, else the selected card or tool.
  let key: string = ui.tool.kind === "card" ? (G.hand[ui.tool.idx]?.m ?? "belt") : ui.tool.kind;
  if (ui.selected) key = at(ui.selected.x, ui.selected.y)?.m ?? key;
  const m = MDEF.get(key);
  title.innerHTML = m ? `${m.name} <span class="deckinfo">${m.cost}c · ${m.kind}</span>` : "—";
  body.innerHTML = infoHTML(key);
}

function paintInspector() {
  const box = document.getElementById("insp")!;
  const body = document.getElementById("inspBody")!;
  const c = ui.selected ? at(ui.selected.x, ui.selected.y) : null;
  if (!c || (c.m !== "filter" && c.m !== "splitter")) { box.classList.remove("on"); return; }
  box.classList.add("on");
  if (c.m === "filter") {
    body.innerHTML =
      `<div class="kv"><span>Eject when quality ≥</span><b>${c.minQ ?? "?"}</b></div>
       <div class="row" style="margin-top:8px">
         <button id="qDown">− gate</button><button id="qUp">+ gate</button>
         <button id="dRot">turn eject</button>
       </div>
       <div id="hint" style="margin-top:9px">Items below the gate carry on round the loop.
         Higher gate = more laps = more value per item, but fewer items get out.</div>`;
    (document.getElementById("qUp") as HTMLElement).onclick =
      () => { cmd(core.set_gate(c.x, c.y, (c.minQ ?? 5) + 1)); paintAll(); };
    (document.getElementById("qDown") as HTMLElement).onclick =
      () => { cmd(core.set_gate(c.x, c.y, (c.minQ ?? 5) - 1)); paintAll(); };
    (document.getElementById("dRot") as HTMLElement).onclick =
      () => { cmd(core.rotate2(c.x, c.y)); paintAll(); };
  } else {
    body.innerHTML = `<div class="kv"><span>Second output</span><b>${c.d2}</b></div>
      <div class="row" style="margin-top:8px"><button id="dRot">turn 2nd output</button></div>`;
    (document.getElementById("dRot") as HTMLElement).onclick =
      () => { cmd(core.rotate2(c.x, c.y)); paintAll(); };
  }
}

function project() {
  const r = read<{ payout?: number; inFlight?: number; jamTicks?: number; err?: string }>(core.project());
  if (r.err !== undefined) return;
  (document.getElementById("pProj") as HTMLElement).textContent = r.payout!.toLocaleString();
  (document.getElementById("pQuota") as HTMLElement).textContent = G.quota.toLocaleString();
  (document.getElementById("pFlight") as HTMLElement).textContent = String(r.inFlight);
  (document.getElementById("pJam") as HTMLElement).textContent = String(r.jamTicks);
  const bar = document.getElementById("pBar") as HTMLElement;
  bar.style.width = Math.min(100, (r.payout! / G.quota) * 100) + "%";
  bar.className = r.payout! >= G.quota ? "" : "short";
}

function paintHeader() {
  const g = (id: string) => document.getElementById(id) as HTMLElement;
  g("hRound").textContent = String(G.round + 1) + (G.audit ? " ⚠" : "");
  g("hQuota").textContent = G.quota.toLocaleString();
  g("hCred").textContent = String(G.credits);
  g("hTick").textContent = `${ui.tick}/${G.shiftLen}`;
  g("hPay").textContent = ui.payout.toLocaleString();
}

function paintDirectives() {
  const sec = document.getElementById("dirSec")!;
  const box = document.getElementById("dirList")!;
  if (!G.directives.length) {
    sec.style.display = "none";
    return;
  }
  sec.style.display = "";
  box.innerHTML = G.directives
    .map((d) =>
      `<span class="dchip" style="border-color:${TAG_COLOR[d.tag]}88;color:${TAG_COLOR[d.tag]}">
        ◆ ${d.name}${d.n > 1 ? ` ×${d.n}` : ""}</span>`)
    .join("");
}

function paintAll() {
  paintHand();
  paintDirectives();
  paintInfo();
  paintInspector();
  paintHeader();
  if (G.phase === "build" && !ui.animating) project();
  draw();
}

function toast(msg: string) {
  const el = document.getElementById("toast")!;
  el.textContent = msg;
  el.classList.add("on");
  setTimeout(() => el.classList.remove("on"), 1800);
}

// ── input: a small drag state machine ────────────────────────────────────────
// belt       drag from an empty tile with the Belt tool: lay a run
// band       Shift+drag anywhere: rubber-band a group selection
// maybe-move pointer down on a machine: becomes a move once it leaves the tile
// move       drag the machine (or the whole selection it belongs to)
type DragState =
  | { mode: "belt"; path: { x: number; y: number }[] }
  | { mode: "band"; x0: number; y0: number; x1: number; y1: number }
  | { mode: "maybe-move"; start: { x: number; y: number } }
  | { mode: "move"; start: { x: number; y: number }; tiles: Set<string>; dx: number; dy: number }
  | null;

let hover: { x: number; y: number } | null = null;
let drag: DragState = null;

const tk = (x: number, y: number) => `${x},${y}`;

function toTile(ev: MouseEvent | Touch) {
  const r = cv.getBoundingClientRect();
  const x = Math.floor((ev.clientX - r.left) / TILE);
  const y = Math.floor((ev.clientY - r.top) / TILE);
  return x >= 0 && y >= 0 && x < G.boardW && y < G.boardH ? { x, y } : null;
}

function commitBeltRun(path: { x: number; y: number }[]) {
  // Lay a belt run along the dragged path, auto-orienting each tile toward
  // the next. Refusals (occupied tiles, empty wallet) surface as toasts.
  for (let i = 0; i < path.length; i++) {
    const a = path[i], b = path[i + 1];
    let d: Dir = ui.dir;
    if (b) {
      d = b.x > a.x ? "E" : b.x < a.x ? "W" : b.y > a.y ? "S" : "N";
    } else if (i > 0) {
      const p = path[i - 1];
      d = a.x > p.x ? "E" : a.x < p.x ? "W" : a.y > p.y ? "S" : "N";
    }
    if (!at(a.x, a.y)) cmd(core.belt(a.x, a.y, DCODE[d]));
  }
}

/** Can the current move-drag land? Pure geometry; the core re-validates. */
function moveValid(tiles: Set<string>, dx: number, dy: number): boolean {
  for (const k of tiles) {
    const [x, y] = k.split(",").map(Number);
    const p = at(x, y);
    if (!p) continue;
    const nx = x + dx, ny = y + dy;
    if (nx < 0 || ny < 0 || nx >= G.boardW || ny >= G.boardH) return false;
    const occ = at(nx, ny);
    if (occ && !tiles.has(tk(nx, ny))) return false;
  }
  return true;
}

function commitMove(m: { tiles: Set<string>; dx: number; dy: number }) {
  if (m.dx === 0 && m.dy === 0) return;
  core.sel_clear();
  for (const k of m.tiles) {
    const [x, y] = k.split(",").map(Number);
    core.sel_add(x, y);
  }
  if (cmd(core.sel_move(m.dx, m.dy))) {
    // carry the selection along with the piece it selects
    ui.multiSel = new Set(
      [...m.tiles]
        .map((k) => { const [x, y] = k.split(",").map(Number); return tk(x + m.dx, y + m.dy); })
        .filter((k) => { const [x, y] = k.split(",").map(Number); return !!at(x, y); }),
    );
    if (ui.selected && m.tiles.has(tk(ui.selected.x, ui.selected.y))) {
      ui.selected = { x: ui.selected.x + m.dx, y: ui.selected.y + m.dy };
    }
  }
}

const buildLocked = () => G.phase !== "build" || ui.animating;

cv.addEventListener("contextmenu", (e) => e.preventDefault());

cv.addEventListener("pointerdown", (e) => {
  if (buildLocked()) return;
  const t = toTile(e);
  if (!t) return;
  cv.setPointerCapture(e.pointerId);

  if (e.button === 2) {
    cmd(core.sell(t.x, t.y));
    ui.multiSel.delete(tk(t.x, t.y));
    paintAll();
    return;
  }

  if (e.shiftKey) {
    drag = { mode: "band", x0: t.x, y0: t.y, x1: t.x, y1: t.y };
    draw();
    return;
  }

  const existing = at(t.x, t.y);
  if (existing) {
    if (existing.m === "vault") return;
    drag = { mode: "maybe-move", start: t }; // click or move — decided on release
    return;
  }

  ui.selected = null;
  ui.multiSel.clear();
  if (ui.tool.kind === "belt") {
    drag = { mode: "belt", path: [t] };
  } else {
    placeAt(t.x, t.y, ui.dir);
  }
  paintAll();
});

cv.addEventListener("pointermove", (e) => {
  const t = toTile(e);
  hover = t;
  if (drag && t) {
    if (drag.mode === "belt") {
      const last = drag.path[drag.path.length - 1];
      // only extend along orthogonal steps, so diagonal flicks don't skip tiles
      if (last && Math.abs(t.x - last.x) + Math.abs(t.y - last.y) === 1 &&
          (last.x !== t.x || last.y !== t.y)) {
        drag.path.push(t);
      }
    } else if (drag.mode === "band") {
      drag.x1 = t.x; drag.y1 = t.y;
    } else if (drag.mode === "maybe-move") {
      if (t.x !== drag.start.x || t.y !== drag.start.y) {
        const startKey = tk(drag.start.x, drag.start.y);
        const tiles = ui.multiSel.has(startKey) ? new Set(ui.multiSel) : new Set([startKey]);
        ui.multiSel = new Set(tiles);
        drag = { mode: "move", start: drag.start, tiles, dx: t.x - drag.start.x, dy: t.y - drag.start.y };
      }
    } else if (drag.mode === "move") {
      drag.dx = t.x - drag.start.x;
      drag.dy = t.y - drag.start.y;
    }
  }
  draw();
});

cv.addEventListener("pointerup", () => {
  if (!drag) { return; }
  const d = drag;
  drag = null;

  if (d.mode === "belt") {
    commitBeltRun(d.path);
  } else if (d.mode === "band") {
    const [xa, xb] = [Math.min(d.x0, d.x1), Math.max(d.x0, d.x1)];
    const [ya, yb] = [Math.min(d.y0, d.y1), Math.max(d.y0, d.y1)];
    ui.multiSel = new Set(
      G.board
        .filter((p) => p.m !== "vault" && p.x >= xa && p.x <= xb && p.y >= ya && p.y <= yb)
        .map((p) => tk(p.x, p.y)),
    );
    ui.selected = null;
  } else if (d.mode === "maybe-move") {
    // never left the tile: it's a click — select, or rotate if already selected
    const { x, y } = d.start;
    if (ui.selected && ui.selected.x === x && ui.selected.y === y) {
      cmd(core.rotate(x, y));
    }
    if (!ui.multiSel.has(tk(x, y))) ui.multiSel.clear();
    ui.selected = { x, y };
    const now = at(x, y);
    if (now?.d) ui.dir = now.d;
  } else if (d.mode === "move") {
    commitMove(d);
  }
  paintAll();
});

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    ui.multiSel.clear();
    ui.selected = null;
    drag = null;
    paintAll();
    return;
  }
  if (buildLocked()) {
    if (e.key === " ") e.preventDefault();
    return;
  }
  if (e.key === "r" || e.key === "R") {
    if (ui.selected) cmd(core.rotate(ui.selected.x, ui.selected.y));
    else ui.dir = turn(ui.dir);
    paintAll();
  }
  if (e.key === "b" || e.key === "B") { ui.tool = { kind: "belt" }; paintAll(); }
  if (e.key === "j" || e.key === "J") { ui.tool = { kind: "junction" }; paintAll(); }
  if (e.key === "m" || e.key === "M") { ui.tool = { kind: "merger" }; paintAll(); }
  if (e.key === "s" || e.key === "S") { ui.tool = { kind: "splitter" }; paintAll(); }
  if (e.key === " ") { e.preventDefault(); runShift(); }
  const n = parseInt(e.key, 10);
  if (n >= 1 && n <= 9 && G.hand[n - 1]) {
    ui.tool = { kind: "card", idx: n - 1 };
    paintAll();
  }
});

// ── the shift ────────────────────────────────────────────────────────────────
let raf = 0, acc = 0, last = 0;

function runShift() {
  if (buildLocked()) return;
  if (!cmd(core.shift_start())) return;
  ui.animating = true;
  ui.positions.clear();
  ui.tick = 0;
  ui.payout = 0;
  (document.getElementById("bRun") as HTMLButtonElement).disabled = true;
  last = performance.now();
  acc = 0;
  raf = requestAnimationFrame(frameLoop);
}

function applyFrame(f: Frame) {
  const prev = new Map(ui.positions);
  ui.positions.clear();
  const from = new Map(f.moves.map((m) => [m.id, m]));
  for (const it of f.items) {
    const m = from.get(it.id);
    const old = prev.get(it.id);
    const px = m ? m.fx : old ? old.x : it.x;
    const py = m ? m.fy : old ? old.y : it.y;
    ui.positions.set(it.id, { x: it.x, y: it.y, px, py, t: it.t, q: it.q });
  }
  // consumed items still animate their last hop — into the machine or vault —
  // and are gone from the next frame
  for (const h of f.hops) {
    ui.positions.set(h.id, { x: h.x, y: h.y, px: h.fx, py: h.fy, t: h.t, q: h.q });
  }
  ui.tick = f.tick;
  ui.payout = f.payout;
}

function frameLoop(now: number) {
  const msPerTick = 1000 / (7 * ui.speed);
  acc += Math.min(now - last, 200);
  last = now;

  let done = false;
  while (acc >= msPerTick && !done) {
    acc -= msPerTick;
    const f = read<Frame>(core.shift_step());
    applyFrame(f);
    done = f.done;
  }
  ui.alpha = Math.min(1, acc / msPerTick);

  paintHeader();
  draw();

  if (done) { endShift(); return; }
  raf = requestAnimationFrame(frameLoop);
}

function endShift() {
  cancelAnimationFrame(raf);
  ui.animating = false;
  ui.positions.clear();
  (document.getElementById("bRun") as HTMLButtonElement).disabled = false;
  cmd(core.shift_finish());
  if (G.phase === "shop") openShop();
  else if (G.phase === "over") G.won ? victory() : gameOver();
  paintAll();
}

// ── modals ───────────────────────────────────────────────────────────────────
function modal(html: string) {
  const m = document.getElementById("modal")!;
  document.getElementById("modalCard")!.innerHTML = html;
  m.classList.add("on");
}
function closeModal() { document.getElementById("modal")!.classList.remove("on"); }

function openShop() {
  const o = G.last!;
  const surplus = o.payout - o.quota;
  const full = G.hand.length >= G.handMax;
  modal(
    `<h3>The shop</h3>
     <p>Delivered <b style="color:var(--vault)">${o.payout.toLocaleString()}</b> against a quota of
        ${o.quota.toLocaleString()} — surplus of ${surplus.toLocaleString()} banked.
        ${o.inFlight ? `${o.inFlight} items stranded on belts were forfeit.` : ""}
        <b>Round ${G.round + 2}</b> needs
        <b style="color:var(--extractor)">${(G.nextQuota ?? 0).toLocaleString()}</b>${G.nextAudit
          ? ` — an <b style="color:var(--processor)">AUDIT</b>.`
          : "."}</p>
     <div class="kv" style="margin-bottom:10px;gap:7px;justify-content:flex-start"><span>Credits</span>
        <b style="color:var(--extractor)">${G.credits}</b>
        <span style="margin-left:auto">Hand</span><b>${G.hand.length}/${G.handMax}</b></div>
     <div class="offers">${G.offers.map((o, i) => {
       if (o.type === "directive") {
         const afford = G.credits >= o.price;
         return `<div class="off dir${afford ? "" : " no"}" data-i="${i}"
             style="border-color:${TAG_COLOR[o.tag]}66">
           <div class="sw" style="background:${TAG_COLOR[o.tag]}">◆</div>
           <div class="nm">${o.name}</div>
           <div class="ds"><b>${o.price}c</b> · ${o.tag.toUpperCase()} doctrine</div>
           <div class="bl">${o.blurb}</div></div>`;
       }
       const afford = G.credits >= o.price && !full;
       return `<div class="off${afford ? "" : " no"}" data-i="${i}">
         <div class="sw" style="background:${CAT[o.kind]}">${SHORT[o.m] || "▸"}</div>
         <div class="nm">${o.name}</div>
         <div class="ds"><b>${o.price}c</b> · ${mechShort(o.m)}</div>
         <div class="bl">${MDEF.get(o.m)?.blurb ?? ""}</div></div>`;
     }).join("")}</div>
     <div class="row">
       <button id="reroll"${G.credits >= G.rerollPrice ? "" : " disabled"}>Reroll rack — ${G.rerollPrice}c</button>
       <button class="go" id="shopDone">Start round ${G.round + 2}</button>
     </div>
     ${full ? `<p style="margin:10px 0 0;font-size:12px">Hand full — right-click a hand card in the sidebar to sell it.</p>` : ""}`,
  );
  document.querySelectorAll<HTMLElement>(".off[data-i]").forEach((el) => {
    el.onclick = () => {
      if (cmd(core.shop_buy(+el.dataset.i!))) paintAll();
      openShop(); // re-render the rack either way
    };
  });
  (document.getElementById("reroll") as HTMLElement).onclick = () => {
    cmd(core.shop_reroll());
    openShop();
  };
  (document.getElementById("shopDone") as HTMLElement).onclick = () => {
    cmd(core.shop_done());
    closeModal();
    paintAll();
  };
}

function gameOver() {
  const o = G.last!;
  modal(
    `<h3>Quota missed</h3>
     <p>You delivered <b style="color:var(--processor)">${o.payout.toLocaleString()}</b> against
        <b>${o.quota.toLocaleString()}</b> on round ${o.round + 1} — short by
        ${(o.quota - o.payout).toLocaleString()}.</p>
     <div class="row">
       <button class="go" id="retry">Retry the round</button>
       <button id="again">New run</button>
     </div>`,
  );
  (document.getElementById("retry") as HTMLElement).onclick = () => {
    if (cmd(core.retry())) {
      closeModal();
      paintAll();
    }
  };
  (document.getElementById("again") as HTMLElement).onclick = newRun;
}

function victory() {
  modal(
    `<h3>Run complete</h3>
     <p>All twelve rounds cleared, final audit included. That's the whole arc —
        and if this was fun, the design is worth building properly.</p>
     <button class="go" id="again">Run it again</button>`,
  );
  (document.getElementById("again") as HTMLElement).onclick = newRun;
}

function newRun() {
  cmd(core.boot(seed()));
  ui.tool = { kind: "belt" };
  ui.selected = null;
  ui.tick = 0;
  ui.payout = 0;
  closeModal();
  paintAll();
}

// ── boot ─────────────────────────────────────────────────────────────────────
(document.getElementById("bRun") as HTMLElement).onclick = runShift;
(document.getElementById("bSpeed") as HTMLElement).onclick = (e) => {
  ui.speed = ui.speed === 1 ? 4 : ui.speed === 4 ? 16 : 1;
  (e.target as HTMLElement).textContent = `Speed ${ui.speed}×`;
};
(document.getElementById("bReset") as HTMLElement).onclick = () => {
  if (buildLocked()) return;
  for (const c of [...G.board]) if (c.m !== "vault") cmd(core.sell(c.x, c.y));
  paintAll();
};

window.addEventListener("resize", () => { layout(); draw(); });

cmd(core.boot(seed()));
layout();
paintAll();

modal(
  `<h3>OVERFLOW</h3>
   <p>Machines are <b>blueprints</b> you own: place them, move them, pull them back
      to your hand — all free. Belts and junctions are cheap infrastructure. Route
      items into the <b style="color:var(--vault)">Vault</b> and beat the quota in
      ${G.shiftLen} ticks; the surplus is yours, and between rounds the
      <b>shop</b> sells new blueprints. That surplus is your growth — overshoot
      the quota as far as you can.</p>
   <p>Start simple: a <b>Drill</b>, a run of <b>Belt</b> dragged toward the Vault, and a
      <b>Furnace</b> in the middle to turn Ore into Ingots. Watch the projection panel —
      it runs the whole shift for you before you commit.</p>
   <button class="go" id="start">Start shift 1</button>`,
);
(document.getElementById("start") as HTMLElement).onclick = closeModal;

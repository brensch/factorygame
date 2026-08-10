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
  audit: boolean; phase: "build" | "reward" | "over"; won: boolean;
  qualityCap: number; boardW: number; boardH: number;
  board: Pl[]; hand: Card[]; offers: Card[];
  deckDraw: number; deckDiscard: number;
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
  boot(seed: number): number;
  state(): number;
  belt(x: number, y: number, d: number): number;
  play(i: number, x: number, y: number, d: number, d2: number, minQ: number): number;
  sell(x: number, y: number): number;
  rotate(x: number, y: number): number;
  rotate2(x: number, y: number): number;
  set_gate(x: number, y: number, q: number): number;
  pick_reward(i: number): number;
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
type Tool = { kind: "belt" } | { kind: "card"; idx: number };

let G: State;
const ui = {
  tool: { kind: "belt" } as Tool,
  dir: "E" as Dir,
  selected: null as { x: number; y: number } | null,
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

    if (c.m === "belt" || c.m === "merger") {
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
  if (G.phase === "build" && !ui.animating && hover && !at(hover.x, hover.y) && !dragging) {
    const mKey = ui.tool.kind === "belt" ? "belt" : G.hand[ui.tool.idx]?.m;
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
      if (m.transport || m.produces || m.recipe) {
        const hcx = hover.x * TILE + TILE / 2, hcy = hover.y * TILE + TILE / 2;
        arrow(hcx, hcy, ui.dir, TILE / 2 - 6, "#ffffffaa");
      }
    }
  }

  // live preview of the belt run being dragged
  if (dragging && dragPath.length) {
    ctx.strokeStyle = "#8ea1b877";
    ctx.lineWidth = Math.max(5, TILE * 0.14);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.beginPath();
    dragPath.forEach((p, i) => {
      const px = p.x * TILE + TILE / 2, py = p.y * TILE + TILE / 2;
      if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
    });
    ctx.stroke();
    const last = dragPath[dragPath.length - 1];
    const prev = dragPath[dragPath.length - 2];
    const d: Dir = prev
      ? (last.x > prev.x ? "E" : last.x < prev.x ? "W" : last.y > prev.y ? "S" : "N")
      : ui.dir;
    arrow(last.x * TILE + TILE / 2, last.y * TILE + TILE / 2, d, TILE / 2 - 8, "#ffffffaa");
  }
}

// ── placement ────────────────────────────────────────────────────────────────
function placeAt(x: number, y: number, d: Dir) {
  if (ui.tool.kind === "belt") {
    cmd(core.belt(x, y, DCODE[d]));
    return;
  }
  const card = G.hand[ui.tool.idx];
  if (!card) return;
  // Filters eject sideways by default; splitters split sideways. The core
  // validates — these are just sensible initial edges the player can rotate.
  const d2 = card.m === "filter" || card.m === "splitter" ? turn(d) : null;
  const minQ = card.m === "filter" ? 5 : -1;
  if (cmd(core.play(ui.tool.idx, x, y, DCODE[d], dc(d2), minQ))) {
    ui.tool = { kind: "belt" }; // the card is consumed; drop back to belts
  }
}

// ── side panel ───────────────────────────────────────────────────────────────
function paintHand() {
  const pal = document.getElementById("pal")!;
  pal.innerHTML = "";

  const beltEl = document.createElement("div");
  beltEl.className = "pi" + (ui.tool.kind === "belt" ? " on" : "") + (G.credits >= 1 ? "" : " no");
  beltEl.innerHTML =
    `<div class="sw" style="background:${CAT.logistics}">▸</div>` +
    `<div class="nm">Belt</div><div class="cs">1c</div>`;
  beltEl.onclick = () => { ui.tool = { kind: "belt" }; ui.selected = null; paintAll(); };
  pal.appendChild(beltEl);

  G.hand.forEach((card, i) => {
    const el = document.createElement("div");
    const afford = G.credits >= card.cost;
    const on = ui.tool.kind === "card" && ui.tool.idx === i;
    el.className = "pi" + (on ? " on" : "") + (afford ? "" : " no");
    el.innerHTML =
      `<div class="sw" style="background:${CAT[card.kind]}">${SHORT[card.m] || "▸"}</div>` +
      `<div class="nm">${card.name}</div><div class="cs">${card.cost}c</div>`;
    el.onclick = () => { ui.tool = { kind: "card", idx: i }; ui.selected = null; paintAll(); };
    pal.appendChild(el);
  });

  (document.getElementById("deckInfo") as HTMLElement).textContent =
    `deck ${G.deckDraw} · discard ${G.deckDiscard}`;
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
  // Priority: a selected placed machine, else the card in hand, else the belt.
  let key = "belt";
  if (ui.selected) key = at(ui.selected.x, ui.selected.y)?.m ?? "belt";
  else if (ui.tool.kind === "card") key = G.hand[ui.tool.idx]?.m ?? "belt";
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

function paintAll() {
  paintHand();
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

// ── input ────────────────────────────────────────────────────────────────────
let hover: { x: number; y: number } | null = null;
let dragging = false;
let dragPath: Array<{ x: number; y: number }> = [];

function toTile(ev: MouseEvent | Touch) {
  const r = cv.getBoundingClientRect();
  const x = Math.floor((ev.clientX - r.left) / TILE);
  const y = Math.floor((ev.clientY - r.top) / TILE);
  return x >= 0 && y >= 0 && x < G.boardW && y < G.boardH ? { x, y } : null;
}

function commitDrag() {
  // Lay a belt run along the dragged path, auto-orienting each tile toward
  // the next. Refusals (occupied tiles, empty wallet) surface as toasts.
  for (let i = 0; i < dragPath.length; i++) {
    const a = dragPath[i], b = dragPath[i + 1];
    let d: Dir = ui.dir;
    if (b) {
      d = b.x > a.x ? "E" : b.x < a.x ? "W" : b.y > a.y ? "S" : "N";
    } else if (i > 0) {
      const p = dragPath[i - 1];
      d = a.x > p.x ? "E" : a.x < p.x ? "W" : a.y > p.y ? "S" : "N";
    }
    if (!at(a.x, a.y)) cmd(core.belt(a.x, a.y, DCODE[d]));
  }
  dragPath = [];
}

const buildLocked = () => G.phase !== "build" || ui.animating;

cv.addEventListener("contextmenu", (e) => e.preventDefault());

cv.addEventListener("pointerdown", (e) => {
  if (buildLocked()) return;
  const t = toTile(e);
  if (!t) return;
  cv.setPointerCapture(e.pointerId);

  if (e.button === 2) { cmd(core.sell(t.x, t.y)); paintAll(); return; }

  const existing = at(t.x, t.y);
  if (existing) {
    if (existing.m === "vault") return;
    // clicking a placed machine selects it; clicking again rotates it
    if (ui.selected && ui.selected.x === t.x && ui.selected.y === t.y) {
      cmd(core.rotate(t.x, t.y));
    }
    ui.selected = { x: t.x, y: t.y };
    const now = at(t.x, t.y);
    if (now?.d) ui.dir = now.d;
    paintAll();
    return;
  }

  ui.selected = null;
  if (ui.tool.kind === "belt") {
    dragging = true;
    dragPath = [t];
  } else {
    placeAt(t.x, t.y, ui.dir);
  }
  paintAll();
});

cv.addEventListener("pointermove", (e) => {
  const t = toTile(e);
  hover = t;
  if (dragging && t) {
    const last = dragPath[dragPath.length - 1];
    if (!last || last.x !== t.x || last.y !== t.y) {
      // only extend along orthogonal steps, so diagonal flicks don't skip tiles
      if (last && Math.abs(t.x - last.x) + Math.abs(t.y - last.y) === 1) dragPath.push(t);
    }
  }
  draw();
});

cv.addEventListener("pointerup", () => {
  if (dragging) { commitDrag(); dragging = false; paintAll(); }
});

window.addEventListener("keydown", (e) => {
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
  if (G.phase === "reward") offerRewards();
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

function offerRewards() {
  const o = G.last!;
  const surplus = o.payout - o.quota;
  modal(
    `<h3>Shift complete</h3>
     <p>Delivered <b style="color:var(--vault)">${o.payout.toLocaleString()}</b> against a quota of
        ${o.quota.toLocaleString()}. Surplus of ${surplus.toLocaleString()} banked —
        you now have <b>${G.credits}</b> credits.
        ${o.inFlight} items were still on belts and were forfeit.</p>
     <p style="margin-bottom:10px"><b>Round ${G.round + 2}</b> needs
        <b style="color:var(--extractor)">${(G.nextQuota ?? 0).toLocaleString()}</b>${G.nextAudit
          ? ` — and it's an <b style="color:var(--processor)">AUDIT</b>.`
          : "."} Add one blueprint card to your deck:</p>
     <div class="offers">${G.offers.map((c, i) =>
       `<div class="off" data-i="${i}">
         <div class="sw" style="background:${CAT[c.kind]}">${SHORT[c.m] || "▸"}</div>
         <div class="nm">${c.name}</div>
         <div class="ds">${c.cost}c · ${mechShort(c.m)}</div>
         <div class="bl">${MDEF.get(c.m)?.blurb ?? ""}</div></div>`).join("")}</div>
     <button data-i="-1" style="width:100%">Skip — keep the deck lean</button>`,
  );
  document.querySelectorAll<HTMLElement>("[data-i]").forEach((el) => {
    el.onclick = () => {
      cmd(core.pick_reward(+el.dataset.i!));
      closeModal();
      paintAll();
    };
  });
}

function gameOver() {
  const o = G.last!;
  modal(
    `<h3>Quota missed</h3>
     <p>You delivered <b style="color:var(--processor)">${o.payout.toLocaleString()}</b> against
        <b>${o.quota.toLocaleString()}</b> on round ${o.round + 1}. The contract is terminated.</p>
     <button class="go" id="again">New run</button>`,
  );
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
   <p>Your run is a <b>deck of blueprint cards</b>. Each round deals you a hand —
      you can only place what you were dealt. Belts are always available for 1 credit.
      Route items into the <b style="color:var(--vault)">Vault</b> and beat the quota
      in ${G.shiftLen} ticks; surplus becomes credits, and each cleared shift adds
      one card of your choice to the deck.</p>
   <p>Start simple: a <b>Drill</b>, a run of <b>Belt</b> dragged toward the Vault, and a
      <b>Furnace</b> in the middle to turn Ore into Ingots. Watch the projection panel —
      it runs the whole shift for you before you commit. Placed machines are consumed
      from the deck; selling one (right-click) refunds it back.</p>
   <button class="go" id="start">Start shift 1</button>`,
);
(document.getElementById("start") as HTMLElement).onclick = closeModal;

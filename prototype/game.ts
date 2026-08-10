/**
 * OVERFLOW prototype — playable shell around the reference simulation.
 *
 * This file owns presentation and the run structure (rounds, quota, shop).
 * It owns NO game rules: every rule comes from ../sim/src. That's deliberate —
 * the sim is under test, and the prototype must not be allowed to quietly grow
 * a second, untested copy of the mechanics.
 */

import { Sim, type Placement, type Item } from "../sim/src/sim";
import { DEFS, DIRV, type Dir } from "../sim/src/defs";

// ── board / run configuration ────────────────────────────────────────────────
const W = 10, H = 7;
const QUOTAS = [20, 45, 90, 200, 400, 700, 1200, 2400, 4500, 8000, 14000, 30000];
const AUDIT = [3, 7, 11];                    // zero-based round indices
const SHIFT = (r: number) => (r === 11 ? 30 : 60); // final audit halves the shift

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

const START_UNLOCKED = ["belt", "drill", "furnace"];
const UNLOCK_POOL = [
  "polisher", "heatsink", "overclock", "merger", "fab", "splitter", "filter",
  "buffer", "dup", "tap", "retort", "geode", "lapidary", "circuit", "lens",
  "compress", "engine",
];

// ── game state ───────────────────────────────────────────────────────────────
type Phase = "build" | "shift" | "over";

interface Cell extends Placement {}

const S = {
  round: 0,
  credits: 15,
  phase: "build" as Phase,
  cells: new Map<string, Cell>(),
  unlocked: new Set(START_UNLOCKED),
  tool: "belt",
  dir: "E" as Dir,
  selected: null as string | null,
  sim: null as Sim | null,
  speed: 1,
  rngSeed: 12345,
  positions: new Map<number, { x: number; y: number; px: number; py: number; item: Item }>(),
  alpha: 0,
};

const key = (x: number, y: number) => `${x},${y}`;
const quota = () => QUOTAS[S.round];
const isAudit = () => AUDIT.includes(S.round);

function rand() {
  S.rngSeed = (S.rngSeed * 1664525 + 1013904223) >>> 0;
  return S.rngSeed / 4294967296;
}

// The Vault starts bolted to the east edge, as in the design doc.
S.cells.set(key(W - 1, 3), { x: W - 1, y: 3, t: "vault" });

// ── canvas ───────────────────────────────────────────────────────────────────
const cv = document.getElementById("cv") as HTMLCanvasElement;
const ctx = cv.getContext("2d")!;
let TILE = 54, OX = 0, OY = 0;

function layout() {
  const stage = document.getElementById("stage")!;
  const r = stage.getBoundingClientRect();
  TILE = Math.max(26, Math.floor(Math.min((r.width - 24) / W, (r.height - 24) / H)));
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  cv.width = W * TILE * dpr;
  cv.height = H * TILE * dpr;
  cv.style.width = W * TILE + "px";
  cv.style.height = H * TILE + "px";
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  OX = 0; OY = 0;
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

function draw() {
  ctx.clearRect(0, 0, cv.width, cv.height);

  // floor
  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
    ctx.fillStyle = "#151a22";
    ctx.strokeStyle = "#222b36";
    ctx.lineWidth = 1;
    rr(OX + x * TILE + 1.5, OY + y * TILE + 1.5, TILE - 3, TILE - 3, 5);
    ctx.fill(); ctx.stroke();
  }

  // aura halos, under machines
  for (const c of S.cells.values()) {
    const def = DEFS[c.t];
    if (!def?.aura) continue;
    for (const d of Object.keys(DIRV) as Dir[]) {
      const [dx, dy] = DIRV[d];
      const n = S.cells.get(key(c.x + dx, c.y + dy));
      if (!n) continue;
      if (def.aura.onlyTag && !DEFS[n.t].tags.includes(def.aura.onlyTag)) continue;
      ctx.strokeStyle = CAT.modifier;
      ctx.lineWidth = 1.5;
      ctx.setLineDash([4, 3]);
      rr(OX + n.x * TILE + 3, OY + n.y * TILE + 3, TILE - 6, TILE - 6, 6);
      ctx.stroke();
      ctx.setLineDash([]);
    }
  }

  // machines
  for (const c of S.cells.values()) {
    const def = DEFS[c.t];
    const col = CAT[def.kind];
    const cx = OX + c.x * TILE + TILE / 2, cy = OY + c.y * TILE + TILE / 2;

    if (c.t === "belt" || c.t === "merger") {
      ctx.strokeStyle = "#3a4655";
      ctx.lineWidth = Math.max(6, TILE * 0.17);
      ctx.lineCap = "round";
      const [ox, oy] = DIRV[c.d!];
      ctx.beginPath();
      ctx.moveTo(cx - ox * (TILE / 2 - 3), cy - oy * (TILE / 2 - 3));
      ctx.lineTo(cx + ox * (TILE / 2 - 3), cy + oy * (TILE / 2 - 3));
      ctx.stroke();
      arrow(cx, cy, c.d!, TILE / 2 - 9, "#8ea1b8");
      if (c.t === "merger") {
        ctx.fillStyle = "#8ea1b8";
        ctx.font = `700 ${Math.max(7, TILE * 0.18)}px ui-monospace,monospace`;
        ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText("MRG", cx, cy + TILE * 0.3);
      }
    } else {
      ctx.fillStyle = col;
      ctx.globalAlpha = 0.92;
      rr(OX + c.x * TILE + 4, OY + c.y * TILE + 4, TILE - 8, TILE - 8, 6);
      ctx.fill();
      ctx.globalAlpha = 1;

      if (S.selected === key(c.x, c.y)) {
        ctx.strokeStyle = "#fff"; ctx.lineWidth = 2;
        rr(OX + c.x * TILE + 1.5, OY + c.y * TILE + 1.5, TILE - 3, TILE - 3, 7);
        ctx.stroke();
      }

      ctx.fillStyle = "#0b0e13";
      ctx.font = `700 ${Math.max(8, TILE * 0.21)}px ui-monospace,monospace`;
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillText(SHORT[c.t] ?? "", cx, cy);

      if (c.d && def.kind !== "modifier" && def.kind !== "vault") arrow(cx, cy, c.d, TILE / 2 - 4, col);
      if (c.d2) arrow(cx, cy, c.d2, TILE / 2 - 4, "#ffffff88");

      if (c.t === "filter") {
        ctx.fillStyle = "#0b0e13";
        ctx.font = `700 ${Math.max(7, TILE * 0.16)}px ui-monospace,monospace`;
        ctx.fillText("≥" + (c.cfg?.minQuality ?? 5), cx, cy + TILE * 0.26);
      }
    }
  }

  // items
  if (S.sim) {
    for (const p of S.positions.values()) {
      const x = p.px + (p.x - p.px) * S.alpha;
      const y = p.py + (p.y - p.py) * S.alpha;
      const cx = OX + x * TILE + TILE / 2, cy = OY + y * TILE + TILE / 2;
      const r = Math.max(3.5, TILE * 0.11);
      ctx.fillStyle = ITEM_COLOR[p.item.type] ?? "#fff";
      ctx.strokeStyle = "#0b0e13";
      ctx.lineWidth = 2;
      ctx.beginPath(); ctx.arc(cx, cy, r, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
      if (p.item.quality > 0) {
        ctx.fillStyle = "#fff";
        ctx.font = `700 ${Math.max(7, TILE * 0.15)}px ui-monospace,monospace`;
        ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText(String(p.item.quality), cx, cy - r - Math.max(5, TILE * 0.13));
      }
    }
  }

  // ghost of the tool under the cursor
  if (S.phase === "build" && hover && !S.cells.has(key(hover.x, hover.y))) {
    const def = DEFS[S.tool];
    ctx.globalAlpha = 0.32;
    ctx.fillStyle = CAT[def.kind];
    rr(OX + hover.x * TILE + 4, OY + hover.y * TILE + 4, TILE - 8, TILE - 8, 6);
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.strokeStyle = "#ffffff55"; ctx.lineWidth = 1.5;
    rr(OX + hover.x * TILE + 2, OY + hover.y * TILE + 2, TILE - 4, TILE - 4, 7);
    ctx.stroke();
  }
}

// ── placement ────────────────────────────────────────────────────────────────
function cost(t: string) { return DEFS[t].cost; }

function place(x: number, y: number, t: string, d: Dir) {
  if (x < 0 || y < 0 || x >= W || y >= H) return false;
  if (S.cells.has(key(x, y))) return false;
  if (S.credits < cost(t)) return false;
  S.credits -= cost(t);
  const c: Cell = { x, y, t, d };
  if (t === "filter") { c.d2 = turn(d, 1); c.cfg = { minQuality: 5 }; }
  if (t === "splitter") { c.d2 = turn(d, 1); }
  S.cells.set(key(x, y), c);
  return true;
}

function remove(x: number, y: number) {
  const k = key(x, y);
  const c = S.cells.get(k);
  if (!c || c.t === "vault") return;
  S.credits += cost(c.t);          // full refund during build: experimenting is free
  S.cells.delete(k);
  if (S.selected === k) S.selected = null;
}

const ORDER: Dir[] = ["N", "E", "S", "W"];
function turn(d: Dir, n: number): Dir {
  return ORDER[(ORDER.indexOf(d) + n + 4) % 4];
}

// ── projection (the preview sim from the design doc) ──────────────────────────
function project() {
  const cells = [...S.cells.values()];
  const r = new Sim(W, H, cells, 1).run(SHIFT(S.round));
  (document.getElementById("pProj") as HTMLElement).textContent = r.payout.toLocaleString();
  (document.getElementById("pQuota") as HTMLElement).textContent = quota().toLocaleString();
  (document.getElementById("pFlight") as HTMLElement).textContent = String(r.inFlight);
  (document.getElementById("pJam") as HTMLElement).textContent = String(r.jamTicks);
  const bar = document.getElementById("pBar") as HTMLElement;
  const pct = Math.min(100, (r.payout / quota()) * 100);
  bar.style.width = pct + "%";
  bar.className = r.payout >= quota() ? "" : "short";
  return r;
}

// ── UI ───────────────────────────────────────────────────────────────────────
function paintPalette() {
  const pal = document.getElementById("pal")!;
  pal.innerHTML = "";
  const list = [...S.unlocked].sort((a, b) => cost(a) - cost(b));
  list.forEach((t) => {
    const def = DEFS[t];
    const el = document.createElement("div");
    const afford = S.credits >= def.cost;
    el.className = "pi" + (S.tool === t ? " on" : "") + (afford ? "" : " no");
    el.innerHTML =
      `<div class="sw" style="background:${CAT[def.kind]}">${SHORT[t] || "▸"}</div>` +
      `<div class="nm">${def.name}</div><div class="cs">${def.cost}c</div>`;
    el.onclick = () => { S.tool = t; S.selected = null; paintAll(); };
    pal.appendChild(el);
  });
}

function paintInspector() {
  const box = document.getElementById("insp")!;
  const body = document.getElementById("inspBody")!;
  const c = S.selected ? S.cells.get(S.selected) : null;
  if (!c || (c.t !== "filter" && c.t !== "splitter")) { box.classList.remove("on"); return; }
  box.classList.add("on");
  if (c.t === "filter") {
    body.innerHTML =
      `<div class="kv"><span>Eject when quality ≥</span><b>${c.cfg?.minQuality ?? 5}</b></div>
       <div class="row" style="margin-top:8px">
         <button id="qDown">− gate</button><button id="qUp">+ gate</button>
         <button id="dRot">turn eject</button>
       </div>
       <div id="hint" style="margin-top:9px">Items below the gate carry on round the loop.
         Higher gate = more laps = more value per item, but fewer items get out.</div>`;
    (document.getElementById("qUp") as HTMLElement).onclick = () => {
      c.cfg = { minQuality: Math.min(10, (c.cfg?.minQuality ?? 5) + 1) }; paintAll();
    };
    (document.getElementById("qDown") as HTMLElement).onclick = () => {
      c.cfg = { minQuality: Math.max(0, (c.cfg?.minQuality ?? 5) - 1) }; paintAll();
    };
    (document.getElementById("dRot") as HTMLElement).onclick = () => {
      c.d2 = turn(c.d2 ?? "N", 1); paintAll();
    };
  } else {
    body.innerHTML = `<div class="kv"><span>Second output</span><b>${c.d2}</b></div>
      <div class="row" style="margin-top:8px"><button id="dRot">turn 2nd output</button></div>`;
    (document.getElementById("dRot") as HTMLElement).onclick = () => {
      c.d2 = turn(c.d2 ?? "N", 1); paintAll();
    };
  }
}

function paintHeader(tick = 0, payout = 0) {
  const g = (id: string) => document.getElementById(id) as HTMLElement;
  g("hRound").textContent = String(S.round + 1) + (isAudit() ? " ⚠" : "");
  g("hQuota").textContent = quota().toLocaleString();
  g("hCred").textContent = String(Math.floor(S.credits));
  g("hTick").textContent = `${tick}/${SHIFT(S.round)}`;
  g("hPay").textContent = payout.toLocaleString();
}

function paintAll() {
  paintPalette();
  paintInspector();
  paintHeader(S.sim?.tick ?? 0, S.sim?.result().payout ?? 0);
  if (S.phase === "build") project();
  draw();
}

// ── input ────────────────────────────────────────────────────────────────────
let hover: { x: number; y: number } | null = null;
let dragging = false;
let dragPath: Array<{ x: number; y: number }> = [];

function toTile(ev: MouseEvent | Touch) {
  const r = cv.getBoundingClientRect();
  const x = Math.floor((ev.clientX - r.left) / TILE);
  const y = Math.floor((ev.clientY - r.top) / TILE);
  return x >= 0 && y >= 0 && x < W && y < H ? { x, y } : null;
}

function commitDrag() {
  // Lay a belt run along the dragged path, auto-orienting each tile toward the next.
  for (let i = 0; i < dragPath.length; i++) {
    const a = dragPath[i], b = dragPath[i + 1];
    let d: Dir = S.dir;
    if (b) {
      d = b.x > a.x ? "E" : b.x < a.x ? "W" : b.y > a.y ? "S" : "N";
    } else if (i > 0) {
      const p = dragPath[i - 1];
      d = a.x > p.x ? "E" : a.x < p.x ? "W" : a.y > p.y ? "S" : "N";
    }
    place(a.x, a.y, S.tool, d);
  }
  dragPath = [];
}

cv.addEventListener("contextmenu", (e) => e.preventDefault());

cv.addEventListener("pointerdown", (e) => {
  if (S.phase !== "build") return;
  const t = toTile(e);
  if (!t) return;
  cv.setPointerCapture(e.pointerId);

  if (e.button === 2) { remove(t.x, t.y); paintAll(); return; }

  const existing = S.cells.get(key(t.x, t.y));
  if (existing) {
    if (existing.t === "vault") return;
    // clicking a placed machine selects it and rotates it
    if (S.selected === key(t.x, t.y)) existing.d = turn(existing.d ?? "E", 1);
    S.selected = key(t.x, t.y);
    S.dir = existing.d ?? "E";
    paintAll();
    return;
  }

  S.selected = null;
  dragging = true;
  dragPath = [t];
  if (S.tool !== "belt") { place(t.x, t.y, S.tool, S.dir); dragging = false; dragPath = []; }
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
  if (e.key === "r" || e.key === "R") {
    if (S.selected) {
      const c = S.cells.get(S.selected)!;
      c.d = turn(c.d ?? "E", 1);
    } else S.dir = turn(S.dir, 1);
    paintAll();
  }
  if (e.key === " ") { e.preventDefault(); runShift(); }
  const n = parseInt(e.key, 10);
  if (n >= 1 && n <= 9) {
    const list = [...S.unlocked].sort((a, b) => cost(a) - cost(b));
    if (list[n - 1]) { S.tool = list[n - 1]; paintAll(); }
  }
});

// ── shift ────────────────────────────────────────────────────────────────────
let raf = 0, acc = 0, last = 0;

function runShift() {
  if (S.phase !== "build") return;
  S.phase = "shift";
  S.sim = new Sim(W, H, [...S.cells.values()], 1);
  S.positions.clear();
  (document.getElementById("bRun") as HTMLButtonElement).disabled = true;
  last = performance.now();
  acc = 0;
  raf = requestAnimationFrame(frame);
}

function snapshot() {
  const sim = S.sim!;
  const prev = new Map(S.positions);
  S.positions.clear();
  const moved = new Map<number, number>();
  for (const m of sim.moves) moved.set(m.id, m.from);

  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
    const v = sim.view(x, y);
    if (!v.out) continue;
    const from = moved.get(v.out.id);
    let px = x, py = y;
    if (from !== undefined) { px = from % W; py = Math.floor(from / W); }
    else { const old = prev.get(v.out.id); if (old) { px = old.x; py = old.y; } }
    S.positions.set(v.out.id, { x, y, px, py, item: v.out });
  }
}

function frame(now: number) {
  const sim = S.sim!;
  const total = SHIFT(S.round);
  const msPerTick = 1000 / (7 * S.speed);
  acc += Math.min(now - last, 200);
  last = now;

  while (acc >= msPerTick && sim.tick < total) {
    acc -= msPerTick;
    sim.step();
    snapshot();
  }
  S.alpha = Math.min(1, acc / msPerTick);

  paintHeader(sim.tick, sim.result().payout);
  draw();

  if (sim.tick >= total) { endShift(); return; }
  raf = requestAnimationFrame(frame);
}

function endShift() {
  cancelAnimationFrame(raf);
  const r = S.sim!.result(SHIFT(S.round));
  const passed = r.payout >= quota();
  if (!passed) return gameOver(r.payout);

  S.credits += r.payout - quota();
  S.round++;
  if (S.round >= QUOTAS.length) return victory();
  offerUnlock(r);
}

// ── modals ───────────────────────────────────────────────────────────────────
function modal(html: string) {
  const m = document.getElementById("modal")!;
  document.getElementById("modalCard")!.innerHTML = html;
  m.classList.add("on");
}
function closeModal() { document.getElementById("modal")!.classList.remove("on"); }

function offerUnlock(r: { payout: number; inFlight: number }) {
  const locked = UNLOCK_POOL.filter((t) => !S.unlocked.has(t));
  const picks: string[] = [];
  while (picks.length < 3 && locked.length) {
    picks.push(locked.splice(Math.floor(rand() * locked.length), 1)[0]);
  }
  const surplus = r.payout - QUOTAS[S.round - 1];
  modal(
    `<h3>Shift complete</h3>
     <p>Delivered <b style="color:var(--vault)">${r.payout.toLocaleString()}</b> against a quota of
        ${QUOTAS[S.round - 1].toLocaleString()}. Surplus of ${surplus.toLocaleString()} banked —
        you now have <b>${Math.floor(S.credits)}</b> credits.
        ${r.inFlight} items were still on belts and were forfeit.</p>
     <p style="margin-bottom:10px"><b>Round ${S.round + 1}</b> needs
        <b style="color:var(--extractor)">${quota().toLocaleString()}</b>${isAudit()
          ? ` — and it's an <b style="color:var(--processor)">AUDIT</b>.`
          : "."} Pick one machine to unlock:</p>
     <div class="offers">${picks.map((t, i) => {
       const d = DEFS[t];
       return `<div class="off" data-i="${i}">
         <div class="sw" style="background:${CAT[d.kind]}">${SHORT[t] || "▸"}</div>
         <div class="nm">${d.name}</div>
         <div class="ds">${d.cost}c · ${d.tags.join(" ") || "logistics"}</div></div>`;
     }).join("")}</div>` + (picks.length ? "" : `<button class="go" data-i="-1">Continue</button>`),
  );
  document.querySelectorAll<HTMLElement>("[data-i]").forEach((el) => {
    el.onclick = () => {
      const i = +el.dataset.i!;
      if (i >= 0) S.unlocked.add(picks[i]);
      closeModal();
      S.phase = "build";
      S.sim = null;
      S.positions.clear();
      (document.getElementById("bRun") as HTMLButtonElement).disabled = false;
      paintAll();
    };
  });
}

function gameOver(payout: number) {
  S.phase = "over";
  modal(
    `<h3>Quota missed</h3>
     <p>You delivered <b style="color:var(--processor)">${payout.toLocaleString()}</b> against
        <b>${quota().toLocaleString()}</b> on round ${S.round + 1}. The contract is terminated.</p>
     <button class="go" id="again">New run</button>`,
  );
  (document.getElementById("again") as HTMLElement).onclick = () => location.reload();
}

function victory() {
  S.phase = "over";
  modal(
    `<h3>Run complete</h3>
     <p>All twelve rounds cleared, final audit included. That's the whole arc —
        and if this was fun, the design is worth building properly.</p>
     <button class="go" id="again">Run it again</button>`,
  );
  (document.getElementById("again") as HTMLElement).onclick = () => location.reload();
}

// ── boot ─────────────────────────────────────────────────────────────────────
(document.getElementById("bRun") as HTMLElement).onclick = runShift;
(document.getElementById("bSpeed") as HTMLElement).onclick = (e) => {
  S.speed = S.speed === 1 ? 4 : S.speed === 4 ? 16 : 1;
  (e.target as HTMLElement).textContent = `Speed ${S.speed}×`;
};
(document.getElementById("bReset") as HTMLElement).onclick = () => {
  if (S.phase !== "build") return;
  for (const c of [...S.cells.values()]) if (c.t !== "vault") remove(c.x, c.y);
  paintAll();
};

window.addEventListener("resize", () => { layout(); draw(); });
layout();
paintAll();

modal(
  `<h3>OVERFLOW</h3>
   <p>Place machines on the grid, route items into the <b style="color:var(--vault)">Vault</b>,
      and beat the quota in 60 ticks. Surplus becomes credits; credits buy more machines.
      Twelve rounds, quota nearly doubling each time.</p>
   <p>Start simple: a <b>Drill</b>, a run of <b>Belt</b> dragged toward the Vault, and a
      <b>Furnace</b> in the middle to turn Ore into Ingots. Watch the projection panel —
      it runs the whole shift for you before you commit.</p>
   <button class="go" id="start">Start shift 1</button>`,
);
(document.getElementById("start") as HTMLElement).onclick = closeModal;

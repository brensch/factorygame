/**
 * Loads the BUILT wasm artifact — the same bytes the browser gets — and plays
 * a scripted round through the same ABI calls game.ts makes. This is the
 * browser-free end-to-end check: if this passes, the deployed bundle's engine
 * works; only rendering and input remain for human eyes.
 *
 * Run after `bun run build:play` (it reads docs/play/game.wasm).
 */

import { test, expect } from "bun:test";

const WASM = new URL("../docs/play/game.wasm", import.meta.url).pathname;

type Exports = {
  memory: WebAssembly.Memory;
  out_ptr(): number;
  boot(seed: number): number;
  state(): number;
  belt(x: number, y: number, d: number): number;
  play(i: number, x: number, y: number, d: number, d2: number, minQ: number): number;
  sell(x: number, y: number): number;
  rotate(x: number, y: number): number;
  project(): number;
  shift_start(): number;
  shift_step(): number;
  shift_finish(): number;
  shop_buy(i: number): number;
  shop_done(): number;
};

async function load(): Promise<{ core: Exports; read: (len: number) => any }> {
  const { instance } = await WebAssembly.instantiate(await Bun.file(WASM).arrayBuffer(), {});
  const core = instance.exports as unknown as Exports;
  const dec = new TextDecoder();
  const read = (len: number) =>
    JSON.parse(dec.decode(new Uint8Array(core.memory.buffer, core.out_ptr(), len)));
  return { core, read };
}

const E = 1; // direction code for east

test("the shipped wasm plays a full round over the ABI", async () => {
  const { core, read } = await load();

  let s = read(core.boot(42));
  expect(s.phase).toBe("build");
  expect(s.credits).toBe(40);
  expect(s.quota).toBe(85);
  expect(s.board.some((p: any) => p.m === "vault")).toBe(true);
  expect(s.hand.length).toBe(4);

  // Seed 42 deals 2 Drills + 2 Furnaces; build the whole kit compactly.
  for (const y of [9, 8]) {
    const drill = s.hand.findIndex((c: any) => c.m === "drill");
    s = read(core.play(drill, 13, y, E, -1, -1));
    expect(s.err).toBeNull();
    const furnace = s.hand.findIndex((c: any) => c.m === "furnace");
    s = read(core.play(furnace, 14, y, E, -1, -1));
    expect(s.err).toBeNull();
    s = read(core.belt(15, y, E));
    s = read(core.belt(16, y, y === 9 ? E : 2 /* south */));
  }
  expect(s.credits).toBe(40 - 4); // placement is free; belts aren't

  const p = read(core.project());
  expect(p.payout).toBeGreaterThanOrEqual(85);

  // Animate the shift to completion; items must appear along the way, and
  // deliveries must animate their final hop into the vault.
  read(core.shift_start());
  let sawItems = false;
  let sawVaultHop = false;
  let frame: any;
  for (let i = 0; i < 60; i++) {
    frame = read(core.shift_step());
    if (frame.items.length > 0) sawItems = true;
    if (frame.hops.some((h: any) => h.x === 17 && h.y === 9)) sawVaultHop = true;
    if (frame.done) break;
  }
  expect(frame.done).toBe(true);
  expect(frame.tick).toBe(60);
  expect(sawItems).toBe(true);
  expect(sawVaultHop).toBe(true);

  s = read(core.shift_finish());
  expect(s.phase).toBe("shop");
  expect(s.last.cleared).toBe(true);
  expect(s.last.payout).toBe(frame.payout); // what you watched is what you got
  expect(s.offers.length).toBe(5);
  expect(s.nextQuota).toBe(115);

  // prices scale with the curve; the wire carries the chance elements
  expect(typeof s.market).toBe("string");
  expect(s.rerollPrice).toBeGreaterThan(0);

  s = read(core.shop_done());
  expect(s.phase).toBe("build");
  expect(s.round).toBe(1);
  expect(s.quota).toBe(115);
});

test("refused commands surface err and leave state untouched", async () => {
  const { core, read } = await load();
  read(core.boot(7));
  const before = JSON.stringify(read(core.state()));
  const s = read(core.belt(-1, 0, E));
  expect(s.err).not.toBeNull();
  expect(JSON.stringify(read(core.state()))).toBe(before);
});

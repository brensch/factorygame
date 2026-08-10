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
  pick_reward(i: number): number;
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
  expect(s.credits).toBe(15);
  expect(s.quota).toBe(20);
  expect(s.board.some((p: any) => p.m === "vault")).toBe(true);
  expect(s.hand.length).toBe(4);

  // Seed 42 deals 2 Drills + 2 Furnaces (pinned by the Rust tests).
  const drill = s.hand.findIndex((c: any) => c.m === "drill");
  s = read(core.play(drill, 0, 3, E, -1, -1));
  expect(s.err).toBeNull();
  const furnace = s.hand.findIndex((c: any) => c.m === "furnace");
  s = read(core.play(furnace, 4, 3, E, -1, -1));
  expect(s.err).toBeNull();
  for (const x of [1, 2, 3, 5, 6, 7, 8]) {
    s = read(core.belt(x, 3, E));
    expect(s.err).toBeNull();
  }
  expect(s.credits).toBe(15 - 3 - 5 - 7);

  const p = read(core.project());
  expect(p.payout).toBeGreaterThanOrEqual(20);

  // Animate the shift to completion; items must appear along the way.
  read(core.shift_start());
  let sawItems = false;
  let frame: any;
  for (let i = 0; i < 60; i++) {
    frame = read(core.shift_step());
    if (frame.items.length > 0) sawItems = true;
    if (frame.done) break;
  }
  expect(frame.done).toBe(true);
  expect(frame.tick).toBe(60);
  expect(sawItems).toBe(true);

  s = read(core.shift_finish());
  expect(s.phase).toBe("reward");
  expect(s.last.cleared).toBe(true);
  expect(s.last.payout).toBe(frame.payout); // what you watched is what you got
  expect(s.offers.length).toBe(3);

  s = read(core.pick_reward(0));
  expect(s.phase).toBe("build");
  expect(s.round).toBe(1);
  expect(s.quota).toBe(45);
});

test("refused commands surface err and leave state untouched", async () => {
  const { core, read } = await load();
  read(core.boot(7));
  const before = JSON.stringify(read(core.state()));
  const s = read(core.belt(-1, 0, E));
  expect(s.err).not.toBeNull();
  expect(JSON.stringify(read(core.state()))).toBe(before);
});

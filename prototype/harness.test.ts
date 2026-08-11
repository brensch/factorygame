/**
 * Loads the BUILT wasm artifact — the same bytes the browser gets — and plays
 * a scripted round of the consignment model through the same ABI calls
 * game.ts makes. If this passes, the deployed bundle's engine works; only
 * rendering and input remain for human eyes.
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
  buy_lot(i: number, bay: number): number;
  supply_done(): number;
  shift_start(): number;
  shift_step(): number;
  shift_finish(): number;
  shop_done(): number;
  retry(): number;
};

async function load(): Promise<{ core: Exports; read: (len: number) => any }> {
  const { instance } = await WebAssembly.instantiate(await Bun.file(WASM).arrayBuffer(), {});
  const core = instance.exports as unknown as Exports;
  const dec = new TextDecoder();
  const read = (len: number) =>
    JSON.parse(dec.decode(new Uint8Array(core.memory.buffer, core.out_ptr(), len)));
  return { core, read };
}

const E = 1; // east
const S = 2; // south
const N = 0; // north

test("the shipped wasm plays a consignment round over the ABI", async () => {
  const { core, read } = await load();

  let s = read(core.boot(42));
  expect(s.phase).toBe("supply"); // a run opens at the supply window
  expect(s.credits).toBe(75);
  expect(s.quota).toBe(130);
  expect(s.bays.length).toBe(2);
  expect(s.bays[0].total + s.bays[1].total).toBe(120); // the starter consignment
  expect(s.bays[0].slots[0].name).toBe("Starter Ore");
  expect(s.lotOffers.length).toBe(3);
  expect(s.hand.length).toBe(4);
  expect(s.shiftsMax).toBe(3);
  s = read(core.supply_done());
  expect(s.phase).toBe("build");

  // The starter build: a 2×1 furnace kissing each bay, lanes east, spine.
  for (const [row, spineD] of [[6, S], [12, N]] as const) {
    const f = s.hand.findIndex((c: any) => c.m === "furnace");
    s = read(core.play(f, 1, row, E, -1, -1));
    expect(s.err).toBeNull();
    const placed = s.board.find((p: any) => p.m === "furnace" && p.y === row);
    expect(placed.cells.length).toBe(2); // a real body
    expect(placed.inPorts.length).toBe(1); // with a located intake
    for (let x = 3; x <= 15; x++) read(core.belt(x, row, E));
    read(core.belt(16, row, spineD));
    const ys = row < 9 ? [7, 8] : [11, 10];
    for (const y of ys) read(core.belt(16, y, spineD));
    s = read(core.state());
  }
  read(core.belt(16, 9, E));

  // Animate shifts to done (there is no projection), committing each.
  const runShift = () => {
    read(core.shift_start());
    let frame: any;
    let sawVaultHop = false;
    for (let i = 0; i < 60; i++) {
      frame = read(core.shift_step());
      if (frame.hops?.some((h: any) => h.x === 17 && h.y === 9)) sawVaultHop = true;
      if (frame.done) break;
    }
    expect(frame.done).toBe(true);
    return { state: read(core.shift_finish()), sawVaultHop };
  };

  const one = runShift();
  expect(one.state.phase).toBe("build"); // round continues
  expect(one.state.shiftsUsed).toBe(1);
  expect(one.state.roundDelivered).toBeGreaterThan(0);
  expect(one.state.carry).toBeGreaterThan(0); // warm factory
  expect(one.sawVaultHop).toBe(true);

  const two = runShift();
  expect(two.state.phase).toBe("shop"); // quota cleared across two shifts
  expect(two.state.offers.length).toBe(5);
  expect(two.state.contractOffers.length).toBe(2); // the top shelf
  expect(two.state.lotOffers.length).toBe(0); // no shipments in the shop
  expect(typeof two.state.market).toBe("string");

  s = read(core.shop_done());
  expect(s.phase).toBe("supply"); // rounds open at the supply window
  expect(s.round).toBe(1);
  expect(s.quota).toBe(175);
  expect(s.lotOffers.length).toBe(3);

  // Buy a shipment into bay 0's slots, then take the floor.
  const credits = s.credits;
  s = read(core.buy_lot(0, 0));
  expect(s.err).toBeNull();
  expect(s.credits).toBeLessThan(credits);

  s = read(core.supply_done());
  expect(s.phase).toBe("build");
});

test("refused commands surface err and leave state untouched", async () => {
  const { core, read } = await load();
  read(core.boot(7));
  read(core.supply_done());
  const before = JSON.stringify(read(core.state()));
  const s = read(core.belt(-1, 0, E));
  expect(s.err).not.toBeNull();
  expect(JSON.stringify(read(core.state()))).toBe(before);
});

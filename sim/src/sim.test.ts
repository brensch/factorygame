import { describe, expect, test } from "bun:test";
import { Sim, runBoard, type Placement } from "./sim";
import { ROUND_1, ROUND_4, LOOP_RIG } from "./boards";
import { itemValue, QUALITY_CAP } from "./defs";

describe("value arithmetic", () => {
  test("quality multiplies base value by 1 + 0.25q", () => {
    expect(itemValue("ingot", 0)).toBe(4);
    expect(itemValue("gear", 2)).toBe(24);
    expect(itemValue("engine", 10)).toBe(224);
  });
});

describe("Round 1 — the design doc's first board", () => {
  const r = runBoard(ROUND_1.w, ROUND_1.h, ROUND_1.cells, 60);

  test("delivers 13 ingots for 52 credits", () => {
    expect(r.byType).toEqual({ ingot: 13 });
    expect(r.payout).toBe(52);
  });

  test("first ingot lands at tick 12", () => {
    // ore at tick 4, +2 belt tiles, +3 furnace ticks, +3 belt tiles
    expect(r.delivered[0].tick).toBe(12);
  });

  test("two items are still in flight and forfeit at tick 60", () => {
    expect(r.inFlight).toBe(2);
  });

  test("nothing jams — the drill is the bottleneck, so the furnace never blocks", () => {
    expect(r.jamTicks).toBe(0);
  });
});

describe("Round 4 — Efficiency Audit, quota 200", () => {
  const r = runBoard(ROUND_4.w, ROUND_4.h, ROUND_4.cells, 60);

  test("delivers 10 gears for 240 credits, clearing the 200 quota", () => {
    expect(r.byType).toEqual({ gear: 10 });
    expect(r.payout).toBe(240);
    expect(r.payout).toBeGreaterThan(200);
  });

  test("every gear leaves at quality 2 — Heat Sink +1, Polisher +1", () => {
    expect([...new Set(r.delivered.map((d) => d.quality))]).toEqual([2]);
  });
});

describe("belt loops", () => {
  test("a full closed ring rotates instead of deadlocking", () => {
    const s = new Sim(LOOP_RIG.w, LOOP_RIG.h, LOOP_RIG.cells);
    const ring: Array<[number, number]> = [[1, 1], [2, 1], [3, 1], [3, 2], [2, 2], [1, 2]];
    for (let i = 0; i < 60; i++) s.step();
    const q = ring.map(([x, y]) => s.peek(x, y)?.quality ?? -1);
    // If the ring deadlocked, quality would be frozen at whatever it was when
    // the loop filled. Rotation means every tile reaches the cap.
    expect(q).toEqual(new Array(6).fill(QUALITY_CAP));
  });

  test("quality is strictly climbing while the ring turns", () => {
    const s = new Sim(LOOP_RIG.w, LOOP_RIG.h, LOOP_RIG.cells);
    for (let i = 0; i < 20; i++) s.step();
    const early = s.peek(2, 1)?.quality ?? 0;
    for (let i = 0; i < 15; i++) s.step();
    const later = s.peek(2, 1)?.quality ?? 0;
    expect(later).toBeGreaterThan(early);
  });
});

describe("simulation invariants", () => {
  test("deterministic — the same board and seed give identical results", () => {
    const a = runBoard(ROUND_4.w, ROUND_4.h, ROUND_4.cells, 60, 1234);
    const b = runBoard(ROUND_4.w, ROUND_4.h, ROUND_4.cells, 60, 1234);
    expect(a.payout).toBe(b.payout);
    expect(a.delivered).toEqual(b.delivered);
  });

  test("order-independent — shuffling the placement list changes nothing", () => {
    const shuffled: Placement[] = [...ROUND_4.cells];
    // fixed permutation, so this test is itself deterministic
    for (let i = shuffled.length - 1; i > 0; i--) {
      const j = (i * 7 + 3) % (i + 1);
      [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
    }
    const base = runBoard(ROUND_4.w, ROUND_4.h, ROUND_4.cells, 60);
    const perm = runBoard(ROUND_4.w, ROUND_4.h, shuffled, 60);
    expect(perm.payout).toBe(base.payout);
    expect(perm.byType).toEqual(base.byType);
  });

  test("auras with onlyTag are selective — a Heat Sink does nothing for a Drill", () => {
    const withSink: Placement[] = [
      { x: 0, y: 0, t: "drill", d: "E" },
      { x: 1, y: 0, t: "vault" },
      { x: 0, y: 1, t: "heatsink" },
    ];
    const r = runBoard(3, 3, withSink, 60);
    // Drill has no HEAT tag, so no +1 quality leaks onto its ore.
    expect([...new Set(r.delivered.map((d) => d.quality))]).toEqual([0]);
  });

  test("rejects two machines on one tile", () => {
    expect(() =>
      new Sim(3, 3, [{ x: 1, y: 1, t: "drill", d: "E" }, { x: 1, y: 1, t: "belt", d: "E" }]),
    ).toThrow(/two machines/);
  });

  test("rejects out-of-bounds placement", () => {
    expect(() => new Sim(3, 3, [{ x: 5, y: 0, t: "drill", d: "E" }])).toThrow(/out of bounds/);
  });
});

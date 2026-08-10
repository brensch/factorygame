/**
 * The boards from the design document's walkthrough, as data.
 *
 * These exist so the doc's claimed numbers are executable rather than asserted.
 * If you change a number in defs.ts, the tests here tell you which paragraph of
 * the design doc just became a lie.
 */

import type { Placement } from "./sim";

/** Act I, Round 1 — one drill, one furnace, one vault. Quota 20. */
export const ROUND_1: { w: number; h: number; cells: Placement[] } = {
  w: 8, h: 5,
  cells: [
    { x: 0, y: 2, t: "drill", d: "E" },
    { x: 1, y: 2, t: "belt", d: "E" },
    { x: 2, y: 2, t: "belt", d: "E" },
    { x: 3, y: 2, t: "furnace", d: "E" },
    { x: 4, y: 2, t: "belt", d: "E" },
    { x: 5, y: 2, t: "belt", d: "E" },
    { x: 6, y: 2, t: "belt", d: "E" },
    { x: 7, y: 2, t: "vault" },
  ],
};

/**
 * Act I, Round 4 (Efficiency Audit) — two lanes, a Heat Sink between the
 * furnaces, an Overclocker on the Fabricator, a Polisher before the vault.
 * Quota 200.
 *
 * The Overclocker goes on the Fabricator, not the furnaces: two drills supply
 * 0.5 ore/tick, two furnaces can process 0.67/tick, but the Fabricator only
 * consumes 0.4 ingot/tick — so the assembler is the bottleneck and speeding
 * the furnaces would achieve nothing.
 */
export const ROUND_4: { w: number; h: number; cells: Placement[] } = {
  w: 8, h: 5,
  cells: [
    { x: 0, y: 1, t: "drill", d: "E" },
    { x: 1, y: 1, t: "belt", d: "E" },
    { x: 2, y: 1, t: "furnace", d: "E" },
    { x: 3, y: 1, t: "belt", d: "E" },
    { x: 4, y: 1, t: "belt", d: "S" },

    { x: 2, y: 2, t: "heatsink" },

    { x: 0, y: 3, t: "drill", d: "E" },
    { x: 1, y: 3, t: "belt", d: "E" },
    { x: 2, y: 3, t: "furnace", d: "E" },
    { x: 3, y: 3, t: "belt", d: "E" },
    { x: 4, y: 3, t: "belt", d: "N" },

    { x: 4, y: 2, t: "merger", d: "E" },
    { x: 5, y: 1, t: "overclock" },
    { x: 5, y: 2, t: "fab", d: "E" },
    { x: 6, y: 2, t: "polisher", d: "E" },
    { x: 7, y: 2, t: "vault" },
  ],
};

/**
 * A minimal polish loop, isolated for testing: a closed 8-tile ring with two
 * Polishers, fed from outside. There is no Filter in the sim yet (see NOTES),
 * so this board exists to prove the loop ROTATES rather than deadlocks.
 */
export const LOOP_RIG: { w: number; h: number; cells: Placement[] } = {
  w: 5, h: 4,
  cells: [
    { x: 0, y: 1, t: "drill", d: "E" },
    { x: 1, y: 1, t: "belt", d: "E" },
    { x: 2, y: 1, t: "belt", d: "E" },
    { x: 3, y: 1, t: "polisher", d: "S" },
    { x: 3, y: 2, t: "belt", d: "W" },
    { x: 2, y: 2, t: "polisher", d: "W" },
    { x: 1, y: 2, t: "belt", d: "N" },
    // (1,1) -> (2,1) -> (3,1) -> (3,2) -> (2,2) -> (1,2) -> (1,1): a closed ring
  ],
};

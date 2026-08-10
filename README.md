# factorygame

Working title: **OVERFLOW** — a 2D grid factory roguelite. Factorio's belts, Slay the Spire's
run structure, Balatro's escalation.

You build a small production line on a grid, run a 60-tick shift against a quota, and spend the
surplus on machines and relics. The quota steps ~1.8× per round; adding another drill gets you
1.3×. So the game forces you off addition and onto multipliers — adjacency auras, tag resonance,
and closed belt loops that circulate items through Polishers until they qualify to leave.

## 🎮 Play the prototype

**→ [brensch.github.io/factorygame/play/](https://brensch.github.io/factorygame/play/)**

Twelve rounds, quota nearly doubling each time. Drag to lay belts, click machines to rotate,
right-click to remove (full refund — experimenting is free). The projection panel runs the whole
shift before you commit.

This exists to answer one question: **is it fun?** It is not the game. It's a browser shell around
the reference sim, deliberately built in a few hours so the design can be judged before anyone
commits to an engine. Every rule it plays by comes from `sim/` — it contains no game logic of its
own, so it cannot drift from the tested implementation.

Build it locally with `cd sim && bun run build:play && bun run serve`.

## 📄 The design document

**[`docs/index.html`](docs/index.html)** — the whole design, with SVG board diagrams, the machine
catalogue, the six-phase round loop, and a complete 12-round walkthrough.

It's a single self-contained HTML file: no build step, no CDN.

**→ [brensch.github.io/factorygame](https://brensch.github.io/factorygame/)**

Deployed by `.github/workflows/pages.yml` on every change to `docs/`. The same
pipeline will serve playable web builds later.

## 🧪 The reference simulation

**[`sim/`](sim/)** — the tick simulation, in TypeScript, with tests.

This exists so the design document's numbers are *executable rather than asserted*. The round 1
and round 4 boards from the walkthrough are checked in as data, and the tests assert the exact
figures the doc quotes.

```sh
cd sim
bun test        # 14 tests
bun run report  # print what each walkthrough board actually delivers
```

It also proves out the one genuinely hard mechanic: a **closed belt loop rotates rather than
deadlocking**, which is what makes the polish-loop build legal at all. See the comment on
`transfer()` in `sim/src/sim.ts`.

## Engine

**Bevy** — decided 2026-08-10, after measuring rather than guessing. The game wants extreme
on-screen item counts scaling from desktop down to mobile, and the simulation that drives them
is where engines actually differ: GDScript measured **29× slower** than JIT'd JS on the transfer
loop, while the Rust core below does **32,500 items in flight at 4.7 ms/tick** single-threaded.

The choice is deliberately cheap to reverse: the whole game lives in an engine-free Rust crate,
and Bevy will be a renderer over it. The load-bearing rule survives from day one: the factory is
a pure deterministic tick simulation; the renderer reads sim state and never writes back.

## 🦀 The Rust core and lab

[`rust/`](rust/) — the canonical implementation going forward.

- **`core/`** — the game: the tick sim (ported 1:1 from `sim/`, pinned by the same 19 test
  assertions to the same numbers), plus the **blueprint-card deck** and the round/run structure.
  Zero dependencies; compiles to wasm32 unchanged, and CI enforces that it stays wasm-clean.
- **`lab/`** — headless playtesting: bots play thousands of complete seeded runs and the
  outcome distribution becomes balance data.
  `cargo run --release -p overflow-lab -- --runs 2000`

First real finding from the lab: the naive lane-building bot dies at **round 4 in 2,000 of
2,000 runs** — precisely the wall the design doc claims forces players off pure widening.

The TypeScript `sim/` stays for now as the browser prototype's engine and as a cross-check —
two independent implementations agreeing on 19 pinned numbers. It retires when the WASM build of
the core replaces it under the same canvas UI.

## Toolchain

No editor, no IDE. Godot is a single binary and a Godot project is entirely plain text, so the
whole loop — write, import, test, export, screenshot — runs from the command line on a headless
box with no GPU. See [`TOOLCHAIN.md`](TOOLCHAIN.md) for the verified commands and the honest
limits.

## Status

Design + reference sim + a verified headless toolchain. No game yet. See [`NOTES.md`](NOTES.md)
for what's unverified and what comes next.

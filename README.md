# factorygame

Working title: **OVERFLOW** — a 2D grid factory roguelite. Factorio's belts, Slay the Spire's
run structure, Balatro's escalation.

You build a small production line on a grid, run a 60-tick shift against a quota, and spend the
surplus on machines and relics. The quota steps ~1.8× per round; adding another drill gets you
1.3×. So the game forces you off addition and onto multipliers — adjacency auras, tag resonance,
and closed belt loops that circulate items through Polishers until they qualify to leave.

## 🎮 Play it

**→ [brensch.github.io/factorygame/play/](https://brensch.github.io/factorygame/play/)**

Twelve rounds, quota nearly doubling each time. Machines are **blueprints you own**: a
persistent hand (max 10), placed and re-placed freely, and between rounds the **shop** turns
your surplus into new blueprints — buy, reroll, or bank. Belts and junctions are cheap
infrastructure. Drag to lay belt runs, drag machines (or shift-drag groups) to rearrange,
right-click to pull a machine back to hand. The projection panel runs the whole shift before
you commit.

The page is a thin canvas shell over the **Rust core compiled to wasm** (~85 KB). The
JavaScript owns rendering and input and nothing else: every rule — the tick sim, the deck,
quotas, legality — executes inside `rust/core`, the same crate the tests pin and the lab
bots play. The deployed bundle is built from source on every push, so it cannot drift from
the tested implementation.

Build it locally: `bun run build:play && bun run serve` (needs Rust with the
`wasm32-unknown-unknown` target, and bun).

## 🕹 The Bevy frontend

**`rust/game`** — the real frontend, started 2026-08-11: Bevy 0.19 over the same core, 8-bit
factory aesthetic (every sprite procedurally painted into one atlas at startup — no asset
files), responsive from phone portrait to PC fullscreen. Aesthetic mockups and real captured
frames live in [`docs/design/renders/`](docs/design/renders/); architecture in
[`rust/game/README.md`](rust/game/README.md).

```sh
cd rust && cargo run -p overflow-game          # native window
OVERFLOW_WINDOW=390x844 cargo run -p overflow-game   # phone shape
```

## 📄 The design document

**[`docs/index.html`](docs/index.html)** — the whole design, with SVG board diagrams, the machine
catalogue, the six-phase round loop, and a complete 12-round walkthrough.

It's a single self-contained HTML file: no build step, no CDN.

**→ [brensch.github.io/factorygame](https://brensch.github.io/factorygame/)**

Deployed by `.github/workflows/pages.yml` on every change, together with the playable build.

## Engine

**Bevy** — decided 2026-08-10, after measuring rather than guessing. The game wants extreme
on-screen item counts scaling from desktop down to mobile, and the simulation that drives them
is where engines actually differ: GDScript measured **29× slower** than JIT'd JS on the transfer
loop, while the Rust core below does **32,500 items in flight at 4.7 ms/tick** single-threaded.

The choice stays deliberately cheap to reverse — and cheap to defer: the whole game lives in an
engine-free Rust crate, the browser build ships **today** through an 85 KB hand-rolled wasm
bridge, and Bevy arrives as a renderer over the identical crate once the design has proven
it deserves one. The load-bearing rule survives from day one: the factory is a pure
deterministic tick simulation; the renderer reads sim state and never writes back.

## 🦀 The Rust workspace

[`rust/`](rust/) — the canonical implementation. Three crates:

- **`core/`** — the game: the deterministic tick sim, the machine and item definitions, the
  **blueprint-card deck**, and the round/run structure. Zero dependencies; compiles to wasm32
  unchanged, and CI enforces that it stays wasm-clean. The design doc's walkthrough boards are
  checked in as data and the tests assert the exact figures the doc quotes — the numbers are
  *executable rather than asserted*. That includes the one genuinely hard mechanic: a closed
  belt loop rotates rather than deadlocking (see `transfer()` in `core/src/sim.rs`).
- **`web/`** — the browser bridge: a hand-rolled wasm ABI (scalar arguments in, JSON state
  out) so no binding generator sits between the game and the page. The wire format is pinned
  by its own tests, and `prototype/harness.test.ts` plays a full round against the built
  `game.wasm` before every deploy.
- **`lab/`** — headless playtesting: bots play thousands of complete seeded runs and the
  outcome distribution becomes balance data.
  `cargo run --release -p overflow-lab -- --runs 2000`

First real finding from the lab: the naive lane-building bot dies at **round 4 in 2,000 of
2,000 runs** — precisely the wall the design doc claims forces players off pure widening.

The TypeScript reference sim that bootstrapped the project is retired (git history has it).
It ended its life as designed: two independent implementations agreeing on 19 pinned numbers,
until the wasm build of the Rust core replaced it under the same canvas UI.

## Toolchain

No editor, no IDE, no GPU. Build, test, a scripted browser playthrough, and screenshots all
run from the command line on a headless box. See [`TOOLCHAIN.md`](TOOLCHAIN.md) for the
verified commands and the honest limits.

## Status

Design + Rust core + playable card-game prototype in the browser, all pinned by the same
tests. The open question is the one that matters: **is it fun?** See [`NOTES.md`](NOTES.md)
for what's unverified and what comes next.

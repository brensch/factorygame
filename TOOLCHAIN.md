# Headless toolchain

No editor, no IDE, no clicking. Everything below is verified working on this box
(Ubuntu on WSL2, no GPU, no desktop). The game is Rust; the browser is the delivery
mechanism; the whole loop — build, test, play a scripted round, screenshot — runs
from the command line.

## What's installed

| Thing | Where | Notes |
|---|---|---|
| Rust stable + `wasm32-unknown-unknown` | `~/.cargo` | `rustup target add wasm32-unknown-unknown` was the only setup. |
| bun | `~/.bun/bin/bun` | Bundles the canvas shell, runs the wasm harness tests, serves locally. |
| Chrome | `/usr/bin/google-chrome` | Headless screenshots and scripted playthroughs. |

Deliberately **not** installed: `wasm-pack` and `wasm-bindgen-cli`. The ABI in
`rust/web` is hand-rolled (scalars in, JSON out), so a plain `cargo build` produces
the shipping artifact and there is no generator version to keep in sync.

## The loop

```sh
cd rust && cargo test            # 29 tests: sim, deck/run, wire format
cd rust && cargo clippy -- -D warnings

bun run build:play               # core -> wasm (~85 KB) + bundle into docs/play/
bun run test:play                # harness plays a full round against the BUILT wasm
bun run serve                    # http://localhost:8765/play/
```

`prototype/harness.test.ts` is the load-bearing check: it loads the exact
`game.wasm` bytes the browser gets and plays a scripted round through the same
ABI calls the UI makes. The Pages workflow runs it before every deploy.

## Seeing the game

Headless Chrome renders the real page fine without a GPU:

```sh
google-chrome --headless=new --no-sandbox --screenshot=shot.png \
  --window-size=1400,900 --virtual-time-budget=4000 http://localhost:8765/play/
```

For interaction — clicking cards, dragging belts, running a shift — `playwright-core`
(pure JS, drives the installed Chrome, no browser download) scripts the full round.
Verified end to end: a scripted playthrough placed machines via canvas clicks, laid
belts by dragging, ran the shift at 16×, and advanced to round 2.

## Balance work

```sh
cd rust && cargo run --release -p overflow-lab -- --runs 2000 --seed 1
```

Thousands of complete seeded runs per second; the survival histogram is the
balance instrument. See NOTES.md for what it has measured so far.

## Honest limits

- **Feel needs a human.** Screenshots and scripted playthroughs catch layout and
  logic bugs, not whether dragging a belt feels good or whether the game is fun.
  Judging that means opening the deployed page on real hardware.
- **No performance numbers from this box.** Sim throughput measurements are fine
  (CPU-only); anything about rendering performance is not.
- **Bevy, later, changes none of this.** The core stays engine-free and
  browser-testable; Bevy is a renderer over it, and its wasm builds will be
  judged in a real browser the same way.

## History

The first verified toolchain here was Godot 4.7.1 headless (import, GUT tests,
web export, Xvfb screenshots — all worked). The engine decision went to Bevy
after measuring GDScript at 29× slower than JIT'd JS on the transfer loop; the
Godot notes live in git history if ever needed.

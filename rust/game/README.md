# overflow-game — the Bevy frontend

The real frontend: Bevy 0.19 over the same engine-free `overflow-core` the
lab bots and the wasm prototype share. There is no JSON, no wire format,
and no rule anywhere in this crate — the UI queues commands at a bridge
that owns the `Game`, and paints whatever state comes back.

## Architecture

| module | job |
| --- | --- |
| `bridge` | The only door to the rules: a `Cmd` enum (place, belt, allocate, run shift, shop…), one system that applies them to `overflow_core::run::Game`, and the live `Sim` playback while a shift animates. Nothing else may mutate game state. |
| `atlas` | The whole aesthetic, generated: every sprite (ground, belts, machine plates, emblems, ports, items, a 3×5 pixel font) is painted pixel-by-pixel into one texture at startup. No asset files. |
| `layout` | Responsive virtual-pixel layout: an integer zoom keeps the art crisp; portrait (mobile) and landscape (PC) are the same scene with different furniture positions. |
| `scene` | Immediate-mode painter: when the bridge is dirty the scene is despawned and repainted, and every clickable region is registered in a hit list as it is drawn — what you see and what you can click cannot drift apart. |
| `input` | Pointer + keys → `Cmd`s. Tap-to-select, drag cards from the fan to the board, paint belts by dragging, right-click sells, `R` rotates. Mouse and touch are the same gesture. |
| `theme` | The palette: dark industrial ground, rust-orange copper, furnace glow, hazard stripes. |

## Run

```sh
cargo run -p overflow-game                 # native window
OVERFLOW_WINDOW=390x844 cargo run -p ...   # portrait / phone shape
OVERFLOW_SEED=7 cargo run -p ...           # pick the run seed
```

Headless verification (works under WSLg/llvmpipe — no GPU needed):

```sh
OVERFLOW_SHOT=/tmp/f.png cargo run -p overflow-game        # one frame, exits
OVERFLOW_SCRIPT=demo OVERFLOW_SHOT=/tmp/f.png cargo run -p overflow-game
# plays the harness-test factory over the bridge (allocate → build →
# morning shift) and saves f_built / f_shift / f_late / f_after keyframes.
```

## Notes

- The crate is excluded from workspace `default-members`, so bare
  `cargo test` / `cargo clippy` never pay the Bevy compile bill. CI builds
  it via its own cached job.
- The feature set is the `2d` meta-feature minus gamepads (system libudev),
  clipboard, the default font, scenes and picking — sprites, our own pixel
  font, our own hit-testing.
- Browser deployment (wasm + webgl2) is compatible by design but not wired
  up yet: Bevy's wasm path wants `wasm-bindgen`, which the hand-rolled
  prototype deliberately avoided. When the Bevy build replaces the
  prototype, that constraint moves with it.

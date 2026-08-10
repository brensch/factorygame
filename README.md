# factorygame

Working title: **OVERFLOW** — a 2D grid factory roguelite. Factorio's belts, Slay the Spire's
run structure, Balatro's escalation.

You build a small production line on a grid, run a 60-tick shift against a quota, and spend the
surplus on machines and relics. The quota steps ~1.8× per round; adding another drill gets you
1.3×. So the game forces you off addition and onto multipliers — adjacency auras, tag resonance,
and closed belt loops that circulate items through Polishers until they qualify to leave.

## 📄 The design document

**[`docs/index.html`](docs/index.html)** — the whole design, with SVG board diagrams, the machine
catalogue, the six-phase round loop, and a complete 12-round walkthrough.

It's a single self-contained HTML file: no build step, no CDN. Open it directly, or read it at
the GitHub Pages URL once Pages is enabled (Settings → Pages → Source: GitHub Actions; the
workflow in `.github/workflows/pages.yml` handles the rest).

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

**Godot 4.x**, GDScript, no physics. Runner-up was TypeScript + PixiJS.

The load-bearing rule, whichever engine wins: the factory is a **pure deterministic tick
simulation over a 2D array**. No Node per item, no delta time, no engine types in the sim layer.
The renderer reads sim state and never writes back. That's what gives you 8× fast-forward, seed
replay, headless balance testing, and a late-game board with 2,000 items drawn in one
`MultiMeshInstance2D` call.

The TypeScript sim here is deliberately written to that constraint so it ports mechanically.

## Toolchain

No editor, no IDE. Godot is a single binary and a Godot project is entirely plain text, so the
whole loop — write, import, test, export, screenshot — runs from the command line on a headless
box with no GPU. See [`TOOLCHAIN.md`](TOOLCHAIN.md) for the verified commands and the honest
limits.

## Status

Design + reference sim + a verified headless toolchain. No game yet. See [`NOTES.md`](NOTES.md)
for what's unverified and what comes next.

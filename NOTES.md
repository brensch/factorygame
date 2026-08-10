# Notes — open questions and known gaps

Kept honest on purpose. This file is the difference between a design doc and a design doc that
believes its own marketing.

## What is actually verified

| Claim | Status |
|---|---|
| Round 1 board delivers 13 ingots / 52 credits, first at tick 12 | ✅ test |
| Round 4 board delivers 10 quality-2 gears / 240 credits | ✅ test |
| A full closed belt ring rotates and quality climbs to the cap | ✅ test |
| Sim is deterministic under a fixed seed | ✅ test |
| Sim is independent of placement/iteration order | ✅ test |
| Heat Sink's `onlyTag` aura doesn't leak onto non-HEAT machines | ✅ test |
| Filter ejects on a quality gate, and only at the gate | ✅ test |
| Splitter round-robins fairly between two outputs | ✅ test |
| Closed loop + gate actually pays out (18 credits from 7 quality-6 ore) | ✅ test |
| Rounds 7 and 11 payouts (1,580 / 19,500) | ❌ hand-estimated |
| The whole quota curve (§9 of the doc) | ❌ hand-authored target, not fitted |
| That any of this is *fun* | ❌ entirely unproven |

The round-4 board in the doc originally showed the Overclocker between the two furnaces. Building
the sim proved that placement does nothing — the Fabricator is the bottleneck, not the furnaces —
so both the board and the prose were corrected. That is the sim earning its keep.

## Not implemented in the reference sim

**Filter and Splitter now exist** (added with the prototype), which is what unblocked the playable
loop. Still missing:

- **Underpass**, **Buffer** release policy, **Compressor** N-to-1 with mixed qualities.
- **Duplicator** is still approximate: a clone queues behind the original and is released when the
  output slot frees. The real question — what happens when a clone has nowhere to go — is
  unanswered, and it matters, because that is exactly the failure mode the design leans on.
- Relics and audit modifiers. The prototype has rounds, quota and unlocks, but audits are only
  flagged in the header; they do not yet change any rule.

## The card/deck redesign (2026-08-10) — open questions

The palette-shop is gone; machines are now **blueprint cards**: a run deck, a hand dealt each
round, rewards add cards to the deck, placing a machine consumes its card, selling returns it.
Belts stay as 1-credit infrastructure — a deck of belt cards is a deck of dead draws.
Implemented in `rust/core/src/deck.rs` + `run.rs`. Unresolved:

1. **Hand size and dead hands.** A mid-run deck of 15 with hand size 4 can deal zero playable
   cards. Mulligan? Scry? Credits-to-redraw?
2. **Are consumed cards the right scarcity?** Placing removes the card permanently (selling
   recovers it). Alternative: cards as reusable blueprints, credits as the only limit — less
   scarcity, more combo. The lab can answer this empirically: implement both, compare
   run-length distributions.
3. **Rarity/weighting.** Offers are currently uniform over the unlocked pool. Rarity tiers
   change both balance and the dopamine curve.
4. **Should configuration live on the card?** A "Filter ≥7" card vs a "Filter ≥3" card, rather
   than a threshold set at placement — more draft texture, less fiddling.

## What the lab has measured so far

- **LaneBot (widen-only) dies at round 4 in 2,000/2,000 runs.** Quota 200 is exactly the wall
  the design doc says forces a tier jump. The zero variance also says rounds 1–3 are pure
  formality even for a trivial player — consider tightening.
- Full runs simulate at ~6,200/sec single-threaded, so parameter sweeps over `defs.rs` are cheap.
- Next bots, in order: TierBot (knows the Fabricator), LoopBot (knows the gated polish ring),
  then a search-based baseline. The spread between their death rounds is a direct measurement
  of how much depth each mechanic actually buys.

## Design questions I could not resolve on paper

1. **Is the Duplicator spiral tunable, or just a trap?** It's meant to be a thrilling instability.
   It may just be a build that either wins outright or bricks your factory with no middle ground.
   Needs the Filter implemented, then a parameter sweep.
2. **Is 60 ticks the right shift length?** Transit is ~20% of the shift on the round-1 board and
   more later. If travel time dominates, players will optimise layout compactness over everything
   else and the board stops feeling like a factory.
3. **Does the quality cap of 10 arrive too early?** In the loop test the ring hits the cap in
   about 40 ticks, at which point extra Polishers are dead weight. Either the cap wants to be
   higher by default, or `Overengineered` is a mandatory relic rather than an interesting one —
   and mandatory relics are bad relics.
4. **Merger fairness.** Currently a merger is just a belt that accepts from any side, which means
   whichever lane's tile index is lower wins ties forever. A real fair-queue is needed or one lane
   will silently starve.

## Next steps, in order

1. Implement the **Filter**. Then re-measure the round 7 board and find out how wrong the 1,580
   estimate is.
2. Implement Splitter, then build and measure the round 11 board.
3. Sweep quota curve against measured payouts and refit §9 of the doc to real numbers.
4. Only then start the Godot project. Port `sim.ts` more or less line for line; keep the same
   tests.

## Godot port notes

- `sim.ts` maps to a plain `RefCounted` class. Nothing in it should touch `Node`.
- `defs.ts` becomes `.tres` `Resource` files (or JSON if hot-reload matters more than the editor
  inspector).
- Sim runs at 20 Hz on a fixed accumulator, decoupled from frame rate. Items lerp between previous
  and current tile for rendering.
- Speed control (1×/2×/4×/instant) is just how many `step()` calls per frame — free, because the
  sim has no concept of time.
- Items render as one `MultiMeshInstance2D` with per-instance colour. Never a scene node per item.

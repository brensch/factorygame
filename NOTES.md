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

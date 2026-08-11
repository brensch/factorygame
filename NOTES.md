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
| The card/run wire format the browser drives | ✅ test (`rust/web`) |
| The shipped `game.wasm` plays a full round | ✅ test (`prototype/harness.test.ts`) |
| The animated shift equals the committed shift, tick for tick | ✅ test |
| Rounds 7 and 11 payouts (1,580 / 19,500) | ❌ hand-estimated |
| The whole quota curve (§9 of the doc) | ❌ hand-authored target, not fitted |
| That any of this is *fun* | ❌ entirely unproven — but now playable, so answerable |

The round-4 board in the doc originally showed the Overclocker between the two furnaces. Building
the sim proved that placement does nothing — the Fabricator is the bottleneck, not the furnaces —
so both the board and the prose were corrected. That is the sim earning its keep.

## Not implemented in the sim

- **Underpass**, **Buffer** release policy, **Compressor** N-to-1 with mixed qualities.
- **Duplicator** is still approximate: a clone queues behind the original and is released when the
  output slot frees. The real question — what happens when a clone has nowhere to go — is
  unanswered, and it matters, because that is exactly the failure mode the design leans on.
- Relics and audit modifiers. Audits are flagged in the UI (`AUDIT_ROUNDS` in `run.rs`) and the
  final one halves the shift; the rest do not yet change any rule.

## The blueprint-shop design (2026-08-10, v2) — replacing the deal-a-hand deck

The first card design (deck, dealt hands, one reward pick per round) played one day and
failed measurably: acquisition was capped at 1 card/round while quotas compound ~1.9×, so
payout capacity (≈27c per card, measured) hit the quota wall at round 4 for every player and
every bot — and credits piled up with nothing to buy. **The faucet was the wall.**

Current design (`rust/core/src/cards.rs` + `run.rs`):

- The hand is a **persistent inventory** of owned blueprints, capped at 10.
- Clearing a shift opens the **shop**: 5 offers, buy any you can afford (into the hand),
  reroll the rack for 5c. This is the credit sink that lets growth compound with the curve.
- Placing is free (paid at purchase); pulling a machine off the board returns its blueprint
  to the hand; selling a blueprint from the hand refunds half.
- Belts (1c) and **Junctions** (2c, Mindustry-style crossing) are infrastructure, not cards.

Measured immediately: LaneBot went from 0/2000 clearing round 4 to 719/2000, with deaths now
at round 5 — and round 5 is a *board geometry* ceiling (7 rows = 7 lanes ≈ 400c), which is
exactly the "widening must die here" wall the design wants. The next instrument is a TierBot
that knows the Fabricator, to measure where tier-2 play dies.

Same-day revisions after play (2026-08-10, v2.1):

- **Prices track the quota curve** (`shop_price_mult` ≈ next-quota/40, floor 1×) and
  **rerolls escalate** (base × inflation × count, reset per shop). Playtest verdict on flat
  prices: "you just buy everything" — inflation restores the dilemma. Measured: LaneBot
  clears round 4 in 607/2,000 (was 719 flat), geometric wall at 5 unchanged.
- **Directives** — permanent, stacking, tag-keyed run buffs (Superheater/HEAT speed,
  Flywheel/KINETIC speed, Overvolt/VOLT speed, Fine Tolerances/PRECISION quality incl.
  Polisher passes, Enrichment/ORGANIC quality). One directive slot on every rack, competing
  with throughput for the same credits: this is the route-commitment mechanic.
- **Splitter and Merger became infrastructure** (always available, 4c) — routing primitives
  shouldn't be shop RNG. Card pool is now machines-that-do-things only.
- **Retry**: a missed quota offers "retry the round" (board/hand/credits as you left them).
  Free for now; a real run structure may want to price it (a debt? a relic slot?).

Still open:

1. **Rarity/weighting.** The rack is uniform over the unlocked pool. Rarity tiers change
   both balance and the dopamine curve.
2. **Should configuration live on the card?** A "Filter ≥7" card vs a "Filter ≥3" card,
   rather than a threshold set at placement — more draft texture, less fiddling.
3. **Hand cap 10.** Arbitrary; revisit once real runs show hoarding patterns.
4. **Directive depth.** Five directives is a proof of mechanic. The interesting versions
   are conditional (per-tag payouts, "quality can exceed the cap for HEAT", cross-tag
   combos) — add once the flat ones prove the route-commitment loop is fun.
5. **Should retry cost something?** Free retries make quota tension advisory. Fine for
   design iteration; wrong for the shipped roguelite.

## What the lab has measured so far

- **Under the deck design: LaneBot died at round 4 in 2,000/2,000 runs** — which turned out
  to be the acquisition faucet, not the intended skill wall (see the v2 design note above).
- **Under the shop design: LaneBot clears round 4 in 719/2,000 runs and dies at round 5**,
  where the board's 7 rows cap pure widening at ~400c. The wall is now geometric, which is
  the wall the design doc actually claims.
- Full runs simulate at ~5,000/sec single-threaded, so parameter sweeps over `defs.rs` are cheap.
- Next bots, in order: TierBot (knows the Fabricator), LoopBot (knows the gated polish ring),
  then a search-based baseline. The spread between their death rounds is a direct measurement
  of how much depth each mechanic actually buys.

## Design questions I could not resolve on paper

1. **Is the Duplicator spiral tunable, or just a trap?** It's meant to be a thrilling instability.
   It may just be a build that either wins outright or bricks your factory with no middle ground.
   Filter exists now; needs a parameter sweep.
2. **Is 60 ticks the right shift length?** Transit is ~20% of the shift on the round-1 board and
   more later. Worse for loops: rings start empty every shift and a gate-10 ring needs ~40 ticks
   before its first ejection, so loop latency eats most of the shift. Candidate fix worth
   testing: circulating items persist on the board between rounds (the factory stays warm),
   which also removes the stranded-items feel-bad.
3. **Does the quality cap of 10 arrive too early?** In the loop test the ring hits the cap in
   about 40 ticks, at which point extra Polishers are dead weight. Either the cap wants to be
   higher by default, or `Overengineered` is a mandatory relic rather than an interesting one —
   and mandatory relics are bad relics.
4. **Merger fairness.** Currently a merger is just a belt that accepts from any side, which means
   whichever lane's tile index is lower wins ties forever. A real fair-queue is needed or one lane
   will silently starve.

## Next steps, in order

1. **Play it.** The card design is live in the browser; the four card questions above are now
   answered by playing runs, not by argument. Change numbers in `defs.rs`/`run.rs`, push,
   play again.
2. Build and measure the **round 7 and round 11 walkthrough boards** (Filter and Splitter are
   in the sim now) and find out how wrong the 1,580 / 19,500 estimates are.
3. Sweep quota curve against measured payouts and refit §9 of the doc to real numbers.
4. TierBot and LoopBot in the lab, so design changes get judged against the survival
   histogram, not vibes.
5. Bevy — only after the browser build says the game deserves it.

## Renderer notes (for Bevy, later)

The wasm frontend already implements the pattern the Bevy renderer should copy:

- The sim runs on a fixed tick, decoupled from frame rate; the renderer calls `step()` on an
  accumulator and lerps items between previous and current tile (`frameLoop` in
  `prototype/game.ts`).
- Speed control (1×/4×/16×/instant) is just how many `step()` calls per frame — free, because
  the sim has no concept of time.
- Items render as one instanced draw with per-instance colour. Never a scene node per item.
- The renderer reads state and never writes back; every mutation goes through the same
  command surface the wasm ABI exposes today (`rust/web/src/lib.rs` is the de facto spec).

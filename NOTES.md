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

Third revision same day (v2.2) — chance and interplay:

- **Spot market**: every round one item type pays 2× at the vault, rolled when the shop
  opens so purchases can chase it. This is the "keep a second chain alive" incentive —
  single-path boards eat dead markets.
- **Audit inspections**: audit rounds (4/8/12) slow a random tag to 60% speed, revealed in
  the shop. Doctrine over-commitment now carries real risk.
- **Cross-chain assemblers unlocked** (Circuit Bench, Lens Grinder, Engine Works were
  meta-locked, silently forcing the metal chain). Shop inflation gates them instead.
- **Board 14×14** (was 10×7), square for mobile, vault mid-east-edge. Longer transit
  noticeably tightens early rounds.
- **Price retune after measurement**: mult = max(3, next_quota/35). The first attempt
  (max(4, /25)) drove LaneBot's median death to round 2 — too brutal even for the
  wanted crunch. Current: median 3, deaths spread over quotas 90/200/400, near-zero
  round-2 deaths. LaneBot also grew lane-repair and a belt-budget reserve; without them
  it bankrupted itself buying machines it couldn't wire in (an instructive failure —
  human players face the same trap, but they can see it).

Fourth revision (v2.3) — tuning became a measured loop:

- **The overshoot instrument**: the lab now prints mean payout vs quota per round. First
  reading: the hand-authored curve paid **2.16× quota at round 1** ("you still get too many
  points" — confirmed) then cliffed to 0.48× at round 4.
- **The instrument had a bug worth remembering**: LaneBot built full-width lanes, so on the
  18×18 board it measured its own belt bill, not the game. Compact building (drills 4 tiles
  from the vault) changed measured capacity from 43/round-1 to 112. Bots measure their own
  strategy; keep them honest before trusting the numbers.
- **Quotas refit from measurement**: [85, 115, 145, 180, 260, 380, 550, 800, 1150, 1650,
  2400, 3400] — ~0.75× of widening capacity at round 1 (you must place the WHOLE starting
  kit), ~1.0 by round 3, permanently above pure widening from round 5. Starting credits 40
  so the kit's belts are affordable. LaneBot: deaths spread over rounds 2–5, ratios
  1.32 → 0.93 declining.
- **Board 18×18.** Hand is a real card fan now — drag a card onto a tile to place it.

## The consignment model (2026-08-10, v3) — you choose what flows in

The biggest redesign yet, from playtest direction: "change the input to each run…
more like a delivery thing… stuff stays in the machines… jokers that bias inputs
and outputs." Extractors are gone. The factory is a processor now.

- **Loading bays** (2, west edge, bolted down) are the only material source. Each
  streams its **queue** one item per tick. Buying a shipment means choosing which
  bay it queues at — that's the entire manual-input surface.
- **Shipments** are drafted in the shop (3 offers/round, quantities scale with the
  round, priced ~35% of raw value). Points are **margin** now: value added over
  the cost of what you fed in. Finite input also kills lane-widening structurally —
  you can't process more than arrives.
- **Warm factory**: nothing resets. Items in machines, on belts, mid-loop carry
  across shifts AND rounds. "Stranded = forfeit" is gone; it's inventory in the pipes.
- **Rounds are 3 shifts of 40 ticks**, deliveries summing to the quota; spare
  shifts pay a bonus; failing all three offers a full-round retry (snapshot rewind).
- **Fog**: the projection is gone from the UI. Build on judgment, run, find out.
- **Item classes with behavior**: sap wilts (−1q per 10 ticks; refining stabilizes),
  crystal cracks in mergers/splitters/junctions, **flux** catalyzes any recipe batch
  (+2q, wired in as a second input — never hand-fed), **slag** contaminates cheap
  lots and needs type-filters + the scrap **chute**. Filters gained their type gate.
- **Contracts** — the joker layer, one on every shop rack: Tar Sands (+60% ore,
  +slag), Bulk Manifests (+30% lot size), Sweet Tooth (no wilt), Gentle Hands
  (no crack), Gear Syndicate (gears 1.5×), Purist Clause (q6+ pays 1.5×),
  Flux Injector (+3), Night Shifts (+8 ticks). All applied through the same flat
  per-tile/demand resolution as auras and the market.
- **Measured** (DockBot, a naive splitter-fed furnace bank that avoids dirty lots):
  quotas [130, 175, 235, 310, 410, 540, 700, 900, 1170, 1500, 1930, 2500] give
  deaths spread over rounds 2–5, ratios 1.63 → 0.59 declining. The bot understates
  human capacity (no market play, splitter head-of-line losses, sells its fab), so
  felt difficulty is softer than the ratios suggest. First curve fit for feel, not final.

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

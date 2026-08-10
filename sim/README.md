# Reference simulation

The tick sim, in TypeScript, with no dependencies. Requires [bun](https://bun.sh).

```sh
bun test        # the design doc's numbers, as assertions
bun run report  # what each walkthrough board actually delivers
```

## Why this exists

Two reasons.

**It keeps the design document honest.** The walkthrough in `docs/index.html` quotes exact
figures — 13 ingots, first landing on tick 12, 240 credits from 10 quality-2 gears. Those boards
are checked in as data in `boards.ts` and the tests assert those exact numbers. Change a balance
value in `defs.ts` and the tests tell you which paragraph of the doc just became a lie.

**It's the port target.** Written to the constraint the eventual Godot version must honour: pure
data, no engine types, no wall-clock time, deterministic, order-independent. `sim.ts` should port
to a Godot `RefCounted` more or less line for line.

## Layout

| File | What's in it |
|---|---|
| `defs.ts` | Every balance number in the game. The sim contains none of its own. |
| `sim.ts` | The tick loop. `produce()` then `transfer()`. |
| `boards.ts` | The design doc's walkthrough boards, as data. |
| `sim.test.ts` | The doc's claims, as assertions. |
| `report.ts` | Prints what each board delivers. |

## The interesting part

`transfer()`. Moving every item into the tile ahead fails two ways: a full belt line only advances
at its head (items should move as a train), and a full closed loop deadlocks (it should rotate).

The fix is to move **downstream tiles first**, so each target vacates before its source fills it.
That order is a reverse topological sort of the flow graph — which only exists if the graph is
acyclic, and belt loops are deliberately cyclic. So: Tarjan for strongly connected components,
process the condensed DAG sinks-first, and resolve each multi-tile component (a loop) as a
simultaneous rotation.

That's what makes belt loops behave exactly like straight belts, and it's the reason the
polish-loop build is legal at all.

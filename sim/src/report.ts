import { runBoard } from "./sim";
import { ROUND_1, ROUND_4, LOOP_RIG } from "./boards";
for (const [name, b] of [["ROUND_1", ROUND_1], ["ROUND_4", ROUND_4], ["LOOP_RIG", LOOP_RIG]] as const) {
  const r = runBoard(b.w, b.h, b.cells, 60);
  console.log(`${name}: payout=${r.payout} byType=${JSON.stringify(r.byType)} inFlight=${r.inFlight} jamTicks=${r.jamTicks}`);
  const q = [...new Set(r.delivered.map(d => d.quality))];
  console.log(`   qualities=${JSON.stringify(q)} first@tick=${r.delivered[0]?.tick} last@tick=${r.delivered.at(-1)?.tick}`);
}

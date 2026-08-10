//! The tick simulation. Ported 1:1 from the retired TypeScript reference sim
//! (git history: `sim/src/sim.ts`) and pinned by the same test numbers.
//!
//! Invariants preserved exactly:
//!   - Pure data. No engine types, no I/O, no wall-clock time.
//!   - Deterministic. Same board + same seed => same result, every time.
//!   - Order-independent. Tile iteration order never affects the outcome.

use crate::defs::{
    def, item_value, Dir, ItemType, Kind, MachineDef, MachineId, DUP_CLONE_CHANCE, QUALITY_CAP,
};
use crate::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    /// Stable identity, so a renderer can animate an item between tiles.
    pub id: u64,
    pub ty: ItemType,
    pub quality: i32,
}

/// Filter predicate. An item matches if any set condition holds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FilterCfg {
    pub min_quality: Option<i32>,
    pub item_type: Option<ItemType>,
}

/// A machine placed on the board. `d` is the output edge, `d2` the secondary.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub m: MachineId,
    pub d: Option<Dir>,
    /// Filter: the eject edge. Splitter: the second round-robin edge.
    pub d2: Option<Dir>,
    pub cfg: Option<FilterCfg>,
}

impl Placement {
    pub fn new(x: i32, y: i32, m: MachineId, d: Option<Dir>) -> Self {
        Self { x, y, m, d, d2: None, cfg: None }
    }
}

struct Tile {
    def: Option<&'static MachineDef>,
    d: Option<Dir>,
    d2: Option<Dir>,
    cfg: FilterCfg,
    /// The finished item waiting on the output edge. At most one.
    out: Option<Item>,
    /// Items consumed but not yet assembled (and queued Duplicator clones).
    inputs: Vec<Item>,
    /// Work accumulated toward the current cycle, in ticks.
    progress: f64,
    /// Aura-adjusted work rate. 1.0 is baseline.
    speed: f64,
    /// Aura-granted quality added to this machine's output.
    quality_out: i32,
    jam_immune: bool,
    /// Splitter round-robin cursor.
    rr: u32,
    /// Junction: the travel direction of the item currently on the tile —
    /// it exits the way it entered.
    jdir: Option<Dir>,
}

/// One item hop, emitted per tick so a renderer can interpolate motion.
/// Carries a snapshot of the item so hops into machine inputs or the vault —
/// where the item stops being visible board state — can still be animated.
#[derive(Clone, Copy, Debug)]
pub struct Move {
    pub id: u64,
    pub from: usize,
    pub to: usize,
    pub item: Item,
}

#[derive(Clone, Copy, Debug)]
pub struct Delivery {
    pub tick: u32,
    pub ty: ItemType,
    pub quality: i32,
    pub value: f64,
}

#[derive(Clone, Debug)]
pub struct ShiftResult {
    pub ticks: u32,
    pub payout: i64,
    /// Items still on belts when the shift ended — these are forfeit.
    pub in_flight: usize,
    /// Ticks on which a machine finished work it could not hand off.
    pub jam_ticks: u32,
    pub delivered: Vec<Delivery>,
}

impl ShiftResult {
    pub fn count(&self, ty: ItemType) -> usize {
        self.delivered.iter().filter(|d| d.ty == ty).count()
    }
}

pub struct Sim {
    pub w: i32,
    pub h: i32,
    tiles: Vec<Tile>,
    rng: Rng,
    next_id: u64,
    pub tick: u32,
    pub delivered: Vec<Delivery>,
    pub jam_ticks: u32,
    /// Moves that happened on the most recent `step()`. Cleared each tick.
    pub moves: Vec<Move>,
}

impl Sim {
    pub fn new(w: i32, h: i32, placements: &[Placement], seed: u32) -> Result<Sim, String> {
        let mut tiles: Vec<Tile> = (0..w * h)
            .map(|_| Tile {
                def: None, d: None, d2: None, cfg: FilterCfg::default(),
                out: None, inputs: Vec::new(), progress: 0.0,
                speed: 1.0, quality_out: 0, jam_immune: false, rr: 0, jdir: None,
            })
            .collect();

        for p in placements {
            if p.x < 0 || p.y < 0 || p.x >= w || p.y >= h {
                return Err(format!("placement out of bounds at {},{}", p.x, p.y));
            }
            let t = &mut tiles[(p.y * w + p.x) as usize];
            if t.def.is_some() {
                return Err(format!("two machines on tile {},{}", p.x, p.y));
            }
            t.def = Some(def(p.m));
            t.d = p.d;
            t.d2 = p.d2;
            t.cfg = p.cfg.unwrap_or_default();
        }

        let mut sim = Sim {
            w, h, tiles, rng: Rng::new(seed), next_id: 1,
            tick: 0, delivered: Vec::new(), jam_ticks: 0, moves: Vec::new(),
        };
        sim.apply_auras(placements);
        Ok(sim)
    }

    /// Test/debug accessor: the item sitting on a tile's output edge, if any.
    pub fn peek(&self, x: i32, y: i32) -> Option<Item> {
        self.tiles[(y * self.w + x) as usize].out
    }

    pub fn items_in_flight(&self) -> usize {
        self.tiles.iter().map(|t| t.out.iter().count() + t.inputs.len()).sum()
    }

    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    fn mk(&mut self, ty: ItemType, quality: i32) -> Item {
        let id = self.next_id;
        self.next_id += 1;
        Item { id, ty, quality: quality.min(QUALITY_CAP) }
    }

    /// Auras are resolved once, at build time, into flat per-tile stats.
    /// Stacking is unambiguous: multiplicative for speed, additive for quality.
    fn apply_auras(&mut self, placements: &[Placement]) {
        for p in placements {
            let Some(aura) = def(p.m).aura.as_ref() else { continue };
            for d in Dir::ALL {
                let (dx, dy) = d.delta();
                let (nx, ny) = (p.x + dx, p.y + dy);
                if nx < 0 || ny < 0 || nx >= self.w || ny >= self.h {
                    continue;
                }
                let i = self.idx(nx, ny);
                let n = &mut self.tiles[i];
                let Some(ndef) = n.def else { continue };
                if let Some(tag) = aura.only_tag {
                    if !ndef.tags.contains(&tag) {
                        continue;
                    }
                }
                n.speed *= aura.speed;
                n.quality_out += aura.quality_out;
                if aura.no_jam {
                    n.jam_immune = true;
                }
            }
        }
    }

    /// Which edge does this item leave by? Constant for most machines; for a
    /// Filter it depends on the item, for a Splitter on the round-robin cursor.
    fn out_dir(&self, i: usize, item: Item) -> Option<Dir> {
        let t = &self.tiles[i];
        let d = t.def?;
        if d.id == MachineId::Junction {
            return t.jdir; // straight through: exit the way it came in
        }
        if d.id == MachineId::Filter {
            if let Some(d2) = t.d2 {
                let c = t.cfg;
                let matches = c.min_quality.is_some_and(|m| item.quality >= m)
                    || c.item_type.is_some_and(|ty| item.ty == ty);
                return Some(if matches { d2 } else { t.d? });
            }
        }
        if d.id == MachineId::Splitter {
            if let Some(d2) = t.d2 {
                return Some(if t.rr.is_multiple_of(2) { t.d? } else { d2 });
            }
        }
        t.d
    }

    fn neighbour(&self, i: usize, d: Option<Dir>) -> Option<usize> {
        let d = d?;
        let (x, y) = (i as i32 % self.w, i as i32 / self.w);
        let (dx, dy) = d.delta();
        let (nx, ny) = (x + dx, y + dy);
        if nx < 0 || ny < 0 || nx >= self.w || ny >= self.h {
            return None;
        }
        Some(self.idx(nx, ny))
    }

    /// Can this tile take `item` right now, given its current contents?
    fn can_accept(&self, i: usize, item: Item) -> bool {
        let t = &self.tiles[i];
        let Some(d) = t.def else { return false };
        match d.kind {
            Kind::Vault => true,
            _ if d.transport => t.out.is_none(),
            _ => match &d.recipe {
                Some(r) => {
                    let need = r.inputs.iter().filter(|&&x| x == item.ty).count();
                    if need == 0 {
                        return false;
                    }
                    let have = t.inputs.iter().filter(|x| x.ty == item.ty).count();
                    have < need
                }
                None => false, // extractors and pure modifiers never accept
            },
        }
    }

    /// Structural compatibility, ignoring occupancy. Builds the flow graph.
    fn could_accept(&self, i: usize, item: Item) -> bool {
        let t = &self.tiles[i];
        let Some(d) = t.def else { return false };
        if d.kind == Kind::Vault || d.transport {
            return true;
        }
        match &d.recipe {
            Some(r) => r.inputs.contains(&item.ty),
            None => false,
        }
    }

    /// Travel direction of a hop from tile `i` to adjacent tile `j`.
    fn dir_between(&self, i: usize, j: usize) -> Option<Dir> {
        let (dx, dy) = (j as i32 % self.w - i as i32 % self.w, j as i32 / self.w - i as i32 / self.w);
        Dir::ALL.into_iter().find(|d| d.delta() == (dx, dy))
    }

    fn put(&mut self, i: usize, item: Item, travel: Option<Dir>) {
        let d = self.tiles[i].def.unwrap();
        match d.kind {
            Kind::Vault => {
                self.delivered.push(Delivery {
                    tick: self.tick, ty: item.ty, quality: item.quality,
                    value: item_value(item.ty, item.quality),
                });
            }
            _ if d.transport => {
                let q = (item.quality + d.quality_bonus).min(QUALITY_CAP);
                let placed = Item { id: item.id, ty: item.ty, quality: q };
                self.tiles[i].out = Some(placed);
                self.tiles[i].jdir = travel;
                // Duplicator: a clone queues behind the original, released as
                // soon as the output slot frees. Inside a loop, this is the
                // exponential.
                if d.id == MachineId::Dup && self.rng.next_f64() < DUP_CLONE_CHANCE {
                    let clone = self.mk(placed.ty, placed.quality);
                    self.tiles[i].inputs.push(clone);
                }
            }
            _ => self.tiles[i].inputs.push(item),
        }
    }

    /// Production pass: advance cycles, move finished goods to the output edge.
    fn produce(&mut self) {
        for i in 0..self.tiles.len() {
            let Some(d) = self.tiles[i].def else { continue };

            // Belt-like tiles with a queued duplicate release it when free.
            if d.transport {
                let t = &mut self.tiles[i];
                if t.out.is_none() && !t.inputs.is_empty() {
                    t.out = Some(t.inputs.remove(0));
                }
                continue;
            }

            if let Some(produces) = d.produces {
                if self.tiles[i].out.is_some() {
                    if !self.tiles[i].jam_immune {
                        self.jam_ticks += 1;
                    }
                    continue; // jammed: output edge still occupied
                }
                self.tiles[i].progress += self.tiles[i].speed;
                if self.tiles[i].progress >= d.period {
                    self.tiles[i].progress -= d.period;
                    let item = self.mk(produces, d.spawn_quality);
                    self.tiles[i].out = Some(item);
                }
                continue;
            }

            if let Some(r) = &d.recipe {
                let t = &self.tiles[i];
                let ready = r.inputs.iter().all(|need| {
                    let want = r.inputs.iter().filter(|&&x| x == *need).count();
                    t.inputs.iter().filter(|x| x.ty == *need).count() >= want
                });
                if !ready {
                    continue;
                }
                if t.out.is_some() {
                    if !t.jam_immune {
                        self.jam_ticks += 1;
                    }
                    continue;
                }
                self.tiles[i].progress += self.tiles[i].speed;
                if self.tiles[i].progress >= r.ticks {
                    self.tiles[i].progress -= r.ticks;
                    let mut consumed: Vec<Item> = Vec::with_capacity(r.inputs.len());
                    for need in r.inputs {
                        let t = &mut self.tiles[i];
                        let k = t.inputs.iter().position(|x| x.ty == *need).unwrap();
                        consumed.push(t.inputs.remove(k));
                    }
                    let mean_q = consumed.iter().map(|c| c.quality).sum::<i32>() as f64
                        / consumed.len() as f64;
                    let q = mean_q.floor() as i32 + self.tiles[i].quality_out;
                    let item = self.mk(r.output, q);
                    self.tiles[i].out = Some(item);
                }
            }
        }
    }

    fn commit_move(&mut self, i: usize, j: usize) {
        let item = self.tiles[i].out.take().unwrap();
        self.moves.push(Move { id: item.id, from: i, to: j, item });
        if self.tiles[i].def.unwrap().id == MachineId::Splitter {
            self.tiles[i].rr += 1;
        }
        let travel = self.dir_between(i, j);
        self.put(j, item, travel);
    }

    /// Transfer pass — the one genuinely hard problem in the whole design.
    ///
    /// Move DOWNSTREAM TILES FIRST so each target vacates before its source
    /// fills it: reverse topological order over the flow graph. Belt loops are
    /// deliberately cyclic, so: Tarjan SCC, process the condensed DAG
    /// sinks-first, and resolve each multi-tile SCC — a loop — as a
    /// simultaneous rotation.
    fn transfer(&mut self) {
        let n = self.tiles.len();
        let mut edge = vec![-1i32; n];
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            let Some(item) = self.tiles[i].out else { continue };
            if self.tiles[i].def.is_none() {
                continue;
            }
            let Some(j) = self.neighbour(i, self.out_dir(i, item)) else { continue };
            if !self.could_accept(j, item) {
                continue;
            }
            edge[i] = j as i32;
        }

        // ── Tarjan SCC over the (at most one out-edge per node) flow graph ──
        let mut index = vec![-1i32; n];
        let mut low = vec![0i32; n];
        let mut on_stack = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        let mut comps: Vec<Vec<usize>> = Vec::new();
        let mut counter = 0i32;

        for s in 0..n {
            if index[s] != -1 || edge[s] == -1 {
                continue;
            }
            // iterative DFS — boards get large and recursion depth is a risk
            let mut work: Vec<(usize, u8)> = vec![(s, 0)];
            while let Some(frame) = work.last_mut() {
                let v = frame.0;
                if frame.1 == 0 {
                    index[v] = counter;
                    low[v] = counter;
                    counter += 1;
                    stack.push(v);
                    on_stack[v] = true;
                }
                let w_node = if frame.1 == 0 { edge[v] } else { -1 };
                frame.1 = 1;
                if w_node != -1 && index[w_node as usize] == -1 {
                    work.push((w_node as usize, 0));
                    continue;
                }
                if w_node != -1 && on_stack[w_node as usize] {
                    low[v] = low[v].min(index[w_node as usize]);
                }
                work.pop();
                if let Some(&(p, _)) = work.last() {
                    low[p] = low[p].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    loop {
                        let u = stack.pop().unwrap();
                        on_stack[u] = false;
                        comp.push(u);
                        if u == v {
                            break;
                        }
                    }
                    comps.push(comp);
                }
            }
        }

        // Tarjan emits components in reverse topological order — sinks first,
        // which is exactly the order we want to move in.
        for comp in comps {
            if comp.len() == 1 {
                let i = comp[0];
                let j = edge[i];
                let Some(item) = self.tiles[i].out else { continue };
                if j < 0 {
                    continue;
                }
                if self.can_accept(j as usize, item) {
                    self.commit_move(i, j as usize);
                } else {
                    let t = &self.tiles[i];
                    // Belts stalled behind a busy machine are normal
                    // backpressure, not a jam. Only a machine that finished
                    // work it cannot hand off counts.
                    if !t.jam_immune && !t.def.unwrap().transport {
                        self.jam_ticks += 1;
                    }
                }
                continue;
            }

            // Multi-tile SCC: a closed loop. Snapshot first, then commit —
            // otherwise the result depends on iteration order.
            let snapshot: Vec<Option<Item>> = comp.iter().map(|&i| self.tiles[i].out).collect();
            let movable = snapshot.iter().all(|s| s.is_some());
            if !movable {
                // Partially empty ring: the hole propagates backward around
                // the loop. Sort by tile index and mark destinations so
                // nothing moves twice — independent of Tarjan emission order.
                let mut order = comp.clone();
                order.sort_unstable();
                let mut moved = vec![false; n];
                for _ in 0..order.len() {
                    let mut changed = false;
                    for &i in &order {
                        let j = edge[i];
                        let Some(item) = self.tiles[i].out else { continue };
                        if j < 0 || moved[i] {
                            continue;
                        }
                        if !self.can_accept(j as usize, item) {
                            continue;
                        }
                        self.commit_move(i, j as usize);
                        moved[j as usize] = true;
                        changed = true;
                    }
                    if !changed {
                        break;
                    }
                }
                continue;
            }
            for &i in &comp {
                self.tiles[i].out = None;
            }
            for (k, &i) in comp.iter().enumerate() {
                let item = snapshot[k].unwrap();
                let j = edge[i] as usize;
                self.moves.push(Move { id: item.id, from: i, to: j, item });
                if self.tiles[i].def.unwrap().id == MachineId::Splitter {
                    self.tiles[i].rr += 1;
                }
                let travel = self.dir_between(i, j);
                self.put(j, item, travel);
            }
        }
    }

    pub fn step(&mut self) {
        self.tick += 1;
        self.moves.clear();
        self.produce();
        self.transfer();
    }

    pub fn run(&mut self, ticks: u32) -> ShiftResult {
        for _ in 0..ticks {
            self.step();
        }
        self.result(ticks)
    }

    pub fn result(&self, ticks: u32) -> ShiftResult {
        ShiftResult {
            ticks,
            payout: self.delivered.iter().map(|d| d.value).sum::<f64>().round() as i64,
            in_flight: self.items_in_flight(),
            jam_ticks: self.jam_ticks,
            delivered: self.delivered.clone(),
        }
    }
}

pub fn run_board(
    w: i32, h: i32, placements: &[Placement], ticks: u32, seed: u32,
) -> Result<ShiftResult, String> {
    Ok(Sim::new(w, h, placements, seed)?.run(ticks))
}

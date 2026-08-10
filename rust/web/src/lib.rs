//! The wasm ABI: how the browser drives the game.
//!
//! Shape of the contract, chosen so no binding generator is needed:
//!
//!   - Every inbound argument is a scalar (tile coords, hand index, a
//!     direction code 0=N 1=E 2=S 3=W, -1 for "none").
//!   - Every call returns the byte length of a JSON document the JS side
//!     reads out of linear memory at `out_ptr()`. Commands return the full
//!     game state (with an `err` field when the command was refused, in
//!     which case the state is unchanged); `project` and `shift_step`
//!     return their own small documents.
//!
//! The JS layer holds NO game rules — it renders state and forwards input.
//! That invariant carried the first prototype and it carries this one.

use overflow_core::defs::{
    def, Dir, ItemType, Kind, MachineId, Tag, CARD_POOL, DUP_CLONE_CHANCE, QUALITY_CAP,
    QUALITY_STEP,
};
use overflow_core::deck::Card;
use overflow_core::run::{is_audit, shift_len, Game, GamePhase, BOARD_H, BOARD_W, QUOTAS};
use overflow_core::sim::{FilterCfg, Placement, Sim};
use std::cell::RefCell;
use std::fmt::Write as _;

struct App {
    game: Game,
    /// The live, tick-by-tick sim during the shift animation. Same seed as
    /// the commit in `shift_finish`, so what you watch is what you get.
    shift: Option<Sim>,
    /// Tiles staged for a group move (`sel_add` … `sel_move`).
    sel: Vec<(i32, i32)>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static OUT: RefCell<String> = const { RefCell::new(String::new()) };
}

// ── the exported surface ─────────────────────────────────────────────────────

/// Start (or restart) a run. Returns state.
#[no_mangle]
pub extern "C" fn boot(seed: u32) -> usize {
    APP.with(|a| {
        *a.borrow_mut() = Some(App { game: Game::new(seed), shift: None, sel: Vec::new() })
    });
    state()
}

/// Where the last returned JSON document lives in linear memory. Call after
/// every command — the buffer may reallocate.
#[no_mangle]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

/// Current state, unchanged.
#[no_mangle]
pub extern "C" fn state() -> usize {
    with_app(|app| state_json(&app.game, None))
}

/// Lay a belt (infrastructure, not a card).
#[no_mangle]
pub extern "C" fn belt(x: i32, y: i32, d: i32) -> usize {
    command(|g| g.buy_belt(x, y, dir_from(d)?))
}

/// Play the hand card at `hand_idx` onto (x, y). `d2` and `min_q` are -1
/// unless placing a Splitter (`d2`) or Filter (`d2` + `min_q`).
#[no_mangle]
pub extern "C" fn play(hand_idx: u32, x: i32, y: i32, d: i32, d2: i32, min_q: i32) -> usize {
    command(|g| {
        let d = dir_from(d)?;
        let d2 = if d2 < 0 { None } else { Some(dir_from(d2)?) };
        let cfg = (min_q >= 0).then_some(FilterCfg { min_quality: Some(min_q), item_type: None });
        g.play_card(hand_idx as usize, x, y, Some(d), d2, cfg)
    })
}

/// Sell the machine at (x, y): full refund, card back to the discard.
#[no_mangle]
pub extern "C" fn sell(x: i32, y: i32) -> usize {
    command(|g| g.sell(x, y))
}

/// Rotate a machine's output edge clockwise.
#[no_mangle]
pub extern "C" fn rotate(x: i32, y: i32) -> usize {
    command(|g| g.rotate(x, y))
}

/// Rotate the secondary edge (Filter eject / Splitter second output).
#[no_mangle]
pub extern "C" fn rotate2(x: i32, y: i32) -> usize {
    command(|g| g.rotate_d2(x, y))
}

/// Set a Filter's quality gate.
#[no_mangle]
pub extern "C" fn set_gate(x: i32, y: i32, q: i32) -> usize {
    command(|g| g.set_filter_gate(x, y, q))
}

/// Empty the staged move selection.
#[no_mangle]
pub extern "C" fn sel_clear() -> usize {
    with_app(|app| {
        app.sel.clear();
        state_json(&app.game, None)
    })
}

/// Stage a tile for the next `sel_move`.
#[no_mangle]
pub extern "C" fn sel_add(x: i32, y: i32) -> usize {
    with_app(|app| {
        if !app.sel.contains(&(x, y)) {
            app.sel.push((x, y));
        }
        state_json(&app.game, None)
    })
}

/// Move everything staged by (dx, dy) as one rigid piece — all or nothing.
/// The selection is consumed either way.
#[no_mangle]
pub extern "C" fn sel_move(dx: i32, dy: i32) -> usize {
    with_app(|app| {
        let tiles = std::mem::take(&mut app.sel);
        let err = app.game.move_by(&tiles, dx, dy).err();
        state_json(&app.game, err.as_deref())
    })
}

/// Take the reward at `offer_idx`, or skip with -1.
#[no_mangle]
pub extern "C" fn pick_reward(offer_idx: i32) -> usize {
    command(|g| g.pick_reward((offer_idx >= 0).then_some(offer_idx as usize)))
}

/// Dry-run the whole shift: `{"payout":..,"inFlight":..,"jamTicks":..}`.
#[no_mangle]
pub extern "C" fn project() -> usize {
    with_app(|app| match app.game.project() {
        Ok(r) => format!(
            "{{\"payout\":{},\"inFlight\":{},\"jamTicks\":{}}}",
            r.payout, r.in_flight, r.jam_ticks
        ),
        Err(e) => err_json(&e),
    })
}

/// Begin the animated shift. Returns state.
#[no_mangle]
pub extern "C" fn shift_start() -> usize {
    with_app(|app| match app.game.shift_sim() {
        Ok(sim) => {
            app.shift = Some(sim);
            state_json(&app.game, None)
        }
        Err(e) => state_json(&app.game, Some(&e)),
    })
}

/// Advance the animated shift one tick and return a render frame:
/// `{"tick":..,"total":..,"payout":..,"done":..,"items":[..],"moves":[..]}`.
#[no_mangle]
pub extern "C" fn shift_step() -> usize {
    with_app(|app| {
        let total = shift_len(app.game.round);
        let Some(sim) = app.shift.as_mut() else {
            return err_json("no shift running");
        };
        if sim.tick < total {
            sim.step();
        }
        frame_json(sim, total)
    })
}

/// Commit the shift: the game re-runs the identical sim and advances the
/// round state machine. Returns state (phase becomes reward/over).
#[no_mangle]
pub extern "C" fn shift_finish() -> usize {
    with_app(|app| {
        app.shift = None;
        let err = app.game.run_shift().err();
        state_json(&app.game, err.as_deref())
    })
}

/// The machine catalogue: every definition the game runs on, as data, so the
/// UI can explain a card — recipe, aura, tags, item values — without a single
/// rule living in JavaScript. Static; fetch once at boot.
#[no_mangle]
pub extern "C" fn catalog() -> usize {
    let doc = catalog_json();
    OUT.with(|o| {
        *o.borrow_mut() = doc;
        o.borrow().len()
    })
}

/// Host-side test access to the same buffer JS reads.
pub fn out_string() -> String {
    OUT.with(|o| o.borrow().clone())
}

// ── plumbing ─────────────────────────────────────────────────────────────────

fn with_app<F: FnOnce(&mut App) -> String>(f: F) -> usize {
    let doc = APP.with(|a| match a.borrow_mut().as_mut() {
        Some(app) => f(app),
        None => err_json("not booted"),
    });
    OUT.with(|o| {
        *o.borrow_mut() = doc;
        o.borrow().len()
    })
}

fn command<F: FnOnce(&mut Game) -> Result<(), String>>(f: F) -> usize {
    with_app(|app| {
        let err = f(&mut app.game).err();
        state_json(&app.game, err.as_deref())
    })
}

fn dir_from(code: i32) -> Result<Dir, String> {
    match code {
        0 => Ok(Dir::N),
        1 => Ok(Dir::E),
        2 => Ok(Dir::S),
        3 => Ok(Dir::W),
        _ => Err(format!("bad direction code {code}")),
    }
}

// ── JSON building ────────────────────────────────────────────────────────────

fn machine_key(m: MachineId) -> &'static str {
    use MachineId as M;
    match m {
        M::Drill => "drill",
        M::Tap => "tap",
        M::Geode => "geode",
        M::Furnace => "furnace",
        M::Retort => "retort",
        M::Lapidary => "lapidary",
        M::Compress => "compress",
        M::Fab => "fab",
        M::CircuitBench => "circuit",
        M::LensGrinder => "lens",
        M::EngineWorks => "engine",
        M::Belt => "belt",
        M::Merger => "merger",
        M::Splitter => "splitter",
        M::Buffer => "buffer",
        M::Filter => "filter",
        M::Overclock => "overclock",
        M::Polisher => "polisher",
        M::Heatsink => "heatsink",
        M::Dup => "dup",
        M::Vault => "vault",
    }
}

fn kind_key(k: Kind) -> &'static str {
    match k {
        Kind::Extractor => "extractor",
        Kind::Processor => "processor",
        Kind::Assembler => "assembler",
        Kind::Logistics => "logistics",
        Kind::Modifier => "modifier",
        Kind::Vault => "vault",
    }
}

fn item_key(t: ItemType) -> &'static str {
    use ItemType as I;
    match t {
        I::Ore => "ore",
        I::Sap => "sap",
        I::Crystal => "crystal",
        I::Ingot => "ingot",
        I::Resin => "resin",
        I::Shard => "shard",
        I::Gear => "gear",
        I::Circuit => "circuit",
        I::Lens => "lens",
        I::Engine => "engine",
        I::Core => "core",
        I::Beacon => "beacon",
    }
}

fn tag_key(t: Tag) -> &'static str {
    match t {
        Tag::Heat => "heat",
        Tag::Kinetic => "kinetic",
        Tag::Volt => "volt",
        Tag::Precision => "precision",
        Tag::Organic => "organic",
    }
}

/// One or two sentences of behaviour per machine. Mechanics numbers in the
/// catalogue come from `defs.rs`; these cover the behaviours that live in the
/// sim's code paths (gates, round-robin, cloning) in plain words.
fn blurb(m: MachineId) -> String {
    use MachineId as M;
    match m {
        M::Drill => "Pulls Ore out of the ground on a fixed cycle. The start of every metal chain.".into(),
        M::Tap => "Slow, but its Sap starts at quality 1 — the only extractor with a head start.".into(),
        M::Geode => "Cracks out Crystal, slowly. The way into the Precision chain.".into(),
        M::Furnace => "Smelts one Ore into one Ingot — a 4× value jump per item.".into(),
        M::Retort => "Cooks Sap into Resin.".into(),
        M::Lapidary => "Cuts Crystal into Shards.".into(),
        M::Compress => "Crushes FOUR Ore into one Ingot. Trades throughput for belt space — feed it well or it starves.".into(),
        M::Fab => "Two Ingots become one Gear — another 4× jump. Usually the first bottleneck worth boosting.".into(),
        M::CircuitBench => "Ingot + Shard → Circuit. Marries the metal and precision chains.".into(),
        M::LensGrinder => "Shard + Resin → Lens. Marries the precision and organic chains.".into(),
        M::EngineWorks => "Gear + Circuit → Engine. Top of the chain, 64 base value.".into(),
        M::Belt => "Moves one item per tick in its arrow direction. 1 credit, always available, never a card.".into(),
        M::Merger => "A belt that accepts from every side and sends everything out one edge. Joins lanes.".into(),
        M::Splitter => "Alternates items between its two output edges, one-for-one. Splits a lane fairly.".into(),
        M::Buffer => "Currently behaves as a plain belt — its hold-and-release policy is an open design question (NOTES.md).".into(),
        M::Filter => "Passes items straight through until one meets its quality gate, then ejects it out the side edge. The exit valve that makes polish loops pay.".into(),
        M::Overclock => "Aura: adjacent machines work faster. Park it next to your bottleneck.".into(),
        M::Polisher => "Every item passing through gains +1 quality. In a closed belt loop, items lap it and climb toward the cap.".into(),
        M::Heatsink => "Aura: adjacent HEAT machines gain +1 output quality and never jam. Does nothing for anything else.".into(),
        M::Dup => format!(
            "Items passing through have a {:.0}% chance to be cloned; the clone follows once the path ahead clears. In a loop, this is the exponential — and the brick.",
            DUP_CLONE_CHANCE * 100.0
        ),
        M::Vault => "Deliver here. Pays base value × (1 + 0.25 × quality). Items still on belts at the horn are forfeit.".into(),
    }
}

fn push_machine_def(s: &mut String, m: MachineId) {
    let d = def(m);
    let _ = write!(
        s,
        "{{\"m\":\"{}\",\"name\":\"{}\",\"kind\":\"{}\",\"cost\":{},\"tags\":[",
        machine_key(m),
        d.name,
        kind_key(d.kind),
        d.cost
    );
    for (i, t) in d.tags.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "\"{}\"", tag_key(*t));
    }
    s.push_str("],");
    match d.produces {
        Some(p) => {
            let _ = write!(
                s,
                "\"produces\":\"{}\",\"period\":{},\"spawnQ\":{},",
                item_key(p),
                d.period,
                d.spawn_quality
            );
        }
        None => s.push_str("\"produces\":null,\"period\":0,\"spawnQ\":0,"),
    }
    match &d.recipe {
        Some(r) => {
            s.push_str("\"recipe\":{\"inputs\":[");
            for (i, inp) in r.inputs.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                let _ = write!(s, "\"{}\"", item_key(*inp));
            }
            let _ = write!(s, "],\"output\":\"{}\",\"ticks\":{}}},", item_key(r.output), r.ticks);
        }
        None => s.push_str("\"recipe\":null,"),
    }
    let _ = write!(
        s,
        "\"transport\":{},\"qualityBonus\":{},",
        d.transport, d.quality_bonus
    );
    match &d.aura {
        Some(a) => {
            let _ = write!(
                s,
                "\"aura\":{{\"speed\":{},\"q\":{},\"noJam\":{},\"onlyTag\":",
                a.speed, a.quality_out, a.no_jam
            );
            match a.only_tag {
                Some(t) => {
                    let _ = write!(s, "\"{}\"}},", tag_key(t));
                }
                None => s.push_str("null},"),
            }
        }
        None => s.push_str("\"aura\":null,"),
    }
    s.push_str("\"blurb\":\"");
    push_escaped(s, &blurb(m));
    s.push_str("\"}");
}

fn catalog_json() -> String {
    const ITEMS: [ItemType; 12] = [
        ItemType::Ore, ItemType::Sap, ItemType::Crystal,
        ItemType::Ingot, ItemType::Resin, ItemType::Shard,
        ItemType::Gear, ItemType::Circuit, ItemType::Lens,
        ItemType::Engine, ItemType::Core, ItemType::Beacon,
    ];
    let mut s = String::with_capacity(8192);
    let _ = write!(
        s,
        "{{\"qualityStep\":{QUALITY_STEP},\"qualityCap\":{QUALITY_CAP},\"dupChance\":{DUP_CLONE_CHANCE},\"items\":{{"
    );
    for (i, it) in ITEMS.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "\"{}\":{}", item_key(*it), it.base_value());
    }
    s.push_str("},\"machines\":[");
    for (i, m) in CARD_POOL
        .iter()
        .copied()
        .chain([MachineId::Belt, MachineId::Vault])
        .enumerate()
    {
        if i > 0 {
            s.push(',');
        }
        push_machine_def(&mut s, m);
    }
    s.push_str("]}");
    s
}

fn dir_key(d: Dir) -> &'static str {
    match d {
        Dir::N => "N",
        Dir::S => "S",
        Dir::E => "E",
        Dir::W => "W",
    }
}

fn push_dir(out: &mut String, key: &str, d: Option<Dir>) {
    match d {
        Some(d) => {
            let _ = write!(out, "\"{key}\":\"{}\"", dir_key(d));
        }
        None => {
            let _ = write!(out, "\"{key}\":null");
        }
    }
}

fn push_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}

fn err_json(msg: &str) -> String {
    let mut s = String::from("{\"err\":\"");
    push_escaped(&mut s, msg);
    s.push_str("\"}");
    s
}

fn push_card(out: &mut String, c: Card) {
    let d = def(c.machine);
    let _ = write!(
        out,
        "{{\"m\":\"{}\",\"name\":\"{}\",\"cost\":{},\"kind\":\"{}\"}}",
        machine_key(d.id),
        d.name,
        d.cost,
        kind_key(d.kind)
    );
}

fn push_placement(out: &mut String, p: &Placement) {
    let d = def(p.m);
    let _ = write!(out, "{{\"x\":{},\"y\":{},\"m\":\"{}\",\"kind\":\"{}\",", p.x, p.y, machine_key(p.m), kind_key(d.kind));
    push_dir(out, "d", p.d);
    out.push(',');
    push_dir(out, "d2", p.d2);
    match p.cfg.and_then(|c| c.min_quality) {
        Some(q) => {
            let _ = write!(out, ",\"minQ\":{q}}}");
        }
        None => out.push_str(",\"minQ\":null}"),
    }
}

fn state_json(g: &Game, err: Option<&str>) -> String {
    let (phase, won) = match g.phase {
        GamePhase::Build => ("build", false),
        GamePhase::Reward => ("reward", false),
        GamePhase::Over { won } => ("over", won),
    };
    let mut s = String::with_capacity(2048);
    let _ = write!(
        s,
        "{{\"round\":{},\"credits\":{},\"quota\":{},\"shiftLen\":{},\"audit\":{},\
          \"phase\":\"{}\",\"won\":{},\"qualityCap\":{},\"boardW\":{},\"boardH\":{},",
        g.round,
        g.credits,
        g.quota(),
        shift_len(g.round),
        is_audit(g.round),
        phase,
        won,
        QUALITY_CAP,
        BOARD_W,
        BOARD_H,
    );

    // During the reward phase the modal advertises the round about to start,
    // which the game hasn't advanced to yet.
    match g.phase {
        GamePhase::Reward if g.round + 1 < QUOTAS.len() => {
            let next = g.round + 1;
            let _ = write!(
                s,
                "\"nextQuota\":{},\"nextAudit\":{},",
                QUOTAS[next],
                is_audit(next)
            );
        }
        _ => s.push_str("\"nextQuota\":null,\"nextAudit\":false,"),
    }

    s.push_str("\"board\":[");
    for (i, p) in g.board.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        push_placement(&mut s, p);
    }
    s.push_str("],\"hand\":[");
    for (i, c) in g.hand.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        push_card(&mut s, *c);
    }
    s.push_str("],\"offers\":[");
    for (i, c) in g.offers.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        push_card(&mut s, *c);
    }
    let _ = write!(
        s,
        "],\"deckDraw\":{},\"deckDiscard\":{},",
        g.deck.draw_count(),
        g.deck.discard_count()
    );

    // The flow graph: every output edge on the board, and whether the tile it
    // points at can actually take items from it. This is what lets the
    // renderer draw belts as connected paths and flag misrouted arrows,
    // without owning any routing rules.
    //   ok:   the target accepts (transport, vault, or a compatible recipe)
    //   open: points at an empty tile — an unfinished line, not an error
    //   bad:  off the board, or a machine that will never take this item
    s.push_str("\"flows\":[");
    let mut first = true;
    for p in &g.board {
        let d = def(p.m);
        let emitter = d.transport || d.produces.is_some() || d.recipe.is_some();
        if !emitter {
            continue;
        }
        // What this tile is known to emit; None for transport (cargo unknown).
        let emitted = d.produces.or(d.recipe.as_ref().map(|r| r.output));
        let mut edges: Vec<(Dir, bool)> = Vec::new();
        if let Some(dir) = p.d {
            edges.push((dir, false));
        }
        if matches!(p.m, MachineId::Filter | MachineId::Splitter) {
            if let Some(d2) = p.d2 {
                edges.push((d2, true));
            }
        }
        for (dir, secondary) in edges {
            let (dx, dy) = dir.delta();
            let (tx, ty) = (p.x + dx, p.y + dy);
            let status = if tx < 0 || ty < 0 || tx >= BOARD_W || ty >= BOARD_H {
                "bad"
            } else {
                match g.board.iter().find(|q| q.x == tx && q.y == ty) {
                    None => "open",
                    Some(q) => {
                        let qd = def(q.m);
                        if qd.kind == Kind::Vault || qd.transport {
                            "ok"
                        } else if let Some(r) = &qd.recipe {
                            match emitted {
                                Some(t) if !r.inputs.contains(&t) => "bad",
                                _ => "ok",
                            }
                        } else {
                            "bad" // extractor or pure-aura modifier: never accepts
                        }
                    }
                }
            };
            if !first {
                s.push(',');
            }
            first = false;
            let _ = write!(
                s,
                "{{\"fx\":{},\"fy\":{},\"tx\":{tx},\"ty\":{ty},\"d\":\"{}\",\"status\":\"{status}\",\"secondary\":{secondary}}}",
                p.x,
                p.y,
                dir_key(dir)
            );
        }
    }
    s.push_str("],");

    // Tiles receiving an aura, so the renderer can halo them without ever
    // knowing the adjacency rules itself.
    s.push_str("\"auras\":[");
    let mut first = true;
    for p in &g.board {
        let Some(aura) = def(p.m).aura.as_ref() else { continue };
        for d in Dir::ALL {
            let (dx, dy) = d.delta();
            let (nx, ny) = (p.x + dx, p.y + dy);
            let Some(n) = g.board.iter().find(|q| q.x == nx && q.y == ny) else { continue };
            if let Some(tag) = aura.only_tag {
                if !def(n.m).tags.contains(&tag) {
                    continue;
                }
            }
            if !first {
                s.push(',');
            }
            first = false;
            let _ = write!(s, "{{\"x\":{nx},\"y\":{ny}}}");
        }
    }
    s.push_str("],");

    match g.history.last() {
        Some(o) => {
            let _ = write!(
                s,
                "\"last\":{{\"round\":{},\"payout\":{},\"quota\":{},\"cleared\":{},\"inFlight\":{},\"jamTicks\":{}}},",
                o.round, o.result.payout, o.quota, o.cleared, o.result.in_flight, o.result.jam_ticks
            );
        }
        None => s.push_str("\"last\":null,"),
    }

    match err {
        Some(e) => {
            s.push_str("\"err\":\"");
            push_escaped(&mut s, e);
            s.push_str("\"}");
        }
        None => s.push_str("\"err\":null}"),
    }
    s
}

fn frame_json(sim: &Sim, total: u32) -> String {
    let mut s = String::with_capacity(1024);
    let payout = sim.result(sim.tick).payout;
    let _ = write!(
        s,
        "{{\"tick\":{},\"total\":{},\"payout\":{},\"done\":{},\"items\":[",
        sim.tick,
        total,
        payout,
        sim.tick >= total
    );
    let mut first = true;
    let mut visible: Vec<u64> = Vec::new();
    for y in 0..sim.h {
        for x in 0..sim.w {
            let Some(item) = sim.peek(x, y) else { continue };
            visible.push(item.id);
            if !first {
                s.push(',');
            }
            first = false;
            let _ = write!(
                s,
                "{{\"id\":{},\"x\":{},\"y\":{},\"t\":\"{}\",\"q\":{}}}",
                item.id,
                x,
                y,
                item_key(item.ty),
                item.quality
            );
        }
    }
    s.push_str("],\"moves\":[");
    for (i, m) in sim.moves.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(
            s,
            "{{\"id\":{},\"fx\":{},\"fy\":{}}}",
            m.id,
            m.from as i32 % sim.w,
            m.from as i32 / sim.w
        );
    }
    // Hops: moves whose item was consumed on arrival (machine input, vault
    // delivery) and so appears in no out slot. Without these, the last leg of
    // every journey — and the whole journey for machine→machine transfers —
    // would never be seen.
    s.push_str("],\"hops\":[");
    first = true;
    for m in sim.moves.iter().filter(|m| !visible.contains(&m.id)) {
        if !first {
            s.push(',');
        }
        first = false;
        let _ = write!(
            s,
            "{{\"id\":{},\"fx\":{},\"fy\":{},\"x\":{},\"y\":{},\"t\":\"{}\",\"q\":{}}}",
            m.id,
            m.from as i32 % sim.w,
            m.from as i32 / sim.w,
            m.to as i32 % sim.w,
            m.to as i32 / sim.w,
            item_key(m.item.ty),
            m.item.quality
        );
    }
    s.push_str("]}");
    s
}

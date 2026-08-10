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

use overflow_core::defs::{def, Dir, ItemType, Kind, MachineId, QUALITY_CAP};
use overflow_core::deck::Card;
use overflow_core::run::{is_audit, shift_len, Game, GamePhase, BOARD_H, BOARD_W};
use overflow_core::sim::{FilterCfg, Placement, Sim};
use std::cell::RefCell;
use std::fmt::Write as _;

struct App {
    game: Game,
    /// The live, tick-by-tick sim during the shift animation. Same seed as
    /// the commit in `shift_finish`, so what you watch is what you get.
    shift: Option<Sim>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static OUT: RefCell<String> = const { RefCell::new(String::new()) };
}

// ── the exported surface ─────────────────────────────────────────────────────

/// Start (or restart) a run. Returns state.
#[no_mangle]
pub extern "C" fn boot(seed: u32) -> usize {
    APP.with(|a| *a.borrow_mut() = Some(App { game: Game::new(seed), shift: None }));
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
    for y in 0..sim.h {
        for x in 0..sim.w {
            let Some(item) = sim.peek(x, y) else { continue };
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
    s.push_str("]}");
    s
}

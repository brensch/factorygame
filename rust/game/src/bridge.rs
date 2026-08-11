//! The only door between the UI and the rules. Input systems push [`Cmd`]s;
//! one system applies them to the core [`Game`] and flips `dirty` so the
//! scene repaints. Nothing else in this crate may mutate game state —
//! the coupling the old JSON ABI enforced at the wasm boundary is enforced
//! here by ownership.

use bevy::prelude::*;
use overflow_core::defs::Dir;
use overflow_core::run::Game;
use overflow_core::sim::Sim;

/// Everything the player can do, as data.
#[derive(Clone, Debug)]
pub enum Cmd {
    // build phase
    PlayCard { hand_idx: usize, x: i32, y: i32, d: Dir },
    Belt { x: i32, y: i32, d: Dir },
    Junction { x: i32, y: i32 },
    Merger { x: i32, y: i32, d: Dir },
    Splitter { x: i32, y: i32, d: Dir },
    Chute { x: i32, y: i32 },
    Sell { x: i32, y: i32 },
    Rotate { x: i32, y: i32 },
    // supply window
    BuyLot(usize),
    Allocate { supply_idx: usize, bay: usize },
    Unslot { bay: usize, slot: usize },
    SupplyDone,
    // shifts
    StartShift,
    // shop
    ShopBuy(usize),
    BuyContract(usize),
    SellContract(usize),
    Reroll,
    ShopDone,
    // run flow
    Retry,
    NewRun(u32),
}

/// Live shift playback: the sim being stepped for the animation, plus the
/// wall-clock accumulator that paces it.
pub struct Playback {
    pub sim: Sim,
    pub acc: f32,
}

#[derive(Resource)]
pub struct Bridge {
    pub game: Game,
    pub shift: Option<Playback>,
    pub queue: Vec<Cmd>,
    /// Most recent refusal from the core, shown as a toast and faded.
    pub toast: Option<(String, f32)>,
    /// Sim ticks per second while a shift plays.
    pub speed: f32,
    pub dirty: bool,
}

impl Bridge {
    pub fn new(seed: u32) -> Self {
        Bridge {
            game: Game::new(seed),
            shift: None,
            queue: Vec::new(),
            toast: None,
            speed: 12.0,
            dirty: true,
        }
    }
    pub fn push(&mut self, cmd: Cmd) {
        self.queue.push(cmd);
    }
}

pub fn apply_cmds(mut bridge: ResMut<Bridge>) {
    if bridge.queue.is_empty() {
        return;
    }
    let cmds = std::mem::take(&mut bridge.queue);
    for cmd in cmds {
        let b = &mut *bridge;
        let res: Result<(), String> = match cmd.clone() {
            Cmd::PlayCard { hand_idx, x, y, d } => {
                b.game.play_card(hand_idx, x, y, Some(d), None, None)
            }
            Cmd::Belt { x, y, d } => b.game.buy_belt(x, y, d),
            Cmd::Junction { x, y } => b.game.buy_junction(x, y),
            Cmd::Merger { x, y, d } => b.game.buy_merger(x, y, d),
            Cmd::Splitter { x, y, d } => b.game.buy_splitter(x, y, d),
            Cmd::Chute { x, y } => b.game.buy_chute(x, y),
            Cmd::Sell { x, y } => b.game.sell(x, y),
            Cmd::Rotate { x, y } => b.game.rotate(x, y),
            Cmd::BuyLot(i) => b.game.buy_lot(i),
            Cmd::Allocate { supply_idx, bay } => b.game.allocate(supply_idx, bay),
            Cmd::Unslot { bay, slot } => b.game.unslot(bay, slot),
            Cmd::SupplyDone => b.game.supply_done(),
            Cmd::StartShift => match b.game.shift_sim() {
                Ok(sim) => {
                    b.shift = Some(Playback { sim, acc: 0.0 });
                    Ok(())
                }
                Err(e) => Err(e),
            },
            Cmd::ShopBuy(i) => b.game.shop_buy(i),
            Cmd::BuyContract(i) => b.game.buy_contract(i),
            Cmd::SellContract(i) => b.game.sell_contract(i),
            Cmd::Reroll => b.game.shop_reroll(),
            Cmd::ShopDone => b.game.shop_done(),
            Cmd::Retry => b.game.retry_round(),
            Cmd::NewRun(seed) => {
                b.game = Game::new(seed);
                b.shift = None;
                Ok(())
            }
        };
        if let Err(e) = res {
            bridge.toast = Some((e, 2.5));
        }
        bridge.dirty = true;
    }
}

/// Pace the live shift: advance sim ticks on the wall clock, and when the
/// last tick lands, commit the identical shift through the core.
pub fn tick_shift(time: Res<Time>, mut bridge: ResMut<Bridge>) {
    let total = bridge.game.shift_ticks();
    let speed = bridge.speed;
    let Some(pb) = bridge.shift.as_mut() else { return };
    pb.acc += time.delta_secs() * speed;
    let mut stepped = false;
    while pb.acc >= 1.0 && pb.sim.tick < total {
        pb.acc -= 1.0;
        pb.sim.step();
        stepped = true;
    }
    let done = pb.sim.tick >= total;
    if stepped {
        bridge.dirty = true;
    }
    if done {
        bridge.shift = None;
        if let Err(e) = bridge.game.run_shift() {
            bridge.toast = Some((e, 2.5));
        }
        bridge.dirty = true;
    }
}

/// Fade the toast.
pub fn tick_toast(time: Res<Time>, mut bridge: ResMut<Bridge>) {
    if let Some((_, t)) = bridge.toast.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            bridge.toast = None;
            bridge.dirty = true;
        }
    }
}

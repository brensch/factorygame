//! Pointer and keyboard → [`Cmd`]s. Clicks resolve against the hit list the
//! scene registered while painting; board gestures (tool painting, card
//! drops, belt drags) resolve geometrically against the layout. Works the
//! same for mouse and touch — a touch is a press, a drag, a release.

use bevy::prelude::*;

use crate::bridge::{Bridge, Cmd};
use crate::layout::{cursor_virtual, Layout};
use crate::scene::{Act, Hits, Tool, UiState};
use overflow_core::defs::Dir;
use overflow_core::run::GamePhase;

/// Where the current press started, to tell taps from drags.
#[derive(Resource, Default)]
pub struct Press {
    pub start: Option<Vec2>,
    pub started_on: Option<Act>,
    /// Last tile a belt was painted onto during this drag.
    pub last_belt: Option<(i32, i32)>,
}

pub fn pointer(
    windows: Query<&Window>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut bridge: ResMut<Bridge>,
    layout: Res<Layout>,
    hits: Res<Hits>,
    mut ui: ResMut<UiState>,
    mut press: ResMut<Press>,
) {
    let Ok(win) = windows.single() else { return };
    let Some(v) = cursor_virtual(win, &layout) else { return };
    if ui.pointer != v {
        ui.pointer = v;
        let hover = layout.tile_at(v);
        if ui.hover != hover {
            ui.hover = hover;
        }
    }

    // ── press ──
    if buttons.just_pressed(MouseButton::Left) {
        press.start = Some(v);
        press.started_on = hits.at(v);
        press.last_belt = None;
        match press.started_on {
            Some(Act::HandCard(i)) => ui.drag_card = Some(i),
            Some(Act::SupplyCard(i)) => {
                ui.sel_supply = if ui.sel_supply == Some(i) { None } else { Some(i) };
            }
            _ => {}
        }
    }

    // ── drag ──
    if buttons.pressed(MouseButton::Left) {
        if let (Some(Tool::Belt), Some(start)) = (ui.tool, press.start) {
            // belt painting: each new tile entered lays a belt pointing
            // from the previous tile toward this one
            if press.started_on.is_none() || press.last_belt.is_some() {
                if let (Some((px, py)), Some((cx, cy))) =
                    (layout.tile_at(start).or(press.last_belt), layout.tile_at(v))
                {
                    let from = press.last_belt.unwrap_or((px, py));
                    if from != (cx, cy) {
                        let d = dir_between(from, (cx, cy));
                        if let Some(d) = d {
                            bridge.push(Cmd::Belt { x: from.0, y: from.1, d });
                            press.last_belt = Some((cx, cy));
                        }
                    }
                }
            }
        }
    }

    // ── release ──
    if buttons.just_released(MouseButton::Left) {
        let start = press.start.take();
        let started_on = press.started_on.take();
        let painted = press.last_belt.take();

        // card drop onto the board
        if let Some(hand_idx) = ui.drag_card.take() {
            if let Some((tx, ty)) = layout.tile_at(v) {
                let rot = ui.rot;
                bridge.push(Cmd::PlayCard { hand_idx, x: tx, y: ty, d: rot });
            }
            return;
        }

        let moved = start.map(|s| s.distance(v) > 3.0).unwrap_or(false);
        if painted.is_some() {
            return; // the drag already placed belts
        }
        if moved {
            return;
        }
        // a tap: resolve against the hit list first, then the board
        if let Some(act) = hits.at(v).or(started_on) {
            apply_act(act, &mut bridge, &mut ui);
            return;
        }
        if let Some((tx, ty)) = layout.tile_at(v) {
            match ui.tool {
                Some(Tool::Belt) => bridge.push(Cmd::Belt { x: tx, y: ty, d: ui.rot }),
                Some(Tool::Chute) => bridge.push(Cmd::Chute { x: tx, y: ty }),
                Some(Tool::Junction) => bridge.push(Cmd::Junction { x: tx, y: ty }),
                Some(Tool::Splitter) => bridge.push(Cmd::Splitter { x: tx, y: ty, d: ui.rot }),
                Some(Tool::Merger) => bridge.push(Cmd::Merger { x: tx, y: ty, d: ui.rot }),
                Some(Tool::Erase) => bridge.push(Cmd::Sell { x: tx, y: ty }),
                None => {
                    // bare tap on a machine rotates it — the lightest
                    // touch-friendly gesture for adjusting a build
                    if bridge.game.phase == GamePhase::Build {
                        bridge.push(Cmd::Rotate { x: tx, y: ty });
                    }
                }
            }
        }
    }

    // ── right click: sell on board, sell contract in tray ──
    if buttons.just_pressed(MouseButton::Right) {
        if let Some(Act::TrayContract(i)) = hits.at(v) {
            bridge.push(Cmd::SellContract(i));
            return;
        }
        if let Some((tx, ty)) = layout.tile_at(v) {
            bridge.push(Cmd::Sell { x: tx, y: ty });
        }
    }
}

fn dir_between(a: (i32, i32), b: (i32, i32)) -> Option<Dir> {
    match (b.0 - a.0, b.1 - a.1) {
        (1, 0) => Some(Dir::E),
        (-1, 0) => Some(Dir::W),
        (0, 1) => Some(Dir::S),
        (0, -1) => Some(Dir::N),
        _ => None,
    }
}

fn apply_act(act: Act, bridge: &mut Bridge, ui: &mut UiState) {
    match act {
        Act::Tool(t) => {
            ui.tool = if ui.tool == Some(t) { None } else { Some(t) };
            ui.sel_supply = None;
        }
        Act::HandCard(_) | Act::SupplyCard(_) => {} // handled on press
        Act::Bay(bi) => {
            if let Some(si) = ui.sel_supply.take() {
                bridge.push(Cmd::Allocate { supply_idx: si, bay: bi });
            } else if let Some(slots) = bridge.game.bay_slots.get(bi) {
                // nothing armed: pull the newest crate back off the rack
                if !slots.is_empty() {
                    bridge.push(Cmd::Unslot { bay: bi, slot: slots.len() - 1 });
                }
            }
        }
        Act::TrayContract(_) => {} // left click is a no-op; right click sells
        Act::Run => bridge.push(Cmd::StartShift),
        Act::Speed => {
            bridge.speed = match bridge.speed as u32 {
                0..=5 => 12.0,
                6..=15 => 30.0,
                _ => 4.0,
            };
            bridge.dirty = true;
        }
        Act::Retry => bridge.push(Cmd::Retry),
        Act::NewRun => {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(42);
            bridge.push(Cmd::NewRun(seed));
        }
        Act::LotBuy(i) => bridge.push(Cmd::BuyLot(i)),
        Act::SupplyDone => bridge.push(Cmd::SupplyDone),
        Act::ShopOffer(i) => bridge.push(Cmd::ShopBuy(i)),
        Act::ShopContract(i) => bridge.push(Cmd::BuyContract(i)),
        Act::Reroll => bridge.push(Cmd::Reroll),
        Act::ShopDone => bridge.push(Cmd::ShopDone),
    }
}

pub fn keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut ui: ResMut<UiState>,
    mut bridge: ResMut<Bridge>,
) {
    if keyboard.just_pressed(KeyCode::KeyR) {
        // rotate the pending placement, or the machine under the cursor
        if ui.drag_card.is_some() || ui.tool.is_some() {
            ui.rot = ui.rot.turn_cw();
        } else if let Some((x, y)) = ui.hover {
            bridge.push(Cmd::Rotate { x, y });
        }
        bridge.dirty = true;
    }
    if keyboard.just_pressed(KeyCode::Escape) {
        ui.tool = None;
        ui.drag_card = None;
        ui.sel_supply = None;
        bridge.dirty = true;
    }
    if keyboard.just_pressed(KeyCode::Space) && bridge.game.phase == GamePhase::Build {
        bridge.push(Cmd::StartShift);
    }
}

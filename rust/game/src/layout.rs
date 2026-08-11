//! Responsive virtual-pixel layout. The screen is a grid of "virtual
//! pixels": an integer zoom is chosen from the window so pixel art stays
//! crisp, then every region — board, HUD bar, contracts tray, card fan —
//! is placed in virtual coordinates. Portrait (mobile) and landscape (PC)
//! are the same scene with different furniture positions.

use bevy::prelude::*;

pub const TILE: f32 = 16.0;
pub const BOARD_W: i32 = 18;
pub const BOARD_H: i32 = 18;

#[derive(Resource, Default, Clone, PartialEq)]
pub struct Layout {
    pub zoom: f32,
    /// Virtual screen size (window / zoom).
    pub vw: f32,
    pub vh: f32,
    pub portrait: bool,
    /// Board origin in virtual pixels.
    pub board: Vec2,
    /// Top HUD bar height.
    pub hud_h: f32,
    /// Contracts tray origin.
    pub tray: Vec2,
    /// Card fan baseline (cards are drawn up from here).
    pub fan_y: f32,
    /// Run/speed buttons anchor (right edge).
    pub run: Vec2,
    /// Tool rail origin (landscape only; portrait tools sit above the fan).
    pub tools: Vec2,
}

impl Layout {
    fn compute(win_w: f32, win_h: f32) -> Layout {
        let portrait = win_h > win_w;
        let board_px = BOARD_W as f32 * TILE; // 288
        // Minimum virtual canvas the furniture needs around the board.
        let (min_w, min_h) = if portrait {
            (board_px + 4.0, board_px + 30.0 + 18.0 + 78.0)
        } else {
            (board_px + 96.0 + 80.0, board_px + 20.0 + 50.0)
        };
        let zoom = (win_w / min_w).min(win_h / min_h).floor().max(1.0);
        let vw = (win_w / zoom).ceil();
        let vh = (win_h / zoom).ceil();
        if portrait {
            let hud_h = 30.0;
            let tray = Vec2::new(4.0, hud_h + 2.0);
            let fan_y = vh - 46.0;
            let tools_y = fan_y - 20.0;
            // centre the board between the tray and the tool strip
            let top = hud_h + 20.0;
            let board_y = top + ((tools_y - 4.0 - top - board_px).max(0.0)) / 2.0;
            let board = Vec2::new((vw - board_px) / 2.0, board_y);
            Layout {
                zoom, vw, vh, portrait,
                board, hud_h, tray, fan_y,
                run: Vec2::new(vw - 66.0, tools_y),
                tools: Vec2::new(4.0, tools_y),
            }
        } else {
            let hud_h = 20.0;
            let tray = Vec2::new(4.0, hud_h + 2.0);
            let tools_w = 92.0;
            let board = Vec2::new(
                tools_w + (vw - tools_w - board_px) / 2.0,
                hud_h + 22.0 + (vh - hud_h - 22.0 - board_px - 8.0).max(0.0) / 2.0,
            );
            Layout {
                zoom, vw, vh, portrait,
                board, hud_h, tray,
                fan_y: vh - 44.0,
                run: Vec2::new(vw - 76.0, vh - 26.0),
                tools: Vec2::new(0.0, hud_h + 26.0),
            }
        }
    }

    /// Virtual position of a board tile's top-left corner.
    pub fn tile_pos(&self, x: i32, y: i32) -> Vec2 {
        self.board + Vec2::new(x as f32 * TILE, y as f32 * TILE)
    }

    /// Board tile under a virtual point, if any.
    pub fn tile_at(&self, v: Vec2) -> Option<(i32, i32)> {
        let rel = (v - self.board) / TILE;
        let (x, y) = (rel.x.floor() as i32, rel.y.floor() as i32);
        (x >= 0 && y >= 0 && x < BOARD_W && y < BOARD_H).then_some((x, y))
    }
}

/// Filter matching either scene root (UI or items).
pub type AnyRootFilter =
    Or<(With<crate::scene::UiRoot>, With<crate::scene::ItemRoot>)>;

/// Recompute when the window changes; flips the scene dirty via change
/// detection (`Res<Layout>::is_changed`).
pub fn update_layout(
    windows: Query<&Window>,
    mut layout: ResMut<Layout>,
    mut roots: Query<&mut Transform, AnyRootFilter>,
) {
    let Ok(win) = windows.single() else { return };
    let next = Layout::compute(win.width(), win.height());
    if *layout != next {
        // Roots map virtual pixels (y-down, origin top-left) onto the
        // camera's world space (y-up, origin centre).
        for mut tf in roots.iter_mut() {
            tf.translation = Vec3::new(-win.width() / 2.0, win.height() / 2.0, 0.0);
            tf.scale = Vec3::new(next.zoom, next.zoom, 1.0);
        }
        *layout = next;
    }
}

/// Window-space cursor → virtual pixels.
pub fn cursor_virtual(win: &Window, layout: &Layout) -> Option<Vec2> {
    win.cursor_position().map(|p| p / layout.zoom)
}

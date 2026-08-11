//! Paints the whole screen from state, immediate-mode style: whenever the
//! bridge is dirty the previous scene is despawned and repainted, and every
//! clickable region is registered in [`Hits`] as it is drawn — so what you
//! see and what you can click can never drift apart.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::atlas::{item_key, machine_key, Sprites, GLYPH_W};
use crate::bridge::Bridge;
use crate::layout::{Layout, BOARD_H, BOARD_W, TILE};
use crate::theme::{self, col};
use overflow_core::defs::{
    self, contract, def, directive, item_value, Dir, ItemType, MachineId,
};
use overflow_core::run::{Game, GamePhase, SHIFTS_PER_ROUND};

#[derive(Component)]
pub struct UiRoot;

#[derive(Component)]
pub struct ItemRoot;

/// A moving item: lerped between tile centres by `animate_items`.
#[derive(Component)]
pub struct ItemLerp {
    pub from: Vec2,
    pub to: Vec2,
}

/// Belt tread animation: which phase-set rects to cycle.
#[derive(Component)]
pub struct BeltAnim {
    pub horizontal: bool,
}

#[derive(Resource, Default)]
pub struct BeltPhase {
    pub timer: f32,
    pub phase: usize,
}

/// What a click means, registered while painting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Tool(Tool),
    HandCard(usize),
    SupplyCard(usize),
    Bay(usize),
    TrayContract(usize),
    Run,
    Speed,
    Retry,
    NewRun,
    LotBuy(usize),
    SupplyDone,
    ShopOffer(usize),
    ShopContract(usize),
    Reroll,
    ShopDone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Belt,
    Chute,
    Junction,
    Splitter,
    Merger,
    Erase,
}

#[derive(Resource, Default)]
pub struct Hits(pub Vec<(Rect, Act)>);

impl Hits {
    pub fn at(&self, p: Vec2) -> Option<Act> {
        self.0.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, a)| *a)
    }
}

/// Transient UI state the painter needs but the core must not know about.
#[derive(Resource)]
pub struct UiState {
    pub tool: Option<Tool>,
    /// Rotation applied to the next placement.
    pub rot: Dir,
    /// Hand card being dragged, with the current pointer tile.
    pub drag_card: Option<usize>,
    /// Supply card selected (click) or dragged, waiting for a bay.
    pub sel_supply: Option<usize>,
    pub hover: Option<(i32, i32)>,
    pub pointer: Vec2,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            tool: None,
            rot: Dir::E,
            drag_card: None,
            sel_supply: None,
            hover: None,
            pointer: Vec2::ZERO,
        }
    }
}

// ── painting primitives ──────────────────────────────────────────────────

pub struct Painter<'w, 's, 'a> {
    pub cmds: &'a mut Commands<'w, 's>,
    pub sprites: &'a Sprites,
    pub root: Entity,
}

impl Painter<'_, '_, '_> {
    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, -y, z)
    }
    /// Blit an atlas sprite at virtual (x, y), top-left anchored.
    pub fn blit(&mut self, name: &str, x: f32, y: f32, z: f32) -> Entity {
        self.blit_tinted(name, x, y, z, Color::WHITE)
    }
    pub fn blit_tinted(&mut self, name: &str, x: f32, y: f32, z: f32, tint: Color) -> Entity {
        let rect = self.sprites.rect(name);
        let e = self
            .cmds
            .spawn((
                Sprite {
                    image: self.sprites.image.clone(),
                    rect: Some(rect),
                    color: tint,
                    ..default()
                },
                Anchor::TOP_LEFT,
                Transform::from_translation(Self::v(x, y, z)),
            ))
            .id();
        self.cmds.entity(self.root).add_child(e);
        e
    }
    /// Flat colored rectangle (the tinted white pixel).
    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, z: f32, c: theme::Rgba) -> Entity {
        let rect = self.sprites.rect("white");
        let e = self
            .cmds
            .spawn((
                Sprite {
                    image: self.sprites.image.clone(),
                    rect: Some(rect),
                    color: col(c),
                    custom_size: Some(Vec2::new(w, h)),
                    ..default()
                },
                Anchor::TOP_LEFT,
                Transform::from_translation(Self::v(x, y, z)),
            ))
            .id();
        self.cmds.entity(self.root).add_child(e);
        e
    }
    /// Bevelled UI panel.
    pub fn panel(&mut self, x: f32, y: f32, w: f32, h: f32, z: f32) {
        self.fill(x, y, w, h, z, theme::PANEL);
        self.fill(x, y, w, 1.0, z + 0.1, theme::PANEL_HI);
        self.fill(x, y, 1.0, h, z + 0.1, theme::PANEL_HI);
        self.fill(x, y + h - 1.0, w, 1.0, z + 0.1, theme::PANEL_LO);
        self.fill(x + w - 1.0, y, 1.0, h, z + 0.1, theme::PANEL_LO);
        self.fill(x + 1.0, y + 1.0, w - 2.0, 1.0, z + 0.1, theme::PANEL_EDGE);
    }
    /// Pixel-font text. Returns the x after the last glyph.
    pub fn text(&mut self, s: &str, x: f32, y: f32, z: f32, c: theme::Rgba, scale: f32) -> f32 {
        let mut cx = x;
        for ch in s.to_uppercase().chars() {
            if ch != ' ' {
                let name = format!("g_{ch}");
                if self.sprites.rects_contains(&name) {
                    let rect = self.sprites.rect(&name);
                    let e = self
                        .cmds
                        .spawn((
                            Sprite {
                                image: self.sprites.image.clone(),
                                rect: Some(rect),
                                color: col(c),
                                custom_size: Some(Vec2::new(3.0 * scale, 5.0 * scale)),
                                ..default()
                            },
                            Anchor::TOP_LEFT,
                            Transform::from_translation(Self::v(cx, y, z)),
                        ))
                        .id();
                    self.cmds.entity(self.root).add_child(e);
                }
            }
            cx += GLYPH_W * scale;
        }
        cx
    }
    pub fn text_w(s: &str, scale: f32) -> f32 {
        s.chars().count() as f32 * GLYPH_W * scale
    }
    /// A pressable green action button.
    pub fn button(&mut self, label: &str, x: f32, y: f32, w: f32, z: f32) {
        self.fill(x, y, w, 14.0, z, theme::GREEN_LO);
        self.fill(x, y, w, 1.0, z + 0.1, theme::GREEN_HI);
        self.fill(x, y + 13.0, w, 1.0, z + 0.1, [0x1b, 0x3c, 0x22, 0xff]);
        self.text(&format!(">{label}"), x + 5.0, y + 4.0, z + 0.2, [0xd9, 0xf2, 0xdd, 0xff], 1.0);
    }
}

impl Sprites {
    fn rects_contains(&self, name: &str) -> bool {
        self.try_rect(name).is_some()
    }
}

// ── the scene ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn rebuild(
    mut cmds: Commands,
    sprites: Option<Res<Sprites>>,
    mut bridge: ResMut<Bridge>,
    layout: Res<Layout>,
    ui: Res<UiState>,
    mut hits: ResMut<Hits>,
    roots: Query<Entity, With<UiRoot>>,
    item_roots: Query<Entity, With<ItemRoot>>,
    children: Query<&Children>,
) {
    let Some(sprites) = sprites else { return };
    if !bridge.dirty && !layout.is_changed() && !ui.is_changed() {
        return;
    }
    bridge.dirty = false;
    let Ok(root) = roots.single() else { return };
    let Ok(item_root) = item_roots.single() else { return };
    for r in [root, item_root] {
        if let Ok(kids) = children.get(r) {
            for c in kids.iter() {
                cmds.entity(c).despawn();
            }
        }
    }
    hits.0.clear();

    let g = &bridge.game;
    let l = &*layout;
    let mut p = Painter { cmds: &mut cmds, sprites: &sprites, root };

    draw_board(&mut p, g, l, &ui);
    draw_hud(&mut p, g, l, &bridge, &mut hits);
    draw_tray(&mut p, g, l, &mut hits);
    draw_tools(&mut p, l, &ui, &mut hits);
    draw_fan(&mut p, g, l, &ui, &mut hits);
    draw_run_controls(&mut p, g, l, &bridge, &mut hits);
    match g.phase {
        GamePhase::Supply => draw_supply(&mut p, g, l, &ui, &mut hits),
        GamePhase::Shop => draw_shop(&mut p, g, l, &mut hits),
        GamePhase::Over { .. } => draw_over(&mut p, g, l, &mut hits),
        GamePhase::Build => {}
    }
    if let Some((msg, _)) = &bridge.toast {
        let w = Painter::text_w(msg, 1.0) + 12.0;
        let x = (l.vw - w) / 2.0;
        p.panel(x, l.vh - 64.0, w, 13.0, 200.0);
        p.text(msg, x + 6.0, l.vh - 60.0, 200.2, theme::RED, 1.0);
    }
    draw_ghost(&mut p, g, l, &ui);

    // items live under their own root so the lerp system can move them
    let mut ip = Painter { cmds: &mut cmds, sprites: &sprites, root: item_root };
    draw_items(&mut ip, &bridge, l);
}

fn tile_center(l: &Layout, x: i32, y: i32) -> Vec2 {
    l.tile_pos(x, y) + Vec2::splat(TILE / 2.0)
}

fn draw_board(p: &mut Painter, g: &Game, l: &Layout, ui: &UiState) {
    for y in 0..BOARD_H {
        for x in 0..BOARD_W {
            let pos = l.tile_pos(x, y);
            let name = if (x + y) % 2 == 0 { "ground0" } else { "ground1" };
            p.blit(name, pos.x, pos.y, 0.0);
        }
    }
    // board frame
    let o = l.board;
    let side = BOARD_W as f32 * TILE;
    p.fill(o.x - 1.0, o.y - 1.0, side + 2.0, 1.0, 0.5, theme::PANEL_HI);
    p.fill(o.x - 1.0, o.y + side, side + 2.0, 1.0, 0.5, theme::PANEL_LO);
    p.fill(o.x - 1.0, o.y - 1.0, 1.0, side + 2.0, 0.5, theme::PANEL_HI);
    p.fill(o.x + side, o.y - 1.0, 1.0, side + 2.0, 0.5, theme::PANEL_LO);

    let bays = g.bays();
    for pl in &g.board {
        draw_placement(p, g, l, pl, &bays, 1.0, Color::WHITE);
    }
    // hover cursor while a tool is armed
    if let (Some(_), Some((hx, hy))) = (ui.tool, ui.hover) {
        let pos = l.tile_pos(hx, hy);
        p.fill(pos.x, pos.y, TILE, 1.0, 15.0, theme::COPPER_HI);
        p.fill(pos.x, pos.y + TILE - 1.0, TILE, 1.0, 15.0, theme::COPPER_HI);
        p.fill(pos.x, pos.y, 1.0, TILE, 15.0, theme::COPPER_HI);
        p.fill(pos.x + TILE - 1.0, pos.y, 1.0, TILE, 15.0, theme::COPPER_HI);
    }
}

fn draw_placement(
    p: &mut Painter,
    g: &Game,
    l: &Layout,
    pl: &overflow_core::sim::Placement,
    bays: &[(i32, i32)],
    alpha: f32,
    tint_override: Color,
) {
    let tint = if alpha < 1.0 {
        tint_override.with_alpha(alpha)
    } else {
        Color::WHITE
    };
    let cells = Game::cells_of(pl);
    match pl.m {
        MachineId::Belt => {
            let d = pl.d.unwrap_or(Dir::E);
            let pos = l.tile_pos(pl.x, pl.y);
            let horiz = matches!(d, Dir::E | Dir::W);
            let name = if horiz { "belt_h0" } else { "belt_v0" };
            let e = p.blit_tinted(name, pos.x, pos.y, 2.0, tint);
            p.cmds.entity(e).insert(BeltAnim { horizontal: horiz });
            let nib = match d {
                Dir::E => "nib_e",
                Dir::W => "nib_w",
                Dir::S => "nib_s",
                Dir::N => "nib_n",
            };
            let (nx, ny) = match d {
                Dir::E => (11.0, 5.0),
                Dir::W => (1.0, 5.0),
                Dir::S => (5.0, 11.0),
                Dir::N => (5.0, 1.0),
            };
            p.blit_tinted(nib, pos.x + nx, pos.y + ny, 3.0, tint);
        }
        MachineId::Bay => {
            let pos = l.tile_pos(pl.x, pl.y);
            p.blit_tinted("bay", pos.x, pos.y, 2.0, tint);
            // slot LEDs from the matching bay rack
            if let Some(bi) = bays.iter().position(|&(bx, by)| (bx, by) == (pl.x, pl.y)) {
                let slots = &g.bay_slots[bi];
                for k in 0..overflow_core::run::BAY_SLOTS {
                    let (x0, y0) = (pos.x + 3.0 + k as f32 * 4.0, pos.y + 6.0);
                    match slots.get(k) {
                        Some(lot) => {
                            let frac = lot.remaining() as f32 / lot.size.max(1) as f32;
                            p.fill(x0, y0, 3.0, 4.0, 3.0, [0x2a, 0x25, 0x1c, 0xff]);
                            let h = (4.0 * frac).ceil().max(1.0);
                            p.fill(x0, y0 + (4.0 - h), 3.0, h, 3.1, theme::COPPER_HI);
                        }
                        None => {
                            p.fill(x0, y0, 3.0, 4.0, 3.0, [0x2a, 0x25, 0x1c, 0xff]);
                        }
                    }
                }
            }
        }
        MachineId::Vault => {
            let pos = l.tile_pos(pl.x, pl.y);
            p.blit_tinted("vault_tile", pos.x, pos.y, 2.0, tint);
        }
        MachineId::Chute => {
            let pos = l.tile_pos(pl.x, pl.y);
            p.blit_tinted("chute_tile", pos.x, pos.y, 2.0, tint);
        }
        m => {
            let kind = def(m).kind;
            let plate = format!("plate_{}", crate::atlas::kind_key(kind));
            for &(cx, cy) in &cells {
                let pos = l.tile_pos(cx, cy);
                p.blit_tinted(&plate, pos.x, pos.y, 2.0, tint);
            }
            // emblem at the bounding-box centre
            let minx = cells.iter().map(|c| c.0).min().unwrap_or(pl.x);
            let maxx = cells.iter().map(|c| c.0).max().unwrap_or(pl.x);
            let miny = cells.iter().map(|c| c.1).min().unwrap_or(pl.y);
            let maxy = cells.iter().map(|c| c.1).max().unwrap_or(pl.y);
            let c = Vec2::new(
                (l.tile_pos(minx, miny).x + l.tile_pos(maxx, maxy).x + TILE) / 2.0,
                (l.tile_pos(minx, miny).y + l.tile_pos(maxx, maxy).y + TILE) / 2.0,
            );
            p.blit_tinted(&format!("em_{}", machine_key(m)), c.x - 8.0, c.y - 8.0, 3.0, tint);
            // shaped ports
            let (ins, out) = defs::shape_ports(m, pl.x, pl.y, pl.d.unwrap_or(Dir::E));
            for (ix, iy, edge) in ins {
                let pos = l.tile_pos(ix, iy);
                p.blit_tinted(&format!("port_in_{}", dir_key(edge)), pos.x, pos.y, 4.0, tint);
            }
            if let Some((ox, oy, edge)) = out {
                let pos = l.tile_pos(ox, oy);
                p.blit_tinted(&format!("port_out_{}", dir_key(edge)), pos.x, pos.y, 4.0, tint);
            } else if let Some(d) = pl.d {
                // 1×1 directed machines: mark the output edge
                let pos = l.tile_pos(pl.x, pl.y);
                p.blit_tinted(&format!("port_out_{}", dir_key(d)), pos.x, pos.y, 4.0, tint);
            }
            // furnace flourish: chimney on the top-right cell
            if m == MachineId::Furnace {
                let pos = l.tile_pos(maxx, miny);
                p.blit_tinted("chimney", pos.x + 6.0, pos.y - 6.0, 5.0, tint);
            }
        }
    }
}

fn dir_key(d: Dir) -> &'static str {
    match d {
        Dir::N => "n",
        Dir::E => "e",
        Dir::S => "s",
        Dir::W => "w",
    }
}

fn draw_items(p: &mut Painter, bridge: &Bridge, l: &Layout) {
    if let Some(pb) = &bridge.shift {
        let sim = &pb.sim;
        for y in 0..sim.h {
            for x in 0..sim.w {
                let Some(item) = sim.peek(x, y) else { continue };
                let to = tile_center(l, x, y);
                let from = sim
                    .moves
                    .iter()
                    .find(|m| m.id == item.id)
                    .map(|m| {
                        let (fx, fy) = (m.from as i32 % sim.w, m.from as i32 / sim.w);
                        tile_center(l, fx, fy)
                    })
                    .unwrap_or(to);
                let e = p.blit(
                    &format!("it_{}", item_key(item.ty)),
                    from.x - 3.0,
                    from.y - 3.0,
                    20.0,
                );
                p.cmds.entity(e).insert(ItemLerp {
                    from: from - Vec2::splat(3.0),
                    to: to - Vec2::splat(3.0),
                });
            }
        }
    } else {
        // the warm factory between shifts
        for seed in &bridge.game.carry {
            if seed.buffered {
                continue;
            }
            let c = tile_center(l, seed.x, seed.y);
            p.blit(&format!("it_{}", item_key(seed.ty)), c.x - 3.0, c.y - 3.0, 20.0);
        }
    }
}

/// Lerp item sprites across the current tick interval.
pub fn animate_items(bridge: Res<Bridge>, mut q: Query<(&ItemLerp, &mut Transform)>) {
    let t = bridge.shift.as_ref().map(|pb| pb.acc.clamp(0.0, 1.0)).unwrap_or(1.0);
    for (lerp, mut tf) in q.iter_mut() {
        let pos = lerp.from.lerp(lerp.to, t);
        tf.translation.x = pos.x;
        tf.translation.y = -pos.y;
    }
}

/// March the belt treads.
pub fn animate_belts(
    time: Res<Time>,
    sprites: Option<Res<Sprites>>,
    mut phase: ResMut<BeltPhase>,
    mut q: Query<(&BeltAnim, &mut Sprite)>,
) {
    let Some(sprites) = sprites else { return };
    phase.timer += time.delta_secs();
    if phase.timer < 0.12 {
        return;
    }
    phase.timer = 0.0;
    phase.phase = (phase.phase + 1) % 3;
    for (anim, mut spr) in q.iter_mut() {
        let name = if anim.horizontal {
            format!("belt_h{}", phase.phase)
        } else {
            format!("belt_v{}", phase.phase)
        };
        spr.rect = Some(sprites.rect(&name));
    }
}

fn draw_hud(p: &mut Painter, g: &Game, l: &Layout, bridge: &Bridge, _hits: &mut Hits) {
    p.panel(0.0, 0.0, l.vw, l.hud_h, 50.0);
    let quota = g.quota();
    let delivered = g.round_delivered
        + bridge
            .shift
            .as_ref()
            .map(|pb| pb.sim.result(pb.sim.tick).payout)
            .unwrap_or(0);
    let shift_names = ["MORNING", "AFTERNOON", "NIGHT"];
    let shift = shift_names[(g.shifts_used as usize).min(2)];
    if l.portrait {
        p.text(&format!("DAY {}/12", g.round + 1), 4.0, 4.0, 51.0, theme::INK, 1.0);
        p.text(&format!("$ {}", g.credits), 4.0, 12.0, 51.0, theme::INK_GOLD, 1.0);
        p.text(shift, 44.0, 4.0, 51.0, theme::INK, 1.0);
        draw_pips(p, 44.0 + Painter::text_w(shift, 1.0) + 4.0, 4.0, g.shifts_used);
        let mk = format!("{} X2", item_key(g.market).to_uppercase());
        p.text(&mk, l.vw - Painter::text_w(&mk, 1.0) - 4.0, 4.0, 51.0, theme::PURPLE, 1.0);
        draw_meter(p, 42.0, 20.0, l.vw - 100.0, delivered, quota);
        p.text(
            &format!("{delivered}/{quota}"),
            l.vw - 54.0,
            21.0,
            51.5,
            theme::INK,
            1.0,
        );
    } else {
        p.text(&format!("DAY {}/12", g.round + 1), 4.0, 4.0, 51.0, theme::INK, 1.0);
        p.text(&format!("$ {}", g.credits), 4.0, 12.0, 51.0, theme::INK_GOLD, 1.0);
        draw_meter(p, 48.0, 5.0, 150.0, delivered, quota);
        p.text(&format!("{delivered}/{quota}"), 202.0, 7.0, 51.0, theme::INK, 1.0);
        let sx = 260.0;
        p.text(shift, sx, 7.0, 51.0, theme::INK, 1.0);
        draw_pips(p, sx + Painter::text_w(shift, 1.0) + 4.0, 7.0, g.shifts_used);
        let mk = format!("MKT {} X2", item_key(g.market).to_uppercase());
        p.text(&mk, l.vw - Painter::text_w(&mk, 1.0) - 4.0, 7.0, 51.0, theme::PURPLE, 1.0);
        if let Some(tag) = g.audit_tag {
            let at = format!("AUDIT {tag:?}");
            p.text(&at, l.vw - 120.0, 7.0, 51.0, theme::RED, 1.0);
        }
    }
}

fn draw_meter(p: &mut Painter, x: f32, y: f32, w: f32, val: i64, quota: i64) {
    p.panel(x, y, w, 9.0, 51.0);
    let segs = ((w - 4.0) / 3.0) as i32;
    let lit = ((segs as f32) * (val as f32 / quota.max(1) as f32).min(1.0)).round() as i32;
    for k in 0..segs {
        let c = if k < lit {
            if val >= quota { theme::GREEN } else { theme::COPPER_HI }
        } else {
            [0x24, 0x1f, 0x16, 0xff]
        };
        p.fill(x + 2.0 + k as f32 * 3.0, y + 2.0, 2.0, 5.0, 51.2, c);
    }
}

fn draw_pips(p: &mut Painter, x: f32, y: f32, used: u32) {
    for k in 0..SHIFTS_PER_ROUND {
        let name = if k < used { "pip_off" } else { "pip_on" };
        p.blit(name, x + k as f32 * 6.0, y, 51.0);
    }
}

fn draw_tray(p: &mut Painter, g: &Game, l: &Layout, hits: &mut Hits) {
    let mut x = l.tray.x;
    let y = l.tray.y;
    for (i, ci) in g.contracts.iter().enumerate() {
        let cdef = contract(ci.id);
        let name_w = Painter::text_w(cdef.name, 1.0).max(52.0);
        let w = name_w + 12.0;
        if x + w > l.vw - 2.0 {
            break;
        }
        p.fill(x, y, w, 16.0, 52.0, theme::TICKET_BG);
        p.fill(x, y, w, 1.0, 52.1, theme::TICKET_EDGE);
        for k in 0..(w as i32) / 3 {
            p.fill(x + k as f32 * 3.0, y + 15.0, 1.0, 1.0, 52.1, theme::TICKET_EDGE);
        }
        p.text("*", x + 2.0, y + 2.0, 52.2, theme::INK_GOLD, 1.0);
        p.text(cdef.name, x + 7.0, y + 2.0, 52.2, theme::INK_GOLD, 1.0);
        if let overflow_core::defs::ContractKind::Term { deliver, count, reward, .. } = cdef.kind {
            let sub = format!(
                "{}/{} {} {}D ${}",
                ci.progress,
                count,
                item_key(deliver).to_uppercase(),
                ci.rounds_left.unwrap_or(0),
                reward
            );
            p.text(&sub, x + 2.0, y + 8.0, 52.2, theme::INK_DIM, 1.0);
            p.fill(x + 2.0, y + 13.0, w - 4.0, 1.0, 52.2, [0x24, 0x1f, 0x16, 0xff]);
            let frac = (ci.progress as f32 / count.max(1) as f32).min(1.0);
            p.fill(x + 2.0, y + 13.0, (w - 4.0) * frac, 1.0, 52.3, theme::INK_GOLD);
        } else {
            // ongoing: first words of the blurb won't fit; show the kind
            p.text("ONGOING", x + 2.0, y + 8.0, 52.2, theme::INK_DIM, 1.0);
        }
        hits.0.push((Rect::new(x, y, x + w, y + 16.0), Act::TrayContract(i)));
        x += w + 4.0;
    }
}

fn draw_tools(p: &mut Painter, l: &Layout, ui: &UiState, hits: &mut Hits) {
    let tools: [(Tool, &str, &str); 6] = [
        (Tool::Belt, "BELT", "$1"),
        (Tool::Chute, "CHUT", "$2"),
        (Tool::Junction, "JUNC", "$2"),
        (Tool::Splitter, "SPLT", "$3"),
        (Tool::Merger, "MRGE", "$3"),
        (Tool::Erase, "SELL", ""),
    ];
    if l.portrait {
        // compact strip above the fan, left side
        let y = l.tools.y;
        let mut x = l.tools.x;
        for (tool, name, _) in tools {
            let w = Painter::text_w(name, 1.0) + 6.0;
            let selected = ui.tool == Some(tool);
            p.fill(x, y, w, 12.0, 60.0, if selected { theme::PANEL_HI } else { theme::PANEL });
            p.text(name, x + 3.0, y + 3.0, 60.2, if selected { theme::INK } else { theme::INK_DIM }, 1.0);
            hits.0.push((Rect::new(x, y, x + w, y + 12.0), Act::Tool(tool)));
            x += w + 2.0;
        }
    } else {
        p.panel(l.tools.x, l.tools.y, 66.0, 118.0, 55.0);
        p.text("TOOLS", l.tools.x + 4.0, l.tools.y + 4.0, 55.2, theme::INK_DIM, 1.0);
        for (k, (tool, name, price)) in tools.iter().enumerate() {
            let y = l.tools.y + 14.0 + k as f32 * 17.0;
            let selected = ui.tool == Some(*tool);
            if selected {
                p.fill(l.tools.x + 2.0, y - 2.0, 62.0, 15.0, 55.1, theme::PANEL_HI);
            }
            p.text(name, l.tools.x + 6.0, y + 2.0, 55.3, if selected { theme::INK } else { theme::INK_DIM }, 1.0);
            p.text(price, l.tools.x + 46.0, y + 2.0, 55.3, theme::INK_DIM, 1.0);
            hits.0.push((
                Rect::new(l.tools.x + 2.0, y - 2.0, l.tools.x + 64.0, y + 13.0),
                Act::Tool(*tool),
            ));
        }
    }
}

fn card_face(p: &mut Painter, x: f32, y: f32, z: f32, m: MachineId) {
    let kind = def(m).kind;
    p.fill(x + 6.0, y + 4.0, 18.0, 13.0, z, crate::atlas::plate_colors(kind).0);
    p.fill(x + 6.0, y + 4.0, 18.0, 2.0, z + 0.05, crate::atlas::plate_colors(kind).1);
    p.blit(&format!("em_{}", machine_key(m)), x + 7.0, y + 2.0, z + 0.1);
}

fn draw_fan(p: &mut Painter, g: &Game, l: &Layout, ui: &UiState, hits: &mut Hits) {
    let total = g.hand.len() + g.supply_hand.len();
    if total == 0 {
        return;
    }
    let cw = 34.0;
    let gap = 3.0;
    let span = total as f32 * (cw + gap) - gap;
    let mut x = (l.vw - span) / 2.0;
    let y0 = l.fan_y;
    for (i, card) in g.hand.iter().enumerate() {
        let lift = if ui.drag_card == Some(i) { 6.0 } else { 0.0 };
        let y = y0 - lift;
        p.fill(x, y, cw, 36.0, 70.0, theme::CARD_BG);
        p.fill(x, y, cw, 1.0, 70.1, theme::PANEL_HI);
        p.fill(x, y + 35.0, cw, 1.0, 70.1, theme::PANEL_LO);
        card_face(p, x, y, 70.2, card.machine);
        let d = def(card.machine);
        let label: String = d.name.chars().take(8).collect();
        p.text(&label, x + 2.0, y + 20.0, 70.3, theme::INK, 1.0);
        let cells = defs::shape_cells(card.machine, 0, 0, Dir::E).len();
        let chip = if cells > 1 { format!("{cells} CELL") } else { format!("${}", d.cost) };
        p.text(&chip, x + 2.0, y + 27.0, 70.3, theme::INK_DIM, 1.0);
        hits.0.push((Rect::new(x, y, x + cw, y + 36.0), Act::HandCard(i)));
        x += cw + gap;
    }
    for (i, lot) in g.supply_hand.iter().enumerate() {
        let selected = ui.sel_supply == Some(i);
        let y = y0 - if selected { 6.0 } else { 0.0 };
        p.fill(x, y, cw, 36.0, 70.0, theme::CARD_BG);
        p.fill(x, y, cw, 1.0, 70.1, if selected { theme::COPPER_HI } else { theme::PANEL_HI });
        p.fill(x, y + 35.0, cw, 1.0, 70.1, theme::PANEL_LO);
        p.blit("crate", x + 6.0, y + 4.0, 70.2);
        let label: String = lot.name.chars().take(8).collect();
        p.text(&label, x + 2.0, y + 20.0, 70.3, theme::INK, 1.0);
        p.text(&format!("{}", lot.remaining()), x + 2.0, y + 27.0, 70.3, theme::INK_DIM, 1.0);
        hits.0.push((Rect::new(x, y, x + cw, y + 36.0), Act::SupplyCard(i)));
        x += cw + gap;
    }
    // bays are tap targets whenever a crate is armed (allocate) and all
    // through the supply window (tap with nothing armed to pull one back)
    if ui.sel_supply.is_some() || g.phase == GamePhase::Supply {
        for (bi, &(bx, by)) in g.bays().iter().enumerate() {
            let pos = l.tile_pos(bx, by);
            if ui.sel_supply.is_some() {
                p.fill(pos.x - 1.0, pos.y - 1.0, TILE + 2.0, TILE + 2.0, 14.0, theme::COPPER_HI);
                p.blit("bay", pos.x, pos.y, 14.1);
            }
            hits.0.push((
                Rect::new(pos.x - 4.0, pos.y - 4.0, pos.x + TILE + 4.0, pos.y + TILE + 4.0),
                Act::Bay(bi),
            ));
        }
    }
}

fn draw_run_controls(p: &mut Painter, g: &Game, l: &Layout, bridge: &Bridge, hits: &mut Hits) {
    if g.phase != GamePhase::Build {
        return;
    }
    let (x, y) = (l.run.x, l.run.y);
    if let Some(pb) = &bridge.shift {
        // playback progress
        let total = g.shift_ticks() as f32;
        p.panel(x, y, 62.0, 14.0, 80.0);
        let frac = pb.sim.tick as f32 / total;
        p.fill(x + 2.0, y + 2.0, (62.0 - 4.0) * frac, 10.0, 80.2, theme::GREEN_LO);
        p.text(
            &format!("T{}/{}", pb.sim.tick, total as u32),
            x + 6.0,
            y + 4.0,
            80.3,
            theme::INK,
            1.0,
        );
    } else {
        let shift_names = ["MORNING", "AFTERNOON", "NIGHT"];
        let label = shift_names[(g.shifts_used as usize).min(2)];
        p.button(label, x, y, 62.0, 80.0);
        hits.0.push((Rect::new(x, y, x + 62.0, y + 14.0), Act::Run));
    }
    // speed chip
    let sy = y - 16.0;
    p.panel(x, sy, 62.0, 13.0, 80.0);
    p.text(
        &format!("{}X SPEED", bridge.speed as u32),
        x + 4.0,
        sy + 3.0,
        80.2,
        theme::INK_DIM,
        1.0,
    );
    hits.0.push((Rect::new(x, sy, x + 62.0, sy + 13.0), Act::Speed));
}

fn draw_ghost(p: &mut Painter, g: &Game, l: &Layout, ui: &UiState) {
    let Some(hand_idx) = ui.drag_card else { return };
    let Some(card) = g.hand.get(hand_idx) else { return };
    let Some((tx, ty)) = l.tile_at(ui.pointer) else { return };
    let cells = defs::shape_cells(card.machine, tx, ty, ui.rot);
    let ok = cells.iter().all(|&(cx, cy)| {
        cx >= 0
            && cy >= 0
            && cx < BOARD_W
            && cy < BOARD_H
            && !g.board.iter().any(|pl| Game::cells_of(pl).contains(&(cx, cy)))
    });
    let c = if ok {
        Color::srgba(0.5, 1.0, 0.6, 0.55)
    } else {
        Color::srgba(1.0, 0.4, 0.3, 0.55)
    };
    for &(cx, cy) in &cells {
        let pos = l.tile_pos(cx, cy);
        let e = p.fill(pos.x, pos.y, TILE, TILE, 100.0, [0xff, 0xff, 0xff, 0xff]);
        p.cmds.entity(e).insert(Sprite {
            image: p.sprites.image.clone(),
            rect: Some(p.sprites.rect("white")),
            color: c,
            custom_size: Some(Vec2::splat(TILE)),
            ..default()
        });
    }
}

// ── phase overlays ───────────────────────────────────────────────────────

fn overlay_dim(p: &mut Painter, l: &Layout) {
    let e = p.fill(0.0, 0.0, l.vw, l.vh, 90.0, [0x08, 0x06, 0x04, 0xff]);
    p.cmds.entity(e).insert(Sprite {
        image: p.sprites.image.clone(),
        rect: Some(p.sprites.rect("white")),
        color: Color::srgba(0.03, 0.02, 0.015, 0.85),
        custom_size: Some(Vec2::new(l.vw, l.vh)),
        ..default()
    });
}

fn draw_supply(p: &mut Painter, g: &Game, l: &Layout, ui: &UiState, hits: &mut Hits) {
    // no dim: the player allocates onto the visible board
    let w = if l.portrait { l.vw - 8.0 } else { 300.0 };
    let x0 = (l.vw - w) / 2.0;
    let y0 = l.hud_h + 22.0;
    p.panel(x0, y0, w, 74.0, 91.0);
    p.text("SUPPLY WINDOW", x0 + 6.0, y0 + 4.0, 91.2, theme::INK, 1.0);
    p.text(
        "BUY SHIPMENTS, DRAG CRATES TO BAYS",
        x0 + 6.0,
        y0 + 12.0,
        91.2,
        theme::INK_DIM,
        1.0,
    );
    let lw = (w - 12.0 - 8.0) / 3.0;
    for (i, lot) in g.lot_offers.iter().enumerate() {
        let x = x0 + 6.0 + i as f32 * (lw + 4.0);
        let y = y0 + 22.0;
        p.fill(x, y, lw, 34.0, 91.2, theme::CARD_BG);
        p.blit("crate", x + 3.0, y + 2.0, 91.3);
        let label: String = lot.name.chars().take((lw / 4.0) as usize - 1).collect();
        p.text(&label, x + 24.0, y + 4.0, 91.3, theme::INK, 1.0);
        let total: u32 = lot.entries.iter().map(|r| r.1).sum();
        p.text(&format!("{total} ITEMS"), x + 24.0, y + 11.0, 91.3, theme::INK_DIM, 1.0);
        p.text(&format!("${}", lot.price), x + 3.0, y + 22.0, 91.3, theme::INK_GOLD, 1.0);
        hits.0.push((Rect::new(x, y, x + lw, y + 34.0), Act::LotBuy(i)));
    }
    let bx = x0 + w - 66.0;
    p.button("START DAY", bx, y0 + 58.0, 60.0, 91.4);
    hits.0.push((Rect::new(bx, y0 + 58.0, bx + 60.0, y0 + 72.0), Act::SupplyDone));
    if ui.sel_supply.is_some() {
        p.text("TAP A BAY", x0 + 6.0, y0 + 62.0, 91.3, theme::COPPER_HI, 1.0);
    }
}

fn draw_shop(p: &mut Painter, g: &Game, l: &Layout, hits: &mut Hits) {
    overlay_dim(p, l);
    let w = if l.portrait { l.vw - 8.0 } else { 330.0 };
    let h = 226.0;
    let x0 = (l.vw - w) / 2.0;
    let y0 = ((l.vh - h) / 2.0).max(2.0);
    p.panel(x0, y0, w, h, 92.0);
    let cleared = g.history.last().map(|o| o.cleared).unwrap_or(true);
    let title = if cleared { format!("DAY {} CLEARED", g.round) } else { format!("DAY {}", g.round) };
    p.text(&title, x0 + 8.0, y0 + 6.0, 92.2, theme::INK, 2.0);
    p.text(
        &format!("NEXT: QUOTA {}", g.quota()),
        x0 + 8.0,
        y0 + 20.0,
        92.2,
        theme::GREEN,
        1.0,
    );
    // pay slip
    let ps = &g.pay;
    p.panel(x0 + 8.0, y0 + 30.0, 130.0, 52.0, 92.2);
    p.text("PAY SLIP", x0 + 12.0, y0 + 34.0, 92.4, theme::INK_DIM, 1.0);
    let rows = [
        ("DAY WAGE", ps.base),
        ("HOME EARLY", ps.early),
        ("INTEREST", ps.interest),
        ("TERMS", ps.term_rewards),
    ];
    let mut ry = y0 + 42.0;
    for (label, amt) in rows {
        p.text(label, x0 + 12.0, ry, 92.4, theme::INK, 1.0);
        let amount = format!("+{amt}");
        p.text(&amount, x0 + 132.0 - Painter::text_w(&amount, 1.0), ry, 92.4, theme::INK, 1.0);
        ry += 7.0;
    }
    p.fill(x0 + 12.0, ry + 1.0, 122.0, 1.0, 92.4, theme::PANEL_EDGE);
    p.text("TOTAL", x0 + 12.0, ry + 4.0, 92.4, theme::INK_GOLD, 1.0);
    let tot = format!("${}", ps.total);
    p.text(&tot, x0 + 132.0 - Painter::text_w(&tot, 1.0), ry + 4.0, 92.4, theme::INK_GOLD, 1.0);

    // contracts shelf
    let cx0 = x0 + 148.0;
    p.text("CONTRACTS", cx0, y0 + 32.0, 92.4, theme::INK_DIM, 1.0);
    for (i, cid) in g.contract_offers.iter().enumerate() {
        let cdef = contract(*cid);
        let x = cx0 + i as f32 * 88.0;
        let y = y0 + 40.0;
        p.fill(x, y, 84.0, 24.0, 92.3, theme::TICKET_BG);
        p.fill(x, y, 84.0, 1.0, 92.4, theme::TICKET_EDGE);
        p.text("*", x + 2.0, y + 2.0, 92.4, theme::INK_GOLD, 1.0);
        let name: String = cdef.name.chars().take(19).collect();
        p.text(&name, x + 7.0, y + 2.0, 92.4, theme::INK_GOLD, 1.0);
        if let overflow_core::defs::ContractKind::Term { rounds, deliver, count, reward } = cdef.kind {
            let sub = format!("{count} {} {rounds}D ${reward}", item_key(deliver).to_uppercase());
            p.text(&sub, x + 2.0, y + 9.0, 92.4, theme::INK_DIM, 1.0);
        } else {
            p.text("ONGOING", x + 2.0, y + 9.0, 92.4, theme::INK_DIM, 1.0);
        }
        p.text(
            &format!("${}", g.offer_price(overflow_core::cards::Offer::Contract(*cid))),
            x + 2.0,
            y + 16.0,
            92.4,
            theme::INK_GOLD,
            1.0,
        );
        hits.0.push((Rect::new(x, y, x + 84.0, y + 24.0), Act::ShopContract(i)));
    }

    // equipment shelf
    p.text("EQUIPMENT", x0 + 8.0, y0 + 90.0, 92.4, theme::INK_DIM, 1.0);
    for (i, offer) in g.offers.iter().enumerate() {
        let x = x0 + 8.0 + i as f32 * 34.0;
        let y = y0 + 98.0;
        p.fill(x, y, 30.0, 44.0, 92.3, theme::CARD_BG);
        p.fill(x, y, 30.0, 1.0, 92.35, theme::PANEL_HI);
        match offer {
            overflow_core::cards::Offer::Machine(card) => {
                card_face(p, x, y, 92.4, card.machine);
                let d = def(card.machine);
                let label: String = d.name.chars().take(8).collect();
                p.text(&label, x + 2.0, y + 20.0, 92.5, theme::INK, 1.0);
            }
            overflow_core::cards::Offer::Directive(did) => {
                p.fill(x + 6.0, y + 4.0, 18.0, 13.0, 92.4, [0x1c, 0x27, 0x33, 0xff]);
                p.text("+", x + 13.0, y + 8.0, 92.5, theme::CYAN, 1.0);
                let label: String = directive(*did).name.chars().take(7).collect();
                p.text(&label, x + 2.0, y + 20.0, 92.5, theme::CYAN, 1.0);
            }
            overflow_core::cards::Offer::Contract(cid) => {
                p.text("*", x + 12.0, y + 8.0, 92.5, theme::INK_GOLD, 1.0);
                let label: String = contract(*cid).name.chars().take(7).collect();
                p.text(&label, x + 2.0, y + 20.0, 92.5, theme::INK_GOLD, 1.0);
            }
        }
        p.text(
            &format!("${}", g.offer_price(*offer)),
            x + 2.0,
            y + 33.0,
            92.5,
            theme::INK_GOLD,
            1.0,
        );
        hits.0.push((Rect::new(x, y, x + 30.0, y + 44.0), Act::ShopOffer(i)));
    }

    // own tray, sellable, shown for reference
    p.text(
        "RIGHT CLICK A TRAY CONTRACT TO SELL IT",
        x0 + 8.0,
        y0 + 152.0,
        92.4,
        theme::INK_DIM,
        1.0,
    );

    let by = y0 + h - 22.0;
    p.panel(x0 + 8.0, by, 76.0, 14.0, 92.3);
    p.text(
        &format!("REROLL ${}", g.reroll_price()),
        x0 + 12.0,
        by + 4.0,
        92.5,
        theme::INK,
        1.0,
    );
    hits.0.push((Rect::new(x0 + 8.0, by, x0 + 84.0, by + 14.0), Act::Reroll));
    p.button("NEXT DAY", x0 + w - 72.0, by, 66.0, 92.3);
    hits.0.push((Rect::new(x0 + w - 72.0, by, x0 + w - 6.0, by + 14.0), Act::ShopDone));
}

fn draw_over(p: &mut Painter, g: &Game, l: &Layout, hits: &mut Hits) {
    overlay_dim(p, l);
    let w = 180.0;
    let x0 = (l.vw - w) / 2.0;
    let y0 = l.vh / 2.0 - 40.0;
    p.panel(x0, y0, w, 80.0, 95.0);
    p.text("SHIFT OVER", x0 + 10.0, y0 + 8.0, 95.2, theme::RED, 2.0);
    p.text(
        &format!("DAY {} SHORT OF QUOTA", g.round + 1),
        x0 + 10.0,
        y0 + 24.0,
        95.2,
        theme::INK_DIM,
        1.0,
    );
    p.text(
        &format!("{}/{}", g.round_delivered, g.quota()),
        x0 + 10.0,
        y0 + 34.0,
        95.2,
        theme::INK,
        1.0,
    );
    p.button("RETRY DAY", x0 + 10.0, y0 + 54.0, 70.0, 95.2);
    hits.0.push((Rect::new(x0 + 10.0, y0 + 54.0, x0 + 80.0, y0 + 68.0), Act::Retry));
    p.button("NEW RUN", x0 + 96.0, y0 + 54.0, 70.0, 95.2);
    hits.0.push((Rect::new(x0 + 96.0, y0 + 54.0, x0 + 166.0, y0 + 68.0), Act::NewRun));
}

/// Item shorthand used by the HUD market chip; quality shown on demand.
#[allow(dead_code)]
fn item_label(t: ItemType, q: i32) -> String {
    format!("{} Q{} ({})", item_key(t).to_uppercase(), q, item_value(t, q))
}

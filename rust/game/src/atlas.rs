//! The whole aesthetic, generated: every sprite — ground, belts, machine
//! plates, emblems, ports, items, glyphs — is painted pixel-by-pixel into
//! one atlas texture at startup. No asset files, no binary blobs in git,
//! and the art style is code you can diff.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;

use crate::theme::*;
use overflow_core::defs::{ItemType, Kind, MachineId, ITEM_TYPES};

pub const ATLAS_W: usize = 512;
pub const ATLAS_H: usize = 256;

/// Names → pixel rects in the atlas image.
#[derive(Resource)]
pub struct Sprites {
    pub image: Handle<Image>,
    rects: HashMap<String, Rect>,
}

impl Sprites {
    pub fn rect(&self, name: &str) -> Rect {
        *self
            .rects
            .get(name)
            .unwrap_or_else(|| panic!("missing sprite {name}"))
    }
    pub fn try_rect(&self, name: &str) -> Option<Rect> {
        self.rects.get(name).copied()
    }
}

struct Painter {
    data: Vec<u8>,
}

impl Painter {
    fn new() -> Self {
        Painter { data: vec![0; ATLAS_W * ATLAS_H * 4] }
    }
    fn px(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= ATLAS_W as i32 || y >= ATLAS_H as i32 {
            return;
        }
        let i = (y as usize * ATLAS_W + x as usize) * 4;
        self.data[i..i + 4].copy_from_slice(&c);
    }
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.px(xx, yy, c);
            }
        }
    }
}

/// The 3×5 pixel font. Each glyph is 15 bits, row-major.
fn glyph(ch: char) -> Option<u16> {
    let s = match ch {
        'A' => "111101111101101", 'B' => "110101110101110", 'C' => "011100100100011",
        'D' => "110101101101110", 'E' => "111100110100111", 'F' => "111100110100100",
        'G' => "011100101101011", 'H' => "101101111101101", 'I' => "111010010010111",
        'J' => "001001001101010", 'K' => "101110100110101", 'L' => "100100100100111",
        'M' => "101111111101101", 'N' => "101111111111101", 'O' => "010101101101010",
        'P' => "110101110100100", 'Q' => "010101101011001", 'R' => "110101110110101",
        'S' => "011100010001110", 'T' => "111010010010010", 'U' => "101101101101011",
        'V' => "101101101010010", 'W' => "101101111111101", 'X' => "101010010010101",
        'Y' => "101101010010010", 'Z' => "111001010100111",
        '0' => "111101101101111", '1' => "010110010010111", '2' => "111001111100111",
        '3' => "111001011001111", '4' => "101101111001001", '5' => "111100111001110",
        '6' => "011100111101111", '7' => "111001010010010", '8' => "111101010101111",
        '9' => "111101111001110",
        '.' => "000000000000010", '/' => "001001010100100", ':' => "000010000010000",
        '-' => "000000111000000", '+' => "000010111010000", '>' => "100110111110100",
        '*' => "010111111010101", '$' => "010111100011110", '%' => "101001010100101",
        '?' => "110001010000010", '!' => "010010010000010", '(' => "010100100100010",
        ')' => "010001001001010", ',' => "000000000010100", '=' => "000111000111000",
        _ => return None,
    };
    let mut bits = 0u16;
    for (i, b) in s.bytes().enumerate() {
        if b == b'1' {
            bits |= 1 << i;
        }
    }
    Some(bits)
}

/// Advance width of a glyph (all are 3+1).
pub const GLYPH_W: f32 = 4.0;

const T: i32 = 16;

/// Base plate color set per machine kind.
pub fn plate_colors(kind: Kind) -> (Rgba, Rgba, Rgba) {
    match kind {
        Kind::Extractor => (CRATE, CRATE_HI, CRATE_LO),
        Kind::Processor => (BRICK, BRICK_HI, BRICK_LO),
        Kind::Assembler => (SLATE, SLATE_HI, SLATE_LO),
        Kind::Logistics => (STEEL_LO, STEEL, [0x26, 0x2b, 0x33, 0xff]),
        Kind::Modifier => (TEAL, TEAL_HI, [0x1f, 0x3d, 0x40, 0xff]),
        Kind::Vault => (GREEN, GREEN_HI, GREEN_LO),
    }
}

pub fn kind_key(kind: Kind) -> &'static str {
    match kind {
        Kind::Extractor => "extractor",
        Kind::Processor => "processor",
        Kind::Assembler => "assembler",
        Kind::Logistics => "logistics",
        Kind::Modifier => "modifier",
        Kind::Vault => "vault",
    }
}

pub fn machine_key(m: MachineId) -> &'static str {
    use MachineId::*;
    match m {
        Drill => "drill", Tap => "tap", Geode => "geode",
        Furnace => "furnace", Retort => "retort", Lapidary => "lapidary",
        Compress => "compress", Fab => "fab", CircuitBench => "circuitbench",
        LensGrinder => "lensgrinder", EngineWorks => "engineworks",
        Belt => "belt", Junction => "junction", Merger => "merger",
        Splitter => "splitter", Buffer => "buffer", Filter => "filter",
        Overclock => "overclock", Polisher => "polisher", Heatsink => "heatsink",
        Dup => "dup", Vault => "vault", Bay => "bay", Chute => "chute",
    }
}

pub fn item_key(t: ItemType) -> &'static str {
    use ItemType::*;
    match t {
        Ore => "ore", Sap => "sap", Crystal => "crystal", Ingot => "ingot",
        Resin => "resin", Shard => "shard", Gear => "gear", Circuit => "circuit",
        Lens => "lens", Engine => "engine", Core => "core", Beacon => "beacon",
        Flux => "flux", Slag => "slag",
    }
}

/// Paint the machine's 10×10 emblem centred in a 16×16 slot.
fn emblem(p: &mut Painter, x: i32, y: i32, m: MachineId) {
    use MachineId::*;
    let (cx, cy) = (x + 8, y + 8); // centre
    match m {
        Drill => {
            p.rect(cx - 3, cy - 4, 6, 3, STEEL_HI);
            p.rect(cx - 2, cy - 1, 4, 3, STEEL);
            p.rect(cx - 1, cy + 2, 2, 3, COPPER_HI);
        }
        Tap => {
            p.rect(cx - 1, cy - 4, 2, 4, STEEL);
            p.rect(cx - 2, cy, 4, 3, [0x7e, 0xc9, 0x6a, 0xff]);
            p.rect(cx - 1, cy + 3, 2, 2, [0x4f, 0xa8, 0x54, 0xff]);
        }
        Geode => {
            p.rect(cx - 1, cy - 4, 2, 2, CYAN);
            p.rect(cx - 3, cy - 2, 6, 4, [0x8f, 0xc8, 0xe8, 0xff]);
            p.rect(cx - 1, cy + 2, 2, 2, CYAN);
        }
        Furnace => {
            p.rect(cx - 4, cy - 4, 8, 8, [0x24, 0x18, 0x12, 0xff]);
            p.rect(cx - 3, cy - 3, 6, 6, FIRE);
            p.rect(cx - 1, cy - 2, 3, 3, FIRE_HI);
        }
        Retort => {
            p.rect(cx - 1, cy - 4, 2, 3, STEEL_HI);
            p.rect(cx - 3, cy - 1, 6, 5, [0x4f, 0xa8, 0x54, 0xff]);
            p.rect(cx - 2, cy + 1, 2, 2, [0x9f, 0xe8, 0x8f, 0xff]);
        }
        Lapidary => {
            p.rect(cx - 3, cy - 1, 6, 2, [0x8f, 0xc8, 0xe8, 0xff]);
            p.rect(cx - 1, cy - 3, 2, 6, [0x8f, 0xc8, 0xe8, 0xff]);
            p.px(cx, cy - 1, [0xff, 0xff, 0xff, 0xff]);
        }
        Compress => {
            p.rect(cx - 4, cy - 4, 8, 2, STEEL_HI);
            p.rect(cx - 4, cy + 2, 8, 2, STEEL_HI);
            p.rect(cx - 2, cy - 1, 4, 2, COPPER_HI);
        }
        Fab => {
            p.rect(cx - 3, cy - 3, 6, 6, PURPLE);
            p.rect(cx - 1, cy - 4, 2, 1, PURPLE);
            p.rect(cx - 1, cy + 3, 2, 1, PURPLE);
            p.rect(cx - 4, cy - 1, 1, 2, PURPLE);
            p.rect(cx + 3, cy - 1, 1, 2, PURPLE);
            p.rect(cx - 1, cy - 1, 2, 2, SLATE_LO);
        }
        CircuitBench => {
            p.rect(cx - 3, cy - 3, 6, 6, [0x14, 0x2b, 0x22, 0xff]);
            p.rect(cx - 2, cy - 2, 2, 2, [0x5f, 0xd8, 0xa0, 0xff]);
            p.rect(cx + 1, cy, 2, 2, [0x5f, 0xd8, 0xa0, 0xff]);
            p.px(cx - 3, cy + 3, [0x5f, 0xd8, 0xa0, 0xff]);
        }
        LensGrinder => {
            p.rect(cx - 3, cy - 3, 6, 6, [0x2b, 0x3d, 0x55, 0xff]);
            p.rect(cx - 2, cy - 2, 4, 4, [0x6f, 0xb8, 0xff, 0xff]);
            p.px(cx - 1, cy - 1, [0xdf, 0xef, 0xff, 0xff]);
        }
        EngineWorks => {
            p.rect(cx - 4, cy - 2, 8, 4, STEEL);
            p.rect(cx - 2, cy - 4, 4, 2, COPPER_HI);
            p.rect(cx - 1, cy + 2, 2, 2, FIRE);
        }
        Junction => {
            p.rect(cx - 1, cy - 4, 2, 8, STEEL_HI);
            p.rect(cx - 4, cy - 1, 8, 2, STEEL_HI);
        }
        Merger => {
            p.rect(cx - 4, cy - 3, 3, 2, STEEL_HI);
            p.rect(cx - 4, cy + 1, 3, 2, STEEL_HI);
            p.rect(cx - 1, cy - 1, 5, 2, COPPER_HI);
        }
        Splitter => {
            p.rect(cx - 4, cy - 1, 3, 2, COPPER_HI);
            p.rect(cx - 1, cy - 3, 5, 2, STEEL_HI);
            p.rect(cx - 1, cy + 1, 5, 2, STEEL_HI);
        }
        Buffer => {
            p.rect(cx - 3, cy - 3, 6, 6, [0x2b, 0x32, 0x3b, 0xff]);
            p.rect(cx - 2, cy - 2, 4, 1, STEEL_HI);
        }
        Filter => {
            p.rect(cx - 4, cy - 3, 8, 2, STEEL_HI);
            p.rect(cx - 2, cy - 1, 4, 2, STEEL);
            p.rect(cx - 1, cy + 1, 2, 3, COPPER_HI);
        }
        Overclock => {
            p.rect(cx, cy - 4, 2, 3, FIRE_HI);
            p.rect(cx - 2, cy - 1, 3, 2, FIRE_HI);
            p.rect(cx - 1, cy + 1, 2, 3, FIRE_HI);
        }
        Polisher => {
            p.px(cx, cy - 3, [0xff, 0xff, 0xff, 0xff]);
            p.rect(cx - 1, cy - 1, 3, 1, [0xff, 0xff, 0xff, 0xff]);
            p.rect(cx, cy - 2, 1, 3, [0xdf, 0xef, 0xff, 0xff]);
            p.px(cx - 3, cy + 2, INK_GOLD);
            p.px(cx + 3, cy - 2, INK_GOLD);
        }
        Heatsink => {
            for k in 0..4 {
                p.rect(cx - 4 + k * 2, cy - 3, 1, 6, STEEL_HI);
            }
        }
        Dup => {
            p.rect(cx - 4, cy - 3, 4, 4, PURPLE);
            p.rect(cx, cy - 1, 4, 4, [0xc5, 0xba, 0xff, 0xff]);
        }
        Vault => {
            p.rect(cx - 4, cy - 4, 8, 8, GREEN_LO);
            p.rect(cx - 2, cy - 2, 4, 4, GREEN_HI);
            p.rect(cx - 1, cy - 1, 2, 2, GREEN_LO);
        }
        Chute => {
            for k in 0..3 {
                p.rect(cx - 3 + k, cy - 2 + k, 6 - 2 * k, 1, [0x3a, 0x3a, 0x44, 0xff]);
            }
        }
        Bay | Belt => {}
    }
}

pub fn build_atlas(mut images: ResMut<Assets<Image>>, mut commands: Commands) {
    let mut p = Painter::new();
    let mut rects: HashMap<String, Rect> = HashMap::new();
    // simple shelf packer
    let (mut cx, mut cy, mut shelf) = (0i32, 0i32, 0i32);
    let mut alloc = |w: i32, h: i32| -> (i32, i32) {
        if cx + w > ATLAS_W as i32 {
            cx = 0;
            cy += shelf;
            shelf = 0;
        }
        let at = (cx, cy);
        cx += w + 1; // 1px gutter against bleed
        shelf = shelf.max(h + 1);
        assert!(cy + h <= ATLAS_H as i32, "atlas full");
        at
    };
    let mut put = |name: String, p: &mut Painter, w: i32, h: i32, f: &dyn Fn(&mut Painter, i32, i32)| {
        let (x, y) = alloc(w, h);
        f(p, x, y);
        rects.insert(name, Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32));
    };

    // ── flat white pixel: every UI rect is this, tinted ──
    put("white".into(), &mut p, 1, 1, &|p, x, y| p.px(x, y, [0xff, 0xff, 0xff, 0xff]));

    // ── ground ──
    for (name, base) in [("ground0", GROUND0), ("ground1", GROUND1)] {
        put(name.into(), &mut p, T, T, &move |p, x, y| {
            p.rect(x, y, T, T, base);
            p.rect(x, y, T, 1, GRIDLINE);
            p.rect(x, y, 1, T, GRIDLINE);
        });
    }

    // ── belts: 3 tread phases × horizontal/vertical ──
    for ph in 0..3i32 {
        put(format!("belt_h{ph}"), &mut p, T, T, &move |p, x, y| {
            p.rect(x, y + 2, 16, 1, STEEL_LO);
            p.rect(x, y + 3, 16, 10, BELT);
            p.rect(x, y + 3, 16, 1, BELT_HI);
            p.rect(x, y + 12, 16, 1, [0x1a, 0x1a, 0x1e, 0xff]);
            p.rect(x, y + 13, 16, 1, STEEL_LO);
            for k in 0..4 {
                let off = (k * 5 + ph * 2) % 16;
                p.rect(x + off, y + 5, 2, 2, TREAD);
                p.rect(x + (off + 8) % 16, y + 9, 2, 2, TREAD);
            }
        });
        put(format!("belt_v{ph}"), &mut p, T, T, &move |p, x, y| {
            p.rect(x + 2, y, 1, 16, STEEL_LO);
            p.rect(x + 3, y, 10, 16, BELT);
            p.rect(x + 3, y, 1, 16, BELT_HI);
            p.rect(x + 12, y, 1, 16, [0x1a, 0x1a, 0x1e, 0xff]);
            p.rect(x + 13, y, 1, 16, STEEL_LO);
            for k in 0..4 {
                let off = (k * 5 + ph * 2) % 16;
                p.rect(x + 5, y + off, 2, 2, TREAD);
                p.rect(x + 9, y + (off + 8) % 16, 2, 2, TREAD);
            }
        });
    }
    // belt direction nib: a quiet steel chevron at the exit edge
    let nib: Rgba = [0x6e, 0x71, 0x7d, 0xff];
    put("nib_e".into(), &mut p, 4, 6, &move |p, x, y| {
        for k in 0..2 {
            p.rect(x + k * 2, y + k, 1, 6 - 2 * k, nib);
            p.rect(x + 1 + k * 2, y + k, 1, 6 - 2 * k, nib);
        }
    });
    put("nib_w".into(), &mut p, 4, 6, &move |p, x, y| {
        for k in 0..2 {
            p.rect(x + 3 - k * 2, y + k, 1, 6 - 2 * k, nib);
            p.rect(x + 2 - k * 2, y + k, 1, 6 - 2 * k, nib);
        }
    });
    put("nib_s".into(), &mut p, 6, 4, &move |p, x, y| {
        for k in 0..2 {
            p.rect(x + k, y + k * 2, 6 - 2 * k, 1, nib);
            p.rect(x + k, y + 1 + k * 2, 6 - 2 * k, 1, nib);
        }
    });
    put("nib_n".into(), &mut p, 6, 4, &move |p, x, y| {
        for k in 0..2 {
            p.rect(x + k, y + 3 - k * 2, 6 - 2 * k, 1, nib);
            p.rect(x + k, y + 2 - k * 2, 6 - 2 * k, 1, nib);
        }
    });

    // ── machine plates per kind: 16×16 bevelled slab ──
    for kind in [
        Kind::Extractor, Kind::Processor, Kind::Assembler,
        Kind::Logistics, Kind::Modifier, Kind::Vault,
    ] {
        let (base, hi, lo) = plate_colors(kind);
        put(format!("plate_{}", kind_key(kind)), &mut p, T, T, &move |p, x, y| {
            p.rect(x + 1, y + 1, 14, 14, base);
            p.rect(x + 1, y + 1, 14, 2, hi);
            p.rect(x + 1, y + 13, 14, 2, lo);
            p.px(x + 2, y + 3, hi);
            p.px(x + 13, y + 12, lo);
        });
    }

    // ── emblems ──
    use MachineId::*;
    for m in [
        Drill, Tap, Geode, Furnace, Retort, Lapidary, Compress, Fab,
        CircuitBench, LensGrinder, EngineWorks, Junction, Merger, Splitter,
        Buffer, Filter, Overclock, Polisher, Heatsink, Dup, Vault, Chute,
    ] {
        put(format!("em_{}", machine_key(m)), &mut p, T, T, &move |p, x, y| emblem(p, x, y, m));
    }

    // ── special whole tiles ──
    put("bay".into(), &mut p, T, T, &|p, x, y| {
        p.rect(x, y + 1, 16, 14, [0x33, 0x30, 0x2b, 0xff]);
        p.rect(x, y + 1, 16, 2, [0x45, 0x41, 0x38, 0xff]);
        for k in 0..8 {
            p.rect(x + k * 2, y + 1, 1, 2, if k % 2 == 1 { HAZ_Y } else { HAZ_K });
            p.rect(x + k * 2, y + 13, 1, 2, if k % 2 == 1 { HAZ_Y } else { HAZ_K });
        }
        p.rect(x + 2, y + 5, 12, 6, [0x1a, 0x18, 0x14, 0xff]);
    });
    put("vault_tile".into(), &mut p, T, T, &|p, x, y| {
        p.rect(x + 1, y + 1, 14, 14, GREEN);
        p.rect(x + 1, y + 1, 14, 2, GREEN_HI);
        p.rect(x + 1, y + 13, 14, 2, GREEN_LO);
        p.rect(x + 4, y + 4, 8, 8, GREEN_LO);
        p.rect(x + 6, y + 6, 4, 4, GREEN_HI);
        p.rect(x + 7, y + 7, 2, 2, GREEN_LO);
    });
    put("chute_tile".into(), &mut p, T, T, &|p, x, y| {
        p.rect(x + 2, y + 2, 12, 12, [0x11, 0x10, 0x13, 0xff]);
        p.rect(x + 3, y + 3, 10, 10, [0x1c, 0x1a, 0x20, 0xff]);
        for k in 0..3 {
            p.rect(x + 5 + k, y + 6 + k, 6 - 2 * k, 1, [0x3a, 0x3a, 0x44, 0xff]);
        }
    });
    put("chimney".into(), &mut p, 7, 7, &|p, x, y| {
        p.rect(x, y + 4, 5, 3, STEEL);
        p.rect(x, y + 4, 5, 1, STEEL_HI);
        p.rect(x + 1, y + 2, 3, 2, [0x3d, 0x3d, 0x43, 0xff]);
        p.rect(x + 2, y, 2, 1, [0x4a, 0x4a, 0x52, 0xff]);
    });

    // ── ports: intake notch (cyan) and out spout (copper), per facing ──
    // Painted for the port sitting on the E edge; the other three follow.
    for (edge, name) in [(0, "n"), (1, "e"), (2, "s"), (3, "w")] {
        put(format!("port_in_{name}"), &mut p, T, T, &move |p, x, y| {
            match edge {
                1 => { p.rect(x + 14, y + 5, 2, 6, HAZ_K); p.rect(x + 15, y + 6, 1, 4, CYAN); }
                3 => { p.rect(x, y + 5, 2, 6, HAZ_K); p.rect(x, y + 6, 1, 4, CYAN); }
                0 => { p.rect(x + 5, y, 6, 2, HAZ_K); p.rect(x + 6, y, 4, 1, CYAN); }
                _ => { p.rect(x + 5, y + 14, 6, 2, HAZ_K); p.rect(x + 6, y + 15, 4, 1, CYAN); }
            }
        });
        put(format!("port_out_{name}"), &mut p, T, T, &move |p, x, y| {
            match edge {
                1 => p.rect(x + 14, y + 6, 2, 4, COPPER_HI),
                3 => p.rect(x, y + 6, 2, 4, COPPER_HI),
                0 => p.rect(x + 6, y, 4, 2, COPPER_HI),
                _ => p.rect(x + 6, y + 14, 4, 2, COPPER_HI),
            }
        });
    }

    // ── items: 6×6 blob with dark outline and a highlight ──
    for t in ITEM_TYPES {
        let c = item_color(t);
        put(format!("it_{}", item_key(t)), &mut p, 6, 6, &move |p, x, y| {
            p.rect(x, y + 1, 6, 4, [0x0b, 0x0a, 0x08, 0xff]);
            p.rect(x + 1, y, 4, 6, [0x0b, 0x0a, 0x08, 0xff]);
            p.rect(x + 1, y + 1, 4, 4, c);
            p.px(x + 1, y + 1, [0xff, 0xff, 0xff, 0x88]);
        });
    }

    // ── crate face for supply cards ──
    put("crate".into(), &mut p, 18, 13, &|p, x, y| {
        p.rect(x, y, 18, 13, CRATE);
        p.rect(x, y, 18, 2, CRATE_HI);
        p.rect(x + 8, y, 2, 13, CRATE_LO);
        p.rect(x, y + 6, 18, 1, CRATE_LO);
    });

    // ── glyphs, painted white and tinted at spawn ──
    let charset = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789./:-+>*$%?!(),=";
    for ch in charset.chars() {
        let bits = glyph(ch).unwrap();
        put(format!("g_{ch}"), &mut p, 3, 5, &move |p, x, y| {
            for i in 0..15 {
                if bits & (1 << i) != 0 {
                    p.px(x + (i % 3), y + (i / 3), [0xff, 0xff, 0xff, 0xff]);
                }
            }
        });
    }
    // pips for shift dots
    put("pip_on".into(), &mut p, 4, 4, &|p, x, y| {
        p.rect(x, y + 1, 4, 2, COPPER_HI);
        p.rect(x + 1, y, 2, 4, COPPER_HI);
    });
    put("pip_off".into(), &mut p, 4, 4, &|p, x, y| {
        p.rect(x, y + 1, 4, 2, [0x3a, 0x33, 0x28, 0xff]);
        p.rect(x + 1, y, 2, 4, [0x3a, 0x33, 0x28, 0xff]);
        p.rect(x + 1, y + 1, 2, 2, PANEL);
    });

    let mut image = Image::new(
        Extent3d { width: ATLAS_W as u32, height: ATLAS_H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        p.data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    let handle = images.add(image);
    commands.insert_resource(Sprites { image: handle, rects });
}

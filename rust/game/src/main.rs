//! OVERFLOW — the Bevy frontend. 8-bit factory over the engine-free core:
//! `bridge` owns the rules, `atlas` paints the art from code, `scene`
//! repaints the screen from state, `input` turns gestures into commands.
//! Portrait is mobile, landscape is PC — same scene, different furniture.

mod atlas;
mod bridge;
mod input;
mod layout;
mod scene;
mod theme;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::WindowResolution;

use crate::scene::{ItemRoot, UiRoot};

fn main() {
    let (w, h): (u32, u32) = std::env::var("OVERFLOW_WINDOW")
        .ok()
        .and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().ok(), b.parse().ok())))
        .and_then(|(a, b)| a.zip(b))
        .unwrap_or((1280, 720));
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "OVERFLOW".into(),
                        resolution: WindowResolution::new(w, h),
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .insert_resource(ClearColor(theme::col(theme::PANEL_LO)))
        .insert_resource(bridge::Bridge::new(seed_from_env()))
        .insert_resource(layout::Layout::default())
        .insert_resource(scene::UiState::default())
        .insert_resource(scene::Hits::default())
        .insert_resource(scene::BeltPhase::default())
        .insert_resource(input::Press::default())
        .add_systems(Startup, (atlas::build_atlas, setup))
        .add_systems(
            Update,
            (
                input::pointer,
                input::keys,
                bridge::apply_cmds,
                bridge::tick_shift,
                bridge::tick_toast,
                layout::update_layout,
                scene::rebuild,
                scene::animate_items,
                scene::animate_belts,
            )
                .chain(),
        )
        .add_systems(Update, (demo_script, shot_when_asked))
        .run();
}

fn seed_from_env() -> u32 {
    std::env::var("OVERFLOW_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(42)
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((UiRoot, Transform::default(), Visibility::default()));
    commands.spawn((ItemRoot, Transform::default(), Visibility::default()));
}

/// Headless verification: OVERFLOW_SHOT=/path.png captures frame ~20 and
/// exits shortly after, so CI or an agent can look at the real render.
/// With OVERFLOW_SCRIPT=demo it instead captures a keyframe series while
/// the demo plays.
fn shot_when_asked(
    mut frames: Local<u32>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(path) = std::env::var("OVERFLOW_SHOT") else { return };
    let scripted = std::env::var("OVERFLOW_SCRIPT").is_ok();
    *frames += 1;
    if !scripted {
        if *frames == 20 {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        }
        if *frames == 40 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let keyframes: [(u32, &str); 4] =
        [(30, "built"), (80, "shift"), (140, "late"), (260, "after")];
    for (f, tag) in keyframes {
        if *frames == f {
            let p = path.replace(".png", &format!("_{tag}.png"));
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(p));
        }
    }
    if *frames == 300 {
        exit.write(AppExit::Success);
    }
}

/// OVERFLOW_SCRIPT=demo: play the harness-test factory over the bridge —
/// allocate the dealt ore, kiss a furnace onto each bay, lane east, spine
/// to the vault, run the morning shift. Proves the whole command surface
/// without a pointer.
fn demo_script(mut frames: Local<u32>, mut bridge: ResMut<bridge::Bridge>) {
    if std::env::var("OVERFLOW_SCRIPT").is_err() {
        return;
    }
    use bridge::Cmd;
    use overflow_core::defs::Dir;
    *frames += 1;
    let b = &mut *bridge;
    match *frames {
        2 => {
            b.speed = 30.0;
            b.push(Cmd::Allocate { supply_idx: 0, bay: 0 });
            b.push(Cmd::Allocate { supply_idx: 0, bay: 1 });
            b.push(Cmd::SupplyDone);
        }
        4 => {
            for (row, spine) in [(6, Dir::S), (12, Dir::N)] {
                let f = b
                    .game
                    .hand
                    .iter()
                    .position(|c| c.machine == overflow_core::defs::MachineId::Furnace);
                if let Some(f) = f {
                    b.push(Cmd::PlayCard { hand_idx: f, x: 1, y: row, d: Dir::E });
                }
                for x in 3..=15 {
                    b.push(Cmd::Belt { x, y: row, d: Dir::E });
                }
                b.push(Cmd::Belt { x: 16, y: row, d: spine });
                let ys: [i32; 2] = if row < 9 { [7, 8] } else { [11, 10] };
                for y in ys {
                    b.push(Cmd::Belt { x: 16, y, d: spine });
                }
            }
            b.push(Cmd::Belt { x: 16, y: 9, d: Dir::E });
        }
        40 => b.push(Cmd::StartShift),
        _ => {}
    }
}

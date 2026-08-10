# Headless toolchain

No editor, no IDE, no clicking. Everything below is verified working on this box
(Ubuntu, no GPU, no desktop — `libGL`/`libX11`/`libvulkan` were not even installed
when Godot was first run).

## What's installed

| Thing | Where | Notes |
|---|---|---|
| Godot 4.7.1 stable | `~/tools/godot` | Single 138 MB binary. Not an install — just a file. |
| Export templates | `~/.local/share/godot/export_templates/4.7.1.stable/` | 1.2 GB extracted, needed only for exports. |
| GUT 9.6.1 | `addons/gut/` in-project | Test framework. Vendored by `git clone`, no editor plugin activation needed. |
| bun | `~/.bun/bin/bun` | For the TypeScript reference sim. |

## The loop

A Godot project is **entirely plain text** — `project.godot` is INI, `.tscn` and
`.tres` are a simple declarative format. All of it is writable and diffable by hand,
which is what makes the editor optional rather than merely avoidable.

```sh
GODOT=~/tools/godot

# 1. import — regenerates .godot/ cache after any file changes
$GODOT --headless --path . --import

# 2. run a script (must extend SceneTree or MainLoop)
$GODOT --headless --path . --script res://tools/whatever.gd

# 3. test — exits NON-ZERO on failure, so CI works
$GODOT --headless --path . -s addons/gut/gut_cmdln.gd -gdir=res://test -gexit

# 4. export a playable web build
$GODOT --headless --path . --export-release "Web" build/index.html
```

Verified: a hand-written `.tres` with a custom `Resource` script loads and
deserializes correctly; a hand-written `.tscn` instantiates with the right child
nodes; a deliberately failing GUT assertion surfaces and returns exit code 1; the
web export produces a complete `index.{html,js,wasm,pck}` set.

## Seeing the game

Headless has no renderer, but rendering works offscreen through Xvfb + Mesa's
software rasterizer (llvmpipe). This is how screenshots get taken.

```sh
xvfb-run -a $GODOT --path . --rendering-driver opengl3 --audio-driver Dummy
```

Put `driver/driver="Dummy"` under `[audio]` in `project.godot` — this box has no
`libpulse`/ALSA card and the audio subsystem will spew errors otherwise. It's
harmless but noisy.

A script can then capture a frame:

```gdscript
await RenderingServer.frame_post_draw
get_viewport().get_texture().get_image().save_png("res://shot.png")
```

Verified end to end: a 480×270 scene rendered six coloured tiles and saved a
correct PNG.

## Honest limits

- **Software rendering is slow.** Fine for screenshots and correctness checks;
  useless for judging performance. Frame-rate numbers from this box mean nothing.
- **No audio.** Can be written and wired, can't be heard here.
- **No input.** Feel, game juice, and anything that depends on how it responds to a
  mouse has to be judged by a human on real hardware. Screenshots catch layout and
  logic bugs, not whether it's fun.
- **Web export is the delivery mechanism.** Serving `build/` via GitHub Pages needs
  the repo to be public — same blocker as the design doc (see `NOTES.md`).

# Platform layer and renderer

`tfp-render` (crates/render) is the wgpu pixel-art renderer; `towerfall-port`
(crates/game) is the binary: SDL2 window, input and audio, the fixed 60 Hz
loop, and (for now) a platform demo scene. Code started as a copy of our
rounds-port platform layer, adapted to XNA/Monocle semantics.

## Building and running (Linux)

```sh
source env.sh
cargo build -p towerfall-port                      # or --release
cargo test -p tfp-render -p towerfall-port         # GPU pixel tests need a GPU or lavapipe

# Headless: no window, no audio device, virtual 60 Hz (one tick per frame), deterministic.
target/debug/towerfall-port --headless --frames 120 --screenshot scratch/demo.png \
    [--native-screenshot scratch/demo_320.png] [--wav scratch/demo.wav] [--music 0] \
    [--window 1280x720] [--wide | --game 640x240] [--scale fit|integer|stretch] \
    [--virtual-pads 12] [--no-assets] [--demo --play-wav some.wav]
target/debug/towerfall-port --list-devices [--virtual-pads 3]   # GPUs, controllers, audio
target/debug/towerfall-port [--window 1280x720] [--fullscreen] [--no-vsync] [--frames N]
```

- `--screenshot` saves the *presented* frame (window size, scaled, overlay
  included); `--native-screenshot` the raw game canvas (320x240 etc.).
- Windowed testing without touching the desktop: the private Xvfb,
  `DISPLAY=$(scripts/dev/xvfb.sh) SDL_AUDIODRIVER=dummy target/debug/towerfall-port --frames 180 --screenshot scratch/w.png`
  (wgpu falls back to llvmpipe there: Xvfb has no DRI3).
- SDL2 links against the system runtime `libSDL2-2.0.so.0` (no -dev
  package): `crates/game/build.rs` adds `tools/lib` (symlink made by
  `scripts/bootstrap-tools.sh`), looked up under `$TFP_ROOT` first (the
  main checkout, set by env.sh, so git worktrees link too), then relative
  to the checkout; `TFP_SDL2_LIB_DIR` overrides. Needs SDL >= 2.26
  at run time (virtual joysticks need 2.24+).
- `WGPU_BACKEND=vulkan|gl`, `WGPU_ADAPTER_NAME=llvmpipe`, `WGPU_POWER_PREF`
  select the GPU. `RUST_LOG=info|debug` for logs.
- Window keys: F11 or Alt+Enter toggles borderless fullscreen, F9 cycles the
  scale mode. Default window: 3x the game size (960x720, 1260x720), like the
  original. Demo keys: arrows/WASD move, Space/C jump, J/X shoot, T toggles
  the slow-motion wave (pads: stick/d-pad, A, X/RB, Y).
- Game data: `TF_GAME_DIR` (else Steam library discovery via `tfp-assets`).
  The demo draws from the real atlases when found, placeholders otherwise.

## Windows (cross-compiled from Linux)

Toolchain in `tools/windows/` (llvm-mingw 20261006 msvcrt + SDL2 2.32.10
mingw devel, SHA-256 pinned; `scripts/fetch-windows-toolchain.sh`).

```sh
scripts/build-windows.sh [--debug]     # -> target/dist/windows/{towerfall-port.exe, SDL2.dll}
scripts/run-windows.sh --headless --frames 10 --screenshot scratch/win.png
scripts/run-windows.sh --list-devices
```

- Target `x86_64-pc-windows-gnu`, linker llvm-mingw clang/lld
  (`scripts/windows-env.sh`); GCC runtime shims in `tools/windows/gcc-shim/`
  (compiler-rt builtins, LLVM libunwind), so the exe needs only system DLLs
  and `SDL2.dll`. Ship `SDL2.dll` next to the exe.
- `scripts/run-windows.sh` runs Proton Experimental's `files/bin/wine`
  directly (Steam and the Proton launcher are not involved) inside bubblewrap:
  `/` read-only (the game install too), only the project writable, `/tmp` =
  `tools/wine-tmp`, private `/dev/shm`, HOME/XDG/`WINEPREFIX` in `tools/`.
  DXVK's `dxgi.dll` is copied into the prefix (wgpu imports dxgi.dll).
- Wine's null graphics driver hangs Vulkan instance creation, so Wine uses
  its X11 driver on the **private Xvfb :64** (started by the script; only that
  socket is bound into the sandbox). Rendering: wgpu Vulkan → winevulkan →
  host Mesa (the Intel GPU in headless mode). `SDL_AUDIODRIVER` defaults to
  `dummy` there; `TF_GAME_DIR` is passed as a `Z:\...` path.
- Verified (Proton experimental-11.0): `--list-devices` (3 adapters, SDL
  2.32.10, WASAPI devices, virtual pads), `--headless --frames 10
  --screenshot --wav --music 0`: PNGs pixel-identical and WAV byte-identical
  to the Linux build; `--virtual-pads 12` works.

## Renderer (`tfp-render`)

```rust
let gpu = Gpu::headless()?;                     // or Gpu::new(new_instance_for_display(..), Some(&surface))
let mut r = Renderer::new(&gpu);
let atlas = r.create_texture(&gpu, &image);     // image.fna_clear_transparent() first for atlas PNGs
let screen = r.create_canvas(&gpu, (320, 240)); // also light maps, layer targets
let mut list = DrawList::new();
list.begin_pass(Some(light), Some(Color::rgb(40, 40, 70)));   // into a canvas
list.sprite(SubTexture::new(atlas, rect), &SpriteParams::at(pos).origin(o).scale(s)
            .rotation(a).color(Color::WHITE * 0.5).flip(true, false));
list.begin_pass(None, Some(Color::BLACK));      // None = the default target
list.set_state(Blend::Alpha, camera_matrix);    // like SpriteBatch.Begin
list.rect / hollow_rect / line / circle / fill_circle / point
list.light_composite(foreground, light, LightParams { alpha: 0.4 });
font.draw(&mut list, "TEXT", pos, color, 1.0);  // SpriteFont (XNB) or debug_font()
r.render(&gpu, &list, screen)?;
let vp = r.present(&gpu, screen, surface_or_offscreen.target(),
                   &PresentSettings { mode, letterbox, offset, wave }, Some(&overlay))?;
r.capture(&gpu, screen)?.save_png("scratch/x.png")?;
```

- **XNA semantics.** Colours are gamma-space bytes, no sRGB decoding
  anywhere (canvases are `Rgba8Unorm`; the window surface prefers a non-sRGB
  format, and with an sRGB-only surface the shaders linearise so stored bytes
  match). Tints are premultiplied like XNA (`Color::WHITE * 0.5` scales all
  channels, truncating: 127); fragment = texel × tint. Blend states:
  `Alpha` (XNA AlphaBlend, premultiplied), `Additive`, `NonPremultiplied`,
  `Opaque`, `Subtract` (reverse subtract, e.g. the kill-freeze overlay),
  `Multiply`. The shipped atlas PNGs are already premultiplied
  (`docs/spec/rendering.md` 3) and are uploaded as they are
  (`Image::fna_clear_transparent` only zeroes alpha-0 pixels, a no-op for
  them); XNB font sheets are premultiplied too. Sampling is nearest, clamp.
- **Sprites** follow `SpriteBatch.Draw(tex, pos, src, color, rotation,
  origin, scale, effects)`: origin in source pixels on the un-flipped quad,
  rotation clockwise (y down), flips mirror the image inside the quad.
  Quads are drawn in submission order (Deferred); consecutive quads with the
  same texture and blend share a draw call. Positions are not snapped by
  the renderer (the original floors entity positions; game code does).
  Shapes are a stretched white pixel like Monocle's `Draw`; `rect`
  truncates its arguments like `Draw.Rect` (filled circles as pixel spans).
- **Passes and canvases.** A draw list is a sequence of passes, each into a
  canvas (or the default target) with optional clear. Drawing a canvas
  into itself is skipped with a warning.
- **Lighting** (spec 7). `light_composite(fg, light, LightParams { alpha })`
  draws the foreground canvas over the current target through the
  original's `LightingEffect`: `out.rgb = fg.rgb - alpha * light.a *
  light.rgb` (clamped), `out.a = fg.a`, AlphaBlend; transparent foreground
  stays transparent, so the background is never darkened. The game fills
  the light canvas (W x 240) each frame: clear to white (or `255 -
  DarknessColor`), then per light the 40x40 inner region of the atlas
  `light` cone, origin (20,20), scale `(r + r*0.4*sine)/20`, tint
  `LightColor * LightAlpha` (black = remove darkness, coloured lights store
  the inverted colour), AlphaBlend, camera matrix.
- **Presentation** (spec 2, 8). `ScaleMode::Fit` (default: the original's
  fractional *LargestScale*, rectangle truncated and centred like
  Monocle's), `Integer` (largest whole scale, Fit if the window is smaller),
  `Stretch`; letterbox colour; `offset`
  in game pixels for screen shake (`Screen.Offset/OffsetAdd`), rounded to
  window pixels, and the image **wraps around** like Monocle's screen (which
  tiles the target) instead of showing letterbox. `wave` is the original's
  `ScreenEffects` shader: rows displaced by `amplitude * sin(v * frequency +
  counter)` of the width, wrapping, applied after the offset (slow motion:
  frequency 6, amplitude `0.05 * (1 - TimeRate)`, counter += 0.01/frame).
  The optional overlay list
  is drawn in game pixels mapped through the viewport at window resolution
  (like the original's HUD/pause layers drawn with `Screen.Matrix`; also the
  aspect bars `aspectBarLeft/Right` at x = -55 and x = W, which land in the
  pillar-box because the overlay is not clipped).
  `Viewport` maps window ↔ game coordinates.
- **Fonts.** `SpriteFont` holds XNA SpriteFont data (glyph bounds, cropping
  offset, kerning triple, line spacing, spacing, default char) and lays out
  text with XNA's rules; `crates/game/src/content.rs` builds it from
  `tfp_assets::xnb::SpriteFont`. `debug_font()` is a built-in 3x5 font.

## Platform layer (`towerfall-port`)

- `app.rs`: the `Scene` trait (`game_size`, `load`, `tick` once per 1/60 s
  with the `RawInput` snapshot, `draw` into the game canvas + overlay +
  present settings) and two hosts. `run_windowed`: SDL window (resizable,
  high-DPI, min 320x240), wgpu surface (instance created with SDL's display
  handle, needed by wgpu 30), vsync (FIFO) or not, event pump, fullscreen
  toggle. `run_headless`: virtual time, offscreen present, offline audio,
  optional SDL virtual pads with scripted input.
- `timing.rs`: fixed 60 Hz, no render interpolation (the latest state is
  drawn; pixel art). Measured frame times are snapped to whole display
  refresh periods (±0.5 ms) so a 60 Hz display runs exactly one tick per
  frame; 30/120/144 Hz work too; hitches clamp at 0.25 s. Pacing: without
  vsync, or when presents return in under half a refresh period (no real
  vblank), the loop sleeps until the next tick is due.
- `input.rs`: `InputHub` (no SDL dependency, unit-tested with 16 pads) keeps
  the keyboard (512 scancodes) and any number of controllers keyed by SDL
  instance id; `snapshot()` produces `RawInput { keyboard, pads (in
  connection order), connected, disconnected }`, each with held / pressed /
  released (taps shorter than a tick are latched) and raw axes (sticks
  -1..1 y-down, triggers 0..1). `SdlInput` opens controllers on hot-plug
  events and feeds the hub (button/axis events, focus loss releases keys);
  mappings from SDL's DB plus `gamecontrollerdb.txt` next to the exe, in
  `$TF_GAME_DIR` (the original ships one) or `$TFP_DATA_DIR`. Hints enable
  background events and RawInput/HIDAPI on Windows (beyond XInput's 4).
  `VirtualPads` attaches SDL virtual controllers for tests. Mapping to player
  actions (`docs/spec/input.md`) belongs to the game layer.
- `audio/`: our mixer: 256 SFX voices (mono/stereo sources, volume, pitch as
  rate (`xna_pitch` converts XNA's −1..1 octaves), XNA balance pan law,
  looping, Music/Sfx buses + master, click-free gain ramps, linear
  resampling), and a music channel: one track + fading-out tracks,
  equal-power crossfade of a given length (0 = hard cut), loop regions,
  resampling. On a real device music sources run on a streaming thread
  (`StreamingSource`, chunked, never blocks the callback; underruns play
  silence); offline they are pulled directly. SDL device: f32 stereo,
  44.1 kHz, 512 frames, commands over a channel. Headless writes a 16-bit
  WAV. `wav.rs` also decodes PCM WAVs.
- `content.rs`: `tfp-assets` → renderer/mixer types (atlas upload, XNB font,
  PCM). `demo.rs`: the demo (real atlas background, tileset, archers from
  sprite data, arrows, light sprites with the lighting composite, the
  original's font, SFX, music track via `--music N`, one archer per device,
  shake on shooting).

## Scenes in the binary

| Run | Scene |
|---|---|
| (default) | the front end: intro, title, menus (`crates/game/src/front/`, see `docs/ui-notes.md`) |
| `--screen NAME [--players N] [--capacity 4\|8\|12]` | one menu screen, pinned (screenshots) |
| `--tower NAME\|ID [--level N] [--eight-player] [--seed N]` | tower viewer: a level's full look, animated |
| `--play [--tower ..] [--level N] [--players N] [--script FILE\|bots\|none]` | sandbox match on the sim |
| `--play --mode lms\|hh\|tdm [--teams 2-6] [--length instant\|quick\|standard\|epic] [--no-replay] [--shots DIR]` | a full Versus match on the sim (`versus::new_match`): tower card .. match results |
| `--play --mode sandbox` | respawning archers, no match logic |
| `--play --mode quest [--tower ID\|NAME] [--hardcore] [--players 1-4] [--quest-end complete@N\|gameover@N] [--shots DIR]` | a Quest tower played by bots (`coop_ui::scene::CoopPlay`) |
| `--play --mode trials --tower X --level TIER [--shots DIR]` | a Trials level played by a bot |
| `--play --mode darkworld [--tower ID\|NAME] [--players 1-4] [--difficulty 0-2] [--variants LIST] [--dw-level N] [--dw-assist N] [--quest-end complete@N\|gameover@N] [--shots DIR]` | a Dark World session played by bots (`coop_ui::scene::CoopPlay`, art and HUD in `coop_ui/darkworld/`) |
| `--demo` | the platform demo |

* `tower.rs`: `TowerGfx` (main + bg atlases on the GPU, `tfp_assets::look`
  assets) and `TowerView` (one `LevelLook`): tile layers pre-rendered once
  into canvases (tiles copied opaquely, stamps blended); per frame the light
  canvas (theme clear colour, tile/decoration lights + extra lights, the
  `light` cone pulsing with the 240-frame sine), the foreground canvas (BG
  tiles, layer −3 decorations, layer 0 decorations + solid tiles at −150 +
  sim drawables merged by depth, the theme foreground), then the screen
  (background elements, subtractive lighting composite). Animated: scroll,
  wavy, sprite animations, fades, ghost ship, banners; lightning flashes
  and node flights are static for now.
* `play.rs`: `--play` builds `tfp_sim::Content` for the one level
  (`loader::content_from_install`), a sandbox `MatchSetup` (archer i per
  slot, respawn after 90 frames), maps devices to `PlayerInput` per
  input.md 2 (pads: d-pad else raw left stick, A jump, X shoot, B alt shoot,
  shoulders/triggers dodge, Y arrows, Start; keyboard layouts: right/down
  win, Tab dodges too; the sim applies the 0.5/0.8/0.3 thresholds and the
  8-way snap), slots without a device are bots (deterministic) or follow a
  script, steps the `World` once per tick, draws the look + `View`
  drawables + a radius-40 light per archer, plays `Event::Sound` /
  `CharSound` (voice set by archer SFX id, `_NN` variants, fallback sets) /
  `Loop`, deduped by (frame, index), and shakes on `ScreenShake`.

### Versus presentation (`crates/game/src/versus_ui/`)

Reads the sim's `View`, `HudView` (`World::versus.hud`) and events only;
everything is local and cosmetic.

* **Tower card / intro** (versus.md 5.3): map block + theme icon zoom, title,
  mode in its colour, variant icon strip (Unlisted excluded, tournament icon);
  ROUND n / OVERTIME, intro arrows (slide, spin, pulse), FIGHT!! (scale 8 -> 2),
  team banners (tinted for teams 3-6), exits.
* **HUD** (12): P-tags for 180 ticks from the intro (colour A/B every 4
  ticks, bob, indicator, crown, wrap copies, staggered when crowded), death
  skulls (13 frames, rise, wiggle, fade; red for suicides).
* **Particles** (rendering.md 9): the type catalogue as data (gameplay and
  Dark World types; editor types left out), per-archer palettes, a
  1500-slot ring buffer with the original update order, drawn inside the
  darkened gameplay layer. Sim `ParticleKind`s map by name (`sim_kind`);
  unknown future kinds use a catalogue type of the same name, so new sim
  events need only a table entry. `DeathSkull` and `LightFade` become HUD
  skulls / fading corpse lights.
* **Final kill:** spotlight (darkness rises to 1 at 0.05/tick, only the
  victor and the victim's death spot lit). **Shake:** `ScreenShake` -> +-2 px
  wrapped present offset. **Lightning** (rendering.md 6.4): the look's
  `LightningFlash` state machine in `TowerView` (sky fill, darkness x1.5,
  `ENV_LIGHTNING`). **Screen flash:** `VersusUi::flash` ready; the sim emits
  no flash events yet.
* **Instant replay** (10.2, our state-ring design): every unfrozen tick of a
  round is a `World` clone in a 210-frame ring (slow motion stores every
  other tick so a frame is 1/60 s of game time) with its sound events; at
  the results the viewer rewinds (to -8x, decelerating under frame 60),
  pauses 5 ticks, plays forward (ramp to 1x, sounds replayed), rendering the
  stored states with the screen wave (frequency 15), scan lines and the
  rewind/play icons. Start skips; holding Alt (pad RB/RT, keyboard Dodge)
  plays forward at half speed (prompts SLOW / SKIP with the first human
  device's icons). On the finished round results Alt watches it again
  (prompt REPLAY; the results come back finished). SAVE REPLAY (GIF) is
  not done.
* **Round results** (10.3, 15.6): coins / skulls with the gain/lose pulse,
  overflow, crowns scaling in/out, sounds, rows per the original (<= 4 FFA
  at 320) or compacted for more; team rows list members leftward.
* **Match results** (10.4, 15.6): panels for 2-12 (Steam, 8p tables, two
  rows of 6 beyond 8), winner background, gem, win/lose portrait, stat lines
  sliding in, up to 3 awards (icon + name; none on small panels), winner
  banner.
* **Gating:** the presentation keeps its own results clocks (they start
  after the replay); Confirm/Start reach the sim only when the local
  sequence is done; bots never confirm; with no human device the presenter
  confirms by itself (headless proof).
* **Archer voices:** `tfp_sim::content::ArcherDef::voice` (added to the sim:
  the archer data's SFX id).
* **Music** (`crates/game/src/music.rs`, audio.md 6-7): `MusicPlayer` opens
  `Content/Music/Win/MusicSoundBank.xsb` + `MusicWaveBank.xwb` and streams a
  cue's wave from its MS-ADPCM bytes (block by block, ~11 MB per song
  instead of ~85 MB decoded). `play` (ignored if current, 1 s crossfade),
  `play_immediate` (hard cut), `stop` (1 s fade). Once/loop per the cue
  table; victory cues mix the jingle with a looping follow-up (TheArchives
  or TheTale, 50/50) starting at the table's timestamp. `VersusUi` queues a
  `MusicCue`: intro t = 70 -> theme cue from `themeData` (`Boss`, or
  `DarkBoss` for `World = Dark` themes, in overtime); match-deciding final
  kill -> stop; cancelled -> theme again; round results t = 10 (after the
  replay) -> `"Victory" + VictoryMusic` of the winner (`VictoryTeam` in
  TDM) if decided, else stop when the next round is overtime. The theme
  keeps playing across rounds (same cue). Menu music: a tower confirm stops
  Title; it comes back when the menus leave the map / level screens.
  `--no-music` turns both off. Check: `--headless --play --wav out.wav`
  prints `music: ...` lines; RMS of the WAV shows continuous music.

### Local 5-12 players (M6)

* **Wide towers:** with more than 4 joined players (or `--eight-player`)
  the match uses the 8-player build's install (`$TF8_GAME_DIR`): its
  Versus towers are the same 16 in the same order, 420x240. The UI keeps
  the Steam menu atlas; the 8p main atlas is loaded for the level when the
  installs differ. Without an 8p install the Steam 320 towers are used.
* **HUD at 12:** P-tags (scale 2, archer colours, staggered when close)
  show for 180 ticks from the intro proper (round 0: after the tower card);
  round results compact to 12 rows; match results in two rows of 6.
* **In-match pause** (`match_pause.rs`, ui.md 10): Start from any device
  in the intro / round (after 15 frames) opens PAUSED (RESUME / QUIT), sim
  frozen, music at half volume, `ui_pause` / `ui_unpause`. QUIT -> versus
  settings. `PauseMenu::step` is the Ctx-free core of the front end's menu.
* **Controllers mid-match** (input.md 5, 10.2): the slots keep their
  devices on any hot-plug (no reshuffle). A human's pad going missing idles
  its archer and pauses with the "CONTROLLER (Pn) DISCONNECTED!" panel
  (device icon + `controls/disconected`, colour B) until a pad is back or
  240 frames pass, then the menu. A newly connected pad takes a slot whose
  pad is missing (same model name first, else the lowest slot); SDL gives
  no stable GUID here, so "the same pad" is approximated by its name.
  Other new pads only drive menus. Test: `--headless --play --players 6
  --virtual-pads 1 --unplug-at 400`.
* **Loading hitches removed:** sound effects are preloaded on a background
  thread (all 14 voice sets, the catalogue's `char_`/`sfx_`/`pu_`/`env_`
  effects, the presentation cues; ~100 MB as f32, the original loads all
  effects at start-up), with synchronous loading as the fallback; music
  waves of the match (theme, boss, jingles, follow-ups) are prefetched the
  same way. Sim sound names are catalogue names resolved through
  `sfx_table()` (array members by index, `char_fireLaserDeflect3`); voice
  loops (`WALLSLIDE_LOOP`) come from the archer's voice set.
* **Performance** (`--frame-times`: every frame waits for the GPU; prints
  percentiles and the frames over 16.7 ms). 12 bots, Sacred Ground 420x240,
  epic HH, 7200 frames, release, Intel Iris Xe (Vulkan), machine load ~20
  from other jobs: tick mean 0.085 ms (p99 0.21); draw CPU mean 0.43 ms;
  total mean 2.03 ms, p50 1.87, p95 4.10, p99 7.23, 5 frames > 16.7 ms
  (GPU/system spikes, tick < 1 ms); ~475 fps uncapped.

### Records (M11 save data, `crates/game/src/save.rs`)

`$TFP_DATA_DIR/records.json` (pretty JSON, serde; unknown or missing
fields load as defaults; written temp-file + rename after every change,
only in windowed runs: headless runs, tests and the soak keep it in
memory). Content: Versus lifetime counters (time, matches, rounds, kills /
deaths with team/self/environment, per archer and per cause, wins and the
70 award counters per archer, arrows shot/collected/caught, treasures,
dodges, jumps, tower plays, random plays, launches), Quest per tower
(revealed, normal / hardcore / no-deaths, best 1P / 2P+, deaths,
attempts), Dark World per tower (revealed, normal / hardcore / legendary,
eye / gold eye, best Legendary time and most curses per player count
1-4, deaths, attempts), Trials per level (attempts, gold / diamond / dev,
best), co-op crowns; plus session statistics in memory. Times are sim
ticks. On the first start the original's `tf_saveData` records are added
once (read only, .NET ticks converted; trials mapped tower x tier ->
our level index); `imported_from` stops repeats, `--import-save` forces.

API (one global store, safe to call from any scene):

* `save::record_versus(&HudView, archer_base, tower, random, round_ticks)`
  (Play does it when the match results show);
* `save::record_dark_world(&DwResult)` (for the Dark World screens);
* `save::record_quest(tower, hardcore, players, cleared, no_deaths, time,
  deaths)`, `save::record_trials(level, time, unlocked)`;
* `save::coop_records()` / `save::store_coop(&Records)` (the co-op scene's
  records come from and go back to the store, `game.rs`);
* `save::with(|d| ...)` reads or changes anything (then saves),
  `save::snapshot()` copies records + session.

ARCHIVES (`front/archives.rs`, ui.md 12.1): STATS, SESSION, DEATHS,
AWARDS (scroll), QUEST, DARK WORLD, TRIALS from the store; the original's
paging controller, our text layouts; GLOBAL / LESSONS / ASCENSION left
out. `TFP_ARCHIVES_PAGE=n --screen archives` for screenshots.

### M5 presentation (treasure, arrows, variants)

* **Env events** (`tfp_sim::entities::EnvEvent`): named particles go to the
  catalogue (torches, crumbling blocks ...), `Explosion*` to the explosion
  effect, `BombParticle:<type>` to debris (tower-entities.md 6.11); env
  sounds play with their volume; entity loops start / change volume / stop
  per owner and stop at a new round. `Event::SoundIndexed` plays the n-th
  member of an array effect. Online and co-op scenes got the same hooks.
* **Effects** (`versus_ui/effects.rs`): explosion lattices (9x9 / the
  super cross, cells by Manhattan distance, Core/Kill/Push and the green
  variants, cells in solids left out), shock circle, small shock, debris
  with bounce/friction on the level grid. The owner calls
  `effects.resolve(level)` once per tick after `after_step`.
* **Marks** (`versus_ui/marks.rs`, in the darkened layer): quivers
  (arrows.md 14), Show Treasure Spawns contours, cooldown stars
  (`StunStars`). **Gunn Style**: a game-size canvas cleared opaque black,
  `gunnStyle` / `gunnStyleB` (+ wrap copies) punched out with the new
  `Blend::Erase` (`dst * (1 - src.a)`), composited at the view's alpha (the
  sim gives no death angle yet, so the corpse cut-out is unrotated).
  **Lights**: the sim's entity lights join the archers' in the light pass
  (not while the final-kill spotlight runs); the dark orb turns the level's
  static lights off and the darkness to full.
* **Treasure Draft UI** and bots picking (ui-notes.md). Showcase:
  `--play --treasure all --variants "MAX TREASURE,GUNN STYLE,START WITH BOMB
  ARROWS@1,..." --shots DIR --shots-every N` captures every N frames plus a
  few frames after chest / explosion / bramble / prism / drill / laser /
  orb / Gunn events (`scratch/m5-sheet.png`). No view item falls back to a
  placeholder rectangle in such runs.

### Archer, corpse and arrow cosmetics (`versus_ui/actors.rs`)

Game-side layers over the sim's view, from the World state and the game
data (no sim changes were needed: the art is registered in the game's own
`Content` with `tex` / `add_sprite`): hair (player.md 3.5, White archer,
hidden under the normal hat, kept on the corpse), the shield's 7-frame
animation with the sine scale and the 30-frame gain tween (16.1), wing
poses idle / flap / glide / fast fall with their scales, the 40-frame
BigBackOut gain tween and the duck offset (16.2), Blue/Red team head,
corpse, aimer and dropped-hat textures, cold breath puffs in cold towers
(19.7), the purple ambience (19.6), the Yellow archer's sleep (ZZZ every 60
frames and the `CHAR_x_SLEEP_LOOP` snore loop), death skull types Cursed /
Suicide / Normal, the corpse revive flash (the corpse `Flash` texture every 4
frames) and levitation, and for arrows: stick wiggle, moonglass sparkles,
burning tails, the strip frames of bomb / super bomb / bolt / drill / prism
/ trigger arrows (the sim drew whole strips) and the trigger eye
(arrows.md 14b). The P-indicator is the presentation's P-tag (19.8: 180
frames from the intro proper, bob, A/B colours, crown). Every match scene installs it
(`actors::install` / `build_art` + `attach`; online registers the art in its
copy of the content with `Arc::make_mut`). Showcase
`scratch/archer-sheet.png`.

### One drawing path for matches (`crates/game/src/match_draw.rs`)

Local play, online Versus, co-op (Quest / Trials / Dark World) and online
co-op draw the level of a match through `match_draw::draw_level`: view
build, the scene's decorations (enemy / boss / debris art via a closure
that also returns extra light sources), the archer cosmetics, lights
(archers + extras + the sim's entity lights unless the final-kill
spotlight runs), the dark orb, marks / effects / particles in the darkened
layer and the Gunn overlay (`GunnCanvas` kept by the scene). Per tick
every scene calls `VersusUi::after_step` then `post_step(w, content)`
(explosions resolved, the theme's crack colour and cold flag, actors
updated). Each scene keeps its HUD, menus and shake (online: HUD from the
confirmed timeline, level from the predicted world, docs/net.md).
Checked with `scripts/net/scene-pair.sh --mid 1800` (online guest frame
next to a local one: `scratch/local-vs-online.png`) and headless Quest /
Dark World runs (`scratch/coop-pair.png`).

### First run, packaging, soak

* **Finding the game** (`tfp_assets::install`): `TF_GAME_DIR` /
  `TF8_GAME_DIR`, then the player's folders (`options.cfg` `game_dir`,
  `--game-dir`, a folder dropped on the window; the 8-player build is
  recognised by its 420 levels), then Steam libraries (`libraryfolders.vdf`
  under ~/.steam, ~/.local/share/Steam, Flatpak, macOS, and on Windows the
  registry via `reg query`) and the usual 8-player places. Without data the
  `Boot` scene (`boot.rs`) explains what is needed and where it looked,
  re-checks every 2 s and starts the menus when data appears.
* **`scripts/package.sh`**: `dist/towerfall-port-<ver>-linux-x86_64.tar.gz`
  (binary with RUNPATH `$ORIGIN/lib` + `lib/libSDL2-2.0.so.0`) and
  `-windows-x86_64.zip` (exe + SDL2.dll), both with README (from
  `packaging/README-player.md`), licences and `THIRD-PARTY/` (SDL2 licence,
  crate list); stripped builds in `target/package`; an allowlist + game
  file-type check of the archive contents; `dist/SHA256SUMS` (the names
  the Warpzone descriptor expects). Nothing is uploaded.
* **Soak** (`soak.rs`, `--soak N [--soak-seed S --soak-from I --soak-draw K
  --soak-frames F --soak-report FILE]`): random bot matches back to back
  (both installs incl. 320 towers for up to 12, modes, 2-12 players, team
  splits, lengths, random variant sets and treasure masks, replays on/off),
  panics caught per match with backtrace, a watchdog for hung ticks, stalls
  reported with the match state, GPU texture leak check. Two runs of 220
  (seeds 1, 2): 440 matches, 0 panics, 0 hangs, 0 errors; 20 "stalls" were
  Headhunters/TDM matches still progressing after 120000 frames (bots rarely
  score with Sudden Death / lava / self-kill variants); found and fixed: the
  Treasure Draft waited forever for bots, and every finished match leaked its
  GPU textures (`Play::release`).

### Options (M11)

`options_store.rs` (format and rules in `docs/ui-notes.md`): options,
keyboard layouts and controller maps live in `$TFP_DATA_DIR/options.cfg`
(windowed runs; written on change by the front end); the first run imports
the original's `tf_saveData` read only (`tfp_assets::savedata`). The match
takes the effects volume and the controller maps from the options
(`PlayOptions::sfx_volume`, `pads`).

## Fidelity sweep (game crate)

An audit of `docs/spec/ui.md`, `input.md`, `audio.md`, `rendering.md` and the
presentation parts of `versus.md` against `crates/game` / `crates/render`.

**Fixed**

- Input: the keyboard configurations reach Versus, co-op and online
  (keyboard players 2-4 crashed Versus before); controller rumble
  (`rumble.rs`: per pad, stronger replaces / equal extends / weaker ignored;
  sim `Event::Rumble` in Versus, co-op and online for the local pad, menu
  join 1.0/20, leave 0.5/10, invalid 1.0/20, the tower card 1.0/30 on every
  pad; off with GAMEPAD RUMBLE); HOLD TO PAUSE (Start held 20 frames);
  losing the window focus pauses a match; the 15-frame pause lockout counts
  from every round's intro; online uses the controller remaps; a device
  joining the rollcall again prefers its earlier slot.
- Audio (`sfx.rs`, one player per match scene): Varied effects pick at
  random and never repeat back to back (also the rollcall READY voices);
  Instanced effects own their ring (`sfx_finalKill` cuts itself); the master
  pitch follows `TimeRate` for effects that obey it and retunes the live
  instanced / looped voices; pause holds the instanced and looped voices
  (mixer `SetPaused`), co-op pause halves the music; the bomb chest's
  `EntityLoop` plays; every loop stops at a new level; player loops follow
  the archer's pan; online plays at the Sounds volume; replays play every
  recorded sound kind (indexed, voices, environment). Menu music goes
  through `MusicPlayer` (`AltTitle`, 1 % sticky `ChipTitle`, `TheArchives`,
  "from archives" rule); leaving a match stops the effects and fades the
  music over 1 s, so the menu song crossfades in; MMG plays at the music
  volume; Sounds 0 plays nothing; defaults 8 / 8. Per-call volume is 1;
  the level calibration stays in `MusicPlayer::volume` (0.8) and the bus
  volume = option / 10.
- Rendering: the sim's camera offset (space orb, Offset World, Always
  Scrolling) is applied in `match_draw::draw_prepared` (level canvas drawn
  back wrapped, as co-op did); the HUD and pause menu are drawn
  counter-shaken (`match_draw::steady`); the time orb / SlowTime wave (not
  the final kill's) behind the SCREEN WAVE option; the round results'
  HUDFade (0 -> 0.8 over 40 ticks); the final-kill spotlight lights the
  victor's side by score index (TDM: the team) and the victim's corpse;
  archers carry one light (the sim's 50; rendering.md 7.5 fixed); the dark
  orb eases its darkness 0.02 / frame and keeps every light; tower card:
  overlay fades in after the wait and out with the 30-tick slide from the
  confirm (`HudView::card_confirmed`, additive sim field), random-looking
  rotation sign per tower, stone sound + 20-tick shake at t = 10.
- UI: variant boxes (selected D8F878 on / 3CBCFC off, unselected only when
  on); logo letters / DARK WORLD words have their own tween and show on
  press-start only; the main menu selects what was chosen last; versus
  settings return to VARIANTS from the variants; the press-start hint
  follows the first device; no `ui_swipeIn` into the level screens;
  ARCHIVES has its title; layouts at 4 / 8 / 12 checked in
  `scratch/menus-sheet.png` (main-menu ONLINE blade, 420 CO-OP, team select
  with 3-6 teams and 12 slots, ready-screen names, no guides on the DW map).

**Second round (from the remaining list)**

- Particles run as the level's three systems (`particles::layer_of`, the
  split in rendering.md 4.3): BG over the back layer, the normal ones at
  depth -80 and FG at -490 inside the gameplay layer's depth sort;
  explosions / shocks at -40 (`tower::InLevel`, `IN_LEVEL_DEPTHS`); the
  quivers and markers stay on top.
- Torches: with the sim's view, the look's static light of a sim-owned
  torch is skipped (the sim's `Tower::Torch` lights it while lit), so a
  put-out torch goes dark.
- Lightning: the flash fill is drawn at its `LightningFlash` element's
  place among the background layers; each phase lasts exactly N frames.
- Coloured lights (rendering.md 7.5, `match_draw::entity_lights`): ghosts
  `inv(2277FF)` / green ghosts `inv(B1FF9E)`, ghost platforms
  `inv(C1FFF3)` (40), jump pads `inv(95F07F)` (40, alpha = the pad's
  light ramp); drawn through `TowerView::dyn_lights` with the static ones.
- Loops (`sfx::Loops`, Versus and co-op): archer loops (wall slide, dark
  orb crickets), entity loops (`EnvEvent::Loop`) and the bomb chest's
  `EntityLoop`; their pan follows the archer / owner every tick, loops of
  removed entities stop, a new level stops them all; co-op plays them now.
- Replays: each recorded frame keeps its presentation (`FxSnap`: live
  particles, explosions, skulls, the light set incl. the spotlight, the
  spotlight darkness, the shake); the replay draws those.
- Badges for players 5-12: the 1-4 art dimmed with the number over it
  (`Painter::badge`; input.md 10.5).
- Keyboard config: Confirm on an existing config opens CLEAR / RESET TO
  DEFAULT (left) vs CONFIGURE (right), confirmed again (input.md 4).
- ARCHIVES TRIALS: "GEMS n/7" in the header once a gem was found.

**Third round**

- End-to-end flow test (`flow.rs`, `scripts/dev/flow-test.sh`,
  `--flow-test DIR [--only 4|8|12]`): the whole app headless with two
  virtual pads: title -> VERSUS (rollcall with 2 joined, settings,
  variants, map, a full match played by bots on the joined slots via
  `PlayOptions::bots_for_humans`, round and match results, match-end QUIT)
  at 4, 8 and 12 archers; CO-OP -> QUEST and DARK WORLD (rollcall, map,
  a level, pause QUIT); TRIALS (the same); ARCHIVES (next page); OPTIONS
  (MUSIC changes). Every step waits for its screen / phase and saves a
  frame (`scratch/flow/`, sheet `scratch/flow-sheet.png`, list
  `steps.tsv`); a step not reached or a panic fails the run (exit 1).
- More lights (`match_draw::entity_lights`): evil / blue / bolt / prism
  crystals (`inv(FF0000 / 00C4C7 / 7AFF99 / FFBDF0)`, 40), floor miasma
  (`inv(9ED1FF)`, 40, the sim's light ramp), the lava's lit region (body
  plus 0.8-0.2 bands, rendering.md 7.5; `TowerView::dyn_rects`), boss parts
  (exit portal 80, Dreadwood eyes 100, tentacles 40, Darkfang fists 50,
  bodies: Amaranth 90, Darkfang 50, Cataclysm 120); ghost lights fade out
  over 20 frames after the ghost is gone. The White Archer prison is
  always broken (everything is unlocked), so it has no light.
- Death skulls and P-tags are drawn in the gameplay layer at depth -1000
  (`VersusUi::draw_world_marks`, `LevelDraw::hud`): the darkness covers
  them and they move with the world.
- Particles: the level's three systems have their own rings (BG 600,
  normal 500, FG 400; `Particles::level`).
- Bots: Versus, online and co-op use the sim's shared `tfp_sim::bot::Bot`
  (one per bot slot, seeded with the slot; read before each step); the
  game crate's own bots and the Dark World exit walk are gone. The
  Treasure Draft pick keeps its small presentation-side chooser.

**Remaining (smaller, or larger work)**

- Rendering: the replay wave still bends the HUD (the HUD is in the game
  canvas, kept there so native screenshots include it); animated light
  alphas of the bosses and intros; `AscensionLightning`; world-tinted
  particle colours; Vortex rings static; HUD wrap copies of quivers;
  Gunn cut-out not rotated; tower-card tint per tower type; results layout
  details (8p FFA rows, TDM tag sizes, REVIVES line, 9-12 panel scale).
- Input: re-plugged pads match by name, not GUID (written into input.md
  10.2 as ours); empty keyboard configs renumber; SNES / generic pad
  glyphs; rollcall Back from unbound devices with 4 pads; a held opposite
  direction suppresses the first press too.
- Audio: x = 0 still means "centre" (no `Option` in the sim's events);
  `TheTale` (credits / workshop are not implemented).
- UI: map scroll mode (return on release, edge arrows), tower icon tints
  and the DARK WORLD plaque, seed-panel animation; the variants button's
  marquee; rollcall ready-banner rise, alt-costume hint, background
  section 2 for co-op modes; option arrows dimmed at limits; blade tint
  speeds; main-mode-button blink / sink / icon scale; vsync dialog slide.
- Spec conflicts left to the sim owner: the final kill's slow motion
  (rendering.md 8.4 "snaps back" vs versus.md 6.7 "+0.01 per tick"; the
  sim does both: approach, then snap) and Gunn Style "after a final kill"
  (rendering.md 8.5) vs "every death" (versus.md 13.2).

## What is left

- The rest of `docs/spec/rendering.md` is game-side and uses this API:
  layer order and depth sorting, `CoreRender`'s 2 px black bars, camera
  matrix (`-(int)pos`, zoom, `+(int)origin` → `set_state`), backgrounds,
  particles, flashes (overlay rects), kill-freeze overlay (`Subtract`),
  archer colours.
- Windowed mode was run on the private Xvfb only (llvmpipe, dummy audio):
  verify on a real desktop: vsync pacing at 60/144 Hz, fullscreen toggle,
  resize, real audio output, physical controller hot-plug.
- Audio: better resampling; device hot-swap.
- Windows: `SDL_AUDIODRIVER=dummy` is passed but SDL still reports WASAPI for
  `--list-devices`; a windowed Wine run (Xvfb) is untested; DX12 backend under
  vkd3d-proton; strip/package.

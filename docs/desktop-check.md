# Desktop check (windowed mode)

So far everything ran headless or on a private Xvfb. Please try towerfall-port
on a real desktop and report back. About 20 minutes; controllers help but a
keyboard is enough for most of it.

## Setup

* From a release archive (`scripts/package.sh` output in `dist/`): unpack,
  run `./towerfall-port` (Linux) or `towerfall-port.exe` (Windows).
* Or from the repo: `source env.sh && cargo run --release -p towerfall-port`.
* Start it from a terminal with `RUST_LOG=info` (Windows: `set RUST_LOG=info`
  first) and keep the terminal output; it lists the GPU, the audio device, the
  controllers and where the options came from.

## What to try

1. **Start-up.** Window opens at 960×720 (3× the game) without flicker; the
   intro and title appear; the title music plays. Does it find your
   TowerFall by itself? (Look for the `options:` and `front end:` lines.)
2. **Pacing.** Move around the menus and watch the background stars / torches:
   smooth, no stutter, no tearing? Note your monitor's refresh rate
   (60 / 120 / 144 Hz). If you can, also try `--no-vsync`.
3. **Fullscreen.** F11 and Alt+Enter toggle borderless fullscreen and back;
   the image stays sharp and centred (black bars at the sides on 16:9);
   *Options → Fullscreen* does the same. Does the window come back at its
   old size and position?
4. **Resize.** Drag the window to odd sizes (tiny, very wide, very tall): the
   game keeps its aspect ratio with bars; nothing is stretched or cut.
   *Options → Video scale* switches fit / integer / stretch. (Known: F9 cycles
   the scale mode only outside the menus.)
5. **Audio.** Menu sounds, title music; in a match: tower music, arrows,
   deaths, archer voices, the victory jingle. Crackles or drop-outs? Change
   *Music* / *Sounds* volume in Options; does it apply at once?
6. **Controllers.** Plug in one or more pads before and after starting: do
   they show up in the archer select with the right name and button icons
   (Xbox / PlayStation)? Unplug one **during a match**: the game should pause
   with "CONTROLLER (Pn) DISCONNECTED!", and plugging it back in should let
   that player continue (any newly connected pad takes the missing slot). Rumble
   on joining in the archer select, on deaths and on the tower card (off with
   *Gamepad rumble*)? Try *Options → Control options → Configure controller*
   and one remapped button in a match; also *Hold to pause* (Start held for a
   third of a second).
7. **Keyboard.** Default keys (arrows, C jump, X shoot, Shift dodge, Enter
   start); *Configure keyboard* (Confirm on an existing keyboard player offers
   CLEAR / RESET TO DEFAULT or CONFIGURE); a second keyboard player in a
   match.
8. **A full match.** 2–4 players (empty slots stay empty: use pads/keyboards),
   any tower: tower card, rounds, instant replay (hold RB/Shift for slow
   motion, Start skips), round results, match results with awards, then the
   end menu (rematch / settings / archer select).
9. **Settings kept.** Change a few options, quit, start again: are they
   still set? (File: `~/.local/share/towerfall-port/options.cfg` or
   `%APPDATA%\towerfall-port\options.cfg`.)
10. **Focus.** Alt+Tab away and back during a match and in menus: keys do not
    stay stuck, and a running match pauses when the window loses focus
    (controllers keep working in the menus while unfocused).
11. **Co-op and online.** CO-OP → Quest and Dark World, TRIALS, a level each;
    ONLINE needs a Warpzone account and a second machine or instance.

## What to report

* OS and desktop (e.g. Ubuntu 24.04 GNOME Wayland / Windows 11), GPU and
  driver, monitor refresh rate, controllers used.
* For each numbered item: OK, or what happened (a phone video is ideal for
  stutter or tearing).
* The terminal output (especially any `[WARN ]` / `[ERROR]` lines) and, after
  a crash, the last 50 lines.
* Anything that looks or sounds different from the original game.

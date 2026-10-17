# towerfall-port — rules for agents

A clean-room Rust reimplementation of TowerFall Ascension (Matt Makes Games,
2013–2015; C#/XNA on the Monocle engine, shipped on FNA) for Linux and Windows
(macOS later): up to 12 players, local on one screen with controllers, online
over Warpzone, or both mixed. Read `docs/ARCHITECTURE.md` and `docs/STATUS.md`
first; `docs/STATUS.md` is the single source of truth for what is done and
what is next.

## Hard rules

- **No writes outside this directory.** Every tool and cache lives in `./tools`
  (`source env.sh` sets RUSTUP_HOME, CARGO_HOME, DOTNET, pip, XDG cache,
  WINEPREFIX, WARPZONE_HOME, the port's data dir). No system packages, no
  `~/.cargo`, no global config, no `/tmp` for anything that matters
  (use `scratch/`). Other projects (e.g. `~/Projects/rounds`) may be *read*
  and code we own may be copied from them, never written to.
- **The game installs are read-only**: `$TF_GAME_DIR` (Steam, Ascension + Dark
  World) and `$TF8_GAME_DIR` (the official 8-player build). Never write into
  them, never run the original game (it writes saves and talks to Steam).
- **Nothing from the game goes into git**: no assets, no decompiled code, no
  extracted data, no screenshots containing game art in committed files. The
  repo will be public open source; the game's data is read from the player's
  own copy at run time.
- **Clean room.** `ref/decomp/steam` and `ref/decomp/8p` (ILSpy output) are
  references only, git-ignored. Behaviour is described in our own words in
  `docs/spec/*.md` (formulas, constants, frame timings and state machines are
  fine; code is not). Rust code is written from the specs; never transliterate
  C# from `ref/decomp`. If a spec is ambiguous, consult the decompilation, then
  fix the spec.
- **The specs are private.** `docs/spec/` is its own git checkout of the
  private repository `jarvis-jhk/towerfall-port-spec` (with the reference
  oracle under `docs/spec/oracle/`) and is ignored here; commit spec changes
  there. Never publish, quote or paste them (this repo, issues, PRs, commit
  messages, release notes); code comments may cite a section
  (`docs/spec/versus.md §15.5`), not quote it.
- **Agents do not run git** unless the orchestrator says so; the orchestrator
  commits.
- CPU-heavy work (> ~30 s: release builds, test suites, long headless runs):
  wrap it in the machine's performance-mode helper when it has one.
- Stop only processes you started, by the PID you recorded when starting
  them. Never `pkill`/`killall`/`kill $(pgrep ...)` by name or pattern:
  other projects on this machine run the same binaries (e.g.
  `warpzone-server`).
- Headless only: no windows on the user's desktop. Use `--headless` for
  screenshots, or the private Xvfb (`scripts/dev/xvfb.sh`, display :64);
  put output in `scratch/` (git-ignored).

## Layout

| Path | What |
|---|---|
| `crates/assets` | `tfp-assets`: readers for atlases (PNG+XML), sprite/game data XML, Ogmo `.oel` levels, Workshop `.tower`, WAV, XACT (xwb/xsb), XNB fonts |
| `crates/sim` | `tfp-sim`: deterministic, rollback-able simulation (no I/O, no rendering) |
| `crates/render` | `tfp-render`: wgpu pixel-art renderer, offscreen/headless capable |
| `crates/net` | `tfp-net`: GGRS rollback over Warpzone peer channels |
| `crates/game` | `towerfall-port` binary: SDL2 platform layer, menus, app flow |
| `docs/spec/` | clean-room behaviour specs written from the reference (private checkout, see above; `oracle/` = reference-comparison harnesses) |
| `scripts/` | bootstrap, Windows build, dev scripts |
| `packaging/warpzone/` | our Warpzone game descriptor |
| `ext/` | re-clonable third-party checkouts (warpzone, rea), git-ignored, read only |
| `ref/`, `cache/`, `scratch/`, `tools/`, `target/` | git-ignored |

## Conventions

- `source env.sh` before cargo/python/dotnet. Python tools: `python -I`.
- Rust 2024 edition, `cargo fmt`, `cargo clippy` clean, unit tests next to the
  code. The sim runs at a fixed 60 Hz (the original's 1/60 s with
  `TimeMult` = 1); determinism-sensitive code uses `libm` for transcendental
  math, no `HashMap` iteration in logic, no wall-clock time, no threads, all
  randomness from a seeded PRNG in the state.
- Keep `docs/STATUS.md` current when finishing a step.

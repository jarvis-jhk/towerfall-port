//! End-to-end app-flow test (`--flow-test DIR`, `scripts/dev/flow-test.sh`):
//! drives the whole app (`Game`: menus, Versus, co-op) headless with SDL-like
//! virtual pads through every top-level path and checks that each screen is
//! reached, without panics. One frame per step goes to `DIR/NN-label.png`
//! (the script makes the sheet). Paths, at 4 archers unless said:
//!
//! * title -> VERSUS: rollcall (2 join) -> settings -> variants -> map -> a
//!   match (bots play the joined slots) -> round results -> match results ->
//!   match-end menu QUIT -> main menu; the same at 8 and 12 archers;
//! * CO-OP -> QUEST: rollcall -> map -> a level -> pause QUIT -> main;
//! * CO-OP -> DARK WORLD (with the expansion): the same;
//! * TRIALS: rollcall -> map -> a level -> pause QUIT -> main;
//! * ARCHIVES: next page -> back; OPTIONS: change MUSIC -> back.
//!
//! Exit status 0 when every step was reached, 1 otherwise; panics are
//! caught and reported with the step they happened in.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sdl2::controller::Button;
use tfp_render::{Gpu, Renderer, ScaleMode};
use tfp_sim::versus::Phase;

use crate::app::{Frame, HostRequests, LoadContext, Scene, TickContext};
use crate::audio::Audio;
use crate::front::{FrontEnd, FrontOptions, ScreenId};
use crate::game::{Game, Where};
use crate::input::{InputHub, RawInput};

struct Flow<'a> {
    gpu: &'a Gpu,
    renderer: &'a mut Renderer,
    audio: Audio,
    game: Game,
    hub: InputHub,
    raw: RawInput,
    host: HostRequests,
    frame: Frame,
    tick: u64,
    out: PathBuf,
    shot_n: &'a mut u32,
    run: String,
    /// Steps reached, in order: (label, file).
    log: &'a mut Vec<(String, String)>,
}

const PADS: [u32; 2] = [1, 2];

impl Flow<'_> {
    /// `n` ticks; the scene draws every tick in the menus (matches and
    /// co-op games are created in `draw`), every 8th in long waits.
    fn step(&mut self, n: u32, draw_every: u32) -> Result<()> {
        for _ in 0..n {
            self.hub.snapshot(&mut self.raw);
            self.game.tick(&mut TickContext {
                input: &self.raw,
                audio: &mut self.audio,
                host: &mut self.host,
                tick: self.tick,
            });
            self.host.rumble.clear();
            if self.host.quit {
                bail!("the app asked to quit");
            }
            if self.tick.is_multiple_of(draw_every.max(1) as u64) {
                self.frame.draw(
                    self.gpu,
                    self.renderer,
                    &mut self.game,
                    self.tick,
                    (960, 720),
                )?;
            }
            self.audio.advance_offline(1.0 / 60.0)?;
            self.tick += 1;
        }
        Ok(())
    }

    fn press(&mut self, pad: u32, b: Button) -> Result<()> {
        self.hub.pad_button(pad, b as usize, true);
        self.step(1, 1)?;
        self.hub.pad_button(pad, b as usize, false);
        self.step(3, 1)
    }

    fn presses(&mut self, pad: u32, b: Button, n: usize) -> Result<()> {
        for _ in 0..n {
            self.press(pad, b)?;
            self.step(4, 1)?;
        }
        Ok(())
    }

    fn at(&self) -> Where {
        self.game.whereabouts()
    }

    /// Steps until `want` holds (and a front-end screen has settled), then
    /// saves a frame labelled `label`.
    fn reach(&mut self, label: &str, max: u32, want: impl Fn(Where) -> bool) -> Result<()> {
        let mut t = 0;
        loop {
            let w = self.at();
            let settled = !matches!(w, Where::Front(_)) || self.game.front_settled();
            if want(w) && settled {
                break;
            }
            if t >= max {
                self.shot(&format!("{label} (NOT REACHED)"))?;
                bail!(
                    "{}: {label} not reached within {max} ticks (at {:?})",
                    self.run,
                    w
                );
            }
            let long = max > 5000;
            self.step(if long { 8 } else { 1 }, if long { 8 } else { 1 })?;
            t += if long { 8 } else { 1 };
        }
        // A few frames more so the picture shows the screen in place.
        self.step(6, 1)?;
        self.shot(label)
    }

    fn screen(&mut self, label: &str, id: ScreenId) -> Result<()> {
        self.reach(label, 900, |w| w == Where::Front(Some(id)))
    }

    fn shot(&mut self, label: &str) -> Result<()> {
        self.frame.draw(
            self.gpu,
            self.renderer,
            &mut self.game,
            self.tick,
            (960, 720),
        )?;
        *self.shot_n += 1;
        let file = format!(
            "{:02}-{}.png",
            self.shot_n,
            slug(&format!("{} {label}", self.run))
        );
        self.frame
            .capture(self.gpu, self.renderer)?
            .save_png(self.out.join(&file))?;
        println!("flow: {} / {label} at tick {}", self.run, self.tick);
        self.log.push((format!("{}: {label}", self.run), file));
        Ok(())
    }

    // ------------------------------------------------------------ paths

    fn title_to_main(&mut self) -> Result<()> {
        // The intro skips on Start; the title takes Start.
        for _ in 0..40 {
            if self.at() == Where::Front(Some(ScreenId::PressStart)) && self.game.front_settled() {
                break;
            }
            self.press(PADS[0], Button::Start)?;
            self.step(20, 1)?;
        }
        self.screen("title", ScreenId::PressStart)?;
        self.press(PADS[0], Button::Start)?;
        self.screen("main menu", ScreenId::Main)
    }

    /// Joins pads 1 and 2 in the rollcall (4 slots: pre-bound, one press;
    /// 8/12: the first press binds, the second joins).
    fn join_two(&mut self, wide: bool) -> Result<()> {
        for &p in &PADS {
            self.press(p, Button::A)?;
            self.step(6, 1)?;
            if wide {
                self.press(p, Button::A)?;
                self.step(6, 1)?;
            }
        }
        Ok(())
    }

    fn versus(&mut self, wide: bool) -> Result<()> {
        self.press(PADS[0], Button::A)?;
        self.screen("rollcall", ScreenId::Rollcall)?;
        self.join_two(wide)?;
        self.step(10, 1)?;
        self.shot("rollcall, 2 joined")?;
        self.press(PADS[0], Button::Start)?;
        self.screen("versus settings", ScreenId::VersusOptions)?;
        // BEGIN is selected: up to VARIANTS.
        self.press(PADS[0], Button::DPadUp)?;
        self.press(PADS[0], Button::A)?;
        self.screen("variants", ScreenId::Variants)?;
        self.press(PADS[0], Button::B)?;
        self.screen("versus settings again", ScreenId::VersusOptions)?;
        self.press(PADS[0], Button::DPadDown)?;
        self.press(PADS[0], Button::A)?;
        self.screen("tower map", ScreenId::Map)?;
        self.press(PADS[0], Button::A)?;
        self.reach("match playing", 3000, |w| {
            w == Where::Versus(Some(Phase::Playing))
        })?;
        self.step(240, 1)?;
        self.shot("match, 4 s in")?;
        self.reach("round results", 60_000, |w| {
            matches!(w, Where::Versus(Some(Phase::Results)))
        })?;
        self.reach("match results", 400_000, |w| {
            matches!(
                w,
                Where::Versus(Some(Phase::MatchResults | Phase::Finished))
            )
        })?;
        self.screen("match-end menu", ScreenId::MatchEnd)?;
        // REMATCH, MATCH SETTINGS, ARCHER SELECT, QUIT.
        self.presses(PADS[0], Button::DPadDown, 3)?;
        self.press(PADS[0], Button::A)?;
        self.screen("main menu after the match", ScreenId::Main)
    }

    /// One archer joins a co-op rollcall; Start if it does not go on alone.
    fn join_one(&mut self) -> Result<()> {
        self.press(PADS[0], Button::A)?;
        self.step(40, 1)?;
        if self.at() == Where::Front(Some(ScreenId::Rollcall)) {
            self.press(PADS[0], Button::Start)?;
        }
        Ok(())
    }

    /// Back from wherever to the main menu.
    fn back_to_main(&mut self, label: &str) -> Result<()> {
        for _ in 0..8 {
            if self.at() == Where::Front(Some(ScreenId::Main)) {
                break;
            }
            if self.game.front_settled() || !matches!(self.at(), Where::Front(_)) {
                self.press(PADS[0], Button::B)?;
            }
            self.step(30, 1)?;
        }
        self.screen(label, ScreenId::Main)
    }

    /// A co-op level from its map: start it, play a little, pause, QUIT.
    fn coop_level(&mut self, what: &str, confirms: usize) -> Result<()> {
        for _ in 0..confirms {
            self.press(PADS[0], Button::A)?;
            self.step(30, 1)?;
        }
        self.reach(&format!("{what} level"), 3000, |w| w == Where::Coop)?;
        // The controls overlay / title card wait for Confirm; then Start
        // pauses once the game runs.
        let mut paused = false;
        for _ in 0..12 {
            self.press(PADS[0], Button::A)?;
            self.step(60, 1)?;
            self.press(PADS[0], Button::Start)?;
            self.step(10, 1)?;
            if self.game.coop_paused() {
                paused = true;
                break;
            }
        }
        if !paused {
            self.shot(&format!("{what} pause (NOT REACHED)"))?;
            bail!("{}: the {what} pause menu did not open", self.run);
        }
        self.shot(&format!("{what} paused"))?;
        // QUIT is the last item.
        self.presses(PADS[0], Button::DPadDown, 5)?;
        self.step(12, 1)?;
        self.press(PADS[0], Button::A)?;
        self.reach(&format!("out of the {what} level"), 2000, |w| {
            matches!(w, Where::Front(_))
        })
    }

    fn coop(&mut self, dark_world: bool) -> Result<()> {
        let what = if dark_world { "dark world" } else { "quest" };
        self.press(PADS[0], Button::A)?;
        self.screen("co-op menu", ScreenId::CoOp)?;
        if dark_world {
            self.press(PADS[0], Button::DPadRight)?;
            self.step(6, 1)?;
        }
        self.press(PADS[0], Button::A)?;
        self.screen(&format!("{what} rollcall"), ScreenId::Rollcall)?;
        self.join_one()?;
        let map = if dark_world {
            ScreenId::DarkWorldMap
        } else {
            ScreenId::QuestMap
        };
        self.screen(&format!("{what} map"), map)?;
        // Tower, then the difficulty (Dark World: and the variants overlay).
        self.coop_level(what, if dark_world { 3 } else { 2 })?;
        self.back_to_main(&format!("main menu after {what}"))
    }

    fn trials(&mut self) -> Result<()> {
        self.press(PADS[0], Button::A)?;
        self.screen("trials rollcall", ScreenId::Rollcall)?;
        self.join_one()?;
        self.screen("trials map", ScreenId::TrialsMap)?;
        self.coop_level("trials", 1)?;
        self.back_to_main("main menu after trials")
    }

    fn archives(&mut self) -> Result<()> {
        self.press(PADS[0], Button::A)?;
        self.screen("archives", ScreenId::Archives)?;
        self.press(PADS[0], Button::DPadRight)?;
        self.step(30, 1)?;
        self.shot("archives, next page")?;
        self.press(PADS[0], Button::B)?;
        self.screen("main menu after archives", ScreenId::Main)
    }

    fn options(&mut self) -> Result<()> {
        self.press(PADS[0], Button::A)?;
        self.screen("options", ScreenId::Options)?;
        let before = self.game.front_options().music;
        // FULLSCREEN, then MUSIC: one step down.
        self.press(PADS[0], Button::DPadDown)?;
        self.press(PADS[0], Button::DPadLeft)?;
        self.step(10, 1)?;
        let after = self.game.front_options().music;
        self.shot(&format!("options, music {before} -> {after}"))?;
        if before == after {
            bail!("{}: MUSIC did not change ({before})", self.run);
        }
        self.press(PADS[0], Button::B)?;
        self.screen("main menu after options", ScreenId::Main)
    }
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// One app instance at `capacity` archers through `paths`.
fn run_app(
    gpu: &Gpu,
    renderer: &mut Renderer,
    out: &Path,
    shot_n: &mut u32,
    log: &mut Vec<(String, String)>,
    capacity: usize,
    paths: &[&str],
) -> Result<()> {
    let mut audio = Audio::offline(None)?;
    let front = FrontEnd::new(FrontOptions {
        capacity: Some(capacity),
        music: false,
        ..Default::default()
    });
    let mut game = Game::new(front, false);
    game.test_bots = true;
    game.load(&mut LoadContext {
        gpu,
        renderer,
        audio: &mut audio,
    })?;
    let mut hub = InputHub::new();
    for &p in &PADS {
        hub.connect(p, "Xbox 360 Controller");
    }
    let frame = Frame::new(gpu, renderer, (320, 240), ScaleMode::Fit);
    let mut f = Flow {
        gpu,
        renderer,
        audio,
        game,
        hub,
        raw: RawInput::default(),
        host: HostRequests::default(),
        frame,
        tick: 0,
        out: out.to_owned(),
        shot_n,
        run: format!("{capacity}p"),
        log,
    };
    f.title_to_main()?;
    let dark_world = tfp_assets::Installs::discover()
        .steam
        .is_some_and(|i| i.dark_world.is_some());
    for p in paths {
        match *p {
            "versus" => f.versus(capacity > 4)?,
            // From VERSUS (selected after the match): right.
            "quest" => {
                f.press(PADS[0], Button::DPadRight)?;
                f.coop(false)?
            }
            // CO-OP stays selected.
            "darkworld" if dark_world => f.coop(true)?,
            "darkworld" => println!("flow: no Dark World expansion; skipping it"),
            // From CO-OP: down (ARCHIVES), left (TRIALS).
            "trials" => {
                f.press(PADS[0], Button::DPadDown)?;
                f.press(PADS[0], Button::DPadLeft)?;
                f.trials()?
            }
            // From TRIALS: right.
            "archives" => {
                f.press(PADS[0], Button::DPadRight)?;
                f.archives()?
            }
            // From ARCHIVES: left (TRIALS), left (OPTIONS).
            "options" => {
                f.presses(PADS[0], Button::DPadLeft, 2)?;
                f.options()?
            }
            other => bail!("unknown path {other}"),
        }
    }
    f.frame_release();
    Ok(())
}

impl Flow<'_> {
    fn frame_release(&mut self) {
        self.game.release_all(self.renderer);
    }
}

/// `--flow-test DIR [--only 4|8|12]`.
pub fn run(args: &[String]) -> Result<()> {
    let out = PathBuf::from(args.first().map(String::as_str).unwrap_or("scratch/flow"));
    let only: Option<usize> = args
        .iter()
        .position(|a| a == "--only")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    for e in std::fs::read_dir(&out)?.flatten() {
        if e.path().extension().is_some_and(|x| x == "png") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    if tfp_assets::Installs::discover().steam.is_none() {
        bail!("flow test: no game data found");
    }
    crate::save::init(None)?;
    let gpu = Gpu::headless().context("headless GPU")?;
    let mut renderer = Renderer::new(&gpu);
    let mut log = Vec::new();
    let mut shot_n = 0;
    let mut failures = Vec::new();
    let runs: [(usize, &[&str]); 3] = [
        (
            4,
            &[
                "versus",
                "quest",
                "darkworld",
                "trials",
                "archives",
                "options",
            ],
        ),
        (8, &["versus"]),
        (12, &["versus"]),
    ];
    for (cap, paths) in runs {
        if only.is_some_and(|o| o != cap) {
            continue;
        }
        let r = catch_unwind(AssertUnwindSafe(|| {
            run_app(&gpu, &mut renderer, &out, &mut shot_n, &mut log, cap, paths)
        }));
        match r {
            Ok(Ok(())) => println!("flow: {cap}p: all steps reached"),
            Ok(Err(e)) => {
                println!("flow: {cap}p: FAILED: {e:#}");
                failures.push(format!("{cap}p: {e:#}"));
            }
            Err(p) => {
                let m = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                let last = log.last().map(|l| l.0.clone()).unwrap_or_default();
                println!("flow: {cap}p: PANIC after \"{last}\": {m}");
                failures.push(format!("{cap}p: panic after {last}: {m}"));
            }
        }
    }
    let list: String = log.iter().map(|(l, f)| format!("{f}\t{l}\n")).collect();
    std::fs::write(out.join("steps.tsv"), list)?;
    println!("flow: {} steps, {} failures", log.len(), failures.len());
    if failures.is_empty() {
        Ok(())
    } else {
        for f in &failures {
            eprintln!("flow failure: {f}");
        }
        std::process::exit(1)
    }
}

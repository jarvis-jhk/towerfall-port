//! Soak test: many random bot matches back to back, headless (`--soak N`).
//! Every match is derived from (seed, index), so a failure is reproduced
//! with `--soak-seed S --soak-from I --soak 1`. Panics are caught per
//! match (message, location, backtrace head); a tick that hangs is caught
//! by a watchdog thread (prints the match, exits with code 3); a match
//! that does not finish within the frame cap is reported as a stall.

use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tfp_render::{Gpu, Renderer, ScaleMode};
use tfp_sim::variants::{Variant, VariantSet};
use tfp_sim::versus::{MatchLength, Mode, Phase};

use crate::app::{Frame, HostRequests, LoadContext, Scene, TickContext};
use crate::audio::Audio;
use crate::input::RawInput;
use crate::play::{Play, PlayOptions};

pub struct SoakOptions {
    pub matches: u32,
    pub seed: u64,
    pub from: u32,
    /// Render every Nth frame (1 = all; 0 = never).
    pub draw_every: u32,
    /// Frames before a match counts as stalled.
    pub max_frames: u32,
    pub report: Option<PathBuf>,
}

/// One match's random setup.
#[derive(Clone, Debug)]
pub struct MatchSpec {
    pub index: u32,
    pub tower: usize,
    pub install: &'static str,
    pub players: usize,
    pub mode: Mode,
    pub teams: u8,
    pub length: MatchLength,
    pub variants: Vec<&'static str>,
    pub treasure: Option<[u8; tfp_sim::pickups::PICKUP_COUNT]>,
    pub seed: i32,
    pub replays: bool,
    pub slots: Vec<(u16, u8)>,
    opts_variants: VariantSet,
}

impl MatchSpec {
    fn describe(&self) -> String {
        format!(
            "#{} tower {} ({}) {}p {:?}{} {:?} seed {} variants [{}] treasure {}",
            self.index,
            self.tower,
            self.install,
            self.players,
            self.mode,
            if self.mode == Mode::TeamDeathmatch {
                format!(" x{}", self.teams)
            } else {
                String::new()
            },
            self.length,
            self.seed,
            self.variants.join(", "),
            match &self.treasure {
                Some(m) => format!("{m:?}"),
                None => "tower".into(),
            }
        )
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        crate::front::variants::next_rand(&mut self.0)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, p: f64) -> bool {
        ((self.next() >> 11) as f64 / (1u64 << 53) as f64) < p
    }
}

pub fn spec(seed: u64, index: u32, have_8p: bool) -> MatchSpec {
    // Mix the seed fully (the generator forces bit 0 to 1, so seeds that
    // differ only there must not map to the same stream).
    let mut r = Rng(seed.wrapping_mul(0xD6E8_FEB8_6659_FD93).rotate_left(17)
        ^ (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    for _ in 0..4 {
        r.next();
    }
    let players = 2 + r.below(11) as usize;
    let mode = match r.below(3) {
        0 => Mode::LastManStanding,
        1 => Mode::Headhunters,
        _ => Mode::TeamDeathmatch,
    };
    let teams = if mode == Mode::TeamDeathmatch {
        2 + r.below((players.min(6) - 1) as u64) as u8
    } else {
        2
    };
    let length = match r.below(10) {
        0..=3 => MatchLength::Instant,
        4..=7 => MatchLength::Quick,
        8 => MatchLength::Standard,
        _ => MatchLength::Epic,
    };
    // Installs: > 4 players normally the 8p towers, sometimes the 320 ones.
    let install = if !have_8p {
        "steam"
    } else if players > 4 {
        if r.chance(0.25) { "steam" } else { "8p" }
    } else if r.chance(0.5) {
        "8p"
    } else {
        "steam"
    };
    let tower = r.below(16) as usize;
    // Team split: every team gets one member, the rest at random.
    let mut order: Vec<usize> = (0..players).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, r.below(i as u64 + 1) as usize);
    }
    let mut team = vec![0u8; players];
    for (k, &p) in order.iter().enumerate() {
        team[p] = if k < teams as usize {
            k as u8
        } else {
            r.below(teams as u64) as u8
        };
    }
    // Archers: distinct while possible (9 + alts are mapped by the sim's
    // archer count), then repeats.
    let mut pool: Vec<u16> = (0..18).collect();
    let slots: Vec<(u16, u8)> = (0..players)
        .map(|p| {
            let a = if pool.is_empty() {
                r.below(18) as u16
            } else {
                pool.remove(r.below(pool.len() as u64) as usize)
            };
            (a, team[p])
        })
        .collect();
    // Variants: each with probability 12 %; per-player ones for a random
    // subset of the players.
    let joined: u16 = (0..players).fold(0, |m, p| m | 1 << p);
    let mut vs = VariantSet::default();
    let mut names = Vec::new();
    for &v in Variant::ALL {
        if !r.chance(0.12) {
            continue;
        }
        if v.per_player() && r.chance(0.5) {
            for p in 0..players {
                if r.chance(0.5) {
                    vs.set(v, p, true);
                }
            }
        } else {
            vs.set_all(v, true, joined);
        }
        names.push(v.title());
    }
    vs.clean(joined);
    let treasure = r.chance(0.6).then(|| {
        let mut m = [0u8; tfp_sim::pickups::PICKUP_COUNT];
        for x in &mut m {
            *x = if r.chance(0.5) { r.below(4) as u8 } else { 0 };
        }
        m
    });
    MatchSpec {
        index,
        tower,
        install,
        players,
        mode,
        teams,
        length,
        variants: names,
        treasure,
        seed: (r.next() & 0x7FFF_FFFF) as i32,
        replays: r.chance(0.7),
        slots,
        opts_variants: vs,
    }
}

#[derive(Default)]
struct Stats {
    finished: u32,
    panics: Vec<(String, String)>,
    stalls: Vec<(String, String)>,
    init_errors: Vec<(String, String)>,
    frames: u64,
    rounds: u64,
    tick_s: f64,
    draw_s: f64,
}

static PANIC: Mutex<Option<String>> = Mutex::new(None);

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        // Keep the frames of our crates (the interesting part).
        let ours: Vec<&str> = bt
            .lines()
            .filter(|l| l.contains("tfp_") || l.contains("towerfall_port") || l.contains("crates/"))
            .take(24)
            .collect();
        let msg = format!("{info}\n{}", ours.join("\n"));
        eprintln!("PANIC: {msg}");
        if let Ok(mut p) = PANIC.lock() {
            *p = Some(msg);
        }
    }));
}

pub fn run(o: &SoakOptions) -> Result<()> {
    let gpu = Gpu::headless().context("creating a headless GPU device")?;
    let mut renderer = Renderer::new(&gpu);
    let mut audio = Audio::offline(None)?;
    let installs = tfp_assets::Installs::discover();
    let have_8p = installs.eight_player.is_some();
    install_panic_hook();
    let heartbeat = Arc::new(AtomicU64::new(0));
    let current = Arc::new(Mutex::new(String::new()));
    {
        let (hb, cur) = (heartbeat.clone(), current.clone());
        std::thread::spawn(move || {
            let mut last = (0, Instant::now());
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let now = hb.load(Ordering::Relaxed);
                if now != last.0 {
                    last = (now, Instant::now());
                } else if last.1.elapsed() > Duration::from_secs(60) {
                    eprintln!(
                        "HANG: no progress for 60 s in {} (frame counter {now})",
                        cur.lock().map(|c| c.clone()).unwrap_or_default()
                    );
                    std::process::exit(3);
                }
            }
        });
    }
    let base_textures = renderer.live_textures();
    let mut st = Stats::default();
    let mut log = String::new();
    let started = Instant::now();
    let input = RawInput::default();
    let mut host = HostRequests::default();
    let mut frame = Frame::new(&gpu, &mut renderer, (320, 240), ScaleMode::Fit);
    for index in o.from..o.from + o.matches {
        let sp = spec(o.seed, index, have_8p);
        let desc = sp.describe();
        if let Ok(mut c) = current.lock() {
            c.clone_from(&desc);
        }
        let opts = PlayOptions {
            tower: sp.tower.to_string(),
            players: sp.players,
            eight_player: sp.install == "8p",
            force_steam: sp.install == "steam",
            versus_mode: sp.mode,
            length: sp.length,
            teams: sp.teams,
            seed: Some(sp.seed),
            no_replay: !sp.replays,
            music: false,
            variants: Some(sp.opts_variants),
            treasure_mask: sp.treasure,
            slots: Some(sp.slots.clone()),
            ..Default::default()
        };
        let mut play = Play::new(opts);
        let t0 = Instant::now();
        let init = catch_unwind(AssertUnwindSafe(|| {
            play.load(&mut LoadContext {
                gpu: &gpu,
                renderer: &mut renderer,
                audio: &mut audio,
            })
        }));
        match init {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                st.init_errors.push((desc.clone(), format!("{e:#}")));
                println!("soak {index}: init error: {e:#}");
                continue;
            }
            Err(_) => {
                let m = PANIC
                    .lock()
                    .ok()
                    .and_then(|mut p| p.take())
                    .unwrap_or_default();
                st.panics.push((desc.clone(), format!("(init) {m}")));
                println!("soak {index}: PANIC in init");
                play.release(&mut renderer);
                continue;
            }
        }
        let (mut ticks, mut tick_s, mut draw_s) = (0u32, 0.0, 0.0);
        let mut outcome = "stall";
        let run = catch_unwind(AssertUnwindSafe(|| {
            while ticks < o.max_frames {
                let a = Instant::now();
                play.tick(&mut TickContext {
                    input: &input,
                    audio: &mut audio,
                    host: &mut host,
                    tick: ticks as u64,
                });
                tick_s += a.elapsed().as_secs_f64();
                if o.draw_every > 0 && ticks.is_multiple_of(o.draw_every) {
                    let b = Instant::now();
                    frame.draw(&gpu, &mut renderer, &mut play, ticks as u64 + 1, (960, 720))?;
                    draw_s += b.elapsed().as_secs_f64();
                }
                audio.advance_offline(1.0 / 60.0)?;
                ticks += 1;
                heartbeat.fetch_add(1, Ordering::Relaxed);
                if play.phase() == Some(Phase::Finished) {
                    outcome = "finished";
                    break;
                }
            }
            anyhow::Ok(())
        }));
        let rounds = play
            .world()
            .and_then(|w| w.versus.as_ref())
            .map_or(0, |m| m.round_index as u64 + 1);
        let state = play
            .world()
            .and_then(|w| {
                w.versus
                    .as_ref()
                    .map(|m| (m.phase, m.round_index, m.scores))
            })
            .map(|(p, r, s)| format!("phase {p:?}, round {r}, scores {:?}", &s[..]))
            .unwrap_or_default();
        match run {
            Ok(Ok(())) if outcome == "finished" => st.finished += 1,
            Ok(Ok(())) => st.stalls.push((desc.clone(), state.clone())),
            Ok(Err(e)) => st.init_errors.push((desc.clone(), format!("run: {e:#}"))),
            Err(_) => {
                outcome = "PANIC";
                let m = PANIC
                    .lock()
                    .ok()
                    .and_then(|mut p| p.take())
                    .unwrap_or_default();
                st.panics
                    .push((desc.clone(), format!("at frame {ticks}, {state}\n{m}")));
            }
        }
        play.release(&mut renderer);
        drop(play);
        st.frames += ticks as u64;
        st.rounds += rounds;
        st.tick_s += tick_s;
        st.draw_s += draw_s;
        let line = format!(
            "soak {index}: {outcome} in {ticks} frames, {rounds} rounds, {:.1} s ({:.3} ms/tick) | {desc}",
            t0.elapsed().as_secs_f64(),
            1e3 * tick_s / ticks.max(1) as f64
        );
        println!("{line}");
        let _ = writeln!(log, "{line}");
        audio.stop_all();
    }
    let n = o.matches.max(1);
    let mut sum = String::new();
    let _ = writeln!(
        sum,
        "soak: {} matches in {:.0} s: {} finished, {} panics, {} stalls (> {} frames), {} errors",
        o.matches,
        started.elapsed().as_secs_f64(),
        st.finished,
        st.panics.len(),
        st.stalls.len(),
        o.max_frames,
        st.init_errors.len()
    );
    let _ = writeln!(
        sum,
        "soak: {} frames ({:.1} min of game), {} rounds, {:.0} frames/match; tick {:.3} ms/frame; draw {:.3} ms per drawn frame (every {})",
        st.frames,
        st.frames as f64 / 3600.0,
        st.rounds,
        st.frames as f64 / n as f64,
        1e3 * st.tick_s / st.frames.max(1) as f64,
        if o.draw_every > 0 {
            1e3 * st.draw_s / (st.frames / o.draw_every as u64).max(1) as f64
        } else {
            0.0
        },
        o.draw_every
    );
    let _ = writeln!(
        sum,
        "soak: GPU textures alive: {} at start, {} at the end",
        base_textures,
        renderer.live_textures()
    );
    for (d, m) in &st.panics {
        let _ = writeln!(sum, "PANIC {d}\n{m}\n");
    }
    for (d, m) in &st.stalls {
        let _ = writeln!(sum, "STALL {d}: {m}");
    }
    for (d, m) in &st.init_errors {
        let _ = writeln!(sum, "ERROR {d}: {m}");
    }
    print!("{sum}");
    if let Some(p) = &o.report {
        std::fs::write(p, format!("{sum}\n{log}"))?;
        println!("soak report: {}", p.display());
    }
    Ok(())
}

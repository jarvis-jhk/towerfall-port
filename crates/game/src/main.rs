//! towerfall-port: clean-room reimplementation of TowerFall Ascension.
//!
//! Runs the front end (menus) by default, or the platform demo
//! (`--demo`); see `docs/platform.md` and `docs/ui-notes.md`.

mod app;
mod audio;
mod boot;
mod content;
mod coop_ui;
mod demo;
mod flow;
mod front;
mod game;
mod input;
mod match_draw;
mod match_pause;
mod music;
mod online;
mod options_store;
mod padmap;
mod platform;
mod play;
mod rumble;
mod save;
mod sfx;
mod soak;
mod timing;
mod tower;
mod versus_ui;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use tfp_render::ScaleMode;

const USAGE: &str = "\
towerfall-port

USAGE:
    towerfall-port [OPTIONS]

OPTIONS:
    --headless               Render offscreen: no window, no audio device, virtual 60 Hz
    --frames N               Frames to run (headless default 120; windowed 0 = until closed)
    --screenshot PATH        Save the last presented frame (window size) as PNG
    --native-screenshot PATH Save the last game canvas (e.g. 320x240) as PNG
    --wav PATH               Headless: write the mixed audio to a WAV file
    --window WxH             Window / headless output size (default 3x the game size)
    --game WxH               Game canvas size (default 320x240)
    --wide                   Game canvas 420x240 (8-player towers)
    --scale MODE             fit (default: the original's fractional scale), integer, stretch
    --fullscreen             Start in borderless fullscreen (F11 / Alt+Enter toggle)
    --no-vsync               Disable vsync (frames paced to the 60 Hz tick)
    --music N                Play track N of the original's music wave bank
    --play-wav PATH.wav      Demo: play a WAV file as looping music
    --no-assets              Ignore the installed game data (placeholders)
    --demo                   Run the platform demo scene instead of the menus
    --screen NAME            Start on (and stay on) a menu screen: intro, title, main,
                             rollcall, versus, variants, teams, map, options, controls,
                             keyboard, pause, match-end, ready, coop, quest-map, trials-map,
                             darkworld-map
    --players N              Pre-join N archers (1-12; sets the layout: 4/8/12 slots)
    --capacity N             Lay the menus out for 4, 8 or 12 archers
    --no-music               No menu music
    --tower NAME|ID          Show a versus tower (index or part of its name)
    --level N                Level of the tower (0 = 00.oel; default 0)
    --eight-player           Use the 8-player build's data (420x240 towers)
    --seed N                 Procedural towers: generator seed
    --play                   Sandbox match on --tower/--level with --players N (default 4):
                             local devices drive P1.. (pads, then keyboard), bots the rest
    --script FILE|bots|none  --play: scripted inputs (see play.rs), bots (default) or idle
    --hitboxes               --play: draw hitboxes
    --mode lms|hh|tdm|sandbox  --play: match mode (default hh; sandbox = respawning archers)
    --mode quest|trials      --play: a Quest tower (--tower ID|NAME, --hardcore, --players 1-4)
                             or a Trials level (--tower X --level TIER); bots play
    --hardcore               --play --mode quest: Hardcore difficulty
    --quest-end complete@N|gameover@N  --play --mode quest|darkworld: from sim frame N
                             force the completion / game over (headless proof)
    --mode darkworld         --play: a Dark World tower (--tower ID|NAME, --players 1-4,
                             --difficulty 0-2, --variants LIST); bots play
    --difficulty N           --play --mode darkworld: 0 Normal, 1 Hardcore, 2 Legendary
    --dw-level N             --play --mode darkworld: start on level N (0-based)
    --dw-assist N            --play --mode darkworld: proof aid: invulnerable bots, and
                             one point of boss damage every N ticks while it can be hurt
    --length instant|quick|standard|epic  --play: match length (default quick)
    --teams N                --play: teams in Team Deathmatch (2-6)
    --no-replay              --play: no instant replays
    --shots DIR              --play headless: save the native frame at each match phase
    --virtual-pads N         Attach N SDL virtual controllers (testing; headless:
                             scripted input; with --list-devices: list them)
    --unplug-at N            Headless: unplug virtual pad 0 at tick N
    --frame-times            Headless: time every frame (GPU waited), print percentiles
    --import-save            Import options + keyboard configs from the original's
                             tf_saveData (read only; done automatically on the first run)
    --save-file PATH         The original save to import from (implies --import-save)
    --game-dir PATH          Your TowerFall folder (if it is not found by itself; the
                             8-player build's folder works too; remembered in the options)
    --variants LIST          --play: variants by title, comma-separated (GUNN STYLE,
                             START WITH BOMB ARROWS@2 = player 2 only; case-insensitive)
    --treasure MASK          --play: all (every pickup), tower (default), or 21
                             comma-separated counts in pickup order
    --shots-every N          --play --shots: also capture every N frames of play
    --soak N                 Headless soak test: N random bot matches back to back
                             (--soak-seed S, --soak-from I, --soak-draw K = render
                             every Kth frame (default 4, 0 = never), --soak-frames F =
                             stall limit (default 120000), --soak-report FILE)
    --list-devices           Print GPUs, controllers and audio devices, then exit
    --online                 Online play over Warpzone: host, join by code, browse rooms
    --warpzone-host          Online: open the host screen (the Warpzone launcher's host)
    --warpzone-join ROOM     Online: join a room id, invite link or join CODE
    --online-bots N          Online (testing): N bot archers on this machine
    --online-autostart N     Online (testing): host starts once N machines are in
    --online-host            Online (testing): host a public room at once
    --online-exit            Online (testing): quit after the first match
    (--mode and --length also set the online host's match settings)
    --net-harness ...        Headless multi-process online test (see src/online/harness.rs)
    -h, --help               Show this help

ENVIRONMENT:
    TF_GAME_DIR=...               The original game (read only), else auto-detected
    WGPU_BACKEND=vulkan|dx12|gl   Force a graphics backend
    RUST_LOG=info|debug           Log level
";

#[derive(Debug, Default)]
struct Args {
    headless: bool,
    frames: Option<u32>,
    screenshot: Option<PathBuf>,
    native_screenshot: Option<PathBuf>,
    wav: Option<PathBuf>,
    window: Option<(u32, u32)>,
    game: Option<(u32, u32)>,
    scale: ScaleMode,
    fullscreen: bool,
    no_vsync: bool,
    music: Option<usize>,
    play: Option<PathBuf>,
    no_assets: bool,
    virtual_pads: usize,
    unplug_at: Option<u64>,
    import_save: bool,
    save_file: Option<PathBuf>,
    soak: Option<u32>,
    game_dir: Option<PathBuf>,
    variants: Option<String>,
    treasure: Option<String>,
    shots_every: Option<u32>,
    soak_seed: Option<u64>,
    soak_from: u32,
    soak_draw: Option<u32>,
    soak_frames: Option<u32>,
    soak_report: Option<PathBuf>,
    frame_times: bool,
    list_devices: bool,
    demo: bool,
    screen: Option<front::ScreenId>,
    players: Option<usize>,
    capacity: Option<usize>,
    no_music: bool,
    tower: Option<String>,
    level: usize,
    eight_player: bool,
    seed: Option<i32>,
    play_mode: bool,
    script: Option<String>,
    hitboxes: bool,
    online: bool,
    wz_host: bool,
    wz_join: Option<String>,
    online_bots: usize,
    online_autostart: Option<usize>,
    online_host: bool,
    online_exit: bool,
    mode: Option<String>,
    length: Option<String>,
    teams: Option<u8>,
    no_replay: bool,
    shots: Option<PathBuf>,
    hardcore: bool,
    quest_end: Option<(bool, u32)>,
    difficulty: u8,
    dw_level: Option<usize>,
    dw_assist: Option<u32>,
}

fn parse_size(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s.split_once(['x', 'X']).context("size must be WxH")?;
    let (w, h): (u32, u32) = (w.parse()?, h.parse()?);
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        bail!("size out of range: {s}");
    }
    Ok((w, h))
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Option<Args>> {
    let mut a = Args::default();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--headless" => a.headless = true,
            "--frames" => a.frames = Some(value("--frames")?.parse()?),
            "--screenshot" => a.screenshot = Some(value("--screenshot")?.into()),
            "--native-screenshot" => {
                a.native_screenshot = Some(value("--native-screenshot")?.into())
            }
            "--wav" => a.wav = Some(value("--wav")?.into()),
            "--window" => a.window = Some(parse_size(&value("--window")?)?),
            "--game" => a.game = Some(parse_size(&value("--game")?)?),
            "--wide" => a.game = Some((420, 240)),
            "--scale" => a.scale = value("--scale")?.parse().map_err(anyhow::Error::msg)?,
            "--fullscreen" => a.fullscreen = true,
            "--no-vsync" => a.no_vsync = true,
            "--music" => a.music = Some(value("--music")?.parse()?),
            "--play-wav" => a.play = Some(value("--play-wav")?.into()),
            "--no-assets" => a.no_assets = true,
            "--virtual-pads" => a.virtual_pads = value("--virtual-pads")?.parse()?,
            "--unplug-at" => a.unplug_at = Some(value("--unplug-at")?.parse()?),
            "--frame-times" => a.frame_times = true,
            "--import-save" => a.import_save = true,
            "--soak" => a.soak = Some(value("--soak")?.parse()?),
            "--game-dir" => a.game_dir = Some(value("--game-dir")?.into()),
            "--variants" => a.variants = Some(value("--variants")?),
            "--treasure" => a.treasure = Some(value("--treasure")?),
            "--shots-every" => a.shots_every = Some(value("--shots-every")?.parse()?),
            "--soak-seed" => a.soak_seed = Some(value("--soak-seed")?.parse()?),
            "--soak-from" => a.soak_from = value("--soak-from")?.parse()?,
            "--soak-draw" => a.soak_draw = Some(value("--soak-draw")?.parse()?),
            "--soak-frames" => a.soak_frames = Some(value("--soak-frames")?.parse()?),
            "--soak-report" => a.soak_report = Some(value("--soak-report")?.into()),
            "--save-file" => a.save_file = Some(value("--save-file")?.into()),
            "--list-devices" => a.list_devices = true,
            "--demo" => a.demo = true,
            "--screen" => a.screen = Some(value("--screen")?.parse().map_err(anyhow::Error::msg)?),
            "--players" => {
                let n: usize = value("--players")?.parse()?;
                if !(1..=12).contains(&n) {
                    bail!("--players must be 1-12");
                }
                a.players = Some(n);
            }
            "--capacity" => {
                let n: usize = value("--capacity")?.parse()?;
                if ![4, 8, 12].contains(&n) {
                    bail!("--capacity must be 4, 8 or 12");
                }
                a.capacity = Some(n);
            }
            "--no-music" => a.no_music = true,
            "--tower" => a.tower = Some(value("--tower")?),
            "--level" => a.level = value("--level")?.parse()?,
            "--eight-player" => a.eight_player = true,
            "--seed" => a.seed = Some(value("--seed")?.parse()?),
            "--play" => a.play_mode = true,
            "--script" => a.script = Some(value("--script")?),
            "--hitboxes" => a.hitboxes = true,
            "--online" => a.online = true,
            "--warpzone-host" => a.wz_host = true,
            "--warpzone-join" => a.wz_join = Some(value("--warpzone-join")?),
            "--online-bots" => a.online_bots = value("--online-bots")?.parse()?,
            "--online-autostart" => {
                a.online_autostart = Some(value("--online-autostart")?.parse()?)
            }
            "--online-host" => a.online_host = true,
            "--online-exit" => a.online_exit = true,
            s if s.starts_with("--warpzone-join=") || s.starts_with("warpzone://") => {
                a.wz_join = Some(s.trim_start_matches("--warpzone-join=").to_string())
            }
            "--mode" => a.mode = Some(value("--mode")?),
            "--length" => a.length = Some(value("--length")?),
            "--teams" => a.teams = Some(value("--teams")?.parse()?),
            "--no-replay" => a.no_replay = true,
            "--shots" => a.shots = Some(value("--shots")?.into()),
            "--hardcore" => a.hardcore = true,
            "--difficulty" => a.difficulty = value("--difficulty")?.parse::<u8>()?.min(2),
            "--dw-level" => a.dw_level = Some(value("--dw-level")?.parse()?),
            "--dw-assist" => a.dw_assist = Some(value("--dw-assist")?.parse()?),
            "--quest-end" => {
                let v = value("--quest-end")?;
                let (k, n) = v.split_once('@').context("--quest-end KIND@FRAME")?;
                let complete = match k {
                    "complete" => true,
                    "gameover" => false,
                    _ => bail!("--quest-end complete@N or gameover@N"),
                };
                a.quest_end = Some((complete, n.parse()?));
            }
            "-h" | "--help" => return Ok(None),
            other => bail!("unknown argument {other:?} (see --help)"),
        }
    }
    Ok(Some(a))
}

/// `--variants`: titles, comma-separated, for every player.
fn parse_variants(list: &str) -> Result<tfp_sim::variants::VariantSet> {
    use tfp_sim::variants::{Variant, VariantSet};
    let mut s = VariantSet::default();
    for item in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        // `TITLE@N`: only player N (1-based; per-player variants).
        let (name, player) = match item.rsplit_once('@') {
            Some((n, p)) => (n.trim(), Some(p.trim().parse::<usize>()?)),
            None => (item, None),
        };
        let v = Variant::ALL
            .iter()
            .find(|v| v.title().eq_ignore_ascii_case(name))
            .with_context(|| format!("unknown variant {name:?}"))?;
        match player {
            Some(p) if (1..=12).contains(&p) => s.set(*v, p - 1, true),
            Some(p) => bail!("--variants {item:?}: player {p} out of 1..12"),
            None => s.set_all(*v, true, 0x0FFF),
        }
    }
    Ok(s)
}

/// `--treasure`: "all", "tower" or 21 counts.
fn parse_treasure(m: &str) -> Result<Option<[u8; tfp_sim::pickups::PICKUP_COUNT]>> {
    const N: usize = tfp_sim::pickups::PICKUP_COUNT;
    match m {
        "tower" => Ok(None),
        "all" => Ok(Some([1; N])),
        _ => {
            let v: Vec<u8> = m
                .split(',')
                .map(|x| x.trim().parse())
                .collect::<Result<_, _>>()?;
            let a: [u8; N] = v
                .try_into()
                .map_err(|_| anyhow::anyhow!("--treasure needs {N} counts"))?;
            Ok(Some(a))
        }
    }
}

fn init_logging() {
    struct Logger(log::LevelFilter);
    impl log::Log for Logger {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.level() <= self.0
        }
        fn log(&self, r: &log::Record) {
            // Keep wgpu/naga internals quiet unless asked for.
            let noisy = r.target().starts_with("wgpu") || r.target().starts_with("naga");
            if self.enabled(r.metadata())
                && (!noisy || r.level() <= log::Level::Warn || self.0 >= log::LevelFilter::Debug)
            {
                eprintln!("[{:5}] {}", r.level(), r.args());
            }
        }
        fn flush(&self) {}
    }
    let level = match std::env::var("RUST_LOG").as_deref() {
        Ok("trace") => log::LevelFilter::Trace,
        Ok("debug") => log::LevelFilter::Debug,
        Ok("info") => log::LevelFilter::Info,
        Ok("error") => log::LevelFilter::Error,
        _ => log::LevelFilter::Warn,
    };
    if log::set_logger(Box::leak(Box::new(Logger(level)))).is_ok() {
        log::set_max_level(level);
    }
}

/// `--list-devices`: GPUs (wgpu), controllers and audio (SDL, no window).
fn list_devices(virtual_pads: usize) -> Result<()> {
    let instance = tfp_render::new_instance();
    let adapters =
        pollster::block_on(instance.enumerate_adapters(tfp_render::wgpu::Backends::all()));
    println!("GPU adapters ({}):", adapters.len());
    for a in &adapters {
        let i = a.get_info();
        println!(
            "  {} [{:?}, {:?}] driver: {} {}",
            i.name, i.backend, i.device_type, i.driver, i.driver_info
        );
    }

    platform::set_hints();
    let sdl = sdl2::init().map_err(anyhow::Error::msg)?;
    let v = sdl2::version::version();
    println!("SDL {}.{}.{}", v.major, v.minor, v.patch);
    let gc = sdl.game_controller().map_err(anyhow::Error::msg)?;
    let _virtual = (virtual_pads > 0)
        .then(|| input::VirtualPads::attach(virtual_pads))
        .transpose()?;
    // Let SDL deliver the initial device-added events (hot-plug path).
    let mut pump = sdl.event_pump().map_err(anyhow::Error::msg)?;
    let mut hub = input::SdlInput::new(gc.clone());
    for _ in 0..10 {
        for e in pump.poll_iter() {
            hub.handle_event(&e);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let n = gc.num_joysticks().map_err(anyhow::Error::msg)?;
    let controllers = hub.controllers();
    println!(
        "Controllers ({} game controllers, {} joysticks):",
        controllers.len(),
        n
    );
    for c in &controllers {
        println!(
            "  #{} {} (rumble: {})",
            c.id,
            c.name,
            if c.has_rumble { "yes" } else { "no" }
        );
    }
    for i in 0..n {
        if !gc.is_game_controller(i) {
            let joy = sdl.joystick().map_err(anyhow::Error::msg)?;
            let name = joy.name_for_index(i).unwrap_or_else(|_| "?".into());
            println!(
                "  joystick #{i} {name} (no controller mapping; add one to gamecontrollerdb.txt)"
            );
        }
    }

    match sdl.audio() {
        Ok(audio) => {
            println!("Audio driver: {}", audio.current_audio_driver());
            let n = audio.num_audio_playback_devices().unwrap_or(0);
            println!("Audio playback devices ({n}):");
            for i in 0..n {
                let name = audio.audio_playback_device_name(i).unwrap_or_else(|e| e);
                println!("  #{i} {name}");
            }
        }
        Err(e) => println!("Audio: unavailable ({e})"),
    }
    let drivers: Vec<_> = sdl2::audio::drivers().collect();
    println!("Audio drivers compiled into SDL: {}", drivers.join(", "));
    Ok(())
}

fn main() -> Result<()> {
    init_logging();
    let argv: Vec<String> = std::env::args().collect();
    match argv.get(1).map(String::as_str) {
        Some("--net-harness") => std::process::exit(online::harness::orchestrate(&argv[2..])?),
        Some("--net-peer") => std::process::exit(online::harness::peer(&argv[2..])?),
        Some("--net-soak") => std::process::exit(online::soak::run(&argv[2..])?),
        Some("--flow-test") => return flow::run(&argv[2..]),
        _ => {}
    }
    let Some(args) = parse_args(std::env::args().skip(1))? else {
        print!("{USAGE}");
        return Ok(());
    };
    if args.list_devices {
        return list_devices(args.virtual_pads);
    }
    if let Some(n) = args.soak {
        return soak::run(&soak::SoakOptions {
            matches: n,
            seed: args.soak_seed.unwrap_or(1),
            from: args.soak_from,
            draw_every: args.soak_draw.unwrap_or(4),
            max_frames: args.soak_frames.unwrap_or(120_000),
            report: args.soak_report.clone(),
        });
    }
    // The original's default window: 3x the game size (960x720, 1260x720).
    let game = args.game.unwrap_or(if args.eight_player {
        (420, 240)
    } else {
        (320, 240)
    });
    let window = args.window.unwrap_or((game.0 * 3, game.1 * 3));
    // Options: ours from the data directory (windowed runs; headless runs
    // use the defaults unless an import is asked for).
    let import = args.import_save || args.save_file.is_some();
    let options = if !args.headless || import {
        let (o, src) = options_store::startup(import, args.save_file.as_deref());
        match &src {
            options_store::Source::Ours(p) => println!("options: {}", p.display()),
            options_store::Source::Imported(p) => {
                println!("options: imported from {} (read only)", p.display());
                if !args.headless
                    && let Some(ours) = options_store::path()
                {
                    match options_store::save_to(&ours, &o) {
                        Ok(()) => println!("options: saved to {}", ours.display()),
                        Err(e) => log::warn!("saving options: {e:#}"),
                    }
                }
            }
            options_store::Source::Defaults => println!("options: defaults"),
        }
        o
    } else {
        front::Options::default()
    };
    let mut options = options;
    if let Some(d) = &args.game_dir {
        let d = tfp_assets::install::normalize_game_dir(d);
        if tfp_assets::GameInstall::open(&d, None).is_none() {
            eprintln!(
                "--game-dir {}: no TowerFall data there (Content/Atlas missing)",
                d.display()
            );
        } else if !options.game_dirs.contains(&d) {
            options.game_dirs.push(d);
            if !args.headless
                && let Some(p) = options_store::path()
                && let Err(e) = options_store::save_to(&p, &options)
            {
                log::warn!("saving options: {e:#}");
            }
        }
    }
    tfp_assets::install::set_extra_dirs(options.game_dirs.clone());
    // Records (save.rs): persistent in windowed runs; headless runs keep
    // them in memory. The original's records are imported once (read only).
    let records = if args.headless { None } else { save::path() };
    if let Err(e) = save::init(records.clone()) {
        log::warn!("records: {e:#}; starting empty");
    }
    if (!args.headless || import)
        && let Some(p) = save::import_once(import, args.save_file.as_deref())
    {
        println!("records: imported from {} (read only)", p.display());
    }
    if let Some(p) = &records {
        println!("records: {}", p.display());
    }
    let mut demo_scene;
    let mut front_scene;
    let mut tower_scene;
    let mut play_scene;
    let mut game_scene;
    let mut online_scene;
    let mut coop_scene;
    let scene: &mut dyn app::Scene = if args.online || args.wz_host || args.wz_join.is_some() {
        let mut launch: Vec<String> = Vec::new();
        if args.wz_host {
            launch.push("--warpzone-host".into());
        }
        if let Some(j) = &args.wz_join {
            launch.extend(["--warpzone-join".into(), j.clone()]);
        }
        online_scene = online::scene::OnlineScene::new(online::scene::OnlineOptions {
            args: launch,
            bots: args.online_bots,
            auto_start: args.online_autostart,
            mode: args.mode.as_deref().map(online::harness::mode_of),
            kind: args.mode.as_deref().map(online::harness::kind_of),
            length: args.length.as_deref().map(online::harness::length_of),
            tower: args.tower.clone(),
            host: args.online_host,
            exit_after_match: args.online_exit,
            embedded: false,
            keys: Some(options.keys.clone()),
            pads: options.pads.clone(),
            sfx_volume: Some(options.sounds as f32 / 10.0),
            rumble: Some(options.rumble),
        });
        &mut online_scene
    } else if args.play_mode && matches!(args.mode.as_deref(), Some("darkworld" | "dw")) {
        if let Some(d) = &args.shots {
            std::fs::create_dir_all(d)?;
        }
        let mut o = coop_ui::scene::cli_options_dw(
            args.tower.as_deref(),
            if args.hardcore {
                args.difficulty.max(1)
            } else {
                args.difficulty
            },
            args.players.unwrap_or(2),
            args.seed,
            args.shots.clone(),
            args.hitboxes,
            args.variants
                .as_deref()
                .map(parse_variants)
                .transpose()?
                .unwrap_or_default(),
        )?;
        o.end = args.quest_end;
        coop_scene = coop_ui::scene::CoopPlay::new(o);
        coop_scene.dw_proof = coop_ui::scene::DwProof {
            level: args.dw_level,
            assist: args.dw_assist,
        };
        &mut coop_scene
    } else if args.play_mode && matches!(args.mode.as_deref(), Some("quest" | "trials")) {
        if let Some(d) = &args.shots {
            std::fs::create_dir_all(d)?;
        }
        let mut o = coop_ui::scene::cli_options(
            args.mode.as_deref() == Some("quest"),
            args.tower.as_deref(),
            args.level,
            args.hardcore,
            args.players.unwrap_or(2),
            args.seed,
            args.shots.clone(),
            args.hitboxes,
        )?;
        o.end = args.quest_end;
        coop_scene = coop_ui::scene::CoopPlay::new(o);
        &mut coop_scene
    } else if args.play_mode {
        use tfp_sim::versus::{MatchLength, Mode};
        let (pmode, vmode) = match args.mode.as_deref().unwrap_or("hh") {
            "sandbox" => (play::PlayMode::Sandbox, Mode::Headhunters),
            "lms" => (play::PlayMode::Versus, Mode::LastManStanding),
            "tdm" => (play::PlayMode::Versus, Mode::TeamDeathmatch),
            "hh" => (play::PlayMode::Versus, Mode::Headhunters),
            m => bail!("unknown --mode {m:?} (lms, hh, tdm, sandbox)"),
        };
        let length = match args.length.as_deref().unwrap_or("quick") {
            "instant" => MatchLength::Instant,
            "quick" => MatchLength::Quick,
            "standard" => MatchLength::Standard,
            "epic" => MatchLength::Epic,
            l => bail!("unknown --length {l:?}"),
        };
        if let Some(d) = &args.shots {
            std::fs::create_dir_all(d)?;
        }
        play_scene = play::Play::new(play::PlayOptions {
            tower: args.tower.clone().unwrap_or_else(|| "sacred".into()),
            level: args.level,
            players: args.players.unwrap_or(4),
            eight_player: args.eight_player,
            script: args.script.clone(),
            seed: args.seed,
            hitboxes: args.hitboxes,
            keyboard: !args.headless,
            setup: None,
            mode: pmode,
            versus_mode: vmode,
            length,
            teams: args.teams.unwrap_or(2),
            no_replay: args.no_replay,
            shots: args.shots.clone(),
            music: !args.no_music,
            sfx_volume: options.sounds as f32 / 10.0,
            pads: options.pads.clone(),
            variants: args.variants.as_deref().map(parse_variants).transpose()?,
            treasure_mask: args
                .treasure
                .as_deref()
                .map(parse_treasure)
                .transpose()?
                .flatten(),
            shots_every: args.shots_every,
            ..Default::default()
        });
        &mut play_scene
    } else if let Some(t) = &args.tower {
        tower_scene = tower::TowerViewer::new(tower::ViewerOptions {
            tower: t.clone(),
            level: args.level,
            eight_player: args.eight_player,
            seed: args.seed,
        });
        &mut tower_scene
    } else if args.demo {
        demo_scene = demo::Demo::new(demo::DemoOptions {
            game_size: game,
            music_track: args.music,
            music_wav: args.play.clone(),
            no_assets: args.no_assets,
        });
        &mut demo_scene
    } else {
        let fopts = front::FrontOptions {
            screen: args.screen,
            players: args.players,
            capacity: args.capacity,
            pin: args.screen.is_some(),
            no_assets: args.no_assets,
            music: !args.no_music,
            initial: Some(options.clone()),
            persist: !args.headless,
        };
        if args.screen.is_some() {
            front_scene = front::FrontEnd::new(fopts);
            &mut front_scene
        } else {
            // The menus, or the "game data not found" screen until there is data.
            game_scene = boot::Boot::new(fopts, !args.headless, !args.headless);
            &mut game_scene
        }
    };
    if args.headless {
        app::run_headless(
            scene,
            &app::HeadlessOptions {
                window,
                mode: args.scale,
                frames: args.frames.unwrap_or(120),
                screenshot: args.screenshot,
                native_screenshot: args.native_screenshot,
                wav: args.wav,
                virtual_pads: args.virtual_pads,
                unplug_at: args.unplug_at,
                frame_times: args.frame_times,
            },
        )
    } else {
        app::run_windowed(
            scene,
            &app::WindowOptions {
                size: window,
                mode: args.scale,
                fullscreen: args.fullscreen || options.fullscreen,
                vsync: !args.no_vsync && options.vsync,
                frames: args.frames.unwrap_or(0),
                screenshot: args.screenshot,
                native_screenshot: args.native_screenshot,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Option<Args>> {
        parse_args(s.split_whitespace().map(String::from))
    }

    #[test]
    fn cli() {
        let a = parse(
            "--headless --frames 10 --screenshot scratch/x.png --window 640x360 --wide --scale fit",
        )
        .unwrap()
        .unwrap();
        assert!(a.headless);
        assert_eq!(a.frames, Some(10));
        assert_eq!(a.window, Some((640, 360)));
        assert_eq!(a.game, Some((420, 240)));
        assert_eq!(a.scale, ScaleMode::Fit);
        assert_eq!(
            a.screenshot.as_deref(),
            Some(std::path::Path::new("scratch/x.png"))
        );
        assert!(parse("--help").unwrap().is_none());
        assert!(parse("--bogus").is_err());
        assert!(parse("--window 0x10").is_err());
        assert!(parse("--frames").is_err());
        assert!(parse("--scale wobbly").is_err());
    }
}

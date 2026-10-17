//! Headless multi-process test of online play: real game processes (no
//! window, no GPU, no audio) play a full Versus match with bots over a
//! Warpzone server and compare their final checksums.
//!
//! ```sh
//! scripts/net/dev-server.sh &          # http://127.0.0.1:3012, udp 3490
//! export WARPZONE_SERVER=http://127.0.0.1:3012
//! towerfall-port --net-harness --locals 2x1 --mode lms --length quick
//! towerfall-port --net-harness --locals 4x3 --delay 40 --jitter 10 --loss 3
//! towerfall-port --net-harness --locals 12x1 --length instant
//! ```
//!
//! Orchestrator (`--net-harness`): starts machine 0 as host of a public
//! room (`--private`: unlisted, with a join code), the others join with
//! `--warpzone-join CODE` (odd machines, private rooms) or `--warpzone-join
//! <room id>` (the launcher path); `--spectators N` more machines watch. Every machine
//! (`--net-peer`) plays its local slots with [`tfp_sim::bot::Bot`]s; at the
//! end of the match (results confirmed) each player reports the first
//! finished state's frame and checksum through `finish_match`, then prints a
//! `RESULT` line. `SUMMARY PASS`: every remaining machine reported, all
//! checksums equal, the host saw agreement, no desync.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tfp_net::warpzone_core::client::{Event, HostOptions};
use tfp_net::warpzone_core::protocol::api::{Entry, Visibility};
use tfp_net::{LinkSim, NetConfig, NetEvent, NetSession, SlotRequest};
use tfp_sim::PlayerInput;

use super::adapter::{
    KIND_DARKWORLD, KIND_QUEST, KIND_TRIALS, KIND_VERSUS, MachineInfo, NO_KEY, NetWorld,
    OnlineSettings, TowerCache, TowerSize, procedural_key, random_procedural_seed, tower_size,
    wide_tower_names,
};
use super::coop::CoopData;
use super::runner::OnlineMatch;
use tfp_sim::bot::Bot;

const STEP: Duration = Duration::from_micros(16_667);
const DEFAULT_SERVER: &str = "http://127.0.0.1:3012";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn num<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    arg(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_locals(s: &str) -> Vec<usize> {
    if let Some((m, n)) = s.split_once('x') {
        return vec![n.parse().unwrap_or(1); m.parse().unwrap_or(2)];
    }
    s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
}

fn parse_at(s: Option<String>) -> Option<(usize, i32)> {
    let (m, f) = s?
        .split_once('@')
        .map(|(a, b)| (a.to_string(), b.to_string()))?;
    Some((m.parse().ok()?, f.parse().ok()?))
}

fn root() -> PathBuf {
    std::env::var_os("TFP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

/// `--net-harness ...`: run the machines, compare, print `SUMMARY`.
pub fn orchestrate(args: &[String]) -> Result<i32> {
    let locals = parse_locals(&arg(args, "--locals").unwrap_or_else(|| "2x1".into()));
    let spectators: usize = num(args, "--spectators", 0);
    let tag = arg(args, "--tag").unwrap_or_else(|| "h".into());
    let leave = parse_at(arg(args, "--leave"));
    let crash = parse_at(arg(args, "--crash"));
    let timeout: u64 = num(args, "--timeout", 900);
    let server = std::env::var("WARPZONE_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.into());
    let exe = std::env::current_exe()?;
    let homes = root().join("scratch/net/homes");
    let n = locals.len() + spectators;
    let mut pass: Vec<String> = Vec::new();
    for k in [
        "--delay",
        "--jitter",
        "--loss",
        "--mode",
        "--length",
        "--tower",
        "--variants",
        "--teams",
        "--kind",
        "--coop-index",
        "--until-wave",
        "--until-level",
        "--difficulty",
        "--dw-assist",
        "--dw-menu",
        "--dw-level",
        "--max-frames",
        "--procedural-seed",
        "--input-delay",
    ] {
        if let Some(v) = arg(args, k) {
            pass.push(k.into());
            pass.push(v);
        }
    }
    for k in ["--relay", "--private", "--no-wide", "--hardcore"] {
        if flag(args, k) {
            pass.push(k.into());
        }
    }
    println!(
        "net-harness: {n} machines, slots {locals:?}, {spectators} spectators, {} ({server})",
        pass.join(" ")
    );
    let (tx, rx) = mpsc::channel::<(usize, String)>();
    let mut children: Vec<Child> = Vec::new();
    let spawn = |i: usize, extra: Vec<String>, children: &mut Vec<Child>| -> Result<()> {
        let home = homes.join(format!("{tag}-m{i}"));
        std::fs::create_dir_all(&home)?;
        let mut c = Command::new(&exe);
        c.arg("--net-peer").args([
            "--index",
            &i.to_string(),
            "--slots",
            &locals.get(i).copied().unwrap_or(0).to_string(),
        ]);
        c.args(&pass).args(extra);
        for (opt, at) in [("--leave-at", leave), ("--crash-at", crash)] {
            if let Some((m, f)) = at
                && m == i
            {
                c.args([opt, &f.to_string()]);
            }
        }
        c.env("WARPZONE_HOME", &home)
            .env("WARPZONE_SERVER", &server)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = c.spawn().context("spawn peer")?;
        let out = child.stdout.take().context("stdout")?;
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send((i, line));
            }
        });
        children.push(child);
        Ok(())
    };
    spawn(
        0,
        vec!["--host".into(), "--expect".into(), n.to_string()],
        &mut children,
    )?;
    let t0 = Instant::now();
    let (mut room, mut code) = (String::new(), String::new());
    while room.is_empty() {
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok((i, l)) => {
                println!("[m{i}] {l}");
                if let Some(rest) = l.strip_prefix("ROOM ") {
                    let mut it = rest.split_whitespace();
                    room = it.next().unwrap_or_default().into();
                    code = it.next().unwrap_or_default().into();
                }
                if l.starts_with("ERROR") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    if room.is_empty() {
        eprintln!("net-harness: the host opened no room (is scripts/net/dev-server.sh running?)");
        for c in &mut children {
            let _ = c.kill();
        }
        return Ok(1);
    }
    for i in 1..n {
        // Private (unlisted) rooms only take the code; public ones are
        // joined by room id (the launcher path) and, when they have a code,
        // every other machine by code.
        let by_code = code != "-" && (flag(args, "--private") || i % 2 == 1);
        let how = vec![
            "--warpzone-join".into(),
            if by_code { code.clone() } else { room.clone() },
        ];
        spawn(i, how, &mut children)?;
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut results: Vec<(usize, String)> = Vec::new();
    while let Ok((i, l)) = rx.recv_timeout(
        deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1)),
    ) {
        println!("[m{i}] {l}");
        if l.starts_with("RESULT") {
            results.push((i, l));
        } else if l.starts_with("ERROR") {
            break; // fail fast
        }
        if Instant::now() > deadline {
            break;
        }
    }
    for c in &mut children {
        let _ = c.kill();
        let _ = c.wait();
    }
    let field = |l: &str, k: &str| {
        l.split_whitespace()
            .find_map(|w| w.strip_prefix(&format!("{k}=")).map(String::from))
            .unwrap_or_default()
    };
    let expected: Vec<usize> = (0..n)
        .filter(|i| leave.map(|l| l.0) != Some(*i) && crash.map(|c| c.0) != Some(*i))
        .collect();
    let mut got: Vec<usize> = results.iter().map(|r| r.0).collect();
    got.sort();
    let mut sums: Vec<String> = results.iter().map(|(_, l)| field(l, "checksum")).collect();
    sums.sort();
    sums.dedup();
    let sum = |k: &str| -> u64 {
        results
            .iter()
            .map(|(_, l)| field(l, k).parse::<u64>().unwrap_or(0))
            .sum()
    };
    let agreed = results.iter().all(|(_, l)| field(l, "agreed") == "true");
    let ok = got == expected && sums.len() == 1 && agreed && sum("desyncs") == 0;
    let first = results.first().map(|r| r.1.clone()).unwrap_or_default();
    println!(
        "SUMMARY {} machines={n} reported={} checksums_equal={} agreed={agreed} frame={} rounds={} winner={} rollbacks={} resim={} maxdepth={} desyncs={} resumes={} time={:.1}s",
        if ok { "PASS" } else { "FAIL" },
        got.len(),
        sums.len() == 1,
        field(&first, "frame"),
        field(&first, "rounds"),
        field(&first, "winner"),
        sum("rollbacks"),
        sum("resim"),
        results
            .iter()
            .map(|(_, l)| field(l, "maxdepth").parse::<u64>().unwrap_or(0))
            .max()
            .unwrap_or(0),
        sum("desyncs"),
        sum("resumes"),
        t0.elapsed().as_secs_f32()
    );
    Ok(if ok { 0 } else { 1 })
}

fn say(s: impl AsRef<str>) {
    println!("{}", s.as_ref());
}

/// A tower by part of its name (`sacred`), or the first one.
fn find_tower(inst: &tfp_assets::GameInstall, q: &str) -> Result<String> {
    let towers = super::adapter::online_towers(inst)?;
    let q = q.to_ascii_lowercase();
    towers
        .iter()
        .find(|t| t.dir_name.to_ascii_lowercase().contains(&q))
        .or(towers.first())
        .map(|t| t.dir_name.clone())
        .context("no versus towers")
}

pub fn kind_of(s: &str) -> u8 {
    match s {
        "darkworld" | "dw" | "dark-world" => KIND_DARKWORLD,
        "quest" => KIND_QUEST,
        "trials" => KIND_TRIALS,
        _ => KIND_VERSUS,
    }
}

pub fn mode_of(s: &str) -> u8 {
    match s {
        "lms" | "last-man-standing" => 0,
        "tdm" | "teams" | "team-deathmatch" => 2,
        _ => 1,
    }
}

pub fn length_of(s: &str) -> u8 {
    match s {
        "instant" => 0,
        "quick" => 1,
        "epic" => 3,
        _ => 2,
    }
}

/// `--variants "NO DODGING,MAX ARROWS@0.0+2.1"`: titles on for everyone, or
/// for single archers `TITLE@machine.player` joined by `+`.
pub fn parse_variants(spec: &str, s: &mut OnlineSettings) {
    for item in spec.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let title = item
            .split('@')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_uppercase();
        match item.split_once('@') {
            None => s.variants.push(title),
            Some((_, who)) => {
                let owners: Vec<(u8, u8)> = who
                    .split('+')
                    .filter_map(|o| {
                        let (m, p) = o.split_once('.')?;
                        Some((m.trim().parse().ok()?, p.trim().parse().ok()?))
                    })
                    .collect();
                s.player_variants.push((title, owners));
            }
        }
    }
}

/// `--net-peer ...`: one machine.
pub fn peer(args: &[String]) -> Result<i32> {
    let index: usize = num(args, "--index", 0);
    let nslots: usize = num(args, "--slots", 1);
    let host = flag(args, "--host");
    let expect: usize = num(args, "--expect", 2);
    let leave_at: Option<i32> = arg(args, "--leave-at").and_then(|v| v.parse().ok());
    let crash_at: Option<i32> = arg(args, "--crash-at").and_then(|v| v.parse().ok());
    let installs = tfp_assets::Installs::discover();
    let inst = installs
        .steam
        .clone()
        .or(installs.eight_player.clone())
        .context("game data not found (TF_GAME_DIR)")?;
    // Every machine preloads the tower the harness plays (the interactive
    // lobby loads it when the host's settings name it).
    let tower_dir = find_tower(
        &inst,
        &arg(args, "--tower").unwrap_or_else(|| "sacred".into()),
    )?;
    let t_load = Instant::now();
    let info = MachineInfo::of(&installs);
    let wide_names = wide_tower_names(&installs);
    let mut towers = TowerCache::default();
    let mut coop: Option<CoopData> = None;
    let (base_archers, mut host_settings) = {
        let t = towers.get(&installs, &tower_dir, false, NO_KEY)?;
        say(format!(
            "loaded {} ({} levels, {} archers) in {} ms; 8-player build: {}",
            t.dir_name,
            t.levels.len(),
            t.base_archers,
            t_load.elapsed().as_millis(),
            info.wide.is_some()
        ));
        (t.base_archers, t.settings())
    };
    let mut cfg = NetConfig::new(&super::net_version());
    cfg.link_sim = LinkSim {
        delay_ms: num(args, "--delay", 0u32),
        jitter_ms: num(args, "--jitter", 0u32),
        loss: num(args, "--loss", 0.0f32) / 100.0,
    };
    cfg.force_relay = flag(args, "--relay");
    if let Some(d) = arg(args, "--input-delay").and_then(|v| v.parse().ok()) {
        cfg.ggrs.input_delay = d;
    }
    let home = std::env::var("WARPZONE_HOME").unwrap_or_default();
    let h = home
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let mut name = format!("tfp{index}x{:04x}", h & 0xffff);
    cfg.auto_register = Some(name.clone());
    let mut net: NetSession<NetWorld> = NetSession::new(cfg).map_err(|e| anyhow::anyhow!("{e}"))?;
    net.set_machine_info(info.encode());
    net.set_local_slots(
        (0..nslots)
            .map(|k| SlotRequest {
                name: format!("M{index}P{}", k + 1),
                archer: ((index * 3 + k) % base_archers.max(1)) as u8,
                skin: (k % 2) as u8,
                team: None,
            })
            .collect(),
    );
    net.set_spectate(nslots == 0);
    net.set_ready(true);
    if host {
        let s = &mut host_settings;
        s.mode = mode_of(&arg(args, "--mode").unwrap_or_else(|| "hh".into()));
        s.length = length_of(&arg(args, "--length").unwrap_or_else(|| "quick".into()));
        s.teams = num(args, "--teams", 2u8).clamp(2, 6);
        if let Some(v) = arg(args, "--variants") {
            parse_variants(&v, s);
        }
        s.kind = kind_of(&arg(args, "--kind").unwrap_or_default());
        if s.kind != KIND_VERSUS {
            let d = coop.get_or_insert(CoopData::load(&inst, s.kind)?);
            s.coop_index = num(args, "--coop-index", 0u16);
            s.hardcore = flag(args, "--hardcore");
            s.difficulty = num(args, "--difficulty", 0u8).min(2);
            s.dw_assist = arg(args, "--dw-assist").and_then(|v| v.parse().ok());
            s.dw_level = arg(args, "--dw-level").and_then(|v| v.parse().ok());
            d.apply_to(s);
        }
        let _ = net.set_settings(s.net_settings(base_archers as u8));
    }
    let until_wave: Option<i32> = arg(args, "--until-wave").and_then(|v| v.parse().ok());
    let until_level: Option<usize> = arg(args, "--until-level").and_then(|v| v.parse().ok());
    let dw_menu: Option<u8> = match arg(args, "--dw-menu").as_deref() {
        Some("continue") if host => Some(super::adapter::ACTION_CONTINUE),
        Some("restart") if host => Some(super::adapter::ACTION_RESTART),
        _ => None,
    };
    let max_frames: Option<i32> = arg(args, "--max-frames").and_then(|v| v.parse().ok());
    let t0 = Instant::now();
    let mut logged_in = false;
    let mut requested = false;
    let mut started = false;
    let mut m: Option<OnlineMatch> = None;
    let mut bots: Vec<(usize, Bot)> = Vec::new();
    let mut ended: Option<(Instant, bool)> = None;
    let mut tower_label = String::new();
    let mut layout: u64 = 0;
    let mut next = Instant::now();
    let mut last_log = Instant::now();
    let mut last_round = u32::MAX;
    loop {
        if t0.elapsed() > Duration::from_secs(1800) {
            say("ERROR timeout");
            return Ok(1);
        }
        net.poll();
        while let Some(e) = net.next_event() {
            match e {
                NetEvent::Warpzone(b) => match *b {
                    Event::LoggedIn {
                        account: Some(a), ..
                    } => {
                        say(format!("logged in as {}", a.username));
                        logged_in = true;
                    }
                    Event::Registered(a) => {
                        say(format!("registered {}", a.username));
                        logged_in = true;
                    }
                    Event::Error { op, code, message } => {
                        say(format!("warpzone error {op}/{code}: {message}"));
                        if op == "register" {
                            name = format!(
                                "{}{}",
                                &name[..name.len().min(12)],
                                t0.elapsed().as_nanos() % 1000
                            );
                            net.register(&name);
                        } else if op == "join" || op == "host" {
                            say("ERROR room");
                            return Ok(1);
                        }
                    }
                    Event::RoomHosted { room, code, .. } => say(format!(
                        "ROOM {} {}",
                        room.id,
                        code.unwrap_or_else(|| "-".into())
                    )),
                    _ => {}
                },
                NetEvent::Welcome { machine } => say(format!("welcome as {machine}")),
                NetEvent::MatchStarted(ms) => {
                    say(format!(
                        "match {} started: {} archers, {} spectators, local {:?}{}",
                        ms.match_id,
                        ms.players.len(),
                        ms.spectators.len(),
                        net.local_slots(),
                        if net.is_spectator() {
                            " (spectating)"
                        } else {
                            ""
                        }
                    ));
                    let os = OnlineSettings::decode(&ms.settings.game).unwrap_or_default();
                    if os.kind != KIND_VERSUS {
                        if coop.as_ref().is_none_or(|c| c.kind != os.kind) {
                            coop = Some(CoopData::load(&inst, os.kind)?);
                        }
                        let d = coop.as_ref().unwrap();
                        layout = d.data_hash;
                        tower_label = format!(
                            "{}_{}",
                            d.names
                                .get(os.coop_index as usize)
                                .cloned()
                                .unwrap_or_default()
                                .replace(' ', "_"),
                            match os.kind {
                                KIND_QUEST => "quest",
                                KIND_TRIALS => "trials",
                                _ => "darkworld",
                            }
                        );
                        match OnlineMatch::new_coop(ms, d) {
                            Ok(mut om) => {
                                om.until_wave = until_wave;
                                om.until_level = until_level;
                                om.stop_at = max_frames;
                                m = Some(om);
                            }
                            Err(e) => {
                                say(format!("ERROR {e:#}"));
                                return Ok(1);
                            }
                        }
                        bots = net
                            .local_slots()
                            .iter()
                            .map(|&s| (s, Bot::new(1000 * index as i32 + s as i32 + 1)))
                            .collect();
                        next = Instant::now();
                        continue;
                    }
                    let tower = match towers.get(&installs, &os.tower, os.wide, procedural_key(&os))
                    {
                        Ok(t) => t,
                        Err(e) => {
                            say(format!("ERROR {e:#}"));
                            return Ok(1);
                        }
                    };
                    layout = tower.data_hash;
                    tower_label = format!(
                        "{}{}",
                        super::adapter::tower_title(&tower.dir_name).replace(' ', "_"),
                        if tower.wide { ":wide" } else { "" }
                    );
                    match OnlineMatch::new(ms, tower) {
                        Ok(mut om) => {
                            om.stop_at = max_frames;
                            m = Some(om);
                        }
                        Err(e) => {
                            say(format!("ERROR {e:#}"));
                            return Ok(1);
                        }
                    }
                    bots = net
                        .local_slots()
                        .iter()
                        .map(|&s| (s, Bot::new(1000 * index as i32 + s as i32 + 1)))
                        .collect();
                    next = Instant::now();
                }
                NetEvent::Running => say("running"),
                NetEvent::PlayersDropped {
                    machines,
                    from_frame,
                    ..
                } => say(format!(
                    "resumed at frame {from_frame} without {machines:?}"
                )),
                NetEvent::Desync { frame, machine, .. } => {
                    say(format!("DESYNC at frame {frame} with {machine}"))
                }
                NetEvent::MatchEnded {
                    agreed, reports, ..
                } => {
                    say(format!(
                        "match ended: {} reports, agreed={agreed}",
                        reports.len()
                    ));
                    ended = Some((Instant::now(), agreed));
                }
                NetEvent::MatchAborted { reason } | NetEvent::RoomLeft { reason } => {
                    if ended.is_none() {
                        say(format!("ERROR aborted: {reason}"));
                        return Ok(1);
                    }
                }
                NetEvent::Rejected { reason } => {
                    say(format!("ERROR rejected: {reason}"));
                    return Ok(1);
                }
                _ => {}
            }
        }
        if logged_in && !requested {
            requested = true;
            if host {
                net.host(HostOptions {
                    max_players: 12,
                    // Private rooms are unlisted and joined by code.
                    visibility: if flag(args, "--private") {
                        Visibility::Unlisted
                    } else {
                        Visibility::Public
                    },
                    entry: Entry::Open,
                    preset: Some("versus".into()),
                    ..Default::default()
                });
            } else if let Some(j) = arg(args, "--warpzone-join") {
                super::join(&mut net, &j);
            }
        }
        if host
            && !started
            && net.lobby().machines.len() >= expect
            && host_settings.kind != KIND_VERSUS
        {
            match net.start_match(None) {
                Ok(_) => started = true,
                Err(e) => say(format!("start: {e}")),
            }
        }
        if host && !started && net.lobby().machines.len() >= expect {
            // The same rule as the lobby: wide towers when every machine
            // has the 8-player build and more than 4 archers play.
            let l = net.lobby();
            let infos: Vec<Option<MachineInfo>> = l
                .machines
                .iter()
                .map(|m| MachineInfo::decode(&m.info))
                .collect();
            let size = tower_size(
                &infos,
                l.preview.len(),
                wide_names.contains(&host_settings.tower),
            );
            let wide = size == TowerSize::Wide && !flag(args, "--no-wide");
            say(format!(
                "towers: {}",
                if wide {
                    TowerSize::Wide.label()
                } else {
                    size.label()
                }
            ));
            // Procedural towers: a fresh layout seed per match (or forced).
            host_settings.procedural_seed = arg(args, "--procedural-seed")
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(random_procedural_seed);
            let key = procedural_key(&host_settings);
            if let Ok(t) = towers.get(&installs, &host_settings.tower.clone(), wide, key) {
                t.apply_to(&mut host_settings);
                let _ = net.set_settings(host_settings.net_settings(base_archers as u8));
            }
            match net.start_match(None) {
                Ok(_) => started = true,
                Err(e) => say(format!("start: {e}")),
            }
        }
        if let Some((t, agreed)) = ended
            && (!host || t.elapsed() > Duration::from_secs(2))
        {
            let s = net
                .last_match_stats()
                .cloned()
                .unwrap_or_else(|| net.stats());
            let (frame, sum) = m.as_ref().and_then(|m| m.done).unwrap_or((0, 0));
            let rounds = m
                .as_ref()
                .and_then(|m| m.game.world.versus.as_ref().map(|v| v.round_index + 1))
                .unwrap_or(0);
            let winner = m
                .as_ref()
                .and_then(|m| m.game.world.versus.as_ref().map(|v| v.winner))
                .unwrap_or(-1);
            let ping = s
                .machines
                .iter()
                .filter_map(|x| x.ggrs.as_ref().map(|g| g.ping))
                .max()
                .unwrap_or(0);
            if let Some(om) = &m {
                say(format!("final: {}", om.summary()));
            }
            say(format!(
                "RESULT machine={index} slots={nslots} spectator={} tower={tower_label} layout={layout:016x} capped={} frame={frame} checksum={sum:032x} agreed={agreed} rounds={rounds} winner={winner} rollbacks={} resim={} maxdepth={} stalls={} desyncs={} resumes={} ping={ping}",
                nslots == 0,
                m.as_ref().is_some_and(|m| m.capped()),
                s.rollbacks,
                s.resimulated_frames,
                s.max_rollback_depth,
                s.prediction_stalls,
                s.desyncs,
                s.resumes,
            ));
            return Ok(0);
        }
        let now = Instant::now();
        if now >= next {
            let step = if net.frames_ahead() > 0 {
                STEP * 11 / 10
            } else {
                STEP
            };
            next += step;
            if now > next + STEP * 10 {
                next = now;
            }
            if let Some(om) = m.as_mut()
                && net.current_match().is_some()
            {
                let w = &om.game.world;
                let c = om.game.content.clone();
                let mut local: Vec<(usize, PlayerInput)> = bots
                    .iter_mut()
                    .map(|(s, b)| (*s, b.input(w, &c, *s as u8)))
                    .collect();
                // Tests: the host answers the Dark World game-over menu
                // (its choice travels in slot 0's input; guests only watch).
                if let Some(action) = dw_menu
                    && w.dark_world.as_ref().is_some_and(|d| {
                        d.phase == tfp_sim::darkworld::DwPhase::GameOver && d.menu_ready
                    })
                    && let Some(first) = local.iter_mut().find(|(s, _)| *s == 0)
                {
                    first.1._pad = action;
                }
                om.step(&mut net, &local);
                if let Some(v) = &om.game.world.versus
                    && v.round_index != last_round
                {
                    last_round = v.round_index;
                    say(format!("frame {}: {}", om.frame, om.summary()));
                }
                let f = net.current_frame();
                if leave_at.is_some_and(|l| f >= l) {
                    say(format!("leaving at frame {f}"));
                    net.leave();
                    let t = Instant::now();
                    while t.elapsed() < Duration::from_millis(500) {
                        net.poll();
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    return Ok(0);
                }
                if crash_at.is_some_and(|c| f >= c) {
                    say(format!("crashing at frame {f}"));
                    std::process::exit(3);
                }
                if last_log.elapsed() > Duration::from_secs(15) {
                    last_log = Instant::now();
                    let s = net.stats();
                    say(format!(
                        "frame {} confirmed {} rollbacks {} stalls {} {}",
                        s.current_frame,
                        s.confirmed_frame,
                        s.rollbacks,
                        s.prediction_stalls,
                        om.summary()
                    ));
                }
            }
        }
        let wait = next
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(2));
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
}

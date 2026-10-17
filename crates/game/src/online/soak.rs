//! `--net-soak`: many random online matches against a Warpzone server, one
//! `--net-harness` run each (separate processes per machine), with random
//! towers, modes, 2-12 archers on 2-4 machines, variants (global and per
//! archer), treasure variants, latency and loss. Prints one line per match
//! and a statistics summary; logs go to `scratch/net/soak/<n>.log`.
//!
//! ```sh
//! towerfall-port --net-soak --count 30 [--seed 1] [--parallel 2] [--timeout 600]
//! ```

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result};
use tfp_sim::variants::Variant;

use super::adapter::online_towers;

pub struct Rng(pub u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn range(&mut self, a: u64, b: u64) -> u64 {
        a + self.below(b - a + 1)
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

/// One random match.
#[derive(Clone, Debug)]
pub struct Case {
    pub locals: Vec<usize>,
    pub mode: &'static str,
    pub length: &'static str,
    pub teams: u8,
    pub tower: String,
    pub variants: String,
    pub delay: u32,
    pub jitter: u32,
    pub loss: u32,
    pub private: bool,
    /// Co-op instead of Versus: (kind "quest"/"darkworld", tower index,
    /// difficulty).
    pub coop: Option<(&'static str, usize, u8)>,
}

impl Case {
    fn args(&self) -> Vec<String> {
        let mut v: Vec<String> = vec![
            "--locals".into(),
            self.locals
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(","),
            "--mode".into(),
            self.mode.into(),
            "--length".into(),
            self.length.into(),
            "--teams".into(),
            self.teams.to_string(),
            "--tower".into(),
            self.tower.clone(),
            "--delay".into(),
            self.delay.to_string(),
            "--jitter".into(),
            self.jitter.to_string(),
            "--loss".into(),
            self.loss.to_string(),
        ];
        if !self.variants.is_empty() {
            v.push("--variants".into());
            v.push(self.variants.clone());
        }
        if self.private {
            v.push("--private".into());
        }
        match self.coop {
            Some(("quest", i, _)) => v.extend(
                [
                    "--kind",
                    "quest",
                    "--coop-index",
                    &i.to_string(),
                    "--until-wave",
                    "2",
                ]
                .map(String::from),
            ),
            Some((_, i, d)) => v.extend(
                [
                    "--kind",
                    "darkworld",
                    "--coop-index",
                    &i.to_string(),
                    "--difficulty",
                    &d.to_string(),
                    "--dw-assist",
                    "0",
                    "--until-level",
                    "1",
                ]
                .map(String::from),
            ),
            None => {}
        }
        v
    }

    fn describe(&self) -> String {
        if let Some((k, i, d)) = self.coop {
            return format!(
                "{} {k} tower {i}{} [{}] link {}ms±{} {}%",
                self.locals
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join("+"),
                if k == "darkworld" {
                    format!(" difficulty {d}")
                } else {
                    String::new()
                },
                self.variants,
                self.delay,
                self.jitter,
                self.loss
            );
        }
        format!(
            "{} {} {}{} on {:?} [{}] link {}ms±{} {}%{}",
            self.locals
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("+"),
            self.mode,
            self.length,
            if self.mode == "tdm" {
                format!(" {} teams", self.teams)
            } else {
                String::new()
            },
            self.tower,
            self.variants,
            self.delay,
            self.jitter,
            self.loss,
            if self.private { " private" } else { "" }
        )
    }
}

const TREASURE: &[Variant] = &[
    Variant::NoTreasure,
    Variant::MaxTreasure,
    Variant::AlwaysBigTreasure,
    Variant::BottomlessTreasure,
    Variant::BombChests,
    Variant::SymmetricalTreasure,
];

/// A random case (`towers`: online tower directory names).
/// `coop_towers`: (quest towers, Dark World towers) available; about one
/// match in five is co-op when there are any.
pub(crate) fn random_case_with(
    rng: &mut Rng,
    towers: &[String],
    coop_towers: (usize, usize),
) -> Case {
    let coop = if (coop_towers.0 > 0 || coop_towers.1 > 0) && rng.chance(20) {
        let dw = coop_towers.1 > 0 && (coop_towers.0 == 0 || rng.chance(50));
        Some(if dw {
            (
                "darkworld",
                rng.below(coop_towers.1 as u64) as usize,
                rng.below(3) as u8,
            )
        } else {
            ("quest", rng.below(coop_towers.0 as u64) as usize, 0)
        })
    } else {
        None
    };
    let machines = if coop.is_some() {
        rng.range(2, 3) as usize
    } else {
        rng.range(2, 4) as usize
    };
    let max = if coop.is_some() { 4 } else { 12 };
    let archers = rng.range(machines as u64, max) as usize;
    let mut locals = vec![1usize; machines];
    for _ in machines..archers {
        let k = rng.below(machines as u64) as usize;
        locals[k] += 1;
    }
    let mode = match rng.below(3) {
        0 => "lms",
        1 if archers >= 3 => "tdm",
        _ => "hh",
    };
    let teams = if mode == "tdm" {
        rng.range(2, archers.min(4) as u64) as u8
    } else {
        2
    };
    // Headhunters with many archers runs long: mostly instant.
    let instant = (mode == "hh" && (archers > 4 || rng.chance(50))) || rng.chance(40);
    let length = if instant { "instant" } else { "quick" };
    let mut vs: Vec<String> = Vec::new();
    let pick = |rng: &mut Rng| Variant::ALL[rng.below(Variant::ALL.len() as u64) as usize];
    for _ in 0..rng.below(5) {
        let v = pick(rng);
        if v.per_player() && rng.chance(50) {
            // For one to three single archers: (machine, local player).
            let mut owners = Vec::new();
            for _ in 0..rng.range(1, 3) {
                let m = rng.below(machines as u64) as usize;
                let p = rng.below(locals[m] as u64);
                owners.push(format!("{m}.{p}"));
            }
            vs.push(format!("{}@{}", v.title(), owners.join("+")));
        } else {
            vs.push(v.title().to_string());
        }
    }
    if rng.chance(50) {
        vs.push(
            TREASURE[rng.below(TREASURE.len() as u64) as usize]
                .title()
                .into(),
        );
    }
    if coop.is_some() {
        vs.clear();
    }
    Case {
        coop,
        locals,
        mode,
        length,
        teams,
        tower: towers[rng.below(towers.len() as u64) as usize].clone(),
        variants: vs.join(","),
        delay: rng.range(0, 60) as u32,
        jitter: rng.range(0, 20) as u32,
        loss: rng.range(0, 5) as u32,
        private: rng.chance(30),
    }
}

fn field(l: &str, k: &str) -> String {
    l.split_whitespace()
        .find_map(|w| w.strip_prefix(&format!("{k}=")).map(String::from))
        .unwrap_or_default()
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

struct Outcome {
    n: usize,
    case: Case,
    summary: String,
    secs: f32,
    log: PathBuf,
}

pub fn run(args: &[String]) -> Result<i32> {
    let count: usize = arg(args, "--count")
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let seed: u64 = arg(args, "--seed")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let parallel: usize = arg(args, "--parallel")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    let timeout = arg(args, "--timeout").unwrap_or_else(|| "600".into());
    // A match that has not ended after this many frames (5 min of play)
    // ends there; its checksums are compared all the same ("capped").
    let max_frames = arg(args, "--max-frames").unwrap_or_else(|| "18000".into());
    let installs = tfp_assets::Installs::discover();
    let inst = installs
        .steam
        .clone()
        .or(installs.eight_player.clone())
        .context("game data not found")?;
    let towers: Vec<String> = online_towers(&inst)?
        .into_iter()
        .map(|t| t.dir_name)
        .collect();
    let root = std::env::var_os("TFP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let dir = root.join("scratch/net/soak");
    std::fs::create_dir_all(&dir)?;
    let exe = std::env::current_exe()?;
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    let idx = tfp_assets::towers::TowerIndex::build(&inst)?;
    let coop_towers = (
        idx.quest.len(),
        if inst.dark_world.is_some() {
            idx.dark_world.len()
        } else {
            0
        },
    );
    let cases: Vec<Case> = (0..count)
        .map(|_| random_case_with(&mut rng, &towers, coop_towers))
        .collect();
    println!("net-soak: {count} matches, seed {seed}, {parallel} at a time");
    let t0 = Instant::now();
    let mut outcomes: Vec<Outcome> = Vec::new();
    let mut next = 0usize;
    let mut running: Vec<(usize, std::process::Child, Instant, PathBuf)> = Vec::new();
    while next < cases.len() || !running.is_empty() {
        while running.len() < parallel && next < cases.len() {
            let n = next;
            next += 1;
            let log = dir.join(format!("{n:03}.log"));
            let f = std::fs::File::create(&log)?;
            let mut c = Command::new(&exe);
            c.arg("--net-harness")
                .args(cases[n].args())
                .args(["--tag", &format!("soak{seed}-{n}"), "--timeout", &timeout])
                .args(["--max-frames", &max_frames])
                .stdout(Stdio::from(f.try_clone()?))
                .stderr(Stdio::from(f));
            println!("[{n:03}] start: {}", cases[n].describe());
            running.push((n, c.spawn()?, Instant::now(), log));
        }
        let mut i = 0;
        while i < running.len() {
            if running[i].1.try_wait()?.is_some() {
                let (n, _, t, log) = running.remove(i);
                let text = std::fs::read_to_string(&log).unwrap_or_default();
                let summary = text
                    .lines()
                    .find(|l| l.starts_with("SUMMARY"))
                    .unwrap_or("SUMMARY FAIL (no summary)")
                    .to_string();
                let o = Outcome {
                    n,
                    case: cases[n].clone(),
                    summary,
                    secs: t.elapsed().as_secs_f32(),
                    log,
                };
                println!(
                    "[{n:03}] {} {:.0}s {}",
                    if o.summary.starts_with("SUMMARY PASS") {
                        "PASS"
                    } else {
                        "FAIL"
                    },
                    o.secs,
                    o.summary.trim_start_matches("SUMMARY ")
                );
                outcomes.push(o);
            } else {
                i += 1;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    outcomes.sort_by_key(|o| o.n);
    let pass: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| o.summary.starts_with("SUMMARY PASS"))
        .collect();
    let num = |o: &Outcome, k: &str| field(&o.summary, k).parse::<f64>().unwrap_or(0.0);
    let total = |k: &str| pass.iter().map(|o| num(o, k)).sum::<f64>();
    let archers: usize = pass
        .iter()
        .map(|o| o.case.locals.iter().sum::<usize>())
        .sum();
    let wide = pass
        .iter()
        .filter(|o| {
            std::fs::read_to_string(&o.log)
                .unwrap_or_default()
                .contains(":wide")
        })
        .count();
    let by_mode = |m: &str| {
        pass.iter()
            .filter(|o| o.case.coop.is_none() && o.case.mode == m)
            .count()
    };
    let by_kind = |k: &str| {
        pass.iter()
            .filter(|o| o.case.coop.is_some_and(|c| c.0 == k))
            .count()
    };
    let cataclysm = pass
        .iter()
        .filter(|o| {
            o.case.coop.is_none() && o.case.tower.to_ascii_lowercase().contains("cataclysm")
        })
        .count();
    let capped = pass
        .iter()
        .filter(|o| {
            std::fs::read_to_string(&o.log)
                .unwrap_or_default()
                .contains("capped=true")
        })
        .count();
    println!("---");
    println!(
        "SOAK {}/{} PASS in {:.1} min: {} archers total, modes lms {} hh {} tdm {}, quest {} dark world {}, {} on Cataclysm, {} on wide towers, {} with variants, {} capped at the frame limit",
        pass.len(),
        outcomes.len(),
        t0.elapsed().as_secs_f32() / 60.0,
        archers,
        by_mode("lms"),
        by_mode("hh"),
        by_mode("tdm"),
        by_kind("quest"),
        by_kind("darkworld"),
        cataclysm,
        wide,
        pass.iter().filter(|o| !o.case.variants.is_empty()).count(),
        capped,
    );
    println!(
        "  frames {:.0} (mean {:.0}), rounds {:.0}, rollbacks {:.0}, re-simulated {:.0}, deepest rollback {:.0}, desyncs {:.0}, mean match {:.0}s",
        total("frame"),
        total("frame") / pass.len().max(1) as f64,
        total("rounds"),
        total("rollbacks"),
        total("resim"),
        pass.iter().map(|o| num(o, "maxdepth")).fold(0.0, f64::max),
        total("desyncs"),
        pass.iter().map(|o| o.secs as f64).sum::<f64>() / pass.len().max(1) as f64,
    );
    for o in outcomes
        .iter()
        .filter(|o| !o.summary.starts_with("SUMMARY PASS"))
    {
        println!(
            "  FAIL [{:03}] {} -> {} ({})",
            o.n,
            o.case.describe(),
            o.summary,
            o.log.display()
        );
    }
    Ok(if pass.len() == outcomes.len() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_cases_are_valid() {
        let mut rng = Rng(7);
        let towers = vec!["00 - A".to_string(), "01 - B".to_string()];
        for _ in 0..500 {
            let c = random_case_with(&mut rng, &towers, (3, 2));
            if let Some((_, i, d)) = c.coop {
                let n: usize = c.locals.iter().sum();
                assert!((2..=3).contains(&c.locals.len()) && n <= 4 && i < 3 && d < 3);
                assert!(c.variants.is_empty());
                continue;
            }
            let n: usize = c.locals.iter().sum();
            assert!((2..=4).contains(&c.locals.len()));
            assert!((2..=12).contains(&n) && c.locals.iter().all(|&k| k >= 1));
            if c.mode == "tdm" {
                assert!(n >= 3 && (2..=4).contains(&c.teams) && c.teams as usize <= n);
            }
            let mut s = super::super::adapter::OnlineSettings::default();
            super::super::harness::parse_variants(&c.variants, &mut s);
            for (_, owners) in &s.player_variants {
                for (m, p) in owners {
                    assert!((*p as usize) < c.locals[*m as usize]);
                }
            }
        }
    }
}

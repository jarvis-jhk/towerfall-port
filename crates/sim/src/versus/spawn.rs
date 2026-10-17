//! Round start spawning (versus.md §5.1, §5.2, §4.6) and the derived spawn
//! points for more archers than a level offers (§15.3).

use crate::actor::wrap_dist_sq;
use crate::collide::{IRect, grid_rect};
use crate::content::{Content, LevelDef};
use crate::entity::Allegiance;
use crate::math::Vec2;
use crate::player::{self, HatState};
use crate::quiver::{ArrowType, Quiver};
use crate::rng::Rng;
use crate::variants::Variant;
use crate::versus::Match;
use crate::versus::levels::{SpawnTier, authored_pool};
use crate::world::{MAX_PLAYERS, PlayerSetup, World};

/// Minimum distance between derived spawn points (px).
const MIN_GAP: f32 = 24.0;

fn standing_ok(lvl: &LevelDef, x: f32, floor_y: f32) -> bool {
    // 8x14 archer box with its feet on the floor top.
    let r = IRect {
        x: (x - 4.0) as i32,
        y: (floor_y - 14.0) as i32,
        w: 8,
        h: 14,
    };
    if grid_rect(lvl, &r) {
        return false;
    }
    !lvl.static_solids.iter().any(|s| {
        (s.pos.x as i32) < r.right()
            && r.x < (s.pos.x + s.w) as i32
            && (s.pos.y as i32) < r.bottom()
            && r.y < (s.pos.y + s.h) as i32
    })
}

/// Every standing position of a level as a spawn marker (feet on a floor
/// top at `marker.y + 10`), minus spots next to chest markers.
fn candidates(lvl: &LevelDef) -> Vec<Vec2> {
    let mut out = vec![];
    for col in 0..lvl.cells_x {
        let x = col as f32 * 10.0 + 5.0;
        for row in 1..lvl.cells_y {
            let top = row as f32 * 10.0;
            let floor = lvl.solid(col as i32, row as i32)
                || lvl
                    .static_solids
                    .iter()
                    .any(|s| s.pos.y == top && s.pos.x <= x && x < s.pos.x + s.w)
                || lvl
                    .jump_thrus
                    .iter()
                    .any(|j| j.pos.y == top && j.pos.x <= x && x < j.pos.x + j.width);
            if !floor || !standing_ok(lvl, x, top) {
                continue;
            }
            let m = Vec2::new(x, top - 10.0);
            if lvl
                .treasure
                .iter()
                .chain(&lvl.big_treasure)
                .any(|t| t.dist_sq(m) < 12.0 * 12.0)
            {
                continue;
            }
            out.push(m);
        }
    }
    out
}

/// `DeriveSpawns(level, need)`: the authored pool (seeded order) plus
/// farthest-point samples on the wrapped torus, >= 24 px apart, mirrored
/// pairs where possible. May return fewer than `need`.
pub fn derive_spawns(_c: &Content, lvl: &LevelDef, need: usize, rng: &mut Rng) -> Vec<Vec2> {
    let (w, h) = (lvl.width as f32, lvl.height as f32);
    let mut chosen = authored_pool(lvl);
    rng.shuffle(&mut chosen);
    if chosen.len() >= need {
        chosen.truncate(need);
        return chosen;
    }
    let cands = candidates(lvl);
    let min_d = |p: Vec2, chosen: &[Vec2]| {
        chosen
            .iter()
            .map(|&q| wrap_dist_sq(p, q, w, h))
            .fold(f32::INFINITY, crate::math::min)
    };
    while chosen.len() < need {
        let mut best: Option<(usize, f32)> = None;
        for (i, &p) in cands.iter().enumerate() {
            let d = min_d(p, &chosen);
            if d < MIN_GAP * MIN_GAP {
                continue;
            }
            if best.is_none_or(|(_, bd)| d > bd) {
                best = Some((i, d));
            }
        }
        let Some((i, _)) = best else { break };
        let p = cands[i];
        chosen.push(p);
        let mirror = Vec2::new(w - p.x, p.y);
        if chosen.len() < need
            && mirror != p
            && cands.contains(&mirror)
            && min_d(mirror, &chosen) >= MIN_GAP * MIN_GAP
        {
            chosen.push(mirror);
        }
    }
    chosen
}

/// The FFA spawn list of a level for `n` archers (tier rule).
fn ffa_points(c: &Content, lvl: &LevelDef, tier: SpawnTier, n: usize, rng: &mut Rng) -> Vec<Vec2> {
    match tier {
        SpawnTier::Authored => lvl.player_spawns.clone(),
        SpawnTier::Pool => authored_pool(lvl),
        SpawnTier::Derived => derive_spawns(c, lvl, n, rng),
    }
}

/// `x` comparison of the team-spawn sorts: differences below 0.1 are equal.
fn cmp10(a: f32, b: f32) -> std::cmp::Ordering {
    (((a - b) * 10.0) as i32).cmp(&0)
}

/// Spawn the round's archers (frozen, flashing) and fill the round's alive
/// flags. Called right after the level scene was built.
pub fn spawn_round(w: &mut World, c: &Content, m: &mut Match) {
    let lvl = c.level(w.level);
    let width = lvl.width as f32;
    let set = m.settings.clone();
    let spawning: Vec<usize> = (0..MAX_PLAYERS).filter(|&p| m.should_spawn(p)).collect();
    let n = spawning.len();
    let tier = m.levels.tier.unwrap_or(SpawnTier::Derived);
    let mut assigned: Vec<(usize, Vec2)> = vec![];
    if !set.team_mode() {
        let mut pts = ffa_points(c, lvl, tier, n, &mut m.rng);
        m.rng.shuffle(&mut pts);
        if pts.is_empty() {
            pts.push(Vec2::new(width / 2.0, 0.0));
        }
        if n == 2 && pts.len() >= 2 && pts[0].x != width / 2.0 {
            let mirror = Vec2::new(width - pts[0].x, pts[0].y);
            if pts.contains(&mirror) {
                pts[1] = mirror;
            }
        }
        for (k, &p) in spawning.iter().enumerate() {
            assigned.push((p, pts[k % pts.len()]));
        }
    } else if set.teams == 2 && tier == SpawnTier::Authored {
        let mut a = lvl.team_spawns_a.clone();
        let mut b = lvl.team_spawns_b.clone();
        a.sort_by(|p, q| cmp10(p.y, q.y).then(cmp10(q.x, p.x)));
        b.sort_by(|p, q| cmp10(p.y, q.y).then(cmp10(p.x, q.x)));
        let seed = m.rng.next_i32();
        Rng::new(seed).shuffle(&mut a);
        Rng::new(seed).shuffle(&mut b);
        let (mut ka, mut kb) = (0, 0);
        for team in 0..2u8 {
            for &p in &spawning {
                if set.slots[p].map(|s| s.team) != Some(team) {
                    continue;
                }
                let pt = if team == 0 {
                    ka += 1;
                    a[(ka - 1) % a.len().max(1)]
                } else {
                    kb += 1;
                    b[(kb - 1) % b.len().max(1)]
                };
                assigned.push((p, pt));
            }
        }
    } else {
        // Derived team spawns / 3+ teams: contiguous bands along x.
        let mut pts = ffa_points(c, lvl, tier, n, &mut m.rng);
        if pts.is_empty() {
            pts.push(Vec2::new(width / 2.0, 0.0));
        }
        pts.sort_by(|p, q| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)));
        let t = set.teams as usize;
        let order: Vec<u8> = if t == 2 {
            vec![0, 1]
        } else {
            let r = m.round_index as usize % t;
            (0..t).map(|i| m.team_order[(i + r) % t]).collect()
        };
        let mut k = 0;
        for team in order {
            let mut members: Vec<usize> = spawning
                .iter()
                .copied()
                .filter(|&p| set.slots[p].map(|s| s.team) == Some(team))
                .collect();
            m.rng.shuffle(&mut members);
            for p in members {
                assigned.push((p, pts[k % pts.len()]));
                k += 1;
            }
        }
        assigned.sort_by_key(|&(p, _)| p);
    }
    let random_type = m.round.random_arrow_type;
    for (p, pt) in assigned {
        let ps = inventory(m, p, random_type);
        let id = player::create(w, c, p as u8, pt + Vec2::new(0.0, 2.0), &ps.0);
        if let Some(pl) = w.player_mut(id) {
            let mut q = Quiver::new(ps.0.arrows as usize, ps.1);
            if set.variant(Variant::NoQuivers, p as i32) {
                q.set_max(1);
            } else if set.variant(Variant::SmallQuivers, p as i32) {
                q.set_max(3);
            }
            pl.arrows = q;
            pl.start_arrow = ps.1;
        }
        if ps.0.hat == HatState::Crown {
            m.stats[p].crown_spawns += 1;
        }
        m.round.alive[p] = true;
        m.round.spawned[p] = true;
    }
    m.round.players_at_start = m.round.spawned.iter().filter(|&&s| s).count() as u8;
}

/// Does slot `p` spawn with a shield (Start With Shields or the
/// autobalance catch-up shield, versus.md §4.6)?
pub fn spawn_shield(m: &Match, p: usize) -> bool {
    let set = &m.settings;
    let pi = p as i32;
    let idx = set.score_index(p);
    set.variant(Variant::StartWithShields, pi)
        || (!m.round.overtime
            && !set.variant(Variant::NoAutobalance, pi)
            && !set.variant(Variant::WeakAutobalance, pi)
            && m.score_lead(idx) <= -set.autobalance_losing_amount())
}

/// `GetPlayerInventory` (versus.md §4.6) plus the round's arrow type.
fn inventory(m: &Match, p: usize, random_type: ArrowType) -> (PlayerSetup, ArrowType) {
    let set = &m.settings;
    let pi = p as i32;
    let idx = set.score_index(p);
    let crowned = m.team_has_crown(idx, &m.scores);
    let no_auto = set.variant(Variant::NoAutobalance, pi);
    let arrows = if set.variant(Variant::NoArrows, pi) {
        0
    } else if set.variant(Variant::SingleArrow, pi) {
        1
    } else if set.variant(Variant::MaxArrows, pi) {
        if crowned && !no_auto { 4 } else { 6 }
    } else {
        let mut base = 3;
        if set.team_mode() {
            let mine = set.player_team_size(p);
            let team = set.slots[p].map(|s| s.team).unwrap_or(0);
            let bigger = (0..set.teams)
                .filter(|&t| t != team)
                .all(|t| set.team_size(t) < mine);
            if bigger {
                base = 2;
            }
        }
        if !no_auto && crowned {
            (base - 1).max(2)
        } else {
            base
        }
    };
    let shield = spawn_shield(m, p);
    let team = set.slots[p].map(|s| s.team).unwrap_or(0);
    let allegiance = if set.team_mode() {
        Allegiance::team(team)
    } else {
        Allegiance::Neutral
    };
    let ps = PlayerSetup {
        archer: set.slots[p].map(|s| s.archer).unwrap_or(0),
        allegiance,
        team_color: allegiance,
        hat: if crowned {
            HatState::Crown
        } else {
            HatState::Normal
        },
        arrows,
        shield,
        wings: set.variant(Variant::StartWithWings, pi),
        speed_boots: set.variant(Variant::StartWithSpeedBoots, pi),
        invisible: set.variant(Variant::StartInvisible, pi),
        spawn: None,
    };
    (ps, set.start_arrow_type(pi).unwrap_or(random_type))
}

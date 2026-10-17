//! Building [`Content`] from a game install through `tfp-assets` (reads
//! files; call it once at startup, never from `World::step`).

use crate::content::{Content, LevelDef};
use std::path::{Path, PathBuf};
use tfp_assets::atlas::{Atlas, AtlasKind};
use tfp_assets::game_data::GameData;
use tfp_assets::install::GameInstall;
use tfp_assets::level::Level;
use tfp_assets::sprite_data::SpriteData;
use tfp_assets::towers::{TowerIndex, game_relative_path, versus_load_seed};

/// The Cataclysm layout seed used when the caller does not pick one
/// (`RandomLevelSeed` is a match setting; see [`content_from_install_seeded`]).
pub use crate::versus::settings::DEFAULT_PROCEDURAL_SEED;

/// Content with the given level files plus the archers' sprites.
pub fn content_from_install(inst: &GameInstall, levels: &[PathBuf]) -> tfp_assets::Result<Content> {
    content_from_install_seeded(inst, levels, DEFAULT_PROCEDURAL_SEED)
}

/// [`content_from_install`] with the procedural (Cataclysm) layout seed.
pub fn content_from_install_seeded(
    inst: &GameInstall,
    levels: &[PathBuf],
    procedural_seed: i32,
) -> tfp_assets::Result<Content> {
    content_for_match(inst, levels, (procedural_seed, false))
}

/// Content for a Versus match: procedural layouts generated from the
/// match's [`MatchSettings::procedural_key`](crate::versus::MatchSettings::procedural_key)
/// (seed, Team Deathmatch).
pub fn content_for_match(
    inst: &GameInstall,
    levels: &[PathBuf],
    procedural: (i32, bool),
) -> tfp_assets::Result<Content> {
    let (procedural_seed, teams) = procedural;
    let gd = GameData::load(&inst.content)?;
    let towers = TowerIndex::build(inst).ok();
    let mut defs = vec![];
    for path in levels {
        let mut lvl = Level::load(path)?;
        let tower = towers
            .as_ref()
            .and_then(|t| t.versus.iter().find(|v| v.levels.iter().any(|l| l == path)));
        if tower.is_some_and(|t| t.procedural) {
            let g = tfp_assets::procedural::generate(&lvl, procedural_seed, teams);
            lvl = lvl.with_generated(&g);
        }
        let rel = game_relative_path(inst, path).unwrap_or_else(|| path.display().to_string());
        let mut def = LevelDef::from_level(&level_name(path), &lvl, versus_load_seed(&rel));
        // The theme comes from whichever tower list owns the file
        // (Versus, Quest, Dark World or Trials).
        let theme_name = tower.map(|t| t.theme.as_str()).or_else(|| {
            let t = towers.as_ref()?;
            t.quest
                .iter()
                .find(|q| &q.level == path)
                .map(|q| q.theme.as_str())
                .or_else(|| {
                    t.dark_world
                        .iter()
                        .find(|d| d.levels.iter().any(|l| l == path))
                        .map(|d| d.theme.as_str())
                })
                .or_else(|| {
                    t.trials
                        .iter()
                        .find(|l| &l.path == path)
                        .map(|l| l.theme.as_str())
                })
        });
        if let Some(theme) = theme_name.and_then(|n| gd.theme(n)) {
            def.tower.theme = crate::entities::ThemeInfo::from_theme(theme);
        }
        defs.push(def);
    }
    let mut c = Content::with_levels(defs);
    let sd = inst.content.sprite_data_dir();
    let sprites = SpriteData::load(&sd.join("spriteData.xml"))?;
    let corpses = SpriteData::load(&sd.join("corpseSpriteData.xml")).ok();
    c.load_archers(&sprites, corpses.as_ref(), &gd.archers);
    crate::view_items::load_art(&mut c, &sprites);
    crate::entities::art::register(&mut c, &sprites);
    if let Some(x) = inst.atlas_xml(AtlasKind::Main) {
        let atlas = Atlas::load(&x)?;
        c.set_texture_sizes(AtlasKind::Main, &atlas);
    }
    Ok(c)
}

/// The Versus level files of an install, tower by tower.
pub fn versus_levels(inst: &GameInstall) -> tfp_assets::Result<Vec<PathBuf>> {
    let idx = TowerIndex::build(inst)?;
    Ok(idx
        .versus
        .iter()
        .flat_map(|t| t.levels.iter().cloned())
        .collect())
}

fn level_name(p: &Path) -> String {
    let file = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tower = p
        .parent()
        .and_then(|d| d.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{tower}/{file}")
}

/// Content for Dark World (darkworld.md §2): every Dark World tower's
/// arenas (file order, towers in index order) and the tower data
/// (`Content::dark_world[i]`, tower id = index). Empty without the
/// expansion.
pub fn dark_world_content(inst: &GameInstall) -> tfp_assets::Result<Content> {
    let idx = TowerIndex::build(inst)?;
    let mut paths: Vec<PathBuf> = vec![];
    let mut firsts = vec![];
    for t in &idx.dark_world {
        firsts.push(paths.len());
        paths.extend(t.levels.iter().cloned());
    }
    let mut c = content_from_install(inst, &paths)?;
    for (i, t) in idx.dark_world.iter().enumerate() {
        c.dark_world.push(crate::darkworld::DwTower::from_assets(
            i as u8, t, firsts[i],
        ));
    }
    Ok(c)
}

/// Content for Quest: every quest tower's arena (`levels[i]`) and wave
/// script (`quest_scripts[i]`), tower id = index, plus the archers.
pub fn quest_content(inst: &GameInstall) -> tfp_assets::Result<Content> {
    let idx = TowerIndex::build(inst)?;
    let levels: Vec<PathBuf> = idx.quest.iter().map(|q| q.level.clone()).collect();
    let mut c = content_from_install(inst, &levels)?;
    for q in &idx.quest {
        let data = tfp_assets::quest_data::QuestData::load(&q.data)?;
        let script = crate::quest::QuestScript::from_data(&q.name, &data)
            .map_err(|e| tfp_assets::Error::format(&q.data, e))?;
        c.quest_scripts.push(script);
    }
    Ok(c)
}

/// Trials levels in `trials.xml` order: content (`levels[i]`) and the
/// settings of each (medal times, arrows, switch timer).
pub fn trials_content(
    inst: &GameInstall,
) -> tfp_assets::Result<(Content, Vec<crate::trials::TrialsSettings>)> {
    let idx = TowerIndex::build(inst)?;
    let levels: Vec<PathBuf> = idx.trials.iter().map(|t| t.path.clone()).collect();
    let c = content_from_install(inst, &levels)?;
    let settings = idx
        .trials
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut s = crate::trials::TrialsSettings::new(i);
            s.arrows = t.arrows.clamp(0, 16) as u8;
            s.switch_timer = t.switch_timer.max(0) as u16;
            s.gold = t.goals[0];
            s.diamond = t.goals[1];
            s.dev = t.goals[2];
            s.seed = (t.index * 10 + t.tier) as i32;
            s
        })
        .collect();
    Ok((c, settings))
}

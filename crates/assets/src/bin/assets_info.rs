//! `assets-info`: inventory of every install found — atlases, sprites, game
//! data, levels, sounds, music — parsed with `tfp-assets`.
//!
//! Usage: `assets-info [--decode-music]` (the flag decodes every music wave
//! and reports the time taken).

use std::time::Instant;
use tfp_assets::atlas::{Atlas, AtlasKind};
use tfp_assets::audio::xgs::GlobalSettings;
use tfp_assets::audio::xsb::SoundBank;
use tfp_assets::audio::xwb::WaveBank;
use tfp_assets::audio::{Pcm, Wav};
use tfp_assets::game_data::GameData;
use tfp_assets::install::Installs;
use tfp_assets::level::Level;
use tfp_assets::sprite_data::{SPRITE_DATA_FILES, SpriteData};
use tfp_assets::towers::TowerIndex;
use tfp_assets::workshop::WorkshopTower;
use tfp_assets::xnb;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let decode_music = std::env::args().any(|a| a == "--decode-music");
    let installs = Installs::discover();
    for inst in installs.iter() {
        println!("== {}", inst.summary());
        let mut subs = 0;
        let mut atlases = 0;
        for set in inst.content_sets() {
            for k in AtlasKind::ALL {
                let p = set.atlas_xml(k);
                if p.is_file() {
                    let a = Atlas::load(&p)?;
                    atlases += 1;
                    subs += a.len();
                    println!("  atlas {:<40} {:>4} subtextures", rel(inst, &p), a.len());
                }
            }
        }
        println!("  {atlases} atlases, {subs} subtextures");
        let mut sprites = 0;
        let mut anims = 0;
        for (stem, _) in SPRITE_DATA_FILES {
            let sd = SpriteData::load(&inst.content.sprite_data_dir().join(format!("{stem}.xml")))?;
            sprites += sd.len();
            anims += sd.sprites.iter().map(|s| s.animations.len()).sum::<usize>();
        }
        println!("  sprite data: {sprites} sprites, {anims} animations");
        let gd = GameData::load(&inst.content)?;
        println!(
            "  game data: {} archers (+{} alt, +{} secret), {} themes, {} tilesets, {} backgrounds, {}+{} tips",
            gd.archers.archers.len(),
            gd.archers.alts.iter().flatten().count(),
            gd.archers.secrets.iter().flatten().count(),
            gd.themes.len(),
            gd.tilesets.len(),
            gd.backgrounds.len(),
            gd.versus_tips.len(),
            gd.quest_tips.len()
        );
        let idx = TowerIndex::build(inst)?;
        let files = idx.all_level_files();
        let mut entities = 0;
        for (_, f) in &files {
            entities += Level::load(f)?.entities.len();
        }
        let vl: usize = idx.versus.iter().map(|t| t.levels.len()).sum();
        let dl: usize = idx.dark_world.iter().map(|t| t.levels.len()).sum();
        println!(
            "  levels: {} files, {entities} entities; versus {} towers / {vl} levels, quest {}, dark world {} towers / {dl} levels, trials {}",
            files.len(),
            idx.versus.len(),
            idx.quest.len(),
            idx.dark_world.len(),
            idx.trials.len()
        );
        for t in &idx.versus {
            println!(
                "    {:<22} theme {:<14} {:>2} levels{}",
                t.dir_name,
                t.theme,
                t.levels.len(),
                if t.procedural { " (procedural)" } else { "" }
            );
        }
        let mut wl = 0;
        for w in &idx.workshop {
            wl += WorkshopTower::load(w)?.levels.len();
        }
        println!("  workshop: {} towers, {wl} levels", idx.workshop.len());
        let mut n_sfx = 0;
        let mut secs = 0.0;
        for set in inst.content_sets() {
            if let Ok(rd) = std::fs::read_dir(set.sfx_dir()) {
                for e in rd.flatten() {
                    if e.path()
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("wav"))
                    {
                        secs += Wav::load(&e.path())?.seconds();
                        n_sfx += 1;
                    }
                }
            }
        }
        println!("  sfx: {n_sfx} WAV files, {secs:.0} s");
        let md = inst.content.music_dir();
        let wb = WaveBank::open(&md.join("MusicWaveBank.xwb"))?;
        let sb = SoundBank::load(&md.join("MusicSoundBank.xsb"))?;
        let gs = GlobalSettings::load(&md.join("TowerFallMusic.xgs"))?;
        let total: f64 = wb
            .entries
            .iter()
            .map(|e| e.duration as f64 / e.format.sample_rate as f64)
            .sum();
        let codecs: std::collections::BTreeSet<_> = wb
            .entries
            .iter()
            .map(|e| format!("{:?}", e.format.codec))
            .collect();
        println!(
            "  music: {} waves ({:?}, {:.0} min), {} cues, categories {:?}",
            wb.entries.len(),
            codecs,
            total / 60.0,
            sb.cues.len(),
            gs.categories
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
        );
        if decode_music {
            let t0 = Instant::now();
            let mut frames = 0usize;
            for i in 0..wb.entries.len() {
                let p: Pcm = wb.decode(i)?;
                frames += p.frames();
            }
            println!(
                "  decoded {frames} frames of music in {:.2} s",
                t0.elapsed().as_secs_f64()
            );
        }
        for f in [
            "Archer",
            "OpenSans",
            "MonocleDefault",
            "LightingEffect",
            "ScreenEffects",
        ] {
            let r = xnb::report(&inst.content.misc_dir().join(format!("{f}.xnb")))?;
            println!(
                "  xnb {f}: {} v{} flags {:#04x}, {} bytes, readers [{}]{}",
                r.header.platform,
                r.header.version,
                r.header.flags,
                r.header.file_size,
                r.header
                    .readers
                    .iter()
                    .map(|t| t.short_name())
                    .collect::<Vec<_>>()
                    .join(", "),
                r.effect_size
                    .map(|s| format!(", effect {s} bytes, names {:?}", r.effect_strings))
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn rel(inst: &tfp_assets::GameInstall, p: &std::path::Path) -> String {
    p.strip_prefix(&inst.root)
        .unwrap_or(p)
        .display()
        .to_string()
}

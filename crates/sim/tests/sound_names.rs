//! Every sound name the sim can emit is a real effect of the SFX catalogue
//! (`tfp_assets::audio::catalogue`, audio.md §10) whose files exist in the
//! player's install. The names are the string literals of the sim sources
//! with a catalogue prefix (`sfx_`, `char_`, `en_`, `env_`, `pu_`, `ui_`,
//! `boss_`); voice-set cues travel as `Event::CharSound` and are not names.
//! The catalogue check needs no install; the file check is skipped without
//! one.

use std::collections::BTreeMap;
use std::path::Path;
use tfp_assets::audio::catalogue::{SfxCatalogue, sfx_table};
use tfp_assets::install::Installs;

const PREFIXES: [&str; 7] = ["sfx_", "char_", "en_", "env_", "pu_", "ui_", "boss_"];

/// name -> (first use, used only by the Dark World code).
fn scan(dir: &Path, out: &mut BTreeMap<String, (String, bool)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            scan(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            let text = std::fs::read_to_string(&p).unwrap();
            for (ln, line) in text.lines().enumerate() {
                for lit in line.split('"').skip(1).step_by(2) {
                    let ok = PREFIXES.iter().any(|pre| lit.starts_with(pre))
                        && lit.len() > 4
                        && lit
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                    if ok {
                        let dw = p.components().any(|c| c.as_os_str() == "darkworld");
                        let e = out
                            .entry(lit.to_owned())
                            .or_insert_with(|| (format!("{}:{}", p.display(), ln + 1), true));
                        e.1 &= dw;
                    }
                }
            }
        }
    }
}

fn names() -> BTreeMap<String, (String, bool)> {
    let mut out = BTreeMap::new();
    scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

#[test]
fn every_emitted_sound_is_in_the_catalogue() {
    let n = names();
    assert!(n.len() > 100, "scanned {} names", n.len());
    let missing: Vec<String> = n
        .iter()
        .filter(|(k, _)| !sfx_table().iter().any(|d| d.name == k.as_str()))
        .map(|(k, (at, _))| format!("{k} ({at})"))
        .collect();
    assert!(missing.is_empty(), "not in the catalogue: {missing:#?}");
}

#[test]
fn every_emitted_sound_has_its_files_in_the_install() {
    let installs: Vec<_> = Installs::discover().iter().cloned().collect();
    if installs.is_empty() {
        eprintln!("no TowerFall install found: skipped");
        return;
    }
    for inst in installs {
        let cat = SfxCatalogue::open(&inst);
        // Dark World cues only matter where Dark World can be played.
        let has_dw =
            tfp_assets::towers::TowerIndex::build(&inst).is_ok_and(|i| !i.dark_world.is_empty());
        let mut absent = vec![];
        for (name, (at, dw_only)) in names() {
            if dw_only && !has_dw {
                continue;
            }
            let Some(def) = cat.def(&name) else {
                continue;
            };
            if def.dark_world && !cat.has_dark_world() {
                continue;
            }
            if !cat.available(def) {
                absent.push(format!("{name} ({at})"));
            }
        }
        assert!(
            absent.is_empty(),
            "{:?}: files missing for {absent:#?}",
            inst.kind
        );
    }
}

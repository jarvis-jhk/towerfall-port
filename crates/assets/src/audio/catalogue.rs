//! The sound-effect catalogue: every named effect of the original with its
//! kind (plain / instanced / varied / looped / array), its files and whether
//! it follows the slow-motion pitch; the 14 per-archer voice sets; lazily
//! decoded, cached PCM. Behaviour of the kinds: `docs/spec/audio.md` §3–4.
//! The table itself is generated from the spec (`sfx_table.rs`).

use super::{Pcm, Wav};
use crate::error::{Error, Result};
use crate::install::GameInstall;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SfxKind {
    /// Every play is an independent copy.
    Plain,
    /// A ring of N instances; each play stops and restarts the next one.
    Instanced(u8),
    /// N files `_01…`; random pick, never the same twice in a row.
    Varied(u8),
    /// One looping instance, (re)started only when not playing.
    Looped,
    /// N separate plain effects `_01…` addressed by index.
    Array(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SfxDef {
    /// The original's field name (`char_crownFall`, `sfx_enemyKill`, …).
    pub name: &'static str,
    /// File name (without `.wav`); for varied/array effects the base the
    /// two-digit `_NN` suffixes are appended to.
    pub file: &'static str,
    pub kind: SfxKind,
    /// Follows the global slow-motion pitch (false for UI/jingles).
    pub obeys_pitch: bool,
    /// Lives in `DarkWorldContent/SFX` (only with the expansion).
    pub dark_world: bool,
}

impl SfxDef {
    /// The WAV file names (without extension) this effect uses.
    pub fn files(&self) -> Vec<String> {
        match self.kind {
            SfxKind::Varied(n) | SfxKind::Array(n) => {
                (1..=n).map(|i| format!("{}_{i:02}", self.file)).collect()
            }
            _ => vec![self.file.to_owned()],
        }
    }

    /// Instances of an instanced effect (1 for the other kinds).
    pub fn instances(&self) -> u8 {
        match self.kind {
            SfxKind::Instanced(n) => n,
            _ => 1,
        }
    }
}

pub fn sfx_table() -> &'static [SfxDef] {
    super::sfx_table::SFX_TABLE
}

/// The things an archer voice says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VoiceAction {
    Ready,
    Deselect,
    Aim,
    AimCancel,
    AimDir,
    ArrowGrab,
    ArrowRecover,
    ArrowSteal,
    Die,
    DieBomb,
    DieEnv,
    DieLaser,
    DieStomp,
    Duck,
    FireArrow,
    Grab,
    Jump,
    Land,
    NoFire,
    Revive,
    WallSlide,
    Sleep,
}

impl VoiceAction {
    pub const ALL: [VoiceAction; 22] = [
        VoiceAction::Ready,
        VoiceAction::Deselect,
        VoiceAction::Aim,
        VoiceAction::AimCancel,
        VoiceAction::AimDir,
        VoiceAction::ArrowGrab,
        VoiceAction::ArrowRecover,
        VoiceAction::ArrowSteal,
        VoiceAction::Die,
        VoiceAction::DieBomb,
        VoiceAction::DieEnv,
        VoiceAction::DieLaser,
        VoiceAction::DieStomp,
        VoiceAction::Duck,
        VoiceAction::FireArrow,
        VoiceAction::Grab,
        VoiceAction::Jump,
        VoiceAction::Land,
        VoiceAction::NoFire,
        VoiceAction::Revive,
        VoiceAction::WallSlide,
        VoiceAction::Sleep,
    ];

    /// File stem after the set prefix (`READY`, `WALLSLIDE_LOOP`, …).
    pub fn stem(self) -> &'static str {
        match self {
            VoiceAction::Ready => "READY",
            VoiceAction::Deselect => "DESELECT",
            VoiceAction::Aim => "AIM",
            VoiceAction::AimCancel => "AIM_CANCEL",
            VoiceAction::AimDir => "AIM_DIR",
            VoiceAction::ArrowGrab => "ARROW_GRAB",
            VoiceAction::ArrowRecover => "ARROW_RECOVER",
            VoiceAction::ArrowSteal => "ARROW_STEAL",
            VoiceAction::Die => "DIE",
            VoiceAction::DieBomb => "DIE_BOMB",
            VoiceAction::DieEnv => "DIE_ENV",
            VoiceAction::DieLaser => "DIE_LASER",
            VoiceAction::DieStomp => "DIE_STOMP",
            VoiceAction::Duck => "DUCK",
            VoiceAction::FireArrow => "FIRE_ARROW",
            VoiceAction::Grab => "GRAB",
            VoiceAction::Jump => "JUMP",
            VoiceAction::Land => "LAND",
            VoiceAction::NoFire => "NOFIRE",
            VoiceAction::Revive => "REVIVE",
            VoiceAction::WallSlide => "WALLSLIDE_LOOP",
            VoiceAction::Sleep => "SLEEP_LOOP",
        }
    }

    /// How the action's file(s) are looked up: `Ready` is always varied,
    /// `Duck`/`Revive` prefer one file and fall back to numbered ones, the
    /// two loops are looped, the rest plain.
    fn lookup(self) -> Lookup {
        match self {
            VoiceAction::Ready => Lookup::Varied,
            VoiceAction::Duck | VoiceAction::Revive => Lookup::PlainOrVaried,
            VoiceAction::WallSlide | VoiceAction::Sleep => Lookup::Looped,
            _ => Lookup::Plain,
        }
    }
}

enum Lookup {
    Plain,
    Varied,
    PlainOrVaried,
    Looped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceSound {
    pub kind: SfxKind,
    pub files: Vec<String>,
    /// Set index the sound was inherited from (alternate voices copy what
    /// they lack from their base voice).
    pub from_set: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceSet {
    pub index: usize,
    pub prefix: &'static str,
    pub fallback: Option<usize>,
    pub dark_world: bool,
    pub sounds: BTreeMap<VoiceAction, VoiceSound>,
}

impl VoiceSet {
    pub fn get(&self, a: VoiceAction) -> Option<&VoiceSound> {
        self.sounds.get(&a)
    }
}

/// (prefix, base-voice fallback, Dark World only) per voice index — the
/// archers' `SFX` field in `archerData.xml` indexes this list.
pub const VOICES: [(&str, Option<usize>, bool); 14] = [
    ("CHAR_B", None, false),
    ("CHAR_C", None, false),
    ("CHAR_D", None, false),
    ("CHAR_A", None, false),
    ("CHAR_F", None, false),
    ("CHAR_E", None, false),
    ("CHAR_H", None, false),
    ("CHAR_G", None, false),
    ("CHAR_I", None, true),
    ("CHAR_E2", Some(5), true),
    ("CHAR_B2", Some(0), true),
    ("CHAR_A2", Some(3), true),
    ("CHAR_D2", Some(2), true),
    ("CHAR_I2", Some(8), true),
];

/// Effects of one install with lazily decoded, shared PCM (thread-safe, so
/// a loader thread can warm the cache).
pub struct SfxCatalogue {
    base_dir: PathBuf,
    dw_dir: Option<PathBuf>,
    voices: Vec<VoiceSet>,
    cache: Mutex<BTreeMap<String, Arc<Pcm>>>,
}

impl SfxCatalogue {
    pub fn open(inst: &GameInstall) -> SfxCatalogue {
        Self::from_dirs(
            inst.content.sfx_dir(),
            inst.dark_world
                .as_ref()
                .map(|d| d.sfx_dir())
                .filter(|d| d.is_dir()),
        )
    }

    pub fn from_dirs(base_dir: PathBuf, dw_dir: Option<PathBuf>) -> SfxCatalogue {
        let mut voices: Vec<VoiceSet> = Vec::new();
        for (index, (prefix, fallback, dark)) in VOICES.iter().enumerate() {
            let dir = if *dark {
                dw_dir.as_deref()
            } else {
                Some(base_dir.as_path())
            };
            let Some(dir) = dir else { continue };
            let exists = |stem: &str| dir.join(format!("{prefix}_{stem}.wav")).is_file();
            let mut sounds = BTreeMap::new();
            for a in VoiceAction::ALL {
                let stem = a.stem();
                let varied = || {
                    let n = (1..100)
                        .take_while(|i| exists(&format!("{stem}_{i:02}")))
                        .count();
                    (n > 0).then(|| VoiceSound {
                        kind: SfxKind::Varied(n as u8),
                        files: (1..=n).map(|i| format!("{prefix}_{stem}_{i:02}")).collect(),
                        from_set: index,
                    })
                };
                let single = |kind| {
                    exists(stem).then(|| VoiceSound {
                        kind,
                        files: vec![format!("{prefix}_{stem}")],
                        from_set: index,
                    })
                };
                let s = match a.lookup() {
                    Lookup::Plain => single(SfxKind::Plain),
                    Lookup::Looped => single(SfxKind::Looped),
                    Lookup::Varied => varied(),
                    Lookup::PlainOrVaried => single(SfxKind::Plain).or_else(varied),
                };
                if let Some(s) = s {
                    sounds.insert(a, s);
                }
            }
            if let Some(fb) = fallback
                && let Some(base) = voices.iter().find(|v| v.index == *fb)
            {
                for (a, s) in &base.sounds {
                    sounds.entry(*a).or_insert_with(|| s.clone());
                }
            }
            voices.push(VoiceSet {
                index,
                prefix,
                fallback: *fallback,
                dark_world: *dark,
                sounds,
            });
        }
        SfxCatalogue {
            base_dir,
            dw_dir,
            voices,
            cache: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn has_dark_world(&self) -> bool {
        self.dw_dir.is_some()
    }

    pub fn def(&self, name: &str) -> Option<&'static SfxDef> {
        sfx_table().iter().find(|d| d.name == name)
    }

    /// Effects usable with this install (Dark World ones need the expansion).
    pub fn defs(&self) -> impl Iterator<Item = &'static SfxDef> + '_ {
        sfx_table()
            .iter()
            .filter(|d| !d.dark_world || self.dw_dir.is_some())
    }

    pub fn voices(&self) -> &[VoiceSet] {
        &self.voices
    }

    /// Voice set by index (`ArcherDef::sfx`).
    pub fn voice(&self, index: i32) -> Option<&VoiceSet> {
        self.voices.iter().find(|v| v.index as i32 == index)
    }

    /// Whether every file of an effect exists in this install (the
    /// 8-player build lacks the Dark World boss and editor sounds).
    pub fn available(&self, def: &SfxDef) -> bool {
        def.files().iter().all(|f| self.path_of(f).is_some())
    }

    /// Where a WAV lives (Dark World folder first for expansion names).
    pub fn path_of(&self, file: &str) -> Option<PathBuf> {
        let name = format!("{file}.wav");
        let dirs = [self.dw_dir.as_deref(), Some(self.base_dir.as_path())];
        dirs.into_iter()
            .flatten()
            .map(|d| d.join(&name))
            .find(|p| p.is_file())
    }

    /// Decoded PCM of a WAV by file name (cached).
    pub fn pcm(&self, file: &str) -> Result<Arc<Pcm>> {
        if let Some(p) = self.cache.lock().unwrap().get(file) {
            return Ok(p.clone());
        }
        let path = self
            .path_of(file)
            .ok_or_else(|| Error::format(Path::new(file), "no such sound file"))?;
        let pcm = Arc::new(Wav::load(&path)?);
        self.cache
            .lock()
            .unwrap()
            .insert(file.to_owned(), pcm.clone());
        Ok(pcm)
    }

    /// All buffers of a named effect, in file order.
    pub fn effect_pcm(&self, name: &str) -> Result<Vec<Arc<Pcm>>> {
        let def = self
            .def(name)
            .ok_or_else(|| Error::format(Path::new(name), "no such effect"))?;
        def.files().iter().map(|f| self.pcm(f)).collect()
    }

    /// Decoded size of everything cached so far, in bytes.
    pub fn cached_bytes(&self) -> usize {
        self.cache
            .lock()
            .unwrap()
            .values()
            .map(|p| p.samples.len() * 2)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::installs;

    #[test]
    fn every_effect_and_voice_file_exists() {
        for inst in installs().iter() {
            let cat = SfxCatalogue::open(inst);
            let mut files = 0;
            let mut missing = Vec::new();
            for d in cat.defs() {
                for f in d.files() {
                    if cat.path_of(&f).is_none() {
                        missing.push(f);
                    }
                    files += 1;
                }
            }
            if inst.kind == crate::install::InstallKind::EightPlayer {
                missing.retain(|f| {
                    !(f.starts_with("UI_EDIT_") || (f.starts_with("EN_") && f.contains("BOSS")))
                });
            }
            assert!(
                missing.is_empty(),
                "{}: missing {missing:?}",
                inst.summary()
            );
            assert_eq!(
                cat.voices().len(),
                if cat.has_dark_world() { 14 } else { 8 }
            );
            for v in cat.voices() {
                for a in [
                    VoiceAction::Ready,
                    VoiceAction::Die,
                    VoiceAction::Jump,
                    VoiceAction::Land,
                ] {
                    assert!(v.get(a).is_some(), "{} lacks {a:?}", v.prefix);
                }
                assert!(matches!(
                    v.get(VoiceAction::Ready).unwrap().kind,
                    SfxKind::Varied(_)
                ));
                for s in v.sounds.values() {
                    for f in &s.files {
                        assert!(cat.path_of(f).is_some(), "{f}");
                    }
                }
            }
            if let Some(alt) = cat.voice(10) {
                // Green's alt borrows what it lacks from Green.
                assert!(alt.sounds.values().any(|s| s.from_set == 0) || alt.sounds.len() == 22);
            }
            let p = cat.effect_pcm("char_crownFall").unwrap();
            assert_eq!(p.len(), 1);
            assert!(cat.cached_bytes() > 0);
            eprintln!(
                "{}: {} effects, {files} files, {} voices",
                inst.summary(),
                cat.defs().count(),
                cat.voices().len()
            );
        }
    }
}

//! Finding the player's copies of the game.
//!
//! Two installs matter: the Steam copy (Ascension, optionally Dark World and
//! Workshop towers) and the official 8-player build (420×240 versus towers).
//! `TF_GAME_DIR` / `TF8_GAME_DIR` override the search; then directories the
//! player chose ([`set_extra_dirs`]: options, `--game-dir`, a folder dropped
//! on the window); then Steam libraries (`libraryfolders.vdf`, Flatpak,
//! the Windows registry) and the usual places of the 8-player build.
//! Nothing here writes.

use crate::atlas::AtlasKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static EXTRA_DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Game directories chosen by the player, searched after the environment
/// variables (each may be a Steam-style or an 8-player install).
pub fn set_extra_dirs(dirs: Vec<PathBuf>) {
    if let Ok(mut d) = EXTRA_DIRS.lock() {
        *d = dirs.into_iter().map(|p| normalize_game_dir(&p)).collect();
    }
}

pub fn extra_dirs() -> Vec<PathBuf> {
    EXTRA_DIRS.lock().map(|d| d.clone()).unwrap_or_default()
}

/// A path the player pointed at -> the install root: a file inside it, the
/// `Content` folder itself, or the root.
pub fn normalize_game_dir(p: &Path) -> PathBuf {
    let mut p = p.to_path_buf();
    if p.is_file() {
        p.pop();
    }
    if p.file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("Content"))
        && let Some(parent) = p.parent()
    {
        return parent.to_path_buf();
    }
    p
}

/// The Warpzone launcher's hand-over of the `[[data]] id = "towerfall"`
/// requirement (`WARPZONE_DATA_<ID>`).
pub const LAUNCHER_ENV: &str = "WARPZONE_DATA_TOWERFALL";

/// Every place `Installs::discover` looks (for the "no game data" screen).
pub fn searched_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = ["TF_GAME_DIR", "TF8_GAME_DIR", LAUNCHER_ENV]
        .iter()
        .filter_map(|k| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
        .collect();
    v.extend(extra_dirs());
    v.extend(
        steam_library_dirs()
            .into_iter()
            .map(|lib| lib.join("steamapps").join("common").join("TowerFall")),
    );
    v.extend(eight_player_candidates());
    v
}

/// Steam app id of TowerFall Ascension.
pub const STEAM_APP_ID: u32 = 251470;

/// One content directory (`Content/` or `DarkWorldContent/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentSet {
    pub root: PathBuf,
    pub dark_world: bool,
}

impl ContentSet {
    pub fn atlas_dir(&self) -> PathBuf {
        self.root.join("Atlas")
    }
    pub fn atlas_xml(&self, kind: AtlasKind) -> PathBuf {
        self.atlas_dir().join(format!("{}.xml", kind.stem()))
    }
    pub fn sprite_data_dir(&self) -> PathBuf {
        self.atlas_dir().join("SpriteData")
    }
    pub fn game_data_dir(&self) -> PathBuf {
        self.atlas_dir().join("GameData")
    }
    pub fn levels_dir(&self) -> PathBuf {
        self.root.join("Levels")
    }
    pub fn sfx_dir(&self) -> PathBuf {
        self.root.join("SFX")
    }
    pub fn music_dir(&self) -> PathBuf {
        self.root.join("Music").join("Win")
    }
    pub fn misc_dir(&self) -> PathBuf {
        self.root.join("Misc")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// The Steam release (Ascension + optional Dark World), 320×240.
    Steam,
    /// The official 8-player build, 420×240 versus towers.
    EightPlayer,
}

/// A located install and which content sets it has.
#[derive(Clone, Debug)]
pub struct GameInstall {
    pub kind: InstallKind,
    pub root: PathBuf,
    /// `Content/` (always present).
    pub content: ContentSet,
    /// `DarkWorldContent/` if the expansion's data is installed.
    pub dark_world: Option<ContentSet>,
    /// `Workshop/` if it holds any `.tower` files.
    pub workshop: Option<PathBuf>,
    /// The save file, if any (never written by us).
    pub savedata: Option<PathBuf>,
    /// Width of the versus levels in pixels (320 or 420).
    pub level_width: u32,
}

impl GameInstall {
    /// Inspects `root`; `None` if it doesn't look like a TowerFall install.
    pub fn open(root: &Path, kind_hint: Option<InstallKind>) -> Option<GameInstall> {
        let content = root.join("Content");
        if !content.join("Atlas").join("atlas.xml").is_file() || !content.join("Levels").is_dir() {
            return None;
        }
        let dw = root.join("DarkWorldContent");
        let dark_world = dw
            .join("Atlas")
            .join("atlas.xml")
            .is_file()
            .then_some(ContentSet {
                root: dw,
                dark_world: true,
            });
        let ws = root.join("Workshop");
        let workshop = (ws.is_dir() && has_tower_files(&ws)).then_some(ws);
        let save = root.join("tf_savedata");
        let level_width = probe_level_width(&content).unwrap_or(320);
        let kind = kind_hint.unwrap_or(if level_width > 320 {
            InstallKind::EightPlayer
        } else {
            InstallKind::Steam
        });
        Some(GameInstall {
            kind,
            root: root.to_owned(),
            content: ContentSet {
                root: content,
                dark_world: false,
            },
            dark_world,
            workshop,
            savedata: save.is_file().then_some(save),
            level_width,
        })
    }

    /// `Content/` then `DarkWorldContent/` (if present).
    pub fn content_sets(&self) -> impl Iterator<Item = &ContentSet> {
        std::iter::once(&self.content).chain(self.dark_world.as_ref())
    }

    /// The set an asset should be taken from: Dark World's copy if it has the
    /// file, else the base one (the game prefers the Dark World `atlas` and
    /// `bossAtlas` when the expansion is present).
    pub fn atlas_xml(&self, kind: AtlasKind) -> Option<PathBuf> {
        let prefer_dw = matches!(kind, AtlasKind::Main | AtlasKind::Boss | AtlasKind::Editor);
        if prefer_dw && let Some(dw) = &self.dark_world {
            let p = dw.atlas_xml(kind);
            if p.is_file() {
                return Some(p);
            }
        }
        let p = self.content.atlas_xml(kind);
        p.is_file().then_some(p)
    }

    pub fn is_wide(&self) -> bool {
        self.level_width > 320
    }

    pub fn has_ascension(&self) -> bool {
        self.content.levels_dir().join("Versus").is_dir()
    }

    pub fn summary(&self) -> String {
        format!(
            "{:?} at {} ({}x240{}{})",
            self.kind,
            self.root.display(),
            self.level_width,
            if self.dark_world.is_some() {
                ", Dark World"
            } else {
                ""
            },
            if self.workshop.is_some() {
                ", Workshop"
            } else {
                ""
            },
        )
    }
}

fn has_tower_files(dir: &Path) -> bool {
    let mut stack = vec![dir.to_owned()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("tower"))
            {
                return true;
            }
        }
    }
    false
}

/// Width attribute of the first versus level found.
fn probe_level_width(content: &Path) -> Option<u32> {
    let versus = content.join("Levels").join("Versus");
    let mut towers: Vec<_> = std::fs::read_dir(versus)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    towers.sort();
    for t in towers {
        let mut levels: Vec<_> = std::fs::read_dir(&t)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "oel"))
            .collect();
        levels.sort();
        if let Some(first) = levels.first() {
            let text = std::fs::read_to_string(first).ok()?;
            let head = &text[..text.len().min(400)];
            let i = head.find("width=\"")? + 7;
            let j = head[i..].find('"')? + i;
            return head[i..j].parse().ok();
        }
    }
    None
}

/// Both installs, as found on this machine.
#[derive(Clone, Debug, Default)]
pub struct Installs {
    pub steam: Option<GameInstall>,
    pub eight_player: Option<GameInstall>,
}

impl Installs {
    /// `TF_GAME_DIR` / `TF8_GAME_DIR` first, then Steam library folders and
    /// the usual places for the 8-player build.
    pub fn discover() -> Installs {
        let env_dir = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        // The Warpzone launcher hands the install it found over in
        // `WARPZONE_DATA_TOWERFALL` (packaging/warpzone/towerfall-port.toml);
        // it may be either build, so it is classified like the extra dirs
        // (after the explicit TF_* overrides).
        let extra: Vec<GameInstall> = env_dir(LAUNCHER_ENV)
            .into_iter()
            .chain(extra_dirs())
            .filter_map(|p| GameInstall::open(&p, None))
            .collect();
        let steam = env_dir("TF_GAME_DIR")
            .and_then(|p| GameInstall::open(&p, Some(InstallKind::Steam)))
            .or_else(|| extra.iter().find(|i| !i.is_wide()).cloned())
            .or_else(|| {
                steam_library_dirs()
                    .into_iter()
                    .map(|lib| lib.join("steamapps").join("common").join("TowerFall"))
                    .find_map(|p| GameInstall::open(&p, Some(InstallKind::Steam)))
            });
        let eight_player = env_dir("TF8_GAME_DIR")
            .and_then(|p| GameInstall::open(&p, Some(InstallKind::EightPlayer)))
            .or_else(|| extra.iter().find(|i| i.is_wide()).cloned())
            .or_else(|| {
                eight_player_candidates()
                    .into_iter()
                    .filter_map(|p| GameInstall::open(&p, None))
                    .find(|i| i.is_wide())
            });
        Installs {
            steam,
            eight_player,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &GameInstall> {
        self.steam.iter().chain(self.eight_player.iter())
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Steam root directories to look in for `steamapps/libraryfolders.vdf`.
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(h) = home() {
        roots.push(h.join(".steam/steam"));
        roots.push(h.join(".local/share/Steam"));
        roots.push(h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        roots.push(h.join("Library/Application Support/Steam"));
    }
    #[cfg(windows)]
    roots.extend(windows_registry_steam());
    for k in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(p) = std::env::var_os(k) {
            roots.push(PathBuf::from(p).join("Steam"));
        }
    }
    roots
}

/// Steam's install path from the registry (`reg query`, no extra crate).
#[cfg(windows)]
fn windows_registry_steam() -> Vec<PathBuf> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut out = Vec::new();
    for (key, value) in [
        (r"HKCU\Software\Valve\Steam", "SteamPath"),
        (r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
        (r"HKLM\SOFTWARE\Valve\Steam", "InstallPath"),
    ] {
        let Ok(o) = std::process::Command::new("reg")
            .args(["query", key, "/v", value])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        else {
            continue;
        };
        if let Some(p) = reg_value(&String::from_utf8_lossy(&o.stdout), value) {
            out.push(PathBuf::from(p));
        }
    }
    out
}

/// The data of `value` in `reg query` output
/// (`    SteamPath    REG_SZ    c:/program files (x86)/steam`).
pub fn reg_value(text: &str, value: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(value)?.trim_start();
        let rest = rest
            .strip_prefix("REG_SZ")
            .or_else(|| rest.strip_prefix("REG_EXPAND_SZ"))?;
        let v = rest.trim();
        (!v.is_empty()).then(|| v.to_owned())
    })
}

/// Every Steam library folder (deduplicated, existing only).
pub fn steam_library_dirs() -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    let mut add = |p: PathBuf| {
        let canon = p.canonicalize().unwrap_or(p);
        if canon.is_dir() && !libs.contains(&canon) {
            libs.push(canon);
        }
    };
    for root in steam_roots() {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for p in library_paths_from_vdf(&text) {
                add(PathBuf::from(p));
            }
        }
        add(root);
    }
    libs
}

/// Extracts every `"path"` value from a `libraryfolders.vdf` (both the
/// current nested format and the old `"1" "D:\\Games"` flat one).
pub fn library_paths_from_vdf(text: &str) -> Vec<String> {
    let kv = match vdf::parse(text) {
        Some(kv) => kv,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    let Some(vdf::Value::Map(top)) = kv
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("libraryfolders"))
        .map(|(_, v)| v)
    else {
        return out;
    };
    for (k, v) in top {
        match v {
            vdf::Value::Map(entry) => {
                for (ek, ev) in entry {
                    if ek.eq_ignore_ascii_case("path")
                        && let vdf::Value::Str(s) = ev
                    {
                        out.push(s.clone());
                    }
                }
            }
            vdf::Value::Str(s) if k.chars().all(|c| c.is_ascii_digit()) => out.push(s.clone()),
            _ => {}
        }
    }
    out
}

fn eight_player_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = home() {
        for d in [
            "Gaming/TowerFall",
            "Games/TowerFall",
            "Games/TowerFall8Player",
            "Games/towerfall8player",
            "TowerFall",
            "TowerFall8Player",
            ".local/share/TowerFall8Player",
        ] {
            v.push(h.join(d));
        }
    }
    v
}

/// A minimal Valve KeyValues (VDF text) parser: quoted keys/values, nested
/// `{}` maps, `//` comments, `\\` `\"` `\n` `\t` escapes.
pub mod vdf {
    #[derive(Clone, Debug, PartialEq)]
    pub enum Value {
        Str(String),
        Map(Vec<(String, Value)>),
    }

    pub fn parse(text: &str) -> Option<Vec<(String, Value)>> {
        let tokens = tokenize(text)?;
        let mut pos = 0;
        let map = parse_map(&tokens, &mut pos, true)?;
        Some(map)
    }

    #[derive(Debug, PartialEq)]
    enum Tok {
        S(String),
        Open,
        Close,
    }

    fn tokenize(text: &str) -> Option<Vec<Tok>> {
        let mut out = Vec::new();
        let mut it = text.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '{' => out.push(Tok::Open),
                '}' => out.push(Tok::Close),
                '"' => {
                    let mut s = String::new();
                    loop {
                        match it.next()? {
                            '"' => break,
                            '\\' => match it.next()? {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                o => s.push(o),
                            },
                            o => s.push(o),
                        }
                    }
                    out.push(Tok::S(s));
                }
                '/' if it.peek() == Some(&'/') => {
                    for c in it.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                c if c.is_whitespace() => {}
                _ => {
                    // Unquoted token (rare); read until whitespace/brace.
                    let mut s = String::from(c);
                    while let Some(&n) = it.peek() {
                        if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                            break;
                        }
                        s.push(n);
                        it.next();
                    }
                    out.push(Tok::S(s));
                }
            }
        }
        Some(out)
    }

    fn parse_map(t: &[Tok], pos: &mut usize, top: bool) -> Option<Vec<(String, Value)>> {
        let mut map = Vec::new();
        loop {
            match t.get(*pos) {
                None if top => return Some(map),
                None => return None,
                Some(Tok::Close) if !top => {
                    *pos += 1;
                    return Some(map);
                }
                Some(Tok::S(k)) => {
                    *pos += 1;
                    let v = match t.get(*pos)? {
                        Tok::S(v) => {
                            *pos += 1;
                            Value::Str(v.clone())
                        }
                        Tok::Open => {
                            *pos += 1;
                            Value::Map(parse_map(t, pos, false)?)
                        }
                        Tok::Close => return None,
                    };
                    map.push((k.clone(), v));
                }
                _ => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_library_paths() {
        let text = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/u/.local/share/Steam"
		"apps" { "251470" "123" }
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        assert_eq!(
            library_paths_from_vdf(text),
            vec!["/home/u/.local/share/Steam", "D:\\SteamLibrary"]
        );
        let old = "\"LibraryFolders\" { \"TimeNextStatsReport\" \"1\" \"1\" \"E:\\\\Games\" }";
        assert_eq!(library_paths_from_vdf(old), vec!["E:\\Games"]);
    }

    #[test]
    fn registry_and_paths() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    c:/program files (x86)/steam\r\n\r\n";
        assert_eq!(
            reg_value(out, "SteamPath").as_deref(),
            Some("c:/program files (x86)/steam")
        );
        assert_eq!(reg_value(out, "InstallPath"), None);
        assert_eq!(
            normalize_game_dir(Path::new("/games/TowerFall/Content")),
            PathBuf::from("/games/TowerFall")
        );
    }

    #[test]
    fn discovers_installs() {
        let i = crate::testutil::installs();
        for inst in i.iter() {
            eprintln!("{}", inst.summary());
        }
        if let Some(s) = &i.steam {
            assert_eq!(s.level_width, 320);
            assert!(s.has_ascension());
        }
        if let Some(e) = &i.eight_player {
            assert_eq!(e.level_width, 420);
        }
    }
}

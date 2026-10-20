//! Our options file (`options.cfg` in the data directory) and the
//! read-only import of the original's `tf_saveData` (options and keyboard
//! configs, via `tfp_assets::savedata`). The original's files are never
//! written.
//!
//! The format is plain `key = value` lines (comments with `#`); unknown
//! keys are ignored and missing ones keep their defaults, so old files
//! keep loading. Keyboard keys are SDL scancode names (one line per key),
//! controller inputs SDL button names.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sdl2::keyboard::Scancode;
use tfp_render::ScaleMode;

use crate::front::minput::{KeyLayout, KeyboardSetup};
use crate::front::{Capacity, Options};
use crate::padmap::PadMap;

pub const FILE: &str = "options.cfg";

/// The port's data directory: `$TFP_DATA_DIR`, else the platform's user
/// data folder (`$XDG_DATA_HOME` or `~/.local/share`, `%APPDATA%`) +
/// `towerfall-port`.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("TFP_DATA_DIR") {
        return Some(PathBuf::from(d));
    }
    if cfg!(windows) {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("towerfall-port"));
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("towerfall-port"))
}

pub fn path() -> Option<PathBuf> {
    data_dir().map(|d| d.join(FILE))
}

fn scale_name(m: ScaleMode) -> &'static str {
    match m {
        ScaleMode::Integer => "integer",
        ScaleMode::Fit => "fit",
        _ => "stretch",
    }
}

/// The text of the options file.
pub fn to_text(o: &Options) -> String {
    let mut s = String::from("# towerfall-port options (written by the game)\n");
    let mut kv = |k: &str, v: String| s.push_str(&format!("{k} = {v}\n"));
    kv("fullscreen", o.fullscreen.to_string());
    kv("music", o.music.to_string());
    kv("sounds", o.sounds.to_string());
    kv("scale", scale_name(o.scale).into());
    kv("screen_wave", o.screen_wave.to_string());
    kv("screen_flash", o.screen_flash.to_string());
    kv("screen_scroll", o.screen_scroll.to_string());
    kv("vsync", o.vsync.to_string());
    kv("lessons", o.lessons.to_string());
    kv("hold_to_pause", o.hold_to_pause.to_string());
    kv("discovery", o.discovery.to_string());
    kv("rumble", o.rumble.to_string());
    kv("crash_reports", o.crash_reports.to_string());
    kv(
        "capacity",
        match o.capacity {
            Capacity::Auto => "auto".into(),
            Capacity::Fixed(n) => n.to_string(),
        },
    );
    kv("shared_keyboard", o.shared_keyboard.to_string());
    for (i, l) in o.keys.layouts.iter().enumerate() {
        let mut l = l.clone();
        // One line per key (SDL key names may contain ',' or spaces).
        for (name, keys) in binding_fields(&mut l) {
            for k in keys.iter() {
                kv(&format!("keyboard.{i}.{name}"), k.name().to_string());
            }
        }
    }
    for d in &o.game_dirs {
        kv("game_dir", d.display().to_string());
    }
    for (name, m) in &o.pads {
        // The controller's name is the rest of the key (may contain spaces).
        kv(&format!("pad.{name}"), m.to_line());
    }
    s
}

fn binding_fields(l: &mut KeyLayout) -> [(&'static str, &mut Vec<Scancode>); 11] {
    [
        ("left", &mut l.left),
        ("right", &mut l.right),
        ("up", &mut l.up),
        ("down", &mut l.down),
        ("jump", &mut l.jump),
        ("shoot", &mut l.shoot),
        ("alt_shoot", &mut l.alt_shoot),
        ("dodge", &mut l.dodge),
        ("arrows", &mut l.arrows),
        ("menu_alt", &mut l.menu_alt),
        ("start", &mut l.start),
    ]
}

/// Parses the options file text over the defaults.
pub fn from_text(text: &str) -> Options {
    let mut o = Options::default();
    let mut layouts: Vec<Option<KeyLayout>> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        let b = v == "true";
        let n = v.parse::<u8>().ok();
        match k {
            "fullscreen" => o.fullscreen = b,
            "music" => o.music = n.unwrap_or(o.music).min(10),
            "sounds" => o.sounds = n.unwrap_or(o.sounds).min(10),
            "scale" => {
                o.scale = match v {
                    "integer" => ScaleMode::Integer,
                    "stretch" => ScaleMode::Stretch,
                    _ => ScaleMode::Fit,
                }
            }
            "screen_wave" => o.screen_wave = b,
            "screen_flash" => o.screen_flash = b,
            "screen_scroll" => o.screen_scroll = b,
            "vsync" => o.vsync = b,
            "lessons" => o.lessons = b,
            "hold_to_pause" => o.hold_to_pause = b,
            "discovery" => o.discovery = b,
            "rumble" => o.rumble = b,
            "crash_reports" => o.crash_reports = b,
            "capacity" => {
                o.capacity = match v.parse::<usize>() {
                    Ok(n @ (4 | 8 | 12)) => Capacity::Fixed(n),
                    _ => Capacity::Auto,
                }
            }
            "shared_keyboard" => o.shared_keyboard = b,
            "game_dir" if !v.is_empty() => o.game_dirs.push(PathBuf::from(v)),
            _ => {
                if let Some(rest) = k.strip_prefix("keyboard.")
                    && let Some((i, field)) = rest.split_once('.')
                    && let Ok(i) = i.parse::<usize>()
                    && i < 4
                {
                    if layouts.len() <= i {
                        layouts.resize(i + 1, None);
                    }
                    let l = layouts[i].get_or_insert_with(KeyLayout::empty);
                    for (name, keys) in binding_fields(l) {
                        if name == field
                            && let Some(k) = Scancode::from_name(v)
                        {
                            keys.push(k);
                        }
                    }
                } else if let Some(name) = k.strip_prefix("pad.") {
                    o.pads.insert(name.to_owned(), PadMap::from_line(v));
                }
            }
        }
    }
    let layouts: Vec<KeyLayout> = layouts.into_iter().flatten().collect();
    if !layouts.is_empty() {
        o.keys = KeyboardSetup { layouts };
    }
    o
}

pub fn load_from(p: &Path) -> Result<Options> {
    let text = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
    Ok(from_text(&text))
}

/// Writes the options (to a temporary file, then renamed over the old).
pub fn save_to(p: &Path, o: &Options) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).with_context(|| format!("creating {}", d.display()))?;
    }
    let tmp = p.with_extension("cfg.tmp");
    std::fs::write(&tmp, to_text(o)).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, p).with_context(|| format!("replacing {}", p.display()))?;
    Ok(())
}

/// XNA `Keys` name -> scancode (the names the original's save uses).
fn xna_key(name: &str) -> Option<Scancode> {
    use Scancode::*;
    if name.len() == 1 && name.as_bytes()[0].is_ascii_uppercase() {
        return Scancode::from_name(name);
    }
    if let Some(d) = name.strip_prefix('D')
        && d.len() == 1
        && d.as_bytes()[0].is_ascii_digit()
    {
        return Scancode::from_name(d);
    }
    if let Some(d) = name.strip_prefix("NumPad") {
        return Scancode::from_name(&format!("Keypad {d}"));
    }
    let k = match name {
        "Left" => Left,
        "Right" => Right,
        "Up" => Up,
        "Down" => Down,
        "Enter" => Return,
        "Tab" => Tab,
        "Space" => Space,
        "Back" => Backspace,
        "Escape" => Escape,
        "LeftShift" => LShift,
        "RightShift" => RShift,
        "LeftControl" => LCtrl,
        "RightControl" => RCtrl,
        "LeftAlt" => LAlt,
        "RightAlt" => RAlt,
        "OemComma" => Comma,
        "OemPeriod" => Period,
        "OemMinus" => Minus,
        "OemPlus" => Equals,
        "OemQuestion" => Slash,
        "OemSemicolon" => Semicolon,
        "OemQuotes" => Apostrophe,
        "OemOpenBrackets" => LeftBracket,
        "OemCloseBrackets" => RightBracket,
        "OemPipe" => Backslash,
        "OemTilde" => Grave,
        _ => return Scancode::from_name(name),
    };
    Some(k)
}

/// Options from the original's save: volumes, fullscreen, effects,
/// pause/discovery/rumble flags and the keyboard configs; the rest stays
/// at `base`.
pub fn import(save: &tfp_assets::savedata::SaveData, base: &Options) -> Options {
    let so = &save.options;
    let vol = |v: f32| ((v * 10.0).round().clamp(0.0, 10.0)) as u8;
    let mut o = base.clone();
    o.fullscreen = so.fullscreen;
    o.music = vol(so.music_volume);
    o.sounds = vol(so.sfx_volume);
    o.rumble = so.gamepad_vibration;
    o.discovery = so.gamepad_discovery;
    o.hold_to_pause = so.hold_to_pause;
    o.lessons = so.show_tips;
    o.screen_wave = !so.remove_slow_motion_wave_effect;
    o.screen_scroll = !so.remove_scroll_effects;
    o.screen_flash = !so.remove_screen_flash_effects;
    o.vsync = so.vertical_sync;
    let layouts: Vec<KeyLayout> = save
        .keyboard
        .iter()
        .flatten()
        .map(|k| {
            let conv = |v: &Vec<String>| v.iter().filter_map(|n| xna_key(n)).collect();
            KeyLayout {
                left: conv(&k.left),
                right: conv(&k.right),
                up: conv(&k.up),
                down: conv(&k.down),
                jump: conv(&k.jump),
                shoot: conv(&k.shoot),
                alt_shoot: conv(&k.alt_shoot),
                dodge: conv(&k.dodge),
                arrows: conv(&k.arrows),
                menu_alt: conv(&k.menu_alt),
                start: conv(&k.start),
            }
        })
        .collect();
    if !layouts.is_empty() {
        o.keys = KeyboardSetup { layouts };
        o.shared_keyboard = false;
    }
    o
}

/// Where the start-up options came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Ours(PathBuf),
    Imported(PathBuf),
    Defaults,
}

/// Start-up options: our file; if there is none yet (or `force_import`),
/// the original's save when one is found (`save_path` or the usual places).
pub fn startup(force_import: bool, save_path: Option<&Path>) -> (Options, Source) {
    if !force_import
        && let Some(p) = path()
        && p.is_file()
    {
        return match load_from(&p) {
            Ok(o) => (o, Source::Ours(p)),
            Err(e) => {
                log::warn!("options: {e:#}; using defaults");
                (Options::default(), Source::Defaults)
            }
        };
    }
    let found = save_path.map(Path::to_path_buf).or_else(|| {
        let i = tfp_assets::Installs::discover();
        i.steam
            .as_ref()
            .or(i.eight_player.as_ref())
            .and_then(tfp_assets::savedata::SaveData::find)
    });
    if let Some(sp) = found {
        match tfp_assets::savedata::SaveData::load(&sp) {
            Ok(save) => return (import(&save, &Options::default()), Source::Imported(sp)),
            Err(e) => log::warn!("original save {}: {e:#}", sp.display()),
        }
    }
    (Options::default(), Source::Defaults)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip() {
        let mut o = Options {
            music: 3,
            sounds: 7,
            scale: ScaleMode::Integer,
            fullscreen: true,
            capacity: Capacity::Fixed(12),
            crash_reports: true,
            ..Default::default()
        };
        o.keys = KeyboardSetup::shared();
        let mut m = PadMap::default();
        m.binds[0] = vec![crate::padmap::PadInput::TriggerRight];
        o.pads.insert("Xbox 360 Controller".into(), m);
        o.game_dirs.push(PathBuf::from("/games/Tower Fall"));
        let t = to_text(&o);
        assert_eq!(from_text(&t), o, "{t}");
        // Garbage and unknown keys are ignored.
        let p = from_text("music = 99\nnonsense\nfoo = bar\n# c\n");
        assert_eq!(p.music, 10);
        assert_eq!(p.sounds, Options::default().sounds);
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::var_os("TFP_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("scratch/test-options");
        let p = dir.join(FILE);
        let o = Options {
            music: 4,
            ..Default::default()
        };
        save_to(&p, &o).unwrap();
        assert_eq!(load_from(&p).unwrap(), o);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn xna_names_round_trip() {
        use crate::front::minput::key_icon;
        let mut all = Vec::new();
        for mut l in [
            KeyLayout::default_config(),
            KeyLayout::shared_left(),
            KeyLayout::shared_right(),
        ] {
            for (_, keys) in binding_fields(&mut l) {
                all.extend(keys.iter().copied());
            }
        }
        for k in all
            .into_iter()
            .chain([Scancode::Num7, Scancode::Kp3, Scancode::Backslash])
        {
            let n = key_icon(k).unwrap();
            assert_eq!(xna_key(n), Some(k), "{n}");
        }
        assert_eq!(xna_key("F5"), Some(Scancode::F5));
        assert_eq!(xna_key("NoSuchKey"), None);
    }

    #[test]
    fn import_of_the_original_save() {
        let mut save = tfp_assets::savedata::SaveData::default();
        save.options.music_volume = 0.6;
        save.options.sfx_volume = 0.25;
        save.options.vertical_sync = true;
        save.options.remove_screen_flash_effects = true;
        save.keyboard = vec![
            None,
            Some(tfp_assets::savedata::KeyboardConfig {
                left: vec!["A".into()],
                right: vec!["D".into()],
                jump: vec!["Space".into(), "OemComma".into()],
                start: vec!["Enter".into()],
                dodge: vec!["LeftShift".into()],
                ..Default::default()
            }),
        ];
        let o = import(&save, &Options::default());
        assert_eq!((o.music, o.sounds), (6, 3));
        assert!(!o.screen_flash && o.screen_wave && o.vsync);
        assert_eq!(o.keys.layouts.len(), 1);
        let l = &o.keys.layouts[0];
        assert_eq!(l.jump, vec![Scancode::Space, Scancode::Comma]);
        assert_eq!(l.start, vec![Scancode::Return]);
        assert_eq!(l.dodge, vec![Scancode::LShift]);
        // A real save, when this machine has one.
        if let Some(inst) = tfp_assets::Installs::discover().steam
            && let Some(p) = tfp_assets::savedata::SaveData::find(&inst)
        {
            let s = tfp_assets::savedata::SaveData::load(&p).unwrap();
            let o = import(&s, &Options::default());
            assert!(o.music <= 10 && !o.keys.layouts.is_empty());
        }
    }
}

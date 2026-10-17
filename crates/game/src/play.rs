//! `--play`: a sandbox match on a real level: the sim (`tfp-sim`) stepped at
//! 60 Hz with local devices mapped to `PlayerInput` (input.md 2) and
//! built-in bots / a script for the other slots; drawn with the level look
//! plus the sim's drawables; the sim's sound events played through the
//! original's SFX and archer voice sets.
//!
//! Script file (`--script FILE`): lines `FROM-TO PLAYER DX DY BUTTONS`,
//! e.g. `10-40 0 1 0 J`; buttons: J jump, S shoot, D dodge, A alt shoot,
//! R arrow toggle, P start, `-` none. Listed players follow the script for
//! those frames (idle otherwise); others are bots. `--script bots` = bots
//! for every slot without a device, `--script none` = idle.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use sdl2::controller::{Axis, Button};
use tfp_render::Color;
use tfp_sim::versus::{MatchSettings, Phase};
use tfp_sim::{Content, Event, MatchSetup, PlayerInput, PlayerSetup, World};

use crate::app::{DrawContext, LoadContext, Scene, TickContext};
use crate::audio::{Audio, PlayParams, SoundData, VoiceId};
use crate::front::assets::UiAssets;
use crate::front::gfx::Painter;
use crate::front::minput::KeyLayout;
use crate::front::minput::MenuInput;
use crate::front::pause::Action;
use crate::input::{PadState, RawInput};
use crate::match_pause::MatchPause;
use crate::music::MusicPlayer;
use crate::padmap::{PadAction, PadMap};
use crate::tower::{TowerGfx, TowerView};
use crate::versus_ui::{MusicCue, VersusUi};

const MAXP: usize = tfp_sim::world::MAX_PLAYERS;

#[derive(Clone, Debug)]
pub struct PlayOptions {
    pub tower: String,
    pub level: usize,
    pub players: usize,
    pub eight_player: bool,
    /// `None` / "bots": bots; "none": idle; else a script file.
    pub script: Option<String>,
    pub seed: Option<i32>,
    pub hitboxes: bool,
    /// Bind the keyboard to a slot (off for headless runs).
    pub keyboard: bool,
    /// The menus' result: roster, archers, variants, tower (overrides
    /// tower / players).
    pub setup: Option<crate::front::settings::FrontEndSetup>,
    pub mode: PlayMode,
    pub versus_mode: tfp_sim::versus::Mode,
    pub length: tfp_sim::versus::MatchLength,
    pub teams: u8,
    pub no_replay: bool,
    /// Headless proof: capture the native frame at each phase into DIR.
    pub shots: Option<PathBuf>,
    /// Tower music and victory jingles.
    pub music: bool,
    /// Effects volume (options, 0..1) and controller maps per model.
    pub sfx_volume: f32,
    pub pads: BTreeMap<String, PadMap>,
    /// Soak/testing overrides: the variant set, the treasure mask (counts
    /// per pickup) and the Steam 320 towers even for more than 4 players.
    pub variants: Option<tfp_sim::variants::VariantSet>,
    pub treasure_mask: Option<[u8; tfp_sim::pickups::PICKUP_COUNT]>,
    pub force_steam: bool,
    /// Per joined slot (CLI matches): archer (mod the archer count) and team.
    pub slots: Option<Vec<(u16, u8)>>,
    /// `--shots-every N`: also capture every N frames of play (and the draft),
    /// and a few frames after showcase events (chests, explosions, orbs...).
    pub shots_every: Option<u32>,
    /// The keyboard configurations (options; empty = the default layout).
    pub keys: Vec<KeyLayout>,
    /// HOLD TO PAUSE (input.md 6): Start must be held 20 frames.
    pub hold_to_pause: bool,
    /// GAMEPAD RUMBLE (input.md 2.4).
    pub rumble: bool,
    /// SCREEN WAVE: the slow-motion wave (rendering.md 8.4).
    pub screen_wave: bool,
    /// Testing (`--flow-test`): bots play the slots the menus gave to
    /// devices, so a menu-started match finishes by itself.
    pub bots_for_humans: bool,
}

/// HOLD TO PAUSE: frames Start must be held (input.md 6).
const HOLD_TO_PAUSE: u32 = 20;

/// Showcase captures: (sound keyword, tag, delay in frames).
const EVENT_SHOTS: [(&str, &str, u32); 10] = [
    ("unlockChest", "chest", 8),
    ("ArrowExplode", "bomb", 4),
    ("superBombExplode", "superbomb", 4),
    ("brambleGrow", "bramble", 20),
    ("prismForm", "prism", 10),
    ("drillIn", "drill", 6),
    ("LaserDeflect", "laser", 2),
    ("shadowFalls", "darkorb", 40),
    ("timeOrb", "timeorb", 20),
    ("gunnFreeze", "gunn", 12),
];

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            tower: "sacred".into(),
            level: 0,
            players: 4,
            eight_player: false,
            script: None,
            seed: None,
            hitboxes: false,
            keyboard: false,
            setup: None,
            mode: PlayMode::Versus,
            versus_mode: tfp_sim::versus::Mode::Headhunters,
            length: tfp_sim::versus::MatchLength::Quick,
            teams: 2,
            no_replay: false,
            shots: None,
            music: true,
            sfx_volume: 1.0,
            pads: BTreeMap::new(),
            variants: None,
            treasure_mask: None,
            force_steam: false,
            slots: None,
            shots_every: None,
            keys: Vec::new(),
            hold_to_pause: false,
            rumble: true,
            screen_wave: true,
            bots_for_humans: false,
        }
    }
}

/// Voice-set prefixes by SFX index (audio.md 4).
pub(crate) const VOICES: [&str; 14] = [
    "CHAR_B", "CHAR_C", "CHAR_D", "CHAR_A", "CHAR_F", "CHAR_E", "CHAR_H", "CHAR_G", "CHAR_I",
    "CHAR_E2", "CHAR_B2", "CHAR_A2", "CHAR_D2", "CHAR_I2",
];
const VOICE_FALLBACK: [usize; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 5, 0, 3, 2, 8];

fn char_sound_name(s: tfp_sim::events::CharSound) -> &'static str {
    use tfp_sim::events::CharSound::*;
    match s {
        Aim => "AIM",
        AimCancel => "AIM_CANCEL",
        AimDir => "AIM_DIR",
        FireArrow => "FIRE_ARROW",
        NoFire => "NOFIRE",
        Jump => "JUMP",
        Land => "LAND",
        Duck => "DUCK",
        Grab => "GRAB",
        ArrowGrab => "ARROW_GRAB",
        ArrowRecover => "ARROW_RECOVER",
        ArrowSteal => "ARROW_STEAL",
        Die => "DIE",
        DieBomb => "DIE_BOMB",
        DieEnv => "DIE_ENV",
        DieLaser => "DIE_LASER",
        DieStomp => "DIE_STOMP",
        Revive => "REVIVE",
    }
}

/// Sound effects by name: catalogue names (`pu_brambleGrow`, audio.md 10)
/// resolve to their files through the SFX table; file bases (`SFX_...`,
/// voice cues) load `BASE.wav` or the varied `BASE_01..`. A background
/// thread preloads what a Versus match can play (the original loads all
/// effects at start-up); a sound not loaded yet loads on first use.
type SfxCache = BTreeMap<String, Vec<Arc<SoundData>>>;

struct SfxBank {
    dirs: Vec<PathBuf>,
    cache: SfxCache,
    shared: Arc<std::sync::Mutex<SfxCache>>,
}

pub(crate) fn load_sfx_set(dirs: &[PathBuf], base: &str) -> Vec<Arc<SoundData>> {
    let load = |file: &str| {
        dirs.iter().find_map(|d| {
            let p = d.join(format!("{file}.wav"));
            p.exists()
                .then(|| tfp_assets::audio::Wav::load(&p).ok())
                .flatten()
                .map(|pcm| crate::content::sound(&pcm))
        })
    };
    let table = tfp_assets::audio::catalogue::sfx_table();
    if let Some(def) = table.iter().find(|d| d.name == base) {
        return def.files().iter().filter_map(|f| load(f)).collect();
    }
    // One member of an array effect by index: `char_fireLaserDeflect3`.
    let stem = base.trim_end_matches(|c: char| c.is_ascii_digit());
    if let Ok(i) = base[stem.len()..].parse::<usize>()
        && let Some(def) = table.iter().find(|d| d.name == stem)
        && let Some(f) = def.files().get(i.wrapping_sub(1))
    {
        return load(f).into_iter().collect();
    }
    if let Some(s) = load(base) {
        return vec![s];
    }
    (1..=9)
        .filter_map(|n| load(&format!("{base}_{n:02}")))
        .collect()
}

impl SfxBank {
    fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs,
            cache: BTreeMap::new(),
            shared: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
        }
    }

    fn get(&mut self, base: &str) -> &[Arc<SoundData>] {
        if !self.cache.contains_key(base) {
            let pre = self.shared.lock().ok().and_then(|m| m.get(base).cloned());
            let v = pre.unwrap_or_else(|| load_sfx_set(&self.dirs, base));
            self.cache.insert(base.to_owned(), v);
        }
        &self.cache[base]
    }

    /// Loads `names` on a background thread into the shared cache.
    fn preload(&self, names: Vec<String>) {
        let dirs = self.dirs.clone();
        let shared = self.shared.clone();
        let spawned = std::thread::Builder::new()
            .name("sfx-preload".into())
            .spawn(move || {
                for n in names {
                    if shared.lock().map_or(true, |m| m.contains_key(&n)) {
                        continue;
                    }
                    let v = load_sfx_set(&dirs, &n);
                    if let Ok(mut m) = shared.lock() {
                        m.insert(n, v);
                    }
                }
            });
        if let Err(e) = spawned {
            log::warn!("sfx preload: {e}");
        }
    }
}

/// One scripted segment.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    from: u32,
    to: u32,
    player: usize,
    input: PlayerInput,
}

fn parse_script(text: &str) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() != 5 {
            bail!("script line {}: want `FROM-TO PLAYER DX DY BUTTONS`", n + 1);
        }
        let (a, b) = f[0].split_once('-').unwrap_or((f[0], f[0]));
        let mut buttons = 0;
        for c in f[4].chars() {
            buttons |= match c {
                'J' => PlayerInput::JUMP,
                'S' => PlayerInput::SHOOT,
                'D' => PlayerInput::DODGE,
                'A' => PlayerInput::ALT_SHOOT,
                'R' => PlayerInput::ARROWS,
                'P' => PlayerInput::START,
                '-' => 0,
                _ => bail!("script line {}: unknown button {c:?}", n + 1),
            };
        }
        out.push(Segment {
            from: a.parse()?,
            to: b.parse()?,
            player: f[1].parse()?,
            input: PlayerInput::dpad(f[2].parse()?, f[3].parse()?, buttons),
        });
    }
    Ok(out)
}

/// Gamepad mapping (input.md 2.1): d-pad, else the left stick (raw).
pub fn pad_input(p: &PadState) -> PlayerInput {
    pad_input_with(p, &PadMap::default())
}

/// `pad_input` with a button map (ours; the d-pad / stick stay fixed).
pub fn pad_input_with(p: &PadState, map: &PadMap) -> PlayerInput {
    let b = |x| p.held.contains(x);
    let (dx, dy) = (
        b(Button::DPadRight) as i32 - b(Button::DPadLeft) as i32,
        b(Button::DPadDown) as i32 - b(Button::DPadUp) as i32,
    );
    let (ax, ay) = if dx != 0 || dy != 0 {
        (dx as f32, dy as f32)
    } else {
        (p.axis(Axis::LeftX), p.axis(Axis::LeftY))
    };
    let q = |v: f32| (v.clamp(-1.0, 1.0) * PlayerInput::AXIS_MAX as f32).round() as i8;
    let mut buttons = 0;
    let mut set = |on: bool, bit: u8| {
        if on {
            buttons |= bit
        }
    };
    for (action, bit) in [
        (PadAction::Jump, PlayerInput::JUMP),
        (PadAction::Shoot, PlayerInput::SHOOT),
        (PadAction::AltShoot, PlayerInput::ALT_SHOOT),
        (PadAction::Dodge, PlayerInput::DODGE),
        (PadAction::Arrows, PlayerInput::ARROWS),
        (PadAction::Start, PlayerInput::START),
    ] {
        set(map.held(p, action), bit);
    }
    PlayerInput {
        aim_x: q(ax),
        aim_y: q(ay),
        buttons,
        _pad: 0,
    }
}

/// Keyboard mapping (input.md 2.2): right wins over left, down over up.
pub fn key_input(l: &KeyLayout, raw: &RawInput) -> PlayerInput {
    let k = |keys: &[sdl2::keyboard::Scancode]| keys.iter().any(|&s| raw.keyboard.held.contains(s));
    let dx = if k(&l.right) {
        1
    } else if k(&l.left) {
        -1
    } else {
        0
    };
    let dy = if k(&l.down) {
        1
    } else if k(&l.up) {
        -1
    } else {
        0
    };
    let mut buttons = 0;
    let mut set = |on: bool, bit: u8| {
        if on {
            buttons |= bit
        }
    };
    set(k(&l.jump), PlayerInput::JUMP);
    set(k(&l.shoot), PlayerInput::SHOOT);
    set(k(&l.alt_shoot), PlayerInput::ALT_SHOOT);
    set(k(&l.dodge) || k(&l.menu_alt), PlayerInput::DODGE);
    set(k(&l.arrows), PlayerInput::ARROWS);
    set(k(&l.start), PlayerInput::START);
    PlayerInput::dpad(dx, dy, buttons)
}

/// How a slot is driven.
#[derive(Clone, Debug, PartialEq)]
enum Driver {
    Pad(u32),
    Keyboard(usize),
    Bot,
    Script,
    Idle,
}

/// Which kind of match `--play` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlayMode {
    /// A Versus match with the full flow (tower card .. match results).
    #[default]
    Versus,
    /// No match logic: archers respawn after 90 frames.
    Sandbox,
}

pub struct Play {
    opts: PlayOptions,
    gfx: Option<TowerGfx>,
    ui_assets: Option<UiAssets>,
    ui: Option<VersusUi>,
    view: Option<TowerView>,
    view_level: usize,
    view_t0: u32,
    content: Option<Content>,
    world: Option<World>,
    sfx: Option<SfxBank>,
    /// Effect kinds, master pitch, pause of live voices (`crate::sfx`).
    sfxp: crate::sfx::SfxPlayer,
    /// The install has the Dark World content.
    dark_world: bool,
    /// Gunn Style overlay canvas.
    gunn_canvas: crate::match_draw::GunnCanvas,
    /// Lifetime statistics: ticks of rounds played, recorded once.
    round_ticks: u64,
    recorded: bool,
    music: Option<MusicPlayer>,
    /// The tower theme's cue and whether it is a Dark World theme.
    theme_music: (String, bool),
    voices: Vec<usize>,
    drivers: Vec<Driver>,
    /// Per slot: in the match, and a driver fixed by the menus.
    active: Vec<bool>,
    fixed: Vec<Option<Driver>>,
    prev_start: Vec<bool>,
    /// A local player pressed Start (the host returns to the menus).
    pub menu_requested: bool,
    /// QUIT from the pause menu (the host goes to the versus settings).
    pub quit_requested: bool,
    /// The in-match pause and the menu input it reads (any device).
    pause: Option<MatchPause>,
    mi: MenuInput,
    /// Per slot: the name of a human's controller that went missing.
    lost: Vec<Option<String>>,
    /// Names of the connected pads (to remember a missing pad's name).
    pad_names: BTreeMap<u32, String>,
    /// The shared bot (`tfp_sim::bot`) per slot, and the Treasure Draft
    /// pick's own generator.
    bots: Vec<tfp_sim::bot::Bot>,
    draft_rng: u64,
    script: Vec<Segment>,
    keys: Vec<KeyLayout>,
    /// Archer and entity loops (`crate::sfx::Loops`).
    loops: crate::sfx::Loops,
    /// The sleep loop per player.
    snore_loops: BTreeMap<u8, VoiceId>,
    seen: BTreeSet<(u32, usize)>,
    size: (u32, u32),
    shake: u32,
    /// Rumble requests for the host: (pad id, strength, frames).
    rumble_out: Vec<(u32, f32, u32)>,
    frame: u32,
    /// Frames since the current round's intro began (the pause lockout),
    /// the phase that was last seen for it, frames Start has been held
    /// (HOLD TO PAUSE) and a focus loss this tick.
    round_frame: u32,
    round_phase: Option<Phase>,
    start_hold: u32,
    focus_lost: bool,
    deaths: u32,
    fired: u32,
    tower_index: usize,
    variant_icons: Vec<String>,
    /// Headless proof: pending capture and the phase moments taken.
    capture: Option<PathBuf>,
    shots: Vec<&'static str>,
    shot_n: u32,
    last_phase: Option<Phase>,
    phase_frames: u32,
    /// Showcase captures: pending (frame, tag) and per-tag (count, last).
    event_shots: Vec<(u32, &'static str)>,
    event_seen: BTreeMap<&'static str, (u32, u32)>,
    /// Enemy / ghost / Team Revive art (`coop_ui::art`).
    art: crate::coop_ui::art::SceneArt,
}

impl Play {
    pub fn new(opts: PlayOptions) -> Self {
        let keys = if opts.keys.is_empty() {
            vec![KeyLayout::default_config()]
        } else {
            opts.keys.clone()
        };
        Self {
            opts,
            gfx: None,
            ui_assets: None,
            ui: None,
            view: None,
            view_level: usize::MAX,
            view_t0: 0,
            content: None,
            world: None,
            sfx: None,
            sfxp: Default::default(),
            dark_world: false,
            gunn_canvas: Default::default(),
            round_ticks: 0,
            recorded: false,
            music: None,
            theme_music: (String::new(), false),
            voices: Vec::new(),
            drivers: Vec::new(),
            active: Vec::new(),
            fixed: Vec::new(),
            prev_start: vec![false; MAXP],
            menu_requested: false,
            quit_requested: false,
            pause: None,
            mi: MenuInput::default(),
            lost: vec![None; MAXP],
            pad_names: BTreeMap::new(),
            bots: Vec::new(),
            draft_rng: 0x5EED_0D4A,
            script: Vec::new(),
            keys,
            loops: Default::default(),
            snore_loops: BTreeMap::new(),
            seen: BTreeSet::new(),
            size: (320, 240),
            shake: 0,
            rumble_out: Vec::new(),
            frame: 0,
            round_frame: 0,
            round_phase: None,
            start_hold: 0,
            focus_lost: false,
            deaths: 0,
            fired: 0,
            tower_index: 0,
            variant_icons: Vec::new(),
            capture: None,
            shots: Vec::new(),
            shot_n: 0,
            last_phase: None,
            phase_frames: 0,
            event_shots: Vec::new(),
            event_seen: BTreeMap::new(),
            art: Default::default(),
        }
    }

    /// The simulation (soak statistics).
    pub fn world(&self) -> Option<&World> {
        self.world.as_ref()
    }

    /// The match phase, if a Versus match runs.
    pub fn phase(&self) -> Option<Phase> {
        self.world.as_ref()?.versus.as_ref().map(|m| m.phase)
    }

    /// Binds devices to slots in order (pads, then keyboards); the rest
    /// are bots / scripted / idle.
    fn assign(&mut self, raw: &RawInput) {
        let mut devices: Vec<Driver> = raw.pads.iter().map(|p| Driver::Pad(p.id)).collect();
        if self.opts.keyboard {
            devices.extend((0..self.keys.len()).map(Driver::Keyboard));
        }
        let scripted: BTreeSet<usize> = self.script.iter().map(|s| s.player).collect();
        let fallback = match self.opts.script.as_deref() {
            Some("none") => Driver::Idle,
            _ => Driver::Bot,
        };
        devices.retain(|d| !self.fixed.contains(&Some(d.clone())));
        let mut di = devices.into_iter();
        self.drivers = (0..MAXP)
            .map(|i| {
                if !self.active[i] {
                    Driver::Idle
                } else if self.opts.bots_for_humans {
                    Driver::Bot
                } else if let Some(d) = self.fixed[i].clone() {
                    d
                } else if scripted.contains(&i) {
                    Driver::Script
                } else if let Some(d) = di.next() {
                    d
                } else {
                    fallback.clone()
                }
            })
            .collect();
        self.refresh_devices(raw);
    }

    /// Bots-only detection and prompt icons from the first human device
    /// (input.md 3.1).
    fn refresh_devices(&mut self, raw: &RawInput) {
        use crate::front::minput::{IconSet, KeyboardSetup, b, classify_pad};
        let set = self.drivers.iter().find_map(|d| match d {
            Driver::Pad(id) => raw
                .pads
                .iter()
                .find(|p| p.id == *id)
                .map(|p| classify_pad(&p.name).1),
            Driver::Keyboard(k) => Some(IconSet::Keyboard(*k as u8)),
            _ => None,
        });
        let keys = KeyboardSetup {
            layouts: self.keys.clone(),
        };
        if let Some(ui) = &mut self.ui {
            ui.auto = !self
                .drivers
                .iter()
                .any(|d| matches!(d, Driver::Pad(_) | Driver::Keyboard(_)))
                && self.lost.iter().all(|l| l.is_none());
            ui.prompt_icons =
                set.map(|s| [b::CONFIRM, b::START, b::ALT].map(|bit| s.icon(bit, &keys)));
        }
    }

    /// Controllers plugged or unplugged mid-match (input.md 5, 10.2): a
    /// human's missing pad leaves its archer idle and pauses with the
    /// disconnect panel; a newly connected pad takes a slot whose pad is
    /// missing (the same model first, else the lowest slot). Other slots
    /// keep their devices.
    fn hotplug(&mut self, raw: &RawInput, audio: &mut Audio) {
        let present = |id: u32| raw.pads.iter().any(|p| p.id == id);
        let mut newly_lost = None;
        for i in 0..MAXP {
            if let Driver::Pad(id) = self.drivers[i]
                && !present(id)
            {
                self.lost[i] = Some(self.pad_names.get(&id).cloned().unwrap_or_default());
                self.drivers[i] = Driver::Idle;
                newly_lost.get_or_insert(i);
            }
        }
        for &id in &raw.connected {
            if self.drivers.contains(&Driver::Pad(id)) {
                continue;
            }
            let name = raw
                .pads
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.as_str());
            let slot = (0..MAXP)
                .find(|&i| self.lost[i].is_some() && self.lost[i].as_deref() == name)
                .or_else(|| (0..MAXP).find(|&i| self.lost[i].is_some()));
            if let Some(i) = slot {
                println!("play: controller {id} takes P{}", i + 1);
                self.lost[i] = None;
                self.drivers[i] = Driver::Pad(id);
                if matches!(self.fixed[i], Some(Driver::Pad(_))) {
                    self.fixed[i] = Some(Driver::Pad(id));
                }
            }
        }
        if let Some(slot) = newly_lost {
            println!("play: P{} controller disconnected", slot + 1);
            let finished = self.phase() == Some(Phase::Finished);
            if !finished {
                if self.pause.is_none() {
                    self.open_pause(audio);
                }
                let color = self.slot_color_b(slot);
                let badge = {
                    use crate::front::minput::classify_pad;
                    classify_pad(self.lost[slot].as_deref().unwrap_or(""))
                        .1
                        .badge(slot)
                };
                if let Some(pz) = &mut self.pause {
                    pz.panel = Some(crate::match_pause::Panel {
                        slot,
                        t: 0,
                        color,
                        badge,
                    });
                }
            }
        }
        self.refresh_devices(raw);
    }

    /// Colour B of the archer in `slot` (the disconnect panel's text).
    fn slot_color_b(&self, slot: usize) -> Color {
        let archer = self
            .world
            .as_ref()
            .and_then(|w| w.versus.as_ref().map(|m| m.hud(w)))
            .and_then(|h| h.slots[slot])
            .map(|s| s.archer as usize);
        let looks = self.ui_assets.as_ref().map(|a| &a.archers);
        match (archer, looks) {
            (Some(a), Some(l)) => l
                .get(a % l.len().max(1))
                .map_or(Color::WHITE, |x| crate::front::gfx::rgba(x.color_b)),
            _ => Color::WHITE,
        }
    }

    fn ui_sound(&mut self, audio: &mut Audio, name: &str) {
        if let Some(s) = self.ui_assets.as_ref().and_then(|a| a.sound(name)) {
            audio.play(
                s,
                PlayParams {
                    bus: crate::audio::Bus::Sfx,
                    volume: self.opts.sfx_volume,
                    ..Default::default()
                },
            );
        }
    }

    /// Pausing: the sim stops, `ui_pause`, music at half volume.
    fn open_pause(&mut self, audio: &mut Audio) {
        self.pause = Some(MatchPause::new());
        self.sfxp.set_paused(audio, true);
        self.ui_sound(audio, "ui_pause");
        if let Some(m) = &self.music {
            audio.set_music_volume(m.volume * 0.5);
        }
    }

    /// Runs the pause for this tick; true while the match is paused.
    fn pause_tick(&mut self, audio: &mut Audio) -> bool {
        if self.pause.is_none() {
            // Start from any device in the intro / the round opens the menu.
            // input.md 6: the lockout counts from each round's intro;
            // HOLD TO PAUSE wants Start held 20 frames; losing the window
            // focus pauses too (ui.md 10.1).
            let can = self.world.as_ref().is_some_and(|w| w.versus.is_some())
                && matches!(self.phase(), Some(Phase::Intro | Phase::Playing))
                && self.round_frame >= crate::match_pause::LOCKOUT;
            let start = if self.mi.h(crate::front::minput::b::START) {
                self.start_hold += 1;
                if self.opts.hold_to_pause {
                    self.start_hold == HOLD_TO_PAUSE
                } else {
                    self.mi.p(crate::front::minput::b::START)
                }
            } else {
                self.start_hold = 0;
                false
            };
            if can && (start || std::mem::take(&mut self.focus_lost)) {
                self.open_pause(audio);
                // The press that opened the menu does not also close it.
                return true;
            }
            return false;
        }
        let lost = self.lost.clone();
        let mut sounds = Vec::new();
        let action = self
            .pause
            .as_mut()
            .and_then(|pz| pz.step(&self.mi, |slot| lost[slot].is_none(), &mut sounds));
        for s in sounds {
            self.ui_sound(audio, s);
        }
        match action {
            Some(Action::Resume) => {
                self.pause = None;
                self.sfxp.set_paused(audio, false);
                if let Some(m) = &self.music {
                    audio.set_music_volume(m.volume);
                }
                // Keep the Start that resumed from reaching the sim.
                self.prev_start = vec![true; MAXP];
                false
            }
            Some(Action::Quit) => {
                self.quit_requested = true;
                true
            }
            _ => true,
        }
    }

    /// audio.md 3: pan -0.5..0.5 across the screen (x = 0: no position).
    fn pan(&self, x: f32) -> f32 {
        if x == 0.0 {
            0.0
        } else {
            (x / self.size.0 as f32).clamp(0.0, 1.0) - 0.5
        }
    }

    fn play_sound(&mut self, audio: &mut Audio, name: &str, x: f32, volume: f32) {
        let Some(sfx) = &mut self.sfx else { return };
        let v = sfx.get(name).to_vec();
        let pan = self.pan(x);
        self.sfxp
            .play(audio, name, &v, pan, volume * self.opts.sfx_volume);
    }

    /// The effect-playing events (live, and again in replays).
    fn sound_event(&mut self, audio: &mut Audio, e: &Event) {
        match *e {
            Event::Sound { name, x } => self.play_sound(audio, name, x, 1.0),
            // One entry of an array effect (0-based): `name` + its number.
            Event::SoundIndexed { name, index, x } => {
                let n = format!("{name}{}", index as u32 + 1);
                self.play_sound(audio, &n, x, 1.0);
            }
            Event::CharSound {
                sound, archer, x, ..
            } => {
                let set = self.voices.get(archer as usize).copied().unwrap_or(0);
                for s in [set, VOICE_FALLBACK[set.min(13)]] {
                    let name = format!("{}_{}", VOICES[s.min(13)], char_sound_name(sound));
                    if self.sfx.as_mut().is_some_and(|b| !b.get(&name).is_empty()) {
                        self.play_sound(audio, &name, x, 1.0);
                        break;
                    }
                }
            }
            Event::Env(tfp_sim::entities::EnvEvent::Sound { name, x, volume }) => {
                self.play_sound(audio, name, x, volume)
            }
            _ => {}
        }
    }

    /// Starts / stops an entity-owned loop (`EnvEvent::Loop`, the bomb
    /// chest's `EntityLoop`).
    fn entity_loop(
        &mut self,
        audio: &mut Audio,
        key: (tfp_sim::EntityId, &'static str),
        on: bool,
        volume: f32,
        pan: f32,
    ) {
        let vol = volume * self.opts.sfx_volume;
        let sfx = &mut self.sfx;
        self.loops
            .entity(&mut self.sfxp, audio, key, on, pan, vol, || {
                sfx.as_mut().and_then(|b| b.get(key.1).first().cloned())
            });
    }

    fn play_events(&mut self, audio: &mut Audio) {
        // The Yellow archer's snore loop (ActorFx requests).
        let reqs = self
            .ui
            .as_mut()
            .and_then(|u| u.actors.as_mut())
            .map(|a| std::mem::take(&mut a.snore_requests))
            .unwrap_or_default();
        for (player, cue, on) in reqs {
            if on {
                let snd = self.sfx.as_mut().and_then(|b| b.get(&cue).first().cloned());
                if let Some(snd) = snd
                    && !self.snore_loops.contains_key(&player)
                {
                    let id = self
                        .sfxp
                        .play_loop(audio, &cue, &snd, 0.0, self.opts.sfx_volume);
                    self.snore_loops.insert(player, id);
                }
            } else if let Some(id) = self.snore_loops.remove(&player) {
                self.sfxp.stop_loop(audio, id);
            }
        }
        let Some(w) = &self.world else { return };
        // The master pitch follows slow motion (audio.md 3).
        let rate = w.time.time_rate;
        self.sfxp.set_time_rate(audio, rate);
        let frame = w.frame;
        let events: Vec<Event> = w.events().to_vec();
        for (i, e) in events.iter().enumerate() {
            if !self.seen.insert((frame, i)) {
                continue;
            }
            match *e {
                Event::Sound { .. }
                | Event::SoundIndexed { .. }
                | Event::Env(tfp_sim::entities::EnvEvent::Sound { .. }) => {
                    self.sound_event(audio, e)
                }
                Event::CharSound { sound, .. } => {
                    if sound == tfp_sim::events::CharSound::FireArrow {
                        self.fired += 1;
                    }
                    self.sound_event(audio, e);
                }
                Event::Loop {
                    name,
                    player,
                    on,
                    x,
                } => {
                    // Archer loops (`WALLSLIDE_LOOP`) come from the voice
                    // set (`CHAR_x_WALLSLIDE_LOOP`), else the plain name.
                    let set = self
                        .world
                        .as_ref()
                        .and_then(|w| w.player_by_index(player).and_then(|id| w.player(id)))
                        .and_then(|p| self.voices.get(p.archer as usize).copied());
                    let pan = self.pan(x);
                    let sfx = &mut self.sfx;
                    self.loops.player(
                        &mut self.sfxp,
                        audio,
                        (player, name),
                        on,
                        pan,
                        self.opts.sfx_volume,
                        || {
                            let mut names: Vec<String> = Vec::new();
                            if let Some(set) = set {
                                for v in [set, VOICE_FALLBACK[set.min(13)]] {
                                    names.push(format!("{}_{name}", VOICES[v.min(13)]));
                                }
                            }
                            names.push(name.to_owned());
                            sfx.as_mut()
                                .and_then(|b| names.iter().find_map(|n| b.get(n).first().cloned()))
                        },
                    );
                }
                Event::Env(tfp_sim::entities::EnvEvent::Loop {
                    name,
                    owner,
                    on,
                    volume,
                }) => self.entity_loop(audio, (owner, name), on, volume, 0.0),
                Event::EntityLoop {
                    name,
                    entity,
                    on,
                    x,
                } => {
                    let pan = self.pan(x);
                    self.entity_loop(audio, (entity, name), on, 1.0, pan);
                }
                Event::Versus(tfp_sim::versus::VersusEvent::RoundIntro { .. }) => {
                    // A new level: every loop of the old one stops, the
                    // instanced voices too, and the pitch is back to 0.
                    self.loops.stop_all(&mut self.sfxp, audio);
                    for (_, id) in std::mem::take(&mut self.snore_loops) {
                        audio.stop(id);
                    }
                    self.sfxp.reset(audio);
                }
                Event::ScreenShake { frames } => self.shake = self.shake.max(frames as u32),
                Event::Rumble {
                    player,
                    strength,
                    frames,
                } if self.opts.rumble => {
                    if let Some(Driver::Pad(id)) = self.drivers.get(player as usize) {
                        self.rumble_out.push((*id, strength, frames as u32));
                    }
                }
                Event::Death { .. } => self.deaths += 1,
                _ => {}
            }
        }
        let cut = frame.saturating_sub(16);
        self.seen.retain(|(f, _)| *f >= cut);
        // Loops pan with their owners (audio.md 3.4).
        if let Some(w) = &self.world {
            self.loops
                .follow(&mut self.sfxp, audio, w, self.size.0 as f32);
        }
        // The presenter's cues (results, replay sounds).
        if let Some(ui) = &mut self.ui {
            let cues = std::mem::take(&mut ui.cues);
            let replay = std::mem::take(&mut ui.replay_sounds);
            for (name, x) in cues {
                self.play_sound(audio, name, x, 1.0);
            }
            for e in replay {
                self.sound_event(audio, &e);
            }
        }
    }

    /// Loads content and builds the world (also used when the menus start
    /// a match).
    /// What a Versus match may play: the voices first, then the gameplay
    /// effects of the catalogue.
    fn preload_list(dark_world: bool) -> Vec<String> {
        use tfp_sim::events::CharSound::*;
        let mut names = Vec::new();
        // All 14 voice sets (small; archers and fallbacks vary per match).
        for v in VOICES {
            for c in [
                Aim,
                AimCancel,
                AimDir,
                FireArrow,
                NoFire,
                Jump,
                Land,
                Duck,
                Grab,
                ArrowGrab,
                ArrowRecover,
                ArrowSteal,
                Die,
                DieBomb,
                DieEnv,
                DieLaser,
                DieStomp,
                Revive,
            ] {
                names.push(format!("{v}_{}", char_sound_name(c)));
            }
            names.push(format!("{v}_WALLSLIDE_LOOP"));
        }
        // The presentation's cues.
        names.extend(
            [
                "SFX_MULTI_STARTLEVEL_ST",
                "SFX_FINALKILL_ST",
                "SFX_REWIND_ST",
                "SFX_PLAY",
                "SFX_MULTI_COINEARNED",
                "SFX_MULTI_SKULLEARNED",
                "SFX_MULTI_SKULLNEGATIVE",
                "SFX_MULTI_CROWNEARNED",
                "SFX_STATSCREEN_FLYIN_ST",
                "SFX_STATSCREEN_KILLS_ST",
                "SFX_STATSCREEN_DEATHS_ST",
                "ENV_LIGHTNING",
            ]
            .map(String::from),
        );
        names.extend(
            tfp_assets::audio::catalogue::sfx_table()
                .iter()
                .filter(|d| dark_world || !d.dark_world)
                .filter(|d| {
                    ["char_", "sfx_", "pu_", "env_"]
                        .iter()
                        .any(|p| d.name.starts_with(p))
                })
                .map(|d| d.name.to_owned()),
        );
        names
    }

    /// Applies the presentation's music request (audio.md 6.2).
    fn play_music(&mut self, audio: &mut Audio) {
        let (Some(ui), Some(m)) = (&mut self.ui, &mut self.music) else {
            return;
        };
        match ui.music.take() {
            Some(MusicCue::Theme { overtime }) => {
                let (theme, dark) = &self.theme_music;
                let cue = match (overtime, *dark) {
                    (true, true) => "DarkBoss",
                    (true, false) => "Boss",
                    _ => theme.as_str(),
                };
                if !cue.is_empty() {
                    m.play(audio, cue);
                }
            }
            Some(MusicCue::Stop) => m.stop(audio),
            Some(MusicCue::Victory(cue)) => m.play_immediate(audio, &cue),
            None => {}
        }
    }

    pub fn init(
        &mut self,
        gpu: &tfp_render::Gpu,
        renderer: &mut tfp_render::Renderer,
    ) -> Result<()> {
        let installs = tfp_assets::Installs::discover();
        // More than 4 players: the 8-player build's 420-wide towers (same
        // tower list and order as the Steam build).
        let joined = match &self.opts.setup {
            Some(s) => s.slots.iter().filter(|s| s.joined).count(),
            None => self.opts.players,
        };
        let wide = !self.opts.force_steam && (self.opts.eight_player || joined > 4);
        let inst = if wide {
            installs.eight_player.clone().or(installs.steam.clone())
        } else {
            installs.steam.clone().or(installs.eight_player.clone())
        };
        let Some(inst) = inst else {
            bail!("game data not found (set TF_GAME_DIR / TF8_GAME_DIR)")
        };
        let ui_assets = UiAssets::load(gpu, renderer, false);
        // The UI's main atlas is shared when it is the same install's.
        let same = ui_assets
            .install
            .as_ref()
            .is_some_and(|u| u.root == inst.root);
        let g = TowerGfx::load_with_main(
            gpu,
            renderer,
            &inst,
            ui_assets.main.as_ref().filter(|_| same).map(|m| m.texture),
        )?;
        let ti = match &self.opts.setup {
            Some(s) => s.settings.tower.tower,
            None => g
                .find(&self.opts.tower)
                .with_context(|| format!("no versus tower {:?}", self.opts.tower))?,
        };
        if ti >= g.index.versus.len() {
            bail!("no versus tower #{ti}");
        }
        self.tower_index = ti;
        self.dark_world = inst.dark_world.is_some();
        let tower = &g.index.versus[ti];
        // Content: every level of the tower (a match cycles through them).
        let mut content = tfp_sim::loader::content_from_install(&inst, &tower.levels)?;
        self.art = crate::coop_ui::art::SceneArt::register(&mut content, &inst);
        let gd = tfp_assets::game_data::GameData::load(&inst.content)?;
        self.voices = content.archers.iter().map(|a| a.voice as usize).collect();
        // Archer looks in the sim's order: base archers, then alt costumes.
        let mut looks: Vec<crate::front::assets::Archer> = ui_assets.archers.clone();
        looks.extend(
            ui_assets
                .archers
                .iter()
                .filter_map(|a| a.alt.as_deref().cloned()),
        );
        let _ = &gd;
        let seed = self.opts.seed.unwrap_or(0x5EED);
        let world = match self.opts.mode {
            PlayMode::Sandbox => {
                let level = self.opts.level.min(content.levels.len().saturating_sub(1));
                let lvl = content.level(level);
                let n = self.opts.players.clamp(1, MAXP);
                let mut s = MatchSetup {
                    level,
                    seed: lvl.load_seed,
                    respawn_frames: Some(90),
                    ..Default::default()
                };
                for i in 0..n {
                    s.players[i] = Some(PlayerSetup {
                        archer: (i % content.archers.len().max(1)) as u16,
                        ..Default::default()
                    });
                }
                self.active = s.players.iter().map(|p| p.is_some()).collect();
                World::new(&content, &s)
            }
            PlayMode::Versus => {
                let settings = self.versus_settings(&content, tower, seed);
                self.active = (0..MAXP).map(|p| settings.joined(p)).collect();
                self.variant_icons = variant_icons(&settings.variants);
                tfp_sim::versus::new_match(&content, settings)
            }
        };
        self.fixed = vec![None; MAXP];
        if let Some(fe) = &self.opts.setup {
            use crate::front::minput::Dev;
            use crate::front::settings::Owner;
            for (i, sl) in fe.slots.iter().enumerate().take(MAXP) {
                self.fixed[i] = match &sl.owner {
                    _ if !sl.joined => None,
                    Some(Owner::Local(Dev::Pad(id))) if *id >= 10_000 => Some(Driver::Bot),
                    Some(Owner::Local(Dev::Pad(id))) => Some(Driver::Pad(*id)),
                    Some(Owner::Local(Dev::Keyboard(k))) => Some(Driver::Keyboard(*k as usize)),
                    Some(Owner::Remote { .. }) => Some(Driver::Idle),
                    None => Some(Driver::Bot),
                };
            }
        }
        self.opts.players = self.active.iter().filter(|a| **a).count();
        println!(
            "play: {} ({} levels), {} archers, {:?}",
            tower.dir_name,
            tower.levels.len(),
            self.opts.players,
            world
                .versus
                .as_ref()
                .map(|m| (m.settings.mode, m.settings.length, m.goal()))
        );
        let bank = SfxBank::new(
            std::iter::once(inst.content.sfx_dir())
                .chain(inst.dark_world.iter().map(|d| d.sfx_dir()))
                .collect(),
        );
        bank.preload(Self::preload_list(inst.dark_world.is_some()));
        self.sfx = Some(bank);
        if self.opts.music {
            self.theme_music = g.music(ti).unwrap_or_default();
            match MusicPlayer::open(&inst.content.music_dir()) {
                Ok(m) => {
                    // The theme first, then overtime and the possible jingles.
                    let (theme, dark) = &self.theme_music;
                    let mut cues = vec![
                        theme.clone(),
                        if *dark { "DarkBoss" } else { "Boss" }.to_owned(),
                    ];
                    let team = matches!(
                        world.versus.as_ref().map(|v| v.settings.mode),
                        Some(tfp_sim::versus::Mode::TeamDeathmatch)
                    );
                    if team {
                        cues.push("VictoryTeam".into());
                    } else {
                        let slots = world.versus.as_ref().map(|v| v.settings.slots);
                        for s in slots.iter().flatten().flatten() {
                            if let Some(a) = looks.get(s.archer as usize) {
                                cues.push(format!("Victory{}", a.victory));
                            }
                        }
                    }
                    cues.extend(["TheArchives".to_owned(), "TheTale".to_owned()]);
                    m.prefetch(&cues);
                    self.music = Some(m);
                }
                Err(e) => log::warn!("music: {e:#}"),
            }
        }
        if let Some(s) = self.opts.script.as_deref()
            && !matches!(s, "bots" | "none")
        {
            let text = std::fs::read_to_string(s).with_context(|| format!("reading script {s}"))?;
            self.script = parse_script(&text)?;
        }
        self.bots = (0..MAXP)
            .map(|i| tfp_sim::bot::Bot::new(i as i32))
            .collect();
        let mut ui = VersusUi::new(looks);
        ui.replays = !self.opts.no_replay;
        // Archer / arrow cosmetics: their art registered in our content.
        crate::versus_ui::actors::install(&mut ui, &mut content, &inst, &gd);
        self.ui = Some(ui);
        self.size = (
            content.level(world.level).width,
            content.level(world.level).height,
        );
        self.content = Some(content);
        self.world = Some(world);
        self.gfx = Some(g);
        self.ui_assets = Some(ui_assets);
        self.ensure_view(gpu, renderer)?;
        Ok(())
    }

    fn versus_settings(
        &self,
        c: &Content,
        tower: &tfp_assets::towers::VersusTower,
        seed: i32,
    ) -> MatchSettings {
        use tfp_sim::versus::{MatchLength, Mode, SlotSettings};
        let levels: Vec<usize> = (0..c.levels.len()).collect();
        let mut s = MatchSettings::new(Mode::Headhunters, 0, levels);
        s.fixed_first = tower.fixed_first;
        s.seed = seed;
        // The tower's treasure lottery and arrow rate (tower.xml).
        s.items = tfp_sim::pickups::ItemSetup {
            dark_world: self.dark_world,
            ..Default::default()
        }
        .with_tower(
            tower.treasure.as_deref(),
            tower.special_arrow_rate,
            tower.arrow_shuffle,
        );
        if let Some(m) = self.opts.treasure_mask {
            s.items.treasure_mask = Some(m);
        }
        match &self.opts.setup {
            Some(fe) => {
                use crate::front::settings as fs;
                s.mode = match fe.settings.mode {
                    fs::Mode::LastManStanding => Mode::LastManStanding,
                    fs::Mode::HeadHunters => Mode::Headhunters,
                    fs::Mode::TeamDeathmatch => Mode::TeamDeathmatch,
                };
                s.length = match fe.settings.length {
                    fs::Length::Instant => MatchLength::Instant,
                    fs::Length::Quick => MatchLength::Quick,
                    fs::Length::Standard => MatchLength::Standard,
                    fs::Length::Epic => MatchLength::Epic,
                };
                s.teams = fe.settings.team_count.clamp(2, 6);
                for (i, sl) in fe.slots.iter().enumerate().take(MAXP) {
                    if sl.joined {
                        let archer = match sl.costume {
                            fs::Costume::Alt => alt_index(c, sl.archer).unwrap_or(sl.archer),
                            _ => sl.archer,
                        };
                        s.slots[i] = Some(SlotSettings {
                            archer: archer as u16,
                            team: fe.settings.teams[i].index().unwrap_or(0),
                        });
                    }
                }
                s.variants = convert_variants(&fe.settings.variants, s.joined_mask());
            }
            None => {
                s.mode = self.opts.versus_mode;
                s.length = self.opts.length;
                s.teams = self.opts.teams.clamp(2, 6);
                let n = self.opts.players.clamp(1, MAXP);
                let base = c.archers.len().clamp(1, 9);
                if let Some(v) = &self.opts.slots {
                    for (i, &(a, t)) in v.iter().enumerate().take(MAXP) {
                        s.slots[i] = Some(SlotSettings {
                            archer: (a as usize % c.archers.len().max(1)) as u16,
                            team: t % s.teams,
                        });
                    }
                }
                for i in 0..n {
                    if s.slots[i].is_some() {
                        continue;
                    }
                    // 10-12 archers: the extra slots wear alt costumes.
                    let archer = if i < base {
                        i
                    } else {
                        alt_index(c, i - base).unwrap_or(i % base)
                    };
                    s.slots[i] = Some(SlotSettings {
                        archer: archer as u16,
                        team: (i % s.teams as usize) as u8,
                    });
                }
            }
        }
        if let Some(v) = &self.opts.variants {
            s.variants = *v;
            s.variants.clean(s.joined_mask());
        }
        s
    }

    /// Frees the GPU textures of this match (level canvases, atlases).
    pub fn release(&mut self, r: &mut tfp_render::Renderer) {
        self.gunn_canvas.release(r);
        if let Some(v) = self.view.take() {
            v.destroy(r);
        }
        if let Some(g) = self.gfx.take() {
            g.destroy(r);
        }
        if let Some(a) = self.ui_assets.take() {
            a.destroy(r);
        }
    }

    /// (Re)builds the level view when the sim moved to another level.
    fn ensure_view(
        &mut self,
        gpu: &tfp_render::Gpu,
        renderer: &mut tfp_render::Renderer,
    ) -> Result<()> {
        let (Some(w), Some(g)) = (&self.world, &self.gfx) else {
            return Ok(());
        };
        if w.level == self.view_level {
            return Ok(());
        }
        let look = g.look(self.tower_index, w.level, self.opts.seed)?;
        if let Some(old) = self.view.take() {
            old.destroy(renderer);
        }
        self.size = (look.width, look.height);
        self.view = Some(TowerView::new(gpu, renderer, g, look)?);
        self.view_level = w.level;
        self.view_t0 = self.frame;
        Ok(())
    }

    /// Headless proof: which phase moments to capture.
    fn shot_due(&mut self) -> Option<&'static str> {
        let phase = self.phase()?;
        if Some(phase) != self.last_phase {
            self.last_phase = Some(phase);
            self.phase_frames = 0;
        }
        self.phase_frames += 1;
        let pf = self.phase_frames;
        let ui = self.ui.as_ref()?;
        if self.opts.shots_every.is_some() {
            if let Some(w) = &self.world {
                for e in w.events() {
                    let name = match e {
                        Event::Sound { name, .. } | Event::SoundIndexed { name, .. } => *name,
                        _ => continue,
                    };
                    for (key, tag, delay) in EVENT_SHOTS {
                        let seen = self.event_seen.entry(tag).or_insert((0, 0));
                        if name.contains(key)
                            && seen.0 < 3
                            && (seen.0 == 0 || self.frame > seen.1 + 120)
                        {
                            *seen = (seen.0 + 1, self.frame);
                            self.event_shots.push((self.frame + delay, tag));
                        }
                    }
                }
            }
            if let Some(i) = self.event_shots.iter().position(|&(f, _)| f <= self.frame) {
                let (_, tag) = self.event_shots.remove(i);
                return Some(tag);
            }
        }
        let tag = match phase {
            Phase::Playing | Phase::Intro
                if self
                    .opts
                    .shots_every
                    .is_some_and(|n| pf.is_multiple_of(n.max(1))) =>
            {
                if phase == Phase::Intro {
                    "draft"
                } else {
                    "action"
                }
            }
            Phase::TowerCard if pf == 75 => "card",
            Phase::Intro if pf == 95 => "intro",
            Phase::Playing if pf == 240 => "play",
            Phase::Playing if pf > 240 && pf.is_multiple_of(420) => "play",
            Phase::Results if ui.viewer.as_ref().is_some_and(|v| v.t == 40) => "rewind",
            Phase::Results if ui.viewer.as_ref().is_some_and(|v| v.t == 200) => "replay",
            Phase::Results if ui.viewer.is_none() && ui.results_done_at(pf) => "results",
            Phase::MatchResults if pf == 300 => "match",
            _ => return None,
        };
        // One "play" shot per round is enough for the sheet.
        if tag == "play"
            && self
                .shots
                .iter()
                .rev()
                .take_while(|t| **t != "intro")
                .any(|t| *t == "play")
        {
            return None;
        }
        self.shots.push(tag);
        Some(tag)
    }
}

/// Index of the alt costume of base archer `base` in the sim's content.
/// The base archer (0..8) of a content archer (alt costumes map back).
fn base_archer(c: &Content, a: usize) -> usize {
    if a < 9 {
        return a;
    }
    (0..9)
        .find(|&b| alt_index(c, b) == Some(a))
        .unwrap_or(a % 9)
}

fn alt_index(c: &Content, base: usize) -> Option<usize> {
    // The loader appends alts in base order after the base archers.
    let n_base = c
        .archers
        .iter()
        .position(|a| a.name.is_empty())
        .unwrap_or(9)
        .min(9);
    let alts = c.archers.len().saturating_sub(n_base);
    (alts > 0).then(|| n_base + base % alts)
}

/// The menu's variants as the sim's set (matched by title).
fn convert_variants(
    v: &crate::front::variants::VariantSet,
    joined: u16,
) -> tfp_sim::variants::VariantSet {
    use crate::front::variants::VARIANTS;
    use tfp_sim::variants::Variant;
    let mut out = tfp_sim::variants::VariantSet::default();
    for &sv in Variant::ALL {
        let Some(i) = VARIANTS.iter().position(|d| d.title() == sv.title()) else {
            continue;
        };
        if !v.value(i) {
            continue;
        }
        if sv.per_player() {
            for p in 0..MAXP {
                if joined & (1 << p) != 0 && v.slot(i, p) {
                    out.set(sv, p, true);
                }
            }
        } else {
            out.set_all(sv, true, joined);
        }
    }
    out
}

/// Icons of the active listed variants (the round-intro strip), or the
/// single tournament icon.
pub(crate) fn variant_icons(v: &tfp_sim::variants::VariantSet) -> Vec<String> {
    use crate::front::variants::VARIANTS;
    let active: Vec<usize> = v
        .active()
        .filter_map(|sv| VARIANTS.iter().position(|d| d.title() == sv.title()))
        .filter(|&i| !VARIANTS[i].unlisted)
        .collect();
    if !active.is_empty() && active.iter().all(|&i| VARIANTS[i].tournament) {
        return vec!["variants/tournamentRules1v1".into()];
    }
    active.iter().map(|&i| VARIANTS[i].icon()).collect()
}

impl Scene for Play {
    fn game_size(&self) -> (u32, u32) {
        self.size
    }

    fn load(&mut self, ctx: &mut LoadContext<'_>) -> Result<()> {
        self.init(ctx.gpu, ctx.renderer)
    }

    fn take_capture(&mut self) -> Option<PathBuf> {
        self.capture.take()
    }

    fn tick(&mut self, ctx: &mut TickContext<'_>) {
        if self.drivers.len() != MAXP {
            self.assign(ctx.input);
        } else if !ctx.input.connected.is_empty() || !ctx.input.disconnected.is_empty() {
            self.hotplug(ctx.input, ctx.audio);
        }
        for p in &ctx.input.pads {
            self.pad_names.insert(p.id, p.name.clone());
        }
        let keys = crate::front::minput::KeyboardSetup {
            layouts: self.keys.clone(),
        };
        self.mi.update(ctx.input, &keys);
        self.focus_lost = ctx.input.focus_lost;
        if self.pause_tick(ctx.audio) {
            return;
        }
        if let Some(v) = &mut self.view
            && v.update()
        {
            self.play_sound(ctx.audio, "ENV_LIGHTNING", 0.0, 0.6);
        }
        let (Some(w), Some(c)) = (&mut self.world, &self.content) else {
            return;
        };
        let frame = self.frame;
        // Treasure Draft: the slot choosing now (bots pick after a think).
        let draft_picker = w
            .versus
            .as_ref()
            .and_then(|m| m.draft.as_ref())
            .filter(|d| d.stage == tfp_sim::versus::draft::Stage::Choosing)
            .and_then(|d| d.order.get(d.current).copied());
        let mut inputs = vec![PlayerInput::default(); MAXP];
        let mut human = vec![false; MAXP];
        for (i, d) in self.drivers.iter().enumerate() {
            human[i] = matches!(d, Driver::Pad(_) | Driver::Keyboard(_));
            inputs[i] = match d {
                Driver::Pad(id) => ctx
                    .input
                    .pads
                    .iter()
                    .find(|p| p.id == *id)
                    .map(|p| match self.opts.pads.get(&p.name) {
                        Some(m) => pad_input_with(p, m),
                        None => pad_input(p),
                    })
                    .unwrap_or_default(),
                Driver::Keyboard(k) => match self.keys.get(*k) {
                    Some(l) => key_input(l, ctx.input),
                    None => PlayerInput::default(),
                },
                Driver::Script => self
                    .script
                    .iter()
                    .rev()
                    .find(|s| s.player == i && (s.from..=s.to).contains(&frame))
                    .map(|s| s.input)
                    .unwrap_or_default(),
                Driver::Idle => PlayerInput::default(),
                Driver::Bot if draft_picker == Some(i as u8) => {
                    // Look around the offers a bit, then take one.
                    let t = frame % 90;
                    let r = (crate::front::variants::next_rand(&mut self.draft_rng) % 3) as i8 - 1;
                    PlayerInput {
                        aim_x: if t == 30 {
                            r * PlayerInput::AXIS_MAX
                        } else {
                            0
                        },
                        buttons: if t == 60 { PlayerInput::JUMP } else { 0 },
                        ..Default::default()
                    }
                }
                // The shared bot (it confirms waiting screens itself).
                Driver::Bot => self.bots[i].input(w, c, i as u8),
            };
        }
        let hud = w.versus.as_ref().map(|m| m.hud(w));
        if let Some(ui) = &mut self.ui {
            ui.gate_inputs(hud.as_ref(), &mut inputs, &human);
        }
        // Start from a local device after the match: back to the menus.
        let finished = w
            .versus
            .as_ref()
            .is_some_and(|m| m.phase == Phase::Finished);
        for (i, d) in self.drivers.iter().enumerate() {
            let local = matches!(d, Driver::Pad(_) | Driver::Keyboard(_));
            let start = local && inputs[i].held(PlayerInput::START);
            if start && !self.prev_start[i] && (finished || w.versus.is_none()) {
                self.menu_requested = true;
            }
            self.prev_start[i] = start;
        }
        if finished && self.ui.as_ref().is_some_and(|u| u.auto) {
            self.menu_requested = true;
        }
        w.step(c, &inputs);
        self.frame += 1;
        let ph = w.versus.as_ref().map(|m| m.phase);
        if ph == Some(Phase::Intro) && self.round_phase != Some(Phase::Intro) {
            self.round_frame = 0;
        } else {
            self.round_frame = self.round_frame.saturating_add(1);
        }
        self.round_phase = ph;
        self.shake = self.shake.saturating_sub(1);
        let hud = w.versus.as_ref().map(|m| m.hud(w));
        if let Some(ui) = &mut self.ui {
            ui.after_step(w, hud.as_ref());
            ui.post_step(w, c);
        }
        // Lifetime statistics (save.rs): round time, then the match once
        // its results show.
        if w.versus.as_ref().is_some_and(|m| m.phase == Phase::Playing) && !w.frozen {
            self.round_ticks += 1;
        }
        if !self.recorded
            && let Some(h) = &hud
            && h.phase == Phase::MatchResults
        {
            self.recorded = true;
            let base: Vec<Option<usize>> = h
                .slots
                .iter()
                .map(|s| s.map(|s| base_archer(c, s.archer as usize)))
                .collect();
            let random = self
                .opts
                .setup
                .as_ref()
                .is_some_and(|s| s.settings.tower.random);
            crate::save::record_versus(h, &base, self.tower_index, random, self.round_ticks);
        }
        self.play_music(ctx.audio);
        self.play_events(ctx.audio);
        if let Some((s, f)) = self.ui.as_mut().and_then(|u| u.rumble_all.take())
            && self.opts.rumble
        {
            for d in &self.drivers {
                if let Driver::Pad(id) = d {
                    self.rumble_out.push((*id, s, f));
                }
            }
        }
        ctx.host.rumble.append(&mut self.rumble_out);
        if self.opts.shots.is_some()
            && let Some(tag) = self.shot_due()
            && let Some(dir) = &self.opts.shots
        {
            self.shot_n += 1;
            self.capture = Some(dir.join(format!("{:02}-{tag}.png", self.shot_n)));
        }
        if self.frame.is_multiple_of(600) {
            println!(
                "play: frame {}, {} arrows fired, {} deaths, phase {:?}",
                self.frame,
                self.fired,
                self.deaths,
                self.phase()
            );
        }
    }

    fn draw(&mut self, ctx: &mut DrawContext<'_>) {
        if let Err(e) = self.ensure_view(ctx.gpu, ctx.renderer) {
            log::warn!("level view: {e:#}");
        }
        let (Some(v), Some(g), Some(live), Some(c), Some(ui), Some(ua)) = (
            &mut self.view,
            &self.gfx,
            &self.world,
            &self.content,
            &self.ui,
            &self.ui_assets,
        ) else {
            return;
        };
        let w = ui.render_world(live);
        let art = &mut self.art;
        let hud = live.versus.as_ref().map(|m| m.hud(live));
        crate::match_draw::draw_level(
            ctx,
            v,
            &crate::match_draw::LevelDraw {
                world: w,
                content: c,
                gfx: g,
                ui,
                ui_assets: ua,
                hitboxes: self.opts.hitboxes,
                level_t: (self.frame - self.view_t0) as u64,
                tick: self.frame as u64,
                size: self.size,
                hud: hud.as_ref(),
            },
            &mut self.gunn_canvas,
            &mut |view| {
                art.apply(view, w, c);
                Vec::new()
            },
        );
        let shake =
            crate::match_draw::shake_offset(self.frame as u64, self.shake > 0 || ui.shaking());
        let (size, frame, tick) = (self.size, self.frame as u64, ctx.tick);
        let (icons, tower) = (&self.variant_icons, ua.towers.get(self.tower_index));
        let pause = &self.pause;
        crate::match_draw::steady(ctx.list, shake, |list| {
            let mut p = Painter {
                list,
                a: ua,
                w: size.0 as f32,
            };
            if let Some(h) = &hud {
                ui.draw_hud(&mut p, h, tower, icons, frame);
            }
            if let Some(pz) = pause {
                pz.draw(&mut p, tick);
            }
        });
        // The replay's wave, else the time orb's / SlowTime's eased slow
        // motion (frequency 6, amplitude 0.05 * (1 - rate), rendering.md
        // 8.4); not the final kill's (no wave there).
        let slow = (self.opts.screen_wave && live.orb.game_rate_ease)
            .then(|| {
                crate::coop_ui::screen_fx::slow_wave(live.time.time_rate, self.frame as f32 * 0.01)
            })
            .flatten();
        ctx.present.wave = ui.wave().or(slow);
        ctx.present.offset = shake;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputHub;

    #[test]
    fn script_format() {
        let s = parse_script("# test\n10-40 0 1 0 J\n41 1 -1 -1 SD\n").unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].input.aim_x, 120);
        assert!(s[0].input.held(PlayerInput::JUMP));
        assert_eq!((s[1].from, s[1].to, s[1].player), (41, 41, 1));
        assert_eq!(s[1].input.buttons, PlayerInput::SHOOT | PlayerInput::DODGE);
        assert!(parse_script("1-2 0 1 J").is_err());
        assert!(parse_script("1-2 0 1 0 Q").is_err());
    }

    #[test]
    fn device_mapping() {
        let mut hub = InputHub::new();
        let mut raw = RawInput::default();
        hub.connect(3, "pad");
        hub.pad_axis(3, Axis::LeftX as usize, 16000);
        hub.pad_axis(3, Axis::TriggerRight as usize, 8000);
        hub.pad_button(3, Button::X as usize, true);
        hub.snapshot(&mut raw);
        let p = pad_input(&raw.pads[0]);
        assert_eq!(p.aim_x, 59, "raw stick, thresholds are the sim's");
        assert!(p.held(PlayerInput::SHOOT) && p.held(PlayerInput::DODGE));
        hub.pad_button(3, Button::DPadUp as usize, true);
        hub.snapshot(&mut raw);
        let p = pad_input(&raw.pads[0]);
        assert_eq!((p.aim_x, p.aim_y), (0, -120), "d-pad overrides the stick");
        use sdl2::keyboard::Scancode;
        hub.key(Scancode::Left as usize, true);
        hub.key(Scancode::Right as usize, true);
        hub.key(Scancode::Up as usize, true);
        hub.key(Scancode::Down as usize, true);
        hub.key(Scancode::Tab as usize, true);
        hub.snapshot(&mut raw);
        let k = key_input(&KeyLayout::default_config(), &raw);
        assert_eq!((k.aim_x, k.aim_y), (120, 120), "right and down win");
        assert!(k.held(PlayerInput::DODGE), "menu alt dodges");
    }

    #[test]
    fn controller_drop_pauses_and_rebinds() {
        let mut audio = Audio::offline(None).unwrap();
        let mut p = Play::new(PlayOptions::default());
        p.active = (0..MAXP).map(|i| i < 3).collect();
        p.fixed = vec![None; MAXP];
        let mut hub = InputHub::new();
        let mut raw = RawInput::default();
        hub.connect(5, "Xbox 360 Controller");
        hub.connect(6, "PS4 Controller");
        hub.snapshot(&mut raw);
        p.assign(&raw);
        for pad in &raw.pads {
            p.pad_names.insert(pad.id, pad.name.clone());
        }
        assert_eq!(
            p.drivers[..3],
            [Driver::Pad(5), Driver::Pad(6), Driver::Bot]
        );
        // P2's pad goes: P2 idles, the panel shows, others keep their pads.
        hub.disconnect(6);
        hub.snapshot(&mut raw);
        p.hotplug(&raw, &mut audio);
        assert_eq!(p.drivers[..3], [Driver::Pad(5), Driver::Idle, Driver::Bot]);
        assert_eq!(
            p.pause
                .as_ref()
                .and_then(|z| z.panel.as_ref())
                .map(|x| x.slot),
            Some(1)
        );
        assert!(p.ui.is_none() || !p.ui.as_ref().unwrap().auto);
        p.mi.update(&raw, &Default::default());
        assert!(p.pause_tick(&mut audio), "paused while the pad is missing");
        // Another pad arrives: it takes P2; the panel gives way to the menu.
        hub.connect(9, "PS4 Controller");
        hub.snapshot(&mut raw);
        p.hotplug(&raw, &mut audio);
        assert_eq!(p.drivers[1], Driver::Pad(9));
        assert!(p.pause_tick(&mut audio));
        let pz = p.pause.as_ref().unwrap();
        assert!(pz.panel.is_none(), "menu after the pad is back");
        // Start resumes.
        hub.pad_button(9, Button::Start as usize, true);
        hub.snapshot(&mut raw);
        p.mi.update(&raw, &Default::default());
        assert!(!p.pause_tick(&mut audio));
        assert!(p.pause.is_none());
    }
}

//! The front end outside the sim (ui.md, input.md): intro, title, main
//! menu, rollcall for up to 12 archers, versus settings, variants, team
//! select, tower map, options, pause menus. It produces a
//! [`settings::FrontEndSetup`] (roster + `MatchSettings`), convertible to
//! the sim's `MatchSetup` (feature `sim`).
//!
//! Layout: the original's 320x240 screens while at most 4 archers can
//! play; the 8-player build's 420x240 geometry for 8, and our 12-player
//! layout (ui.md 18) for 12.

mod archives;
pub mod assets;
pub mod gfx;
mod main_menu;
mod map;
pub mod minput;
mod options;
pub mod pause;
mod remap;
mod rollcall;
pub mod settings;
pub mod variants;
mod versus;

use std::sync::Arc;

use anyhow::Result;
use tfp_render::glam::Vec2;
use tfp_render::{Color, ScaleMode};

use crate::app::{DrawContext, HostRequests, LoadContext, Scene, TickContext};
use crate::audio::{PlayParams, SoundData};
use assets::UiAssets;
use gfx::{Org, Painter, cube_in, cube_out, lerp, pal, sine};
use minput::{KeyboardSetup, MenuInput};
use settings::{FrontEndSetup, MatchSettings, Slot};
use variants::SLOTS;

/// Every screen of the front end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenId {
    Intro,
    PressStart,
    Main,
    Rollcall,
    VersusOptions,
    Variants,
    TeamSelect,
    Map,
    Options,
    ControlOptions,
    KeyboardConfig,
    /// Ours: controller button mapping.
    ControllerConfig,
    /// The statistics browser (ui.md 12.1).
    Archives,
    /// The versus pause menu over a frozen level.
    Pause,
    /// The versus end-of-match menu.
    MatchEnd,
    /// The tower is chosen: the finished setup (until the game scene exists).
    MatchReady,
    /// Co-op menu (QUEST / DARK WORLD), quest tower select, trials grid
    /// (`crate::coop_ui::menus`).
    CoOp,
    QuestMap,
    TrialsMap,
    /// Dark World tower select with the difficulty and variants overlays
    /// (`crate::coop_ui::darkworld::map`).
    DarkWorldMap,
}

impl std::str::FromStr for ScreenId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(match s {
            "intro" => ScreenId::Intro,
            "title" | "press-start" => ScreenId::PressStart,
            "main" => ScreenId::Main,
            "rollcall" => ScreenId::Rollcall,
            "versus" | "versus-options" => ScreenId::VersusOptions,
            "variants" => ScreenId::Variants,
            "teams" | "team-select" => ScreenId::TeamSelect,
            "map" | "tower-select" => ScreenId::Map,
            "options" => ScreenId::Options,
            "controls" | "control-options" => ScreenId::ControlOptions,
            "keyboard" => ScreenId::KeyboardConfig,
            "controller" => ScreenId::ControllerConfig,
            "archives" => ScreenId::Archives,
            "pause" => ScreenId::Pause,
            "match-end" => ScreenId::MatchEnd,
            "ready" | "match-ready" => ScreenId::MatchReady,
            "coop" | "co-op" => ScreenId::CoOp,
            "quest-map" => ScreenId::QuestMap,
            "trials-map" => ScreenId::TrialsMap,
            "darkworld-map" | "dw-map" => ScreenId::DarkWorldMap,
            _ => {
                return Err(format!(
                    "unknown screen {s:?} (intro, title, main, rollcall, versus, variants, teams, map, options, controls, keyboard, controller, pause, match-end, ready, coop, quest-map, trials-map, darkworld-map)"
                ));
            }
        })
    }
}

/// How many archers the front end is laid out for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capacity {
    Auto,
    Fixed(usize),
}

/// User options (ui.md 11; ours where marked).
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub fullscreen: bool,
    pub music: u8,
    pub sounds: u8,
    pub scale: ScaleMode,
    pub screen_wave: bool,
    pub screen_flash: bool,
    pub screen_scroll: bool,
    pub vsync: bool,
    pub lessons: bool,
    pub hold_to_pause: bool,
    pub discovery: bool,
    pub rumble: bool,
    /// Ours: 4 / 8 / 12 archers, or by the number of devices.
    pub capacity: Capacity,
    /// Ours: the shared-keyboard layout pair instead of the original's.
    pub shared_keyboard: bool,
    pub keys: KeyboardSetup,
    /// Ours: gameplay button maps per controller model (SDL name).
    pub pads: std::collections::BTreeMap<String, crate::padmap::PadMap>,
    /// Ours: game folders the player pointed at (searched after
    /// `TF_GAME_DIR`; Steam libraries are found by themselves).
    pub game_dirs: Vec<std::path::PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            fullscreen: false,
            music: 8,
            sounds: 8,
            scale: ScaleMode::Fit,
            screen_wave: true,
            screen_flash: true,
            screen_scroll: true,
            vsync: true,
            lessons: true,
            hold_to_pause: false,
            discovery: true,
            rumble: true,
            capacity: Capacity::Auto,
            shared_keyboard: false,
            keys: KeyboardSetup::default(),
            pads: Default::default(),
            game_dirs: Vec::new(),
        }
    }
}

/// State that survives screen changes (and matches).
pub struct Session {
    pub slots: Vec<Slot>,
    /// Archer slots on screen: 4, 8 or 12.
    pub capacity: usize,
    pub versus: MatchSettings,
    pub options: Options,
    pub presets: [Option<Vec<String>>; 5],
    pub no_random: Vec<bool>,
    pub last_random: Option<usize>,
    pub rng: u64,
    /// Per voice set: the READY variant played last (never twice in a row).
    pub last_ready: std::collections::BTreeMap<usize, usize>,
    /// The finished result of the menus, once a tower is chosen.
    pub setup: Option<FrontEndSetup>,
    /// Where the main menu's selection starts.
    pub main_from: ScreenId,
    /// The screen being left while the next one is made.
    pub prev_screen: Option<ScreenId>,
    /// The rollcall slot each device took last (preferred when it joins
    /// again this session).
    pub last_slot: std::collections::BTreeMap<minput::Dev, usize>,
    /// Quest / Trials: rollcall mode, map choices, records, launch.
    pub coop: crate::coop_ui::CoopSession,
}

impl Session {
    pub fn joined(&self) -> Vec<bool> {
        self.slots.iter().map(|s| s.joined).collect()
    }

    pub fn joined_count(&self) -> usize {
        self.slots.iter().filter(|s| s.joined).count()
    }

    /// Virtual screen width for the capacity.
    pub fn width(&self) -> f32 {
        if self.capacity <= 4 { 320.0 } else { 420.0 }
    }

    /// Sets the capacity from the option and the number of local devices.
    pub fn update_capacity(&mut self, devices: usize) {
        let want = match self.options.capacity {
            Capacity::Fixed(n) => n,
            Capacity::Auto => match devices {
                0..=4 => 4,
                5..=8 => 8,
                _ => 12,
            },
        };
        // Never shrink below the joined slots.
        let joined_max = self
            .slots
            .iter()
            .rposition(|s| s.joined)
            .map_or(0, |i| i + 1);
        let want = [4, 8, 12]
            .into_iter()
            .find(|&c| c >= want.max(joined_max))
            .unwrap_or(12);
        self.capacity = want;
    }

    pub fn rand(&mut self) -> u64 {
        variants::next_rand(&mut self.rng)
    }

    /// A Varied pick (audio.md 3): uniform among `n`, never voice `set`'s
    /// previous one.
    pub fn pick_ready(&mut self, set: usize, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        let i = match self.last_ready.get(&set) {
            Some(&l) if l < n => {
                let k = (self.rand() % (n as u64 - 1)) as usize;
                if k >= l { k + 1 } else { k }
            }
            _ => (self.rand() % n as u64) as usize,
        };
        self.last_ready.insert(set, i);
        i
    }

    pub fn variant_availability(&self, dark_world: bool) -> variants::Availability {
        variants::Availability {
            dark_world,
            scroll_effects: self.options.screen_scroll,
        }
    }
}

/// Side effects of an update.
#[derive(Default)]
pub struct Fx {
    sounds: Vec<(Arc<SoundData>, f32)>,
    /// Jingles at the music volume (the logo's `MMG`, audio.md 2).
    jingles: Vec<Arc<SoundData>>,
    music: Option<&'static str>,
    stop_music: bool,
    pub host: HostRequests,
}

/// Everything a screen's update can use.
pub struct Ctx<'a> {
    pub a: &'a UiAssets,
    pub mi: &'a MenuInput,
    pub s: &'a mut Session,
    pub fx: &'a mut Fx,
    pub tick: u64,
    /// The screen's transition-in has finished (input accepted).
    pub ready: bool,
}

impl Ctx<'_> {
    pub fn w(&self) -> f32 {
        self.s.width()
    }

    /// Plays a UI sound panned by screen x (centre if `None`).
    pub fn play(&mut self, name: &str, x: Option<f32>) {
        if let Some(s) = self.a.sound(name) {
            let pan = x.map_or(0.0, |x| (x / self.w()).clamp(0.0, 1.0) - 0.5);
            self.fx.sounds.push((s.clone(), pan));
        }
    }

    /// A jingle played at the music volume (on the music bus).
    pub fn play_jingle(&mut self, name: &str) {
        if let Some(s) = self.a.sound(name) {
            self.fx.jingles.push(s.clone());
        }
    }

    /// Rumbles a controller (input.md 2.4; off with GAMEPAD RUMBLE off).
    pub fn rumble(&mut self, d: minput::Dev, strength: f32, frames: u32) {
        if let minput::Dev::Pad(id) = d
            && self.s.options.rumble
        {
            self.fx.host.rumble.push((id, strength, frames));
        }
    }

    pub fn play_arc(&mut self, s: Option<Arc<SoundData>>, x: f32) {
        if let Some(s) = s {
            let pan = (x / self.w()).clamp(0.0, 1.0) - 0.5;
            self.fx.sounds.push((s, pan));
        }
    }

    pub fn stop_music(&mut self) {
        self.fx.stop_music = true;
    }
}

/// What a screen's draw can use.
pub struct View<'a> {
    /// Frames since the screen was created.
    pub t: u32,
    /// 0 = off-screen, 1 = in place (tween in/out progress, eased).
    pub k: f32,
    pub tick: u64,
    pub w: f32,
    pub s: &'a Session,
    /// A level is drawn behind the screen (pause / ready screens).
    pub backdrop: bool,
}

impl View<'_> {
    /// Item position tweening from `from` to `to`.
    pub fn at(&self, from: Vec2, to: Vec2) -> Vec2 {
        lerp(from, to, self.k)
    }

    pub fn cx(&self) -> f32 {
        self.w / 2.0
    }
}

/// Screen transitions.
pub enum Go {
    To(ScreenId),
    /// Fade to black, then switch (leaving the menu scene).
    Fade(ScreenId),
    Quit,
    /// Ours: leave the menus for online play (the game scene switches).
    Online,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogoMode {
    None,
    Logo,
    WithLetters,
}

pub trait Screen {
    fn id(&self) -> ScreenId;
    fn update(&mut self, c: &mut Ctx) -> Option<Go>;
    fn draw(&self, p: &mut Painter, v: &View);
    /// Menu background section (0..3, 300 px apart).
    fn section(&self) -> f32 {
        0.0
    }
    fn title(&self) -> Option<&'static str> {
        None
    }
    fn logo(&self) -> LogoMode {
        LogoMode::None
    }
    /// Draws the menu background (false: the screen draws its own).
    fn menu_background(&self) -> bool {
        true
    }
    /// Button guide hints `(label, menu button)` from the bottom up.
    fn guides(&self) -> Vec<(&'static str, u16)> {
        Vec::new()
    }
    /// Frames the item tween-in takes (input accepted afterwards).
    fn tween_in(&self) -> u32 {
        20
    }
}

fn make(id: ScreenId, s: &mut Session, a: &UiAssets) -> Box<dyn Screen> {
    match id {
        ScreenId::Intro => Box::new(main_menu::Intro::new()),
        ScreenId::PressStart => Box::new(main_menu::PressStart::new()),
        ScreenId::Main => Box::new(main_menu::MainMenu::new(s)),
        ScreenId::Rollcall => Box::new(rollcall::Rollcall::new(s, a)),
        ScreenId::VersusOptions => Box::new(versus::VersusOptions::new(s)),
        ScreenId::Variants => Box::new(versus::VariantsMenu::new(s, a)),
        ScreenId::TeamSelect => Box::new(versus::TeamSelect::new(s)),
        ScreenId::Map => Box::new(map::MapScreen::new(s, a)),
        ScreenId::Options => Box::new(options::OptionsMenu::new(false)),
        ScreenId::ControlOptions => Box::new(options::OptionsMenu::new(true)),
        ScreenId::KeyboardConfig => Box::new(options::KeyboardConfig::new()),
        ScreenId::ControllerConfig => Box::new(remap::ControllerConfig::new()),
        ScreenId::Archives => Box::new(archives::Archives::new()),
        ScreenId::Pause => Box::new(pause::PauseScreen::new(pause::MenuType::VersusPause)),
        ScreenId::MatchEnd => Box::new(pause::PauseScreen::new(pause::MenuType::VersusMatchEnd)),
        ScreenId::MatchReady => Box::new(pause::MatchReady::new(s)),
        ScreenId::CoOp => Box::new(crate::coop_ui::menus::CoopMenu::new(s)),
        ScreenId::QuestMap => Box::new(crate::coop_ui::menus::CoopMap::quest(s, a)),
        ScreenId::TrialsMap => Box::new(crate::coop_ui::menus::CoopMap::trials(s, a)),
        ScreenId::DarkWorldMap => Box::new(crate::coop_ui::darkworld::map::DwMap::new(s, a)),
    }
}

/// Command-line control of the front end.
#[derive(Clone, Debug, Default)]
pub struct FrontOptions {
    /// Start on this screen (default: the intro).
    pub screen: Option<ScreenId>,
    /// Pre-join this many archers (screenshots / testing).
    pub players: Option<usize>,
    /// Layout capacity (4 / 8 / 12) instead of automatic.
    pub capacity: Option<usize>,
    /// Stay on the start screen (ignore transitions; for screenshots).
    pub pin: bool,
    pub no_assets: bool,
    /// Menu music on/off.
    pub music: bool,
    /// Start-up options (from `options_store`); defaults when `None`.
    pub initial: Option<Options>,
    /// Save option changes to the data directory.
    pub persist: bool,
}

/// The front-end scene.
pub struct FrontEnd {
    opts: FrontOptions,
    a: Option<UiAssets>,
    s: Session,
    mi: MenuInput,
    cur: Option<Box<dyn Screen>>,
    cur_t: u32,
    old: Option<(Box<dyn Screen>, u32)>,
    cam: (f32, f32, u32),
    fade: f32,
    fade_to: Option<ScreenId>,
    title: TitleFade,
    logo: (LogoMode, f32),
    /// The ASCENSION letters (or DARK / WORLD words): their own tween, in
    /// on press-start only, out (reverse order) when it is left.
    letters: f32,
    tick: u64,
    fx: Fx,
    music_on: bool,
    /// The menu's music player, the sticky `ChipTitle` choice and whether
    /// the archives' song is the one asked for.
    music: Option<crate::music::MusicPlayer>,
    chip: bool,
    archives_music: bool,
    tower: Option<crate::tower::TowerGfx>,
    /// The level behind pause / ready screens: (tower index, view).
    view: Option<(usize, crate::tower::TowerView)>,
    level_t0: u64,
    match_taken: bool,
    online_requested: bool,
    /// The options as last saved (changes are written when they differ).
    saved: Options,
}

/// Screen-title image fade (4 steps of 0.25 every 4 frames, out then in).
struct TitleFade {
    cur: Option<&'static str>,
    next: Option<&'static str>,
    alpha: f32,
    timer: u32,
}

impl FrontEnd {
    pub fn new(opts: FrontOptions) -> Self {
        let mut options = opts.initial.clone().unwrap_or_default();
        if let Some(c) = opts.capacity {
            options.capacity = Capacity::Fixed(c);
        }
        let s = Session {
            slots: (0..SLOTS)
                .map(|i| Slot {
                    archer: i % 9,
                    ..Default::default()
                })
                .collect(),
            capacity: 4,
            versus: MatchSettings::default(),
            options,
            presets: Default::default(),
            no_random: Vec::new(),
            last_random: None,
            rng: 0x9E37_79B9_7F4A_7C15,
            last_ready: Default::default(),
            setup: None,
            main_from: ScreenId::PressStart,
            prev_screen: None,
            last_slot: Default::default(),
            coop: Default::default(),
        };
        let saved = s.options.clone();
        Self {
            opts,
            a: None,
            s,
            mi: MenuInput::default(),
            cur: None,
            cur_t: 0,
            old: None,
            cam: (0.0, 0.0, 99),
            fade: 0.0,
            fade_to: None,
            title: TitleFade {
                cur: None,
                next: None,
                alpha: 0.0,
                timer: 0,
            },
            logo: (LogoMode::None, 0.0),
            letters: 0.0,
            tick: 0,
            fx: Fx::default(),
            music_on: false,
            music: None,
            chip: false,
            archives_music: false,
            tower: None,
            view: None,
            level_t0: 0,
            match_taken: false,
            online_requested: false,
            saved,
        }
    }

    /// The menus reached "match ready": hands out the setup once (the game
    /// scene then starts the match).
    pub fn take_match(&mut self) -> Option<FrontEndSetup> {
        if self.screen() == Some(ScreenId::MatchReady) && !self.match_taken {
            self.match_taken = true;
            return self.s.setup.clone();
        }
        None
    }

    /// A Quest / Trials game chosen on a co-op map (taken once).
    pub fn take_coop(&mut self) -> Option<crate::coop_ui::CoopLaunch> {
        self.s.coop.pending.take()
    }

    /// The co-op session state (records survive games).
    pub fn coop_session(&mut self) -> &mut crate::coop_ui::CoopSession {
        &mut self.s.coop
    }

    /// Back from a co-op game to `id` (its map or the main menu).
    pub fn return_to(&mut self, id: ScreenId) {
        if id == ScreenId::Main {
            for sl in &mut self.s.slots {
                sl.joined = false;
            }
            self.s.main_from = match self.s.coop.mode {
                crate::coop_ui::RollcallMode::Quest | crate::coop_ui::RollcallMode::DarkWorld => {
                    ScreenId::CoOp
                }
                crate::coop_ui::RollcallMode::Trials => ScreenId::TrialsMap,
                crate::coop_ui::RollcallMode::Versus => ScreenId::PressStart,
            };
        }
        self.switch(id);
        self.old = None;
        // The menu music starts again.
        self.music_on = false;
    }

    /// The main menu's ONLINE was chosen (once).
    pub fn take_online(&mut self) -> bool {
        std::mem::take(&mut self.online_requested)
    }

    /// Back from a match: the end-of-match menu over the level.
    /// The current options (the match takes volumes and controller maps).
    pub fn options(&self) -> &Options {
        &self.s.options
    }

    /// QUIT from the in-match pause: the versus settings (ui.md 10.4).
    pub fn quit_from_match(&mut self) {
        self.match_taken = false;
        self.switch(ScreenId::VersusOptions);
    }

    /// Music is on (`--no-music` turns it off for the match too).
    pub fn music_enabled(&self) -> bool {
        self.opts.music
    }

    pub fn return_from_match(&mut self) {
        self.match_taken = false;
        self.switch(ScreenId::MatchEnd);
    }

    /// The current screen has finished its transition and takes input.
    pub fn settled(&self) -> bool {
        self.cur_t > 25 && self.fade_to.is_none() && self.old.is_none()
    }

    /// The current screen.
    #[allow(dead_code)]
    pub fn screen(&self) -> Option<ScreenId> {
        self.cur.as_ref().map(|c| c.id())
    }

    /// Screens drawn over the chosen level.
    fn level_screen(&self) -> bool {
        self.tower.is_some()
            && self.s.setup.is_some()
            && matches!(
                self.cur.as_ref().map(|c| c.id()),
                Some(ScreenId::MatchReady | ScreenId::Pause | ScreenId::MatchEnd)
            )
    }

    /// The result of the menus, once a tower was confirmed.
    #[allow(dead_code)] // For the game scene (M4).
    pub fn setup(&self) -> Option<&FrontEndSetup> {
        self.s.setup.as_ref()
    }

    fn switch(&mut self, id: ScreenId) {
        let a = self.a.as_ref().expect("loaded");
        let old_id = self.cur.as_ref().map(|c| c.id());
        self.s.prev_screen = old_id;
        let next = make(id, &mut self.s, a);
        // Screen sounds (ui.md 2.3 step 5).
        let snd = match (old_id, id) {
            (Some(ScreenId::Rollcall), _) => Some("ui_characterSelectSwipeOut"),
            (Some(ScreenId::PressStart) | None, ScreenId::Main) => None,
            (_, ScreenId::Main) => Some("ui_swipeOut"),
            (Some(ScreenId::Variants), ScreenId::VersusOptions) => Some("ui_swipeOut"),
            (_, ScreenId::VersusOptions) => None,
            // Into the level (match ready / pause / match end stand-ins).
            (_, ScreenId::MatchReady | ScreenId::Pause | ScreenId::MatchEnd) => None,
            (Some(_), _) => Some("ui_swipeIn"),
            (None, _) => None,
        };
        if let Some(n) = snd
            && let Some(sd) = a.sound(n)
        {
            self.fx.sounds.push((sd.clone(), 0.0));
        }
        let cam_now = self.cam_y();
        self.cam = (
            cam_now,
            next.section() * 300.0,
            if old_id.is_none() { 99 } else { 0 },
        );
        self.title.next = next.title();
        self.logo.0 = next.logo();
        if let Some(o) = self.cur.take() {
            self.old = Some((o, 0));
        }
        self.cur = Some(next);
        self.cur_t = 0;
        self.mi.clear();
    }

    fn cam_y(&self) -> f32 {
        let (from, to, t) = self.cam;
        lerp(
            Vec2::splat(from),
            Vec2::splat(to),
            cube_out(t as f32 / 20.0),
        )
        .x
    }

    /// Pre-joins `n` archers for screenshots (stand-in pads).
    fn prejoin(&mut self, n: usize) {
        use minput::{Dev, IconSet};
        let names = [
            ("XBOX 360", IconSet::Xb360),
            ("DUALSHOCK 4", IconSet::DualShock),
        ];
        for (i, slot) in self.s.slots.iter_mut().enumerate() {
            slot.joined = i < n;
            if i < n {
                let (nm, ic) = if i == 0 {
                    ("KEYBOARD", IconSet::Keyboard(0))
                } else {
                    names[i % 2]
                };
                slot.owner = Some(settings::Owner::Local(if i == 0 {
                    Dev::Keyboard(0)
                } else {
                    Dev::Pad(10_000 + i as u32)
                }));
                slot.dev_name = nm.into();
                slot.icons = Some(ic);
                slot.archer = i % 9;
            }
        }
        // 10-12 players without enough looks: alt costumes for the extras.
        for i in 9..n.min(SLOTS) {
            self.s.slots[i].archer = (i - 9) * 2 % 9;
            self.s.slots[i].costume = settings::Costume::Alt;
        }
        let joined = self.s.joined();
        self.s.versus.clean(&joined);
        if n >= 3 {
            self.s.versus.mode = settings::Mode::HeadHunters;
        }
        self.s.setup = Some(FrontEndSetup {
            slots: self.s.slots.clone(),
            settings: self.s.versus.clone(),
            tower_name: self
                .a
                .as_ref()
                .and_then(|a| a.towers.first())
                .map_or("SACRED GROUND".into(), |t| t.name.clone()),
        });
    }

    /// The menu music (audio.md 6.2, `PlayMenuMusic`): `TheArchives` in the
    /// archives; else, if a title cue already plays, nothing; else (all of
    /// the install counts as unlocked, so Ascension is) `AltTitle`, or on
    /// 1 % of calls `ChipTitle`, which sticks for returns from the archives.
    /// `fresh`: another scene had the music voice.
    fn start_music(
        &mut self,
        audio: &mut crate::audio::Audio,
        archives: bool,
        from_archives: bool,
        fresh: bool,
    ) {
        if self.music.is_none() {
            let Some(inst) = self.a.as_ref().and_then(|a| a.install.as_ref()) else {
                return;
            };
            match crate::music::MusicPlayer::open(&inst.content.music_dir()) {
                Ok(m) => self.music = Some(m),
                Err(e) => {
                    log::warn!("menu music: {e:#}");
                    return;
                }
            }
        }
        let roll = self.s.rand().is_multiple_of(100);
        let Some(m) = &mut self.music else { return };
        if fresh {
            m.forget();
        }
        let cue = if archives {
            "TheArchives"
        } else if matches!(m.current(), Some("Title" | "AltTitle" | "ChipTitle")) {
            return;
        } else if from_archives {
            if self.chip { "ChipTitle" } else { "AltTitle" }
        } else {
            self.chip = roll;
            if roll { "ChipTitle" } else { "AltTitle" }
        };
        let cue = if m.has(cue) { cue } else { "Title" };
        m.play(audio, cue);
    }
}

impl FrontEnd {
    /// Finishes loading with the given assets (also used by tests).
    fn init(&mut self, a: UiAssets) {
        println!(
            "front end: {} archers, {} versus towers, {} UI sounds, menu atlas {}, 8P art {}",
            a.archers.len(),
            a.towers.len(),
            a.sounds.len(),
            if a.menu.is_some() { "yes" } else { "no" },
            if a.menu8.is_some() { "yes" } else { "no" }
        );
        self.s.no_random = vec![false; a.towers.len()];
        self.a = Some(a);
        if let Some(n) = self.opts.players {
            self.s.update_capacity(n);
            self.prejoin(n);
        } else {
            self.s.update_capacity(1);
        }
        let start = self.opts.screen.unwrap_or(ScreenId::Intro);
        if self.opts.screen.is_some() && start != ScreenId::Intro {
            self.music_on = !self.opts.music;
        }
        self.switch(start);
        // Screenshots start settled.
        if self.opts.pin {
            self.old = None;
            self.cam.2 = 99;
            self.title.cur = self.title.next;
            self.title.alpha = 1.0;
            self.logo.1 = 1.0;
            self.letters = if self.logo.0 == LogoMode::WithLetters {
                1.0
            } else {
                0.0
            };
        }
    }
}

impl Scene for FrontEnd {
    fn game_size(&self) -> (u32, u32) {
        if self.level_screen()
            && let Some((_, v)) = &self.view
        {
            return v.size;
        }
        (self.s.width() as u32, 240)
    }

    fn load(&mut self, ctx: &mut LoadContext<'_>) -> Result<()> {
        let a = UiAssets::load(ctx.gpu, ctx.renderer, self.opts.no_assets);
        if let Some(inst) = &a.install {
            match crate::tower::TowerGfx::load_with_main(
                ctx.gpu,
                ctx.renderer,
                inst,
                a.main.as_ref().map(|m| m.texture),
            ) {
                Ok(g) => self.tower = Some(g),
                Err(e) => log::warn!("tower views unavailable: {e:#}"),
            }
        }
        self.init(a);
        Ok(())
    }

    fn tick(&mut self, ctx: &mut TickContext<'_>) {
        self.tick = ctx.tick;
        self.mi.update(ctx.input, &self.s.options.keys);
        if self.cur.as_ref().map(|c| c.id()) == Some(ScreenId::Rollcall)
            || self.opts.players.is_none()
        {
            let devices = self.mi.devs.len();
            if self.opts.players.is_none() {
                self.s.update_capacity(devices);
            }
        }
        let cur_id = self.cur.as_ref().map(|c| c.id());
        let archives = cur_id == Some(ScreenId::Archives);
        if (!self.music_on || archives != self.archives_music)
            && self.opts.music
            && !matches!(cur_id, Some(ScreenId::Intro | ScreenId::Map))
            && !self.level_screen()
        {
            let fresh = !self.music_on;
            let from_archives = self.archives_music && !archives;
            self.music_on = true;
            self.archives_music = archives;
            self.start_music(ctx.audio, archives, from_archives, fresh);
        }
        // Transition bookkeeping.
        self.cur_t += 1;
        self.cam.2 = (self.cam.2 + 1).min(99);
        if let Some((_, t)) = &mut self.old {
            *t += 1;
            if *t >= 12 {
                self.old = None;
            }
        }
        let want_logo = self.logo.0 != LogoMode::None;
        let step = if want_logo { 1.0 / 20.0 } else { -1.0 / 12.0 };
        self.logo.1 = (self.logo.1 + step).clamp(0.0, 1.0);
        let lstep = if self.logo.0 == LogoMode::WithLetters {
            1.0 / 20.0
        } else {
            -1.0 / 12.0
        };
        self.letters = (self.letters + lstep).clamp(0.0, 1.0);
        let tf = &mut self.title;
        if tf.next != tf.cur {
            tf.timer += 1;
            if tf.timer.is_multiple_of(4) {
                tf.alpha -= 0.25;
                if tf.alpha <= 0.0 {
                    tf.alpha = 0.0;
                    tf.cur = tf.next;
                }
            }
        } else if tf.alpha < 1.0 && tf.cur.is_some() {
            tf.timer += 1;
            if tf.timer.is_multiple_of(4) {
                tf.alpha = (tf.alpha + 0.25).min(1.0);
            }
        }
        // Fade out to another screen.
        if let Some(to) = self.fade_to {
            self.fade = (self.fade + 0.055).min(1.0);
            if self.fade >= 1.0 {
                self.fade_to = None;
                self.switch(to);
                self.old = None;
            }
        } else {
            self.fade = (self.fade - 0.2).max(0.0);
        }

        let a = self.a.as_ref().expect("loaded");
        let mut cur = self.cur.take().expect("screen");
        let ready = self.cur_t >= cur.tween_in() && self.fade_to.is_none();
        let go = cur.update(&mut Ctx {
            a,
            mi: &self.mi,
            s: &mut self.s,
            fx: &mut self.fx,
            tick: ctx.tick,
            ready,
        });
        self.cur = Some(cur);
        match go {
            _ if self.opts.pin => {}
            Some(Go::To(id)) => self.switch(id),
            Some(Go::Fade(id)) => self.fade_to = Some(id),
            Some(Go::Quit) => self.fx.host.quit = true,
            Some(Go::Online) => self.online_requested = true,
            None => {}
        }

        // Effects.
        let vol = self.s.options.sounds as f32 / 10.0;
        for s in self.fx.jingles.drain(..) {
            ctx.audio.play(
                &s,
                PlayParams {
                    bus: crate::audio::Bus::Music,
                    ..Default::default()
                },
            );
        }
        for (s, pan) in self.fx.sounds.drain(..).filter(|_| vol > 0.0) {
            ctx.audio.play(
                &s,
                PlayParams {
                    volume: vol,
                    pan,
                    ..Default::default()
                },
            );
        }
        if self.fx.stop_music {
            self.fx.stop_music = false;
            ctx.audio.stop_music(1.0);
            // Menu music comes back once we leave the map / match screens.
            self.music_on = false;
        }
        let _ = self.fx.music.take();
        ctx.audio
            .set_bus_volume(crate::audio::Bus::Music, self.s.options.music as f32 / 10.0);
        // Option changes are saved at once (ui.md 2: "saved with the options").
        if self.opts.persist && self.s.options != self.saved {
            self.saved = self.s.options.clone();
            if let Some(p) = crate::options_store::path()
                && let Err(e) = crate::options_store::save_to(&p, &self.saved)
            {
                log::warn!("saving options: {e:#}");
            }
        }
        if let Some(f) = self.fx.host.fullscreen.take() {
            ctx.host.fullscreen = Some(f);
        }
        if self.fx.host.quit {
            ctx.host.quit = true;
        }
        ctx.host.rumble.append(&mut self.fx.host.rumble);
    }

    fn draw(&mut self, ctx: &mut DrawContext<'_>) {
        ctx.present.mode = self.s.options.scale;
        // The chosen level behind pause / ready screens.
        let backdrop = self.level_screen();
        if backdrop {
            let want = self
                .s
                .setup
                .as_ref()
                .map(|s| s.settings.tower.tower)
                .unwrap_or(0);
            if self.view.as_ref().map(|(i, _)| *i) != Some(want)
                && let Some(g) = &self.tower
            {
                if let Some((_, old)) = self.view.take() {
                    old.destroy(ctx.renderer);
                }
                let seed = self
                    .s
                    .setup
                    .as_ref()
                    .map(|s| s.settings.tower.seed as i32)
                    .filter(|&s| s != 0);
                match g
                    .look(want, 0, seed)
                    .and_then(|l| crate::tower::TowerView::new(ctx.gpu, ctx.renderer, g, l))
                {
                    Ok(v) => {
                        self.view = Some((want, v));
                        self.level_t0 = self.tick;
                    }
                    Err(e) => log::warn!("level view: {e:#}"),
                }
            }
        }
        let Some(a) = self.a.as_ref() else { return };
        let w = ctx.game_size.0 as f32;
        let list = &mut *ctx.list;
        let drew_level = match (&self.view, &self.tower) {
            (Some((_, v)), Some(g)) if backdrop && v.size.0 as f32 == w => {
                // Frozen while a menu is open.
                let t = if self.cur.as_ref().map(|c| c.id()) == Some(ScreenId::MatchReady) {
                    self.tick.saturating_sub(self.level_t0)
                } else {
                    0
                };
                v.draw(list, g, t);
                true
            }
            _ => {
                list.begin_pass(None, Some(Color::BLACK));
                false
            }
        };
        let mut p = Painter { list, a, w };
        let cur = self.cur.as_ref().expect("screen");
        let cam = self.cam_y();
        if cur.menu_background() {
            draw_background(&mut p, cam, self.tick);
        }
        if self.logo.1 > 0.0 {
            draw_logo(&mut p, self.logo, self.letters, self.tick, a.dark_world);
        }
        if let Some(t) = self.title.cur
            && self.title.alpha > 0.0
        {
            let al = self.title.alpha;
            let pos = Vec2::new(w / 2.0, 25.0 - 5.0 * al);
            if al >= 1.0 {
                p.m_outlined(
                    &format!("menuTitles/{t}"),
                    pos,
                    Org::Center,
                    1.0,
                    Color::WHITE,
                );
            } else {
                p.m(
                    &format!("menuTitles/{t}"),
                    pos,
                    Org::Center,
                    1.0,
                    Color::WHITE * al,
                );
            }
        }
        if let Some((old, t)) = &self.old {
            let k = 1.0 - cube_in(*t as f32 / 12.0);
            old.draw(
                &mut p,
                &View {
                    t: 99,
                    k,
                    tick: self.tick,
                    w,
                    s: &self.s,
                    backdrop: false,
                },
            );
        }
        let k = if self.opts.pin {
            1.0
        } else {
            cube_out(self.cur_t as f32 / cur.tween_in().max(1) as f32)
        };
        cur.draw(
            &mut p,
            &View {
                t: if self.opts.pin { 999 } else { self.cur_t },
                k,
                tick: self.tick,
                w,
                s: &self.s,
                backdrop: drew_level,
            },
        );
        // Button guides (input.md 3.2), icons of the first device.
        if let Some(first) = self.mi.first() {
            let x = w - 10.0;
            for (i, (label, bit)) in cur.guides().into_iter().enumerate() {
                let y = 226.0 - 18.0 * i as f32;
                let icon = first.icons.icon(bit, &self.s.options.keys);
                let iw = a.m(&icon).map_or(10.0, |s| s.width() as f32);
                p.icon(&icon, Vec2::new(x, y), Org::Just(1.0, 0.5));
                p.text_outlined(
                    label,
                    Vec2::new(x - iw - 4.0, y),
                    (1.0, 0.5),
                    1.0,
                    Color::WHITE,
                );
            }
        }
        if self.fade > 0.0 {
            p.rect(0.0, 0.0, w, 240.0, Color::BLACK * self.fade);
        }
    }
}

/// `MenuBackground` (ui.md 2.4): stars, tower top, repeated wall tile and
/// the sine-warped side overlays; `cam` = background camera y.
fn draw_background(p: &mut Painter, cam: f32, tick: u64) {
    let w = p.w;
    let a = p.a;
    let saved = p.list.transform();
    p.list
        .set_transform(tfp_render::glam::Affine2::from_translation(Vec2::new(
            0.0,
            -cam.round(),
        )));
    // Stars (animated sprite, 800 wide; 0.7 x camera parallax).
    let frame = a.anim_frame("BGStars", "0", tick);
    if let Some((stars, _)) = a.sprite_frame("BGStars", frame) {
        let h = stars.height() as f32;
        let mut y = (cam * 0.7).floor();
        while y < cam - h {
            y += h;
        }
        while y < cam + 240.0 {
            p.tex(stars, Vec2::new(0.0, y), Org::TopLeft, 1.0, Color::WHITE);
            y += h;
        }
    } else {
        p.rect(0.0, cam, w, 240.0, Color::rgb(16, 12, 40));
    }
    if let (Some(top), Some(tile)) = (a.m("towerTop"), a.m("towerTile")) {
        p.tex(
            top,
            Vec2::new((w / 2.0 - (top.width() / 2) as f32).floor(), 0.0),
            Org::TopLeft,
            1.0,
            Color::WHITE,
        );
        let th = tile.height() as f32;
        let mut y = top.height() as f32;
        while y < cam - th {
            y += th;
        }
        while y < cam + 240.0 {
            p.tex(
                tile,
                Vec2::new((w / 2.0 - (tile.width() / 2) as f32).floor(), y),
                Org::TopLeft,
                1.0,
                Color::WHITE,
            );
            y += th;
        }
    }
    if let (Some(l), Some(r)) = (a.m("overlayLeft"), a.m("overlayRight")) {
        let h = l.height() as f32;
        let mut y = (cam * -1.2).floor();
        while y < cam - h {
            y += h;
        }
        let counter = tick as f32 * 0.05;
        let add = std::f32::consts::PI / 12.0;
        while y < cam + 240.0 {
            p.sine_texture_v(l, Vec2::new(-1.0, y), counter, 1.0, 5, add);
            p.sine_texture_v(
                r,
                Vec2::new(w - r.width() as f32 + 1.0, y),
                counter,
                1.0,
                5,
                add,
            );
            y += h;
        }
    }
    p.list.set_transform(saved);
}

/// The title logo (ui.md 2.6) at (W/2, 70); `k` = tween progress.
fn draw_logo(
    p: &mut Painter,
    (mode, k): (LogoMode, f32),
    letters: f32,
    tick: u64,
    dark_world: bool,
) {
    let c = Vec2::new(p.w / 2.0, 70.0);
    let e = if mode == LogoMode::None {
        cube_in(k)
    } else {
        cube_out(k)
    };
    let light = 0.6 + 0.5 * sine(tick, 240.0, 0.0);
    let title_pos = c + Vec2::new(-320.0 * (1.0 - e), 0.0);
    let arrow_pos = c + Vec2::new(320.0 * (1.0 - e), 0.0);
    let tower = c + Vec2::new(0.0, 8.0 + 50.0 * (1.0 - e));
    let core = c + Vec2::new(-43.0, -35.0 + 50.0 * (1.0 - e));
    let bg_scale = gfx::back_out(e);
    let (dw, prefix) = if dark_world && p.a.m("title/darkWorld/title").is_some() {
        (true, "title/darkWorld/")
    } else {
        (false, "title/")
    };
    let (bg_name, bg_org, tw_name, tw_org) = if dw {
        ("bg", Org::At(148.0, 74.0), "towerBase", Org::At(58.0, 2.0))
    } else {
        (
            "titleBg",
            Org::At(141.0, 60.0),
            "ascension",
            Org::At(60.0, 15.0),
        )
    };
    if let Some(bg) = p.a.m(&format!("{prefix}{bg_name}")) {
        p.tex_ex(
            bg,
            c,
            bg_org,
            Vec2::splat(bg_scale),
            0.0,
            Color::WHITE,
            false,
        );
    }
    p.m(
        &format!("{prefix}{tw_name}"),
        tower,
        tw_org,
        1.0,
        Color::WHITE,
    );
    // Animated core (8 frames of 59x55 / Dark World 9 of 55x83, 0.1 s each).
    let (core_name, fw, fh, n) = if dw {
        ("title/darkWorld/towerCore", 55, 83, 9)
    } else {
        ("title/ascensionCoreAnimation", 59, 55, 8)
    };
    if let Some(sheet) = p.a.m(core_name) {
        let i = (tick / 6) as i32 % n;
        let cols = (sheet.width() / fw).max(1);
        let f = sheet.sub((i % cols) * fw, (i / cols) * fh, fw, fh);
        p.tex(f, core, Org::Center, 1.0, Color::WHITE);
    }
    let arrow = if dw {
        "title/darkWorld/arrow"
    } else {
        "title/bigArrow"
    };
    p.m(arrow, arrow_pos, Org::At(156.0, 24.0), 1.0, Color::WHITE);
    let (t, tl) = if dw {
        ("title/darkWorld/title", "title/darkWorld/titleLight")
    } else {
        ("title/titleLetters", "title/titleLettersLight")
    };
    p.m(t, title_pos, Org::At(126.0, 37.0), 1.0, Color::WHITE);
    p.m(
        tl,
        title_pos,
        Org::At(126.0, 37.0),
        1.0,
        Color::WHITE * light.clamp(0.0, 1.0),
    );
    // The letters' own tween: in (CubeOut) with the logo or after it,
    // out (CubeIn) on their own.
    let le = if mode == LogoMode::WithLetters {
        cube_out(letters)
    } else {
        cube_in(letters)
    } * e;
    if dw && le > 0.0 {
        let e = le;
        let glow = (0.7 + 0.3 * sine(tick, 240.0, 0.0)) * e;
        p.m(
            "title/darkWorld/dark",
            c,
            Org::At(76.0, -28.0),
            1.0,
            Color::WHITE * e,
        );
        p.m(
            "title/darkWorld/world",
            c,
            Org::At(-22.0, -28.0),
            1.0,
            Color::WHITE * e,
        );
        p.m(
            "title/darkWorld/darkGlow",
            c,
            Org::At(76.0, -27.0),
            1.0,
            Color::WHITE * glow,
        );
        p.m(
            "title/darkWorld/worldGlow",
            c,
            Org::At(-21.0, -27.0),
            1.0,
            Color::WHITE * glow,
        );
    } else if !dw && le > 0.0 {
        let _ = k;
        for i in 0..9 {
            let bob = sine(tick, 120.0, i as f32 * std::f32::consts::PI / 8.0) * 2.0;
            let target = c + Vec2::new(-80.0 + 26.0 * i as f32, 35.0 + bob);
            // One by one: in from the first, out from the last.
            let li = if mode == LogoMode::WithLetters {
                (le * 1.2 - i as f32 * 0.02).clamp(0.0, 1.0)
            } else {
                (le * 1.2 - (8 - i) as f32 * 0.02).clamp(0.0, 1.0)
            };
            let pos = lerp(target + Vec2::new(30.0, 30.0), target, cube_out(li));
            p.m(
                &format!("title/ascensionLetters/{i}"),
                pos,
                Org::Center,
                1.0,
                Color::WHITE * li,
            );
        }
    }
    let _ = pal::SELECTED;
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::audio::Audio;
    use crate::input::{InputHub, RawInput};
    use sdl2::controller::Button;
    use sdl2::keyboard::Scancode;

    struct H {
        fe: FrontEnd,
        hub: InputHub,
        raw: RawInput,
        audio: Audio,
        host: HostRequests,
        tick: u64,
    }

    impl H {
        fn new(opts: FrontOptions) -> Self {
            let mut a = UiAssets::placeholder(tfp_render::TextureId::WHITE);
            for (i, n) in ["SACRED GROUND", "TWILIGHT SPIRE", "BACKFIRE"]
                .iter()
                .enumerate()
            {
                a.towers.push(assets::Tower {
                    name: n.to_string(),
                    icon: String::new(),
                    map_pos: (100.0 + 50.0 * i as f32, 150.0),
                    block: "levelBlock",
                    procedural: false,
                    dark_world: false,
                    theme: String::new(),
                    levels: vec![],
                });
            }
            let mut fe = FrontEnd::new(opts);
            fe.init(a);
            Self {
                fe,
                hub: InputHub::new(),
                raw: RawInput::default(),
                audio: Audio::offline(None).unwrap(),
                host: HostRequests::default(),
                tick: 0,
            }
        }

        fn step(&mut self, n: u32) {
            for _ in 0..n {
                self.hub.snapshot(&mut self.raw);
                self.fe.tick(&mut TickContext {
                    input: &self.raw,
                    audio: &mut self.audio,
                    host: &mut self.host,
                    tick: self.tick,
                });
                self.tick += 1;
            }
        }

        fn key(&mut self, k: Scancode) {
            self.hub.key(k as usize, true);
            self.step(1);
            self.hub.key(k as usize, false);
            self.step(1);
        }

        fn pad(&mut self, id: u32, b: Button) {
            self.hub.pad_button(id, b as usize, true);
            self.step(1);
            self.hub.pad_button(id, b as usize, false);
            self.step(1);
        }

        /// Steps until `id` is current and accepting input.
        fn until(&mut self, id: ScreenId) {
            for _ in 0..600 {
                if self.fe.screen() == Some(id) && self.fe.cur_t > 25 && self.fe.fade_to.is_none() {
                    return;
                }
                self.step(1);
            }
            panic!("never reached {id:?}; at {:?}", self.fe.screen());
        }
    }

    #[test]
    fn versus_flow_keyboard_and_pad() {
        let mut h = H::new(FrontOptions {
            screen: Some(ScreenId::PressStart),
            music: false,
            ..Default::default()
        });
        h.until(ScreenId::PressStart);
        h.key(Scancode::C);
        h.until(ScreenId::Main);
        h.key(Scancode::C); // VERSUS
        h.until(ScreenId::Rollcall);
        assert_eq!(h.fe.s.capacity, 4);
        // A pad appears: with 4 slots devices are pre-bound in order (pads first).
        h.hub.connect(1, "Xbox 360 Controller");
        h.step(2);
        assert_eq!(
            h.fe.s.slots[0].owner,
            Some(settings::Owner::Local(minput::Dev::Pad(1)))
        );
        assert_eq!(
            h.fe.s.slots[1].owner,
            Some(settings::Owner::Local(minput::Dev::Keyboard(0)))
        );
        // Carousel on the keyboard slot: archer 1 -> 2.
        h.key(Scancode::Right);
        assert_eq!(h.fe.s.slots[1].archer, 2);
        h.pad(1, Button::A);
        h.key(Scancode::C);
        assert_eq!(h.fe.s.joined_count(), 2);
        // Joined slot leaves with Back, rejoins.
        h.key(Scancode::X);
        assert_eq!(h.fe.s.joined_count(), 1);
        h.key(Scancode::C);
        assert_eq!(h.fe.s.joined_count(), 2);
        h.key(Scancode::C); // start from a joined slot
        h.until(ScreenId::VersusOptions);
        assert_eq!(
            h.fe.s.versus.mode,
            settings::Mode::LastManStanding,
            "2 archers"
        );
        // Length: up to the length row, then right (STANDARD -> EPIC).
        h.key(Scancode::Up);
        h.key(Scancode::Up);
        h.key(Scancode::Right);
        assert_eq!(h.fe.s.versus.length, settings::Length::Epic);
        h.key(Scancode::Down); // variants row
        h.key(Scancode::C);
        h.until(ScreenId::Variants);
        // Toggle the first variant past the action and preset rows.
        // 3 actions (no Dark World) + 5 presets.
        for _ in 0..8 {
            h.key(Scancode::Right);
        }
        h.key(Scancode::C);
        assert!(h.fe.s.versus.variants.value(0), "NO AUTOBALANCE on");
        h.key(Scancode::X);
        h.until(ScreenId::VersusOptions);
        h.key(Scancode::Return); // Start begins from anywhere
        h.until(ScreenId::Map);
        h.step(25);
        h.key(Scancode::Right); // RANDOM -> ... first tower is selected initially
        h.key(Scancode::C);
        h.until(ScreenId::MatchReady);
        let s = h.fe.setup().expect("setup");
        assert_eq!(s.slots.iter().filter(|x| x.joined).count(), 2);
        assert_eq!(s.settings.length, settings::Length::Epic);
        assert_eq!(s.tower_name, "TWILIGHT SPIRE");
        assert!(s.settings.variants.value(0));
        // Pause menu and back.
        h.key(Scancode::Return);
        h.until(ScreenId::Pause);
        h.key(Scancode::Return);
        h.until(ScreenId::MatchReady);
    }

    #[test]
    fn twelve_pads_join() {
        let mut h = H::new(FrontOptions {
            screen: Some(ScreenId::Rollcall),
            capacity: Some(12),
            music: false,
            ..Default::default()
        });
        for id in 0..12 {
            h.hub.connect(100 + id, "Virtual Controller");
        }
        h.until(ScreenId::Rollcall);
        assert_eq!(h.fe.s.capacity, 12);
        assert_eq!(h.fe.game_size(), (420, 240));
        // Nothing is bound in advance; the first press binds, the second joins.
        assert!(h.fe.s.slots.iter().all(|s| s.owner.is_none()));
        for id in (0..12).rev() {
            h.pad(100 + id, Button::A);
        }
        // Lowest free slot in press order: pad 111 got P1.
        assert_eq!(
            h.fe.s.slots[0].owner,
            Some(settings::Owner::Local(minput::Dev::Pad(111)))
        );
        for id in 0..11 {
            h.pad(100 + id, Button::A);
        }
        assert_eq!(h.fe.s.joined_count(), 11);
        let archers: std::collections::BTreeSet<usize> =
            h.fe.s
                .slots
                .iter()
                .filter(|s| s.joined)
                .map(|s| s.archer)
                .collect();
        assert_eq!(
            archers.len(),
            8,
            "every usable archer (no Red without Dark World) before duplicates"
        );
        h.pad(111, Button::A);
        assert_eq!(h.fe.s.joined_count(), 12);
        // Full: auto start after the 1-second grace.
        h.until(ScreenId::VersusOptions);
        // Unplugging a pad in the rollcall un-joins it.
        h.fe.switch(ScreenId::Rollcall);
        h.hub.disconnect(105);
        h.until(ScreenId::Rollcall);
        assert_eq!(h.fe.s.joined_count(), 11);
    }
}

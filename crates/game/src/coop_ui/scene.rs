//! `CoopPlay`: a Quest, Trials or Dark World attempt on the sim
//! (`quest::new_quest`, `trials::new_trial`, `darkworld::new_dark_world`;
//! the Dark World art and HUD are in `darkworld`): devices / bots to
//! `PlayerInput`, the level look
//! with the sim's drawables (enemy art swapped in by `art`), particles,
//! death leftovers, sounds, the HUDs, the pause menu and the end menus
//! (restart, next trial, map, quit), session records.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tfp_render::Color;
use tfp_render::glam::Vec2;
use tfp_sim::darkworld::{DwPhase, DwSettings};
use tfp_sim::quest::{QuestPhase, QuestSettings};
use tfp_sim::trials::{TrialsPhase, TrialsSettings};
use tfp_sim::{Content, Event, PlayerInput, World};

use super::art::{self, Anims, Art};
use super::darkworld::bosses::{BossFx, DwArt};
use super::darkworld::hud::{DwHud, Tutorial};
use super::debris::Debris;
use super::quest_hud::{self, QuestHud};
use super::trials_hud::TrialsHud;
use super::{CoopLaunch, CoopMode, Exit, Records};
use crate::app::{DrawContext, LoadContext, Scene, TickContext};
use crate::audio::{Audio, SoundData};
use crate::front::assets::UiAssets;
use crate::front::gfx::{Painter, pal};
use crate::front::minput::{Dev, KeyLayout, KeyboardSetup, MenuInput, b};
use crate::front::settings::Owner;
use crate::tower::{TowerGfx, TowerView};
use crate::versus_ui::VersusUi;

const MAXP: usize = tfp_sim::world::MAX_PLAYERS;

#[derive(Clone, Debug)]
pub struct CoopOptions {
    pub launch: CoopLaunch,
    pub records: Records,
    /// Bind the keyboard (off for headless runs).
    pub keyboard: bool,
    /// Headless proof: capture native frames into DIR.
    pub shots: Option<PathBuf>,
    /// Show the title card (first start from the map).
    pub start_screen: bool,
    pub seed: i32,
    pub hitboxes: bool,
    /// Headless proof of the result screens: from sim frame N, end the
    /// quest (true = complete: enemies die and the program skips to the
    /// level ending; false = game over: lives gone, archers die).
    pub end: Option<(bool, u32)>,
    /// Tower music and jingles (audio.md 6).
    pub music: bool,
    pub sfx_volume: f32,
    /// Controller remaps by pad name (`front::remap`).
    pub pads: BTreeMap<String, crate::padmap::PadMap>,
    /// The keyboard configurations (options).
    pub keys: KeyboardSetup,
    /// GAMEPAD RUMBLE (input.md 2.4).
    pub rumble: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Driver {
    Pad(u32),
    Keyboard(usize),
    Bot,
    Idle,
}

/// Headless proof aids for Dark World (`--dw-*` flags; none in play).
#[derive(Clone, Debug, Default)]
pub struct DwProof {
    /// Start on this level of the session (`darkworld::jump_to_level`).
    pub level: Option<usize>,
    /// Test assistance: archers invulnerable with arrows, leftovers
    /// cleared, archers carried into the open exit portal, and every N
    /// ticks one point of damage to the boss when it can take it.
    pub assist: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Resume,
    Restart,
    NextTrial,
    /// Dark World: spend a continue (game over) / restart the level (pause).
    Continue,
    RestartLevel,
    Map,
    Quit,
}

impl Action {
    /// `cont`: Dark World continues remaining (-1 = unlimited).
    fn label(self, cont: i32) -> String {
        let n = |s: &str| {
            if cont < 0 {
                s.to_string()
            } else {
                format!("{s} ({cont})")
            }
        };
        match self {
            Action::Resume => "RESUME".into(),
            Action::Restart => "RESTART".into(),
            Action::NextTrial => "NEXT TRIAL".into(),
            Action::Continue => n("CONTINUE"),
            Action::RestartLevel => n("RESTART LEVEL"),
            Action::Map => "MAP".into(),
            Action::Quit => "QUIT".into(),
        }
    }
}

/// Pause / end menu (ui.md 10.4, the co-op rows).
struct Menu {
    items: Vec<Action>,
    sel: usize,
    opened: u32,
    paused: bool,
    /// Continues remaining for the Dark World labels.
    cont: i32,
}

impl Menu {
    fn draw(&self, p: &mut Painter, tick: u64) {
        let w = p.w;
        let n = self.items.len() as f32;
        let centre = if self.paused {
            Vec2::new(w / 2.0, 120.0)
        } else {
            Vec2::new(w / 2.0, 212.0)
        };
        let ph = 10.0 * n + 30.0;
        if self.paused {
            p.rect(0.0, 0.0, w, 240.0, Color::BLACK * 0.5);
        }
        p.panel(centre.x - 60.0, centre.y - ph / 2.0, 120.0, ph);
        if self.paused {
            p.text_outlined(
                "PAUSED",
                Vec2::new(centre.x, centre.y - ph / 2.0 - 10.0),
                (0.5, 0.5),
                2.0,
                Color::WHITE,
            );
        }
        let y0 = centre.y - ((n - 1.0) * 14.0 + 39.0) / 2.0 + 20.0;
        for (i, a) in self.items.iter().enumerate() {
            let y = y0 + 14.0 * i as f32;
            if i == self.sel {
                let col = if (tick / 4).is_multiple_of(2) {
                    pal::PAUSE_A
                } else {
                    pal::PAUSE_B
                };
                p.text(
                    &format!("> {}", a.label(self.cont)),
                    Vec2::new(centre.x, y),
                    (0.5, 0.5),
                    1.0,
                    col,
                );
            } else {
                p.text(
                    &a.label(self.cont),
                    Vec2::new(centre.x, y),
                    (0.5, 0.5),
                    1.0,
                    pal::PAUSE_IDLE,
                );
            }
        }
    }
}

/// Sounds by the original's field name (`sfx_slimeDie`, through the SFX
/// catalogue, audio.md) or by WAV base name (voices `CHAR_*`), loaded on
/// first use (`_NN` variants collected).
struct Sfx {
    dirs: Vec<PathBuf>,
    catalogue: tfp_assets::audio::catalogue::SfxCatalogue,
    cache: BTreeMap<String, Vec<Arc<SoundData>>>,
}

impl Sfx {
    fn get(&mut self, base: &str) -> &[Arc<SoundData>] {
        if !self.cache.contains_key(base)
            && let Ok(v) = self.catalogue.effect_pcm(base)
            && !v.is_empty()
        {
            let v = v.iter().map(|p| crate::content::sound(p)).collect();
            self.cache.insert(base.to_owned(), v);
        }
        if !self.cache.contains_key(base) {
            let load = |p: PathBuf| {
                p.exists()
                    .then(|| tfp_assets::audio::Wav::load(&p).ok())
                    .flatten()
                    .map(|pcm| crate::content::sound(&pcm))
            };
            let mut v: Vec<Arc<SoundData>> = self
                .dirs
                .iter()
                .find_map(|d| load(d.join(format!("{base}.wav"))))
                .into_iter()
                .collect();
            if v.is_empty() {
                for n in 1..=9 {
                    if let Some(s) = self
                        .dirs
                        .iter()
                        .find_map(|d| load(d.join(format!("{base}_{n:02}.wav"))))
                    {
                        v.push(s);
                    }
                }
            }
            self.cache.insert(base.to_owned(), v);
        }
        &self.cache[base]
    }
}

/// Voice-set prefixes by SFX index (audio.md 4).
/// Alternate voice sets (9-13) borrow what they lack from their base set
/// (audio.md 4).
const VOICE_FALLBACK: [usize; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 5, 0, 3, 2, 8];

const VOICES: [&str; 14] = [
    "CHAR_B", "CHAR_C", "CHAR_D", "CHAR_A", "CHAR_F", "CHAR_E", "CHAR_H", "CHAR_G", "CHAR_I",
    "CHAR_E2", "CHAR_B2", "CHAR_A2", "CHAR_D2", "CHAR_I2",
];

fn char_sound(s: tfp_sim::events::CharSound) -> String {
    format!("{s:?}")
        .chars()
        .fold(String::new(), |mut acc, ch| {
            if ch.is_uppercase() && !acc.is_empty() {
                acc.push('_');
            }
            acc.push(ch.to_ascii_uppercase());
            acc
        })
        .replace("NO_FIRE", "NOFIRE")
}

pub struct CoopPlay {
    opts: CoopOptions,
    pub records: Records,
    /// Set when the player leaves (MAP / QUIT).
    pub exit: Option<Exit>,
    gfx: Option<TowerGfx>,
    ua: Option<UiAssets>,
    view: Option<TowerView>,
    view_for: Option<usize>,
    content: Option<Content>,
    gunn_canvas: crate::match_draw::GunnCanvas,
    /// Archer / corpse / arrow cosmetics' art (registered in `content`).
    archer_art: Option<crate::versus_ui::actors::ArcherArt>,
    art: Art,
    trials: Vec<TrialsSettings>,
    index: Option<tfp_assets::towers::TowerIndex>,
    alts: Vec<bool>,
    voices: Vec<usize>,
    music: Option<crate::music::MusicPlayer>,
    /// Effect kinds and the master pitch (`crate::sfx`); whether the pause
    /// menu holds them.
    sfxp: crate::sfx::SfxPlayer,
    sfx_paused: bool,
    /// Archer and entity loops, and the level they belong to.
    loops: crate::sfx::Loops,
    loops_level: Option<usize>,
    /// Rumble requests for the host (pad id, strength, frames).
    rumble_out: Vec<(u32, f32, u32)>,
    theme_cue: String,
    music_req: String,
    world: Option<World>,
    ui: Option<VersusUi>,
    debris: Debris,
    anims: Anims,
    qhud: Option<QuestHud>,
    thud: Option<TrialsHud>,
    drivers: Vec<Driver>,
    /// The shared bot (`tfp_sim::bot`) per slot.
    bots: Vec<tfp_sim::bot::Bot>,
    slot_archers: Vec<Option<u16>>,
    keys: KeyboardSetup,
    mi: MenuInput,
    menu: Option<Menu>,
    sfx: Option<Sfx>,
    seen: std::collections::BTreeSet<(u32, usize)>,
    frame: u32,
    shake: u32,
    size: (u32, u32),
    auto_t: u32,
    recorded: bool,
    // Headless proof.
    capture: Option<PathBuf>,
    shot_n: u32,
    shot_tags: Vec<String>,
    last_wave: i32,
    phase_frames: u32,
    last_phase: u8,
    // Dark World.
    /// Proof aids (set after `new`, before `init`).
    pub dw_proof: DwProof,
    /// The "SHOW VERSUS LESSONS" option (the original's `ShowTips`), which
    /// also gates Darkfang's trigger-arrow slide.
    pub show_tips: bool,
    /// SCREEN WAVE EFFECTS (the slow-motion wave).
    pub screen_wave: bool,
    wave_counter: f32,
    dw_art: DwArt,
    bfx: BossFx,
    dhud: Option<DwHud>,
    tutorial: Option<Tutorial>,
    /// Arena `.oel` per content level and the theme of each DW tower.
    dw_paths: Vec<PathBuf>,
    dw_themes: Vec<String>,
    dw_deaths_base: u32,
    dw_last_round: usize,
    /// This session's result went to the save store.
    dw_saved: bool,
    /// The King Reaper's intro (from King's Court's King): first frame
    /// seen; the spotlight stays on it for 120 ticks.
    reaper_intro: Option<(tfp_sim::EntityId, u32)>,
    /// King's Court: the frame the King's transformation began.
    king_spot: Option<u32>,
    /// Quest tower 0 in Normal: the controls screen before the title card.
    controls: Option<super::intro_controls::IntroControls>,
    controls_pending: bool,
    /// The exit-portal camera zoom's canvas (game size).
    zoom_canvas: Option<(tfp_render::TextureId, (u32, u32))>,
}

/// The trigger-arrow slide shows once per run (`TriggerArrowScreen.Shown`).
static TRIGGER_SLIDE_SHOWN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl CoopPlay {
    /// The pause menu is open (flow test).
    pub fn paused(&self) -> bool {
        self.menu.as_ref().is_some_and(|m| m.paused)
    }

    pub fn new(opts: CoopOptions) -> Self {
        let keys = if opts.keys.layouts.is_empty() {
            KeyboardSetup::default()
        } else {
            opts.keys.clone()
        };
        let records = opts.records.clone();
        Self {
            opts,
            records,
            exit: None,
            gfx: None,
            ua: None,
            view: None,
            view_for: None,
            content: None,
            gunn_canvas: Default::default(),
            archer_art: None,
            art: Art::default(),
            trials: Vec::new(),
            index: None,
            alts: Vec::new(),
            voices: Vec::new(),
            music: None,
            sfxp: Default::default(),
            sfx_paused: false,
            loops: Default::default(),
            loops_level: None,
            rumble_out: Vec::new(),
            theme_cue: String::new(),
            music_req: String::new(),
            world: None,
            ui: None,
            debris: Debris::default(),
            anims: Anims::default(),
            qhud: None,
            thud: None,
            drivers: Vec::new(),
            bots: Vec::new(),
            slot_archers: vec![None; MAXP],
            keys,
            mi: MenuInput::default(),
            menu: None,
            sfx: None,
            seen: Default::default(),
            frame: 0,
            shake: 0,
            size: (320, 240),
            auto_t: 0,
            recorded: false,
            capture: None,
            shot_n: 0,
            shot_tags: Vec::new(),
            last_wave: -1,
            phase_frames: 0,
            last_phase: u8::MAX,
            dw_proof: DwProof::default(),
            show_tips: true,
            screen_wave: true,
            wave_counter: 0.0,
            dw_art: DwArt::default(),
            bfx: BossFx::default(),
            dhud: None,
            tutorial: None,
            dw_paths: Vec::new(),
            dw_themes: Vec::new(),
            dw_deaths_base: 0,
            dw_last_round: 0,
            zoom_canvas: None,
            dw_saved: false,
            reaper_intro: None,
            king_spot: None,
            controls: None,
            controls_pending: false,
        }
    }

    /// The Dark World session, for tests and captures.
    pub fn dark_world(&self) -> Option<&tfp_sim::darkworld::DarkWorld> {
        self.world.as_ref()?.dark_world.as_deref()
    }

    /// The quest phase / trials phase, for tests and captures.
    pub fn quest(&self) -> Option<&tfp_sim::quest::Quest> {
        self.world.as_ref()?.quest.as_deref()
    }

    pub fn trial(&self) -> Option<&tfp_sim::trials::Trials> {
        self.world.as_ref()?.trials.as_deref()
    }

    /// What is being played (Next Trial changes it).
    #[allow(dead_code)]
    pub fn mode(&self) -> CoopMode {
        self.opts.launch.mode
    }

    #[cfg(test)]
    pub fn world_content_mut(&mut self) -> (&mut World, &Content) {
        (self.world.as_mut().unwrap(), self.content.as_ref().unwrap())
    }

    /// The frame's drawables as `draw` builds them.
    #[cfg(test)]
    pub fn frame_view(&mut self) -> tfp_sim::View {
        let (w, c) = (self.world.as_ref().unwrap(), self.content.as_ref().unwrap());
        let mut view = tfp_sim::View::build(w, c, tfp_sim::ViewOptions::default());
        let cx = art::Ctx {
            c,
            art: &self.art,
            slot_archers: &self.slot_archers,
            tick: self.frame as u64,
        };
        art::apply(&mut view, w, &cx, &mut self.anims);
        self.bfx.apply(&mut view, w, c, &self.dw_art);
        view
    }

    /// Content archer index of base archer `a` (alt costume: the loader
    /// appends the alts in base order after the nine base archers).
    fn content_archer(&self, a: usize, alt: bool) -> u16 {
        if alt && self.alts.get(a).copied().unwrap_or(false) {
            let before = self.alts.iter().take(a).filter(|x| **x).count();
            (9 + before) as u16
        } else {
            a.min(8) as u16
        }
    }

    pub fn init(&mut self, gpu: &tfp_render::Gpu, r: &mut tfp_render::Renderer) -> Result<()> {
        let installs = tfp_assets::Installs::discover();
        let Some(inst) = installs.steam.clone() else {
            bail!("Quest/Trials need the Steam install (set TF_GAME_DIR)")
        };
        let ua = UiAssets::load(gpu, r, false);
        let mut g = TowerGfx::load_with_main(gpu, r, &inst, ua.main.as_ref().map(|m| m.texture))?;
        if let Err(e) = g.load_boss(gpu, r) {
            log::warn!("boss atlas: {e:#}");
        }
        let index = tfp_assets::towers::TowerIndex::build(&inst)?;
        let gd = tfp_assets::game_data::GameData::load(&inst.content)?;
        self.alts = gd.archers.alts.iter().map(|a| a.is_some()).collect();
        let mut c = match self.opts.launch.mode {
            CoopMode::Quest { .. } => tfp_sim::loader::quest_content(&inst)?,
            CoopMode::Trials { .. } => {
                let (c, s) = tfp_sim::loader::trials_content(&inst)?;
                self.trials = s;
                c
            }
            CoopMode::DarkWorld { .. } => {
                if inst.dark_world.is_none() {
                    bail!("Dark World needs the expansion content (DarkWorldContent)");
                }
                tfp_sim::loader::dark_world_content(&inst)?
            }
        };
        // Dark World: the arenas in the loader's order (towers, then files).
        self.dw_paths = index
            .dark_world
            .iter()
            .flat_map(|t| t.levels.iter().cloned())
            .collect();
        self.dw_themes = index.dark_world.iter().map(|t| t.theme.clone()).collect();
        // The arenas' theme flags (the loader only knows versus towers).
        let themes: Vec<String> = match self.opts.launch.mode {
            CoopMode::Quest { .. } => index.quest.iter().map(|q| q.theme.clone()).collect(),
            CoopMode::Trials { .. } => index.trials.iter().map(|t| t.theme.clone()).collect(),
            CoopMode::DarkWorld { .. } => index
                .dark_world
                .iter()
                .flat_map(|t| std::iter::repeat_n(t.theme.clone(), t.levels.len()))
                .collect(),
        };
        for (lvl, th) in c.levels.iter_mut().zip(&themes) {
            if let Some(t) = gd.theme(th) {
                lvl.tower.theme = tfp_sim::entities::ThemeInfo::from_theme(t);
            }
        }
        self.art = Art::register(&mut c, &inst);
        self.archer_art = Some(crate::versus_ui::actors::build_art(&mut c, &inst, &gd));
        if matches!(self.opts.launch.mode, CoopMode::DarkWorld { .. }) {
            self.dw_art = DwArt::register(&mut c, &inst);
        }
        self.voices = c.archers.iter().map(|a| a.voice as usize).collect();
        self.sfx = Some(Sfx {
            dirs: std::iter::once(inst.content.sfx_dir())
                .chain(inst.dark_world.iter().map(|d| d.sfx_dir()))
                .collect(),
            catalogue: tfp_assets::audio::catalogue::SfxCatalogue::open(&inst),
            cache: BTreeMap::new(),
        });
        if self.opts.music {
            match crate::music::MusicPlayer::open(&inst.content.music_dir()) {
                Ok(m) => self.music = Some(m),
                Err(e) => log::warn!("music: {e:#}"),
            }
        }
        let tips = quest_hud::load_tips(&inst);
        self.content = Some(c);
        self.index = Some(index);
        self.gfx = Some(g);
        self.ua = Some(ua);
        self.build(self.opts.start_screen);
        if let Some(h) = &mut self.qhud {
            h.tips = tips;
        }
        self.ensure_view(gpu, r)?;
        Ok(())
    }

    /// The music cue of the current level's theme.
    fn theme_music(&self) -> String {
        let (Some(index), Some(g)) = (&self.index, &self.gfx) else {
            return String::new();
        };
        let theme = match self.opts.launch.mode {
            CoopMode::Quest { tower, .. } => index.quest.get(tower).map(|q| q.theme.clone()),
            CoopMode::Trials { level } => index.trials.get(level).map(|t| t.theme.clone()),
            CoopMode::DarkWorld { tower, .. } => self.dw_themes.get(tower).cloned(),
        };
        theme
            .and_then(|t| g.la.game_data.theme(&t))
            .map_or(String::new(), |t| t.music.clone())
    }

    fn theme_title(&self, level: usize) -> (String, String) {
        let (Some(index), Some(g)) = (&self.index, &self.gfx) else {
            return (String::new(), String::new());
        };
        let (theme, tier) = match self.opts.launch.mode {
            CoopMode::Quest { .. } => (index.quest.get(level).map(|q| q.theme.clone()), 0),
            CoopMode::Trials { .. } => (
                index.trials.get(level).map(|t| t.theme.clone()),
                index.trials.get(level).map_or(0, |t| t.tier),
            ),
            CoopMode::DarkWorld { .. } => (self.dw_themes.get(level).cloned(), 0),
        };
        let th = theme.and_then(|t| g.la.game_data.theme(&t));
        let name = th.map_or("TOWER".into(), |t| t.name.to_uppercase());
        let name = match tier {
            1 => format!("{name} II"),
            2 => format!("{name} III"),
            _ => name,
        };
        (
            name,
            th.map_or(String::new(), |t| format!("towerIcons/{}", t.icon)),
        )
    }

    /// (Re)creates the world for an attempt.
    fn build(&mut self, start_screen: bool) {
        let Some(c) = &self.content else { return };
        let players = self.opts.launch.players.clone();
        self.slot_archers = vec![None; MAXP];
        for p in &players {
            if p.slot < MAXP {
                self.slot_archers[p.slot] = Some(self.content_archer(p.archer, p.alt));
            }
        }
        let seed = self.opts.seed.wrapping_add(self.frame as i32);
        self.theme_cue = self.theme_music();
        self.music_req.clear();
        if let Some(m) = &self.music {
            let mut cues = vec![self.theme_cue.clone(), "Boss".into(), "GameOver".into()];
            cues.push("VictoryTeam".into());
            if matches!(self.opts.launch.mode, CoopMode::DarkWorld { .. }) {
                cues.push("DarkBoss".into());
            }
            m.prefetch(&cues);
        }
        let world = match self.opts.launch.mode {
            CoopMode::Quest { tower, hardcore } => {
                let mut s = QuestSettings::new(tower, tower, 0);
                s.tower_id = tower as u8;
                s.hardcore = hardcore;
                s.slots = [None; MAXP];
                for p in &players {
                    s.slots[p.slot] = self.slot_archers[p.slot];
                    s.crowns[p.slot] = self.records.crowns.get(p.slot).copied().unwrap_or(false);
                }
                s.extended = players.len() > 2;
                s.seed = seed;
                s.start_screen = start_screen;
                let rec = self.records.quest.entry(tower).or_default();
                rec.attempts += 1;
                let (title, icon) = self.theme_title(tower);
                let gems = players.iter().map(|p| (p.slot, p.archer)).collect();
                let mut h = QuestHud::new(title, icon, gems);
                if let Some(old) = self.qhud.take() {
                    h.tips = old.tips;
                    h.tip = old.tip + 1;
                }
                self.qhud = Some(h);
                self.controls_pending = start_screen && tower == 0 && !hardcore;
                tfp_sim::quest::new_quest(c, s)
            }
            CoopMode::Trials { level } => {
                let mut s = self
                    .trials
                    .get(level)
                    .copied()
                    .unwrap_or(TrialsSettings::new(level));
                s.archer = players
                    .first()
                    .map_or(0, |p| self.content_archer(p.archer, p.alt));
                let rec = self.records.trials.entry(level).or_default();
                rec.attempts += 1;
                s.best = rec.best;
                s.unlocked = rec.unlocked;
                s.gem_found = rec.gem;
                s.start_screen = start_screen;
                let (title, icon) = self.theme_title(level);
                let mut h = TrialsHud::new(title, icon, s.best, s.unlocked);
                h.gems_found = self.records.trials.values().filter(|r| r.gem).count();
                self.thud = Some(h);
                // The sim's Trials player is slot 0.
                self.slot_archers = vec![None; MAXP];
                self.slot_archers[0] = Some(s.archer);
                tfp_sim::trials::new_trial(c, s)
            }
            CoopMode::DarkWorld {
                tower,
                difficulty,
                variants,
            } => {
                let mut s = DwSettings::new(tower, difficulty, 0);
                for p in &players {
                    s.slots[p.slot] = self.slot_archers[p.slot];
                    s.crowns[p.slot] = self.records.crowns.get(p.slot).copied().unwrap_or(false);
                }
                s.variants = variants;
                s.extended = players.len() > 4;
                s.seed = seed;
                s.start_screen = start_screen;
                let rec = self.records.dark_world.entry(tower).or_default();
                rec.attempts += 1;
                self.dw_deaths_base = rec.deaths;
                let (title, icon) = self.theme_title(tower);
                let cata = self
                    .gfx
                    .as_ref()
                    .and_then(|g| g.la.game_data.theme(self.dw_themes.get(tower)?))
                    .is_some_and(|t| t.tower_type.eq_ignore_ascii_case("Cataclysm"));
                let block = if cata {
                    "vortexLevelBlock"
                } else {
                    "darkLevelBlock"
                };
                let cards = players
                    .iter()
                    .map(|p| super::darkworld::hud::Card {
                        slot: p.slot,
                        archer: p.archer,
                        alt: p.alt,
                    })
                    .collect();
                let icons = tfp_sim::variants::Variant::ALL
                    .iter()
                    .filter(|&&v| variants.any(v))
                    .map(|&v| super::darkworld::variant_ui(v).1)
                    .filter(|i| !i.is_empty())
                    .collect();
                self.dhud = Some(DwHud::new(title, block.into(), icon, !cata, cards, icons));
                // Darkfang's first level shows the trigger-arrow slide once.
                self.tutorial = (tower == 2
                    && start_screen
                    && self.show_tips
                    && !TRIGGER_SLIDE_SHOWN.load(std::sync::atomic::Ordering::Relaxed))
                .then(|| Tutorial::new(self.alt_shoot_icon()));
                // The sim runs the slide (its `Tips` phase); the HUD follows it.
                s.trigger_tips = self.tutorial.is_some();
                let mut w = tfp_sim::darkworld::new_dark_world(c, s);
                if let Some(l) = self.dw_proof.level {
                    tfp_sim::darkworld::jump_to_level(&mut w, c, l);
                }
                w
            }
        };
        let ua = self.ua.as_ref();
        let mut looks: Vec<crate::front::assets::Archer> =
            ua.map(|a| a.archers.clone()).unwrap_or_default();
        looks.extend(
            ua.into_iter()
                .flat_map(|a| a.archers.iter().filter_map(|x| x.alt.as_deref().cloned())),
        );
        let mut ui = VersusUi::new(looks);
        ui.replays = false;
        if let Some(a) = &self.archer_art {
            crate::versus_ui::actors::attach(&mut ui, a.clone());
        }
        self.ui = Some(ui);
        self.size = (c.level(world.level).width, c.level(world.level).height);
        self.world = Some(world);
        self.debris.clear();
        self.anims.clear();
        self.bfx.clear();
        self.dw_last_round = 0;
        self.dw_saved = false;
        self.menu = None;
        self.recorded = false;
        self.auto_t = 0;
        self.seen.clear();
        self.bots = (0..MAXP)
            .map(|i| tfp_sim::bot::Bot::new(i as i32))
            .collect();
        self.assign();
    }

    /// Slots to drivers: the menus' owners; bots for the rest.
    fn assign(&mut self) {
        self.drivers = vec![Driver::Idle; MAXP];
        let trials = matches!(self.opts.launch.mode, CoopMode::Trials { .. });
        for (k, p) in self.opts.launch.players.iter().enumerate() {
            let slot = if trials { 0 } else { p.slot };
            if trials && k > 0 {
                break;
            }
            self.drivers[slot] = match &p.owner {
                Some(Owner::Local(Dev::Pad(id))) if *id >= 10_000 => Driver::Bot,
                Some(Owner::Local(Dev::Pad(id))) => Driver::Pad(*id),
                Some(Owner::Local(Dev::Keyboard(k))) if self.opts.keyboard => {
                    Driver::Keyboard(*k as usize)
                }
                Some(Owner::Local(Dev::Keyboard(_))) => Driver::Bot,
                Some(Owner::Remote { .. }) => Driver::Idle,
                None => Driver::Bot,
            };
        }
    }

    fn humans(&self) -> bool {
        self.drivers
            .iter()
            .any(|d| matches!(d, Driver::Pad(_) | Driver::Keyboard(_)))
    }

    fn ensure_view(&mut self, gpu: &tfp_render::Gpu, r: &mut tfp_render::Renderer) -> Result<()> {
        let (Some(w), Some(g), Some(index), Some(c)) =
            (&self.world, &self.gfx, &self.index, &self.content)
        else {
            return Ok(());
        };
        if self.view_for == Some(w.level) {
            return Ok(());
        }
        let (path, theme) = match self.opts.launch.mode {
            CoopMode::Quest { .. } => {
                let q = index.quest.get(w.level).context("no quest tower")?;
                (q.level.clone(), q.theme.clone())
            }
            CoopMode::Trials { .. } => {
                let t = index.trials.get(w.level).context("no trial")?;
                (t.path.clone(), t.theme.clone())
            }
            CoopMode::DarkWorld { tower, .. } => (
                self.dw_paths
                    .get(w.level)
                    .cloned()
                    .context("no Dark World arena")?,
                self.dw_themes.get(tower).cloned().unwrap_or_default(),
            ),
        };
        let lvl = tfp_assets::level::Level::load(&path)?;
        let th =
            g.la.game_data
                .theme(&theme)
                .with_context(|| format!("unknown theme {theme}"))?;
        let look = tfp_assets::look::LevelLook::build(
            &g.la,
            &lvl,
            th,
            c.level(w.level).load_seed,
            &tfp_assets::look::LookOptions::default(),
        )?;
        if let Some(old) = self.view.take() {
            old.destroy(r);
        }
        self.size = (look.width, look.height);
        self.view = Some(TowerView::new(gpu, r, g, look)?);
        self.view_for = Some(w.level);
        Ok(())
    }

    /// Entry `index` of an array effect (`sfx_enemyKill[n]`).
    fn play_sound_indexed(
        &mut self,
        audio: &mut Audio,
        name: &str,
        index: usize,
        x: f32,
        volume: f32,
    ) {
        let Some(sfx) = &mut self.sfx else { return };
        let v = sfx.get(name);
        let Some(s) = v.get(index.min(v.len().saturating_sub(1))).cloned() else {
            return;
        };
        let pan = if x == 0.0 {
            0.0
        } else {
            (x / self.size.0 as f32).clamp(0.0, 1.0) - 0.5
        };
        self.sfxp
            .play(audio, name, &[s], pan, volume * self.opts.sfx_volume);
    }

    fn play_sound(&mut self, audio: &mut Audio, name: &str, x: f32, volume: f32) {
        let Some(sfx) = &mut self.sfx else { return };
        let v = sfx.get(name).to_vec();
        let pan = if x == 0.0 {
            0.0
        } else {
            (x / self.size.0 as f32).clamp(0.0, 1.0) - 0.5
        };
        self.sfxp
            .play(audio, name, &v, pan, volume * self.opts.sfx_volume);
    }

    /// The pause menu pauses the instanced / looped effects and halves the
    /// music (audio.md 3, 2); its end undoes both.
    fn sync_pause(&mut self, audio: &mut Audio) {
        let paused = self.menu.as_ref().is_some_and(|m| m.paused);
        if paused == self.sfx_paused {
            return;
        }
        self.sfx_paused = paused;
        self.sfxp.set_paused(audio, paused);
        if let Some(m) = &self.music {
            audio.set_music_volume(m.volume * if paused { 0.5 } else { 1.0 });
        }
    }

    /// Music per phase (audio.md 6): the tower theme while playing, `Boss`
    /// while the King Reaper is around, `GameOver`, the victory jingle on
    /// a clear (team theme for 2+), silence on Trials results and dev times.
    fn update_music(&mut self, audio: &mut Audio) {
        let Some(w) = &self.world else { return };
        let theme = self.theme_cue.clone();
        let want: Option<String> =
            if let (Some(d), Some(c)) = (w.dark_world.as_deref(), &self.content) {
                dw_music(w, c, d, theme, self.dw_victory_cue(d))
            } else if let Some(q) = w.quest.as_deref() {
                let card_up = self.qhud.as_ref().is_some_and(|h| h.card_up());
                match q.phase {
                    QuestPhase::Intro if card_up => None,
                    QuestPhase::Intro | QuestPhase::Playing | QuestPhase::Ending => {
                        let reaper = w.tagged(tfp_sim::Tag::Enemy).iter().any(|&e| {
                            matches!(w.kind(e), Some(tfp_sim::entity::Kind::Enemy(en))
                            if en.ty == tfp_sim::enemies::EnemyType::KingReaper)
                        });
                        Some(if reaper { "Boss".into() } else { theme })
                    }
                    QuestPhase::GameOver if q.phase_t >= 211.0 => Some("GameOver".into()),
                    QuestPhase::GameOver => Some(String::new()),
                    QuestPhase::Complete | QuestPhase::Finished
                        if self.qhud.as_ref().and_then(|h| h.won) == Some(true) =>
                    {
                        if q.phase_t < 50.0 && q.phase == QuestPhase::Complete {
                            Some(String::new())
                        } else {
                            Some(self.victory_cue(q))
                        }
                    }
                    _ => None,
                }
            } else if let Some(t) = w.trials.as_deref() {
                match t.phase {
                    TrialsPhase::Playing => Some(theme),
                    TrialsPhase::Won
                        if t.result
                            .is_some_and(|r| r.time <= tfp_sim::trials::ticks(t.settings.dev)) =>
                    {
                        Some(String::new())
                    }
                    TrialsPhase::Results | TrialsPhase::Finished => Some(String::new()),
                    _ => None,
                }
            } else {
                None
            };
        let (Some(want), Some(m)) = (want, &mut self.music) else {
            return;
        };
        if want == self.music_req {
            return;
        }
        self.music_req = want.clone();
        if want.is_empty() {
            m.stop(audio);
        } else if want.starts_with("Victory") || want == "GameOver" {
            m.play_immediate(audio, &want);
        } else {
            m.play(audio, &want);
        }
    }

    /// The clear's jingle: the team theme for 2+, else the archer's own.
    fn dw_victory_cue(&self, d: &tfp_sim::darkworld::DarkWorld) -> String {
        if d.n_players() > 1 {
            return "VictoryTeam".into();
        }
        let archer = self.opts.launch.players.first().map_or(0, |p| p.archer);
        self.ua
            .as_ref()
            .and_then(|a| a.archers.get(archer))
            .map_or("VictoryTeam".into(), |a| format!("Victory{}", a.victory))
    }

    /// The alt-shoot button icon of the first human (trigger slide).
    fn alt_shoot_icon(&self) -> Option<String> {
        self.opts
            .launch
            .players
            .iter()
            .find_map(|p| match &p.owner {
                Some(Owner::Local(Dev::Pad(id))) if *id < 10_000 => Some("controls/xb360/b".into()),
                Some(Owner::Local(Dev::Keyboard(k))) => {
                    let l = self.keys.layouts.get(*k as usize)?;
                    let key = *l.alt_shoot.first()?;
                    crate::front::minput::key_icon(key).map(|n| format!("controls/keyboard/{n}"))
                }
                _ => None,
            })
    }

    fn victory_cue(&self, q: &tfp_sim::quest::Quest) -> String {
        if q.active_slots().len() > 1 {
            return "VictoryTeam".into();
        }
        let archer = self.opts.launch.players.first().map_or(0, |p| p.archer);
        self.ua
            .as_ref()
            .and_then(|a| a.archers.get(archer))
            .map_or("VictoryTeam".into(), |a| format!("Victory{}", a.victory))
    }

    fn play_events(&mut self, audio: &mut Audio) {
        let Some(w) = &self.world else { return };
        let frame = w.frame;
        let rate = w.time.time_rate;
        self.sfxp.set_time_rate(audio, rate);
        // A new level: the old one's loops stop.
        if self.loops_level != Some(w.level) {
            self.loops_level = Some(w.level);
            self.loops.stop_all(&mut self.sfxp, audio);
        }
        let width = self.size.0 as f32;
        let vol = self.opts.sfx_volume;
        let events: Vec<Event> = w.events().to_vec();
        for (i, e) in events.iter().enumerate() {
            if !self.seen.insert((frame, i)) {
                continue;
            }
            match *e {
                Event::Sound { name, x } => self.play_sound(audio, name, x, 1.0),
                Event::SoundIndexed { name, index, x } => {
                    self.play_sound_indexed(audio, name, index as usize, x, 1.0)
                }
                Event::Env(tfp_sim::entities::EnvEvent::Sound { name, x, volume }) => {
                    self.play_sound(audio, name, x, volume)
                }
                Event::CharSound {
                    sound, archer, x, ..
                } => {
                    let set = self
                        .voices
                        .get(archer as usize)
                        .copied()
                        .unwrap_or(0)
                        .min(13);
                    // Alternate voice sets fall back to their base set.
                    for s in [set, VOICE_FALLBACK[set]] {
                        let name = format!("{}_{}", VOICES[s], char_sound(sound));
                        if self.sfx.as_mut().is_some_and(|b| !b.get(&name).is_empty()) {
                            self.play_sound(audio, &name, x, 1.0);
                            break;
                        }
                    }
                }
                Event::Loop {
                    name,
                    player,
                    on,
                    x,
                } => {
                    // Archer loops from the voice set, else the plain name.
                    let set = self
                        .world
                        .as_ref()
                        .and_then(|w| w.player_by_index(player).and_then(|id| w.player(id)))
                        .and_then(|p| self.voices.get(p.archer as usize).copied())
                        .map(|s| s.min(13));
                    let sfx = &mut self.sfx;
                    self.loops.player(
                        &mut self.sfxp,
                        audio,
                        (player, name),
                        on,
                        crate::sfx::pan_at(x, width),
                        vol,
                        || {
                            let mut names: Vec<String> = Vec::new();
                            if let Some(set) = set {
                                for v in [set, VOICE_FALLBACK[set]] {
                                    names.push(format!("{}_{name}", VOICES[v]));
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
                }) => {
                    let sfx = &mut self.sfx;
                    self.loops.entity(
                        &mut self.sfxp,
                        audio,
                        (owner, name),
                        on,
                        0.0,
                        volume * vol,
                        || sfx.as_mut().and_then(|b| b.get(name).first().cloned()),
                    );
                }
                Event::EntityLoop {
                    name,
                    entity,
                    on,
                    x,
                } => {
                    let sfx = &mut self.sfx;
                    self.loops.entity(
                        &mut self.sfxp,
                        audio,
                        (entity, name),
                        on,
                        crate::sfx::pan_at(x, width),
                        vol,
                        || sfx.as_mut().and_then(|b| b.get(name).first().cloned()),
                    );
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
                _ => {}
            }
        }
        let cut = frame.saturating_sub(16);
        self.seen.retain(|(f, _)| *f >= cut);
        if let Some(w) = &self.world {
            self.loops.follow(&mut self.sfxp, audio, w, width);
        }
    }

    /// The end menu once the sim's result screen is ready.
    fn end_menu(&self) -> Option<Vec<Action>> {
        let w = self.world.as_ref()?;
        if let Some(d) = w.dark_world.as_deref() {
            // ui.md 10.4: game over [CONTINUE (n)], RESTART, MAP, QUIT;
            // complete (after its sequence and a Confirm) MAP, RESTART, QUIT.
            return match d.phase {
                DwPhase::GameOver if d.menu_ready => {
                    let mut v = vec![Action::Restart, Action::Map, Action::Quit];
                    if d.can_continue() {
                        v.insert(0, Action::Continue);
                    }
                    Some(v)
                }
                DwPhase::Complete
                    if DwHud::complete_done(d) && (!self.humans() || self.mi.p(b::CONFIRM)) =>
                {
                    Some(vec![Action::Map, Action::Restart, Action::Quit])
                }
                _ => None,
            };
        }
        if let Some(q) = w.quest.as_deref() {
            return match q.phase {
                QuestPhase::GameOver if q.ready => {
                    Some(vec![Action::Restart, Action::Map, Action::Quit])
                }
                QuestPhase::Complete if q.ready => {
                    Some(vec![Action::Map, Action::Restart, Action::Quit])
                }
                QuestPhase::Finished => {
                    let won = self.qhud.as_ref().and_then(|h| h.won).unwrap_or(false);
                    Some(if won {
                        vec![Action::Map, Action::Restart, Action::Quit]
                    } else {
                        vec![Action::Restart, Action::Map, Action::Quit]
                    })
                }
                _ => None,
            };
        }
        let t = w.trials.as_deref()?;
        if !(matches!(t.phase, TrialsPhase::Results) && t.ready || t.phase == TrialsPhase::Finished)
        {
            return None;
        }
        let next = self.next_trial().is_some();
        let won = t.result.is_some_and(|r| r.time > 0);
        let mut v = if won {
            vec![
                Action::NextTrial,
                Action::Restart,
                Action::Map,
                Action::Quit,
            ]
        } else {
            vec![
                Action::Restart,
                Action::NextTrial,
                Action::Map,
                Action::Quit,
            ]
        };
        if !next {
            v.retain(|a| *a != Action::NextTrial);
        }
        Some(v)
    }

    /// The next tier of the same trials tower.
    fn next_trial(&self) -> Option<usize> {
        let CoopMode::Trials { level } = self.opts.launch.mode else {
            return None;
        };
        let idx = self.index.as_ref()?;
        let cur = idx.trials.get(level)?;
        idx.trials
            .iter()
            .position(|t| t.index == cur.index && t.tier == cur.tier + 1)
    }

    fn act(&mut self, a: Action) {
        // A Dark World session that ended in a game over is recorded when
        // it is left (CONTINUE keeps it going).
        if matches!(a, Action::Restart | Action::Map | Action::Quit)
            && self
                .dark_world()
                .is_some_and(|d| d.phase == DwPhase::GameOver)
        {
            self.dw_save();
        }
        match a {
            Action::Resume => self.menu = None,
            Action::Restart => self.build(false),
            Action::NextTrial => {
                if let Some(n) = self.next_trial() {
                    self.opts.launch.mode = CoopMode::Trials { level: n };
                    self.build(true);
                }
            }
            Action::Continue | Action::RestartLevel => {
                if let (Some(w), Some(c)) = (&mut self.world, &self.content)
                    && tfp_sim::darkworld::continue_level(w, c)
                {
                    self.menu = None;
                    self.debris.clear();
                    self.anims.clear();
                    self.bfx.clear();
                    self.recorded = false;
                    self.auto_t = 0;
                    self.music_req.clear();
                }
            }
            Action::Map => self.exit = Some(Exit::Map),
            Action::Quit => self.exit = Some(Exit::Quit),
        }
    }

    /// The exit-portal zoom of the Dark World transition: (scale, portal).
    fn dw_zoom(&self) -> Option<(f32, Vec2)> {
        let w = self.world.as_ref()?;
        let d = w.dark_world.as_deref()?;
        if d.phase != DwPhase::Transition || d.phase_t < 31.0 {
            return None;
        }
        let at = d
            .exit_portal
            .filter(|&e| w.exists(e))
            .map(|e| w.core(e).pos)?;
        let x = ((d.phase_t - 31.0) / 40.0).clamp(0.0, 1.0);
        let e = if x < 0.5 {
            4.0 * x * x * x
        } else {
            let u = 2.0 * x - 2.0;
            0.5 * u * u * u + 1.0
        };
        Some((1.0 + 2.0 * e, Vec2::new(at.x.floor(), at.y.floor())))
    }

    /// The controls screen's columns: one per distinct joined device
    /// (a keyboard column when only bots play).
    fn control_columns(
        &self,
        input: &crate::input::RawInput,
    ) -> Vec<super::intro_controls::Column> {
        use super::intro_controls::Column;
        let mut cols = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for (n, p) in self.opts.launch.players.iter().enumerate() {
            let (key, col) = match &p.owner {
                Some(Owner::Local(Dev::Pad(id))) if *id < 10_000 => {
                    let name = input
                        .pads
                        .iter()
                        .find(|x| x.id == *id)
                        .map_or(String::new(), |x| x.name.clone());
                    let set = crate::front::minput::classify_pad(&name).1;
                    (format!("pad{id}"), Column::pad(set, n))
                }
                Some(Owner::Local(Dev::Keyboard(k))) => {
                    let l = self
                        .keys
                        .layouts
                        .get(*k as usize)
                        .cloned()
                        .unwrap_or_else(KeyLayout::default_config);
                    (format!("key{k}"), Column::keyboard(&l))
                }
                _ => continue,
            };
            if !seen.contains(&key) {
                seen.push(key);
                cols.push(col);
            }
        }
        if cols.is_empty() {
            cols.push(Column::keyboard(&KeyLayout::default_config()));
        }
        cols
    }

    /// Records this session's result in the save store (once).
    fn dw_save(&mut self) {
        if self.dw_saved {
            return;
        }
        if let Some(r) = self.dark_world().and_then(|d| d.result) {
            self.dw_saved = true;
            crate::save::record_dark_world(&r);
        }
    }

    /// Continues remaining (menu labels; -1 unlimited).
    fn dw_continues(&self) -> i32 {
        self.dark_world().map_or(-1, |d| d.continues_remaining())
    }

    /// Session records from a finished attempt (once).
    fn record(&mut self) {
        // Dark World: total deaths follow the session; a clear writes the
        // tower's flags, eyes, Legendary time and curses, and the crowns.
        if let Some(d) = self.world.as_ref().and_then(|w| w.dark_world.as_deref()) {
            let tower = d.settings.tower;
            let deaths: u32 = d.deaths.iter().sum();
            let base = self.dw_deaths_base;
            let rec = self.records.dark_world.entry(tower).or_default();
            rec.deaths = rec.deaths.max(base + deaths);
            if !self.recorded
                && d.phase == DwPhase::Complete
                && let Some(r) = d.result
            {
                self.recorded = true;
                rec.complete(&r);
                if !self.dw_saved {
                    self.dw_saved = true;
                    crate::save::record_dark_world(&r);
                }
                if r.players > 1 {
                    self.records.crowns = r.crowns;
                }
            }
            return;
        }
        // A trials gem is kept at once (trials.md 9), whatever the attempt.
        if let Some(t) = self.world.as_ref().and_then(|w| w.trials.as_deref())
            && t.gem_collected
        {
            let level = t.settings.level;
            let rec = self.records.trials.entry(level).or_default();
            if !rec.gem {
                rec.gem = true;
                crate::save::with(|d| d.trials.entry(level).or_default().gem = true);
            }
        }
        if self.recorded {
            return;
        }
        let Some(w) = &self.world else { return };
        if let Some(q) = w.quest.as_deref()
            && q.phase == QuestPhase::Complete
        {
            self.recorded = true;
            let slots = q.active_slots();
            let lives: i32 = slots.iter().map(|&s| q.lives[s].max(0)).sum();
            let no_deaths = lives >= if slots.len() <= 1 { 5 } else { 8 };
            let rec = self
                .records
                .quest
                .entry(q.settings.tower_id as usize)
                .or_default();
            rec.normal = true;
            if q.settings.hardcore {
                rec.hardcore = true;
                rec.no_deaths |= no_deaths;
            }
            let k = usize::from(slots.len() > 1);
            if rec.best[k] == 0 || q.time < rec.best[k] {
                rec.best[k] = q.time;
            }
            let crowned = quest_hud::crowns(q);
            for (i, c) in self.records.crowns.iter_mut().enumerate() {
                *c = crowned.contains(&i);
            }
        }
        if let Some(t) = w.trials.as_deref()
            && t.phase == TrialsPhase::Won
            && let Some(r) = t.result
        {
            self.recorded = true;
            let rec = self.records.trials.entry(t.settings.level).or_default();
            if r.new_best {
                rec.best = r.time;
            }
            for i in 0..3 {
                rec.unlocked[i] |= r.unlocked[i];
            }
        }
    }

    /// Headless proof captures.
    fn shot_due(&mut self) -> Option<String> {
        if self
            .menu
            .as_ref()
            .is_some_and(|m| !m.paused && m.opened == 30)
        {
            return Some("menu".into());
        }
        let w = self.world.as_ref()?;
        if let Some(d) = w.dark_world.as_deref() {
            let key = 20 + d.phase as u8;
            if key != self.last_phase {
                self.last_phase = key;
                self.phase_frames = 0;
            }
            self.phase_frames += 1;
            let pf = self.phase_frames;
            let pt = d.phase_t as u32;
            let lvl = d.round_index + 1;
            let boss = d.boss.as_ref().map(|b| b.id);
            // Boss states: one capture 90 ticks into each new state.
            let bp = tfp_sim::darkworld::boss_phase(w).map_or(-1, |(i, st)| (i * 16 + st) as i32);
            if bp != self.last_wave {
                self.last_wave = bp;
                self.dw_last_round = 0;
            }
            self.dw_last_round = self.dw_last_round.saturating_add(1);
            let tag = match d.phase {
                DwPhase::Title if pf == 110 => "title".to_string(),
                DwPhase::Tips if pf == 60 => "tips".to_string(),
                DwPhase::Playing if boss.is_none() && pt == 100 => format!("L{lvl}-card"),
                DwPhase::Playing if boss.is_none() && pt == 400 => format!("L{lvl}-play"),
                DwPhase::Playing if boss.is_some() && pt == 150 => format!("L{lvl}-boss-title"),
                // The first frame from 90 ticks in without a hit flash.
                DwPhase::Playing | DwPhase::Exit
                    if bp >= 0
                        && (90..1000).contains(&self.dw_last_round)
                        && pt > 200
                        && (self.dw_last_round >= 140
                            || !(d.boss.as_ref().is_some_and(|b| b.base.flash > 0.0)
                                || self.bfx.flashing())) =>
                {
                    self.dw_last_round = 1000;
                    format!("L{lvl}-boss{}-state{}", bp / 16, bp % 16)
                }
                // Late in a boss death: Amaranth's dust (270 ticks in), the
                // Dreadwood leaves and flowers (560), Darkfang (200). The
                // state capture above set the counter to 1000 at ~90.
                DwPhase::Playing | DwPhase::Exit
                    if matches!(bp, 2 | 19 | 35)
                        && self.dw_last_round
                            == 910
                                + match bp {
                                    2 => 270,
                                    19 => 560,
                                    _ => 200,
                                } =>
                {
                    format!("L{lvl}-boss{}-death", bp / 16)
                }
                DwPhase::Playing | DwPhase::Exit
                    if self.bfx.heart_since().is_some_and(|t| t >= 150)
                        && !self.bfx.flashing()
                        && !self.shot_tags.iter().any(|t| t.ends_with("heart")) =>
                {
                    format!("L{lvl}-boss3-heart")
                }
                DwPhase::Playing if boss.is_some() && pt > 0 && pt.is_multiple_of(900) => {
                    format!("L{lvl}-boss-fight")
                }
                DwPhase::Exit if pf == 100 => format!("L{lvl}-exit"),
                DwPhase::Transition if pf == 66 => format!("L{lvl}-zoom"),
                DwPhase::GameOver if pt == 400 => "gameover".into(),
                DwPhase::Complete if pt == 300 => "complete".into(),
                DwPhase::Complete
                    if d.result.is_some_and(|r| {
                        pt == super::darkworld::hud::complete_times(&r, d.n_players()).4 as u32
                    }) =>
                {
                    "complete-cards".into()
                }
                _ => return None,
            };
            if tag.ends_with("fight") && self.shot_tags.iter().filter(|t| **t == tag).count() >= 3 {
                return None;
            }
            self.shot_tags.push(tag.clone());
            return Some(tag);
        }
        let (phase, pt) = if let Some(q) = w.quest.as_deref() {
            (q.phase as u8, q.phase_t as u32)
        } else {
            let t = w.trials.as_deref()?;
            (10 + t.phase as u8, t.phase_t as u32)
        };
        if phase != self.last_phase {
            self.last_phase = phase;
            self.phase_frames = 0;
        }
        self.phase_frames += 1;
        let pf = self.phase_frames;
        let tag = if let Some(q) = w.quest.as_deref() {
            match q.phase {
                QuestPhase::Intro if pf == 110 => "card".to_string(),
                QuestPhase::Playing if q.current_wave != self.last_wave && q.current_wave >= 0 => {
                    self.last_wave = q.current_wave;
                    self.phase_frames = 0;
                    return None;
                }
                QuestPhase::Playing if pf == 70 => format!("wave{}", q.current_wave + 1),
                QuestPhase::Playing if pf > 0 && pf.is_multiple_of(500) => {
                    format!("w{}-play", q.current_wave + 1)
                }
                QuestPhase::GameOver if pt == 420 => "gameover".into(),
                QuestPhase::Complete if pt == 300 => "complete".into(),
                QuestPhase::Complete if pt == 640 => "complete-tally".into(),
                _ => return None,
            }
        } else {
            let t = w.trials.as_deref()?;
            match t.phase {
                TrialsPhase::Intro if pf == 100 => "card".into(),
                TrialsPhase::Playing if pf == 40 || pf.is_multiple_of(150) => "play".into(),
                TrialsPhase::Won if pf == 30 => "won".into(),
                TrialsPhase::Results if pf == 250 => "results".into(),
                TrialsPhase::Failed if pf == 60 => "failed".into(),
                _ => return None,
            }
        };
        if self.shot_tags.iter().filter(|t| **t == tag).count() >= 1 && tag.ends_with("play") {
            return None;
        }
        self.shot_tags.push(tag.clone());
        Some(tag)
    }
}

impl Scene for CoopPlay {
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
        if !ctx.input.connected.is_empty() || !ctx.input.disconnected.is_empty() {
            self.assign();
        }
        self.mi.update(ctx.input, &self.keys);
        self.sync_pause(ctx.audio);
        if let Some(v) = &mut self.view {
            v.update();
        }
        // Quest tower 0 (Normal): the controls screen holds the quest.
        if std::mem::take(&mut self.controls_pending) {
            self.controls = Some(super::intro_controls::IntroControls::new(
                self.control_columns(ctx.input),
            ));
        }
        let humans_now = self.humans();
        if let Some(ic) = &mut self.controls {
            let confirm = if humans_now {
                self.mi.p(b::CONFIRM)
            } else {
                self.frame % 150 == 149
            };
            let sound = ic.tick(confirm);
            let done = ic.finished();
            if ic.ticks() == 60
                && let Some(dir) = &self.opts.shots
            {
                self.shot_n += 1;
                self.capture = Some(dir.join(format!("{:02}-controls.png", self.shot_n)));
            }
            if let Some(name) = sound {
                self.play_sound(ctx.audio, name, 0.0, 0.8);
            }
            if done {
                self.controls = None;
            } else {
                self.frame += 1;
                return;
            }
        }
        // Menus (pause / end) take the menu input.
        // The trigger-arrow slide is the sim's `Tips` phase (darkworld.md
        // 7.6: it comes before the title card); the HUD follows it.
        if let Some(t) = &mut self.tutorial
            && let Some(d) = self.world.as_ref().and_then(|w| w.dark_world.as_deref())
        {
            let sound = t.sync(d);
            let done = t.finished();
            if let Some(name) = sound {
                self.play_sound(ctx.audio, name, 0.0, 0.8);
            }
            if done {
                self.tutorial = None;
                TRIGGER_SLIDE_SHOWN.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let cont = self.dw_continues();
        if self.menu.is_none()
            && let Some(items) = self.end_menu()
        {
            self.menu = Some(Menu {
                items,
                sel: 0,
                opened: 0,
                paused: false,
                cont,
            });
        }
        let playing = self.world.as_ref().is_some_and(|w| {
            w.quest
                .as_deref()
                .is_some_and(|q| q.phase == QuestPhase::Playing)
                || w.trials
                    .as_deref()
                    .is_some_and(|t| t.phase == TrialsPhase::Playing)
                || w.dark_world
                    .as_deref()
                    .is_some_and(|d| matches!(d.phase, DwPhase::Playing | DwPhase::Exit))
        });
        if self.menu.is_none() && playing && self.humans() && self.mi.p(b::START) {
            let can_continue = self.dark_world().is_some_and(|d| d.can_continue());
            self.menu = Some(Menu {
                items: match self.opts.launch.mode {
                    CoopMode::Quest { .. } => {
                        vec![Action::Resume, Action::Restart, Action::Map, Action::Quit]
                    }
                    CoopMode::Trials { .. } => {
                        vec![Action::Restart, Action::Resume, Action::Map, Action::Quit]
                    }
                    CoopMode::DarkWorld { .. } => {
                        let mut v =
                            vec![Action::Resume, Action::Restart, Action::Map, Action::Quit];
                        if can_continue {
                            v.insert(1, Action::RestartLevel);
                        }
                        v
                    }
                },
                sel: 0,
                opened: 0,
                paused: true,
                cont,
            });
            self.play_sound(ctx.audio, "UI_PAUSE_ST", 0.0, 1.0);
            return;
        }
        let humans = self.humans();
        if let Some(m) = &mut self.menu {
            m.opened += 1;
            let mut chosen = None;
            if humans {
                if self.mi.p(b::DOWN) && m.sel + 1 < m.items.len() {
                    m.sel += 1;
                }
                if self.mi.p(b::UP) && m.sel > 0 {
                    m.sel -= 1;
                }
                if m.paused && (self.mi.p(b::START) || self.mi.p(b::BACK)) {
                    chosen = Some(Action::Resume);
                } else if self.mi.p(b::CONFIRM) && m.opened > 10 {
                    chosen = Some(m.items[m.sel]);
                }
            } else {
                // Headless / bots only: leave by itself.
                self.auto_t += 1;
                if self.auto_t > 120 {
                    chosen = Some(Action::Map);
                }
            }
            let paused = m.paused;
            if let Some(a) = chosen {
                self.act(a);
                return;
            }
            if paused {
                return;
            }
        }
        let (Some(w), Some(c)) = (&mut self.world, &self.content) else {
            return;
        };
        let frame = self.frame;
        let mut inputs = vec![PlayerInput::default(); MAXP];
        // Bots play only while the game runs (no confirms on cards and
        // result screens).
        let live = w
            .quest
            .as_deref()
            .is_some_and(|q| matches!(q.phase, QuestPhase::Playing | QuestPhase::Ending))
            || w.trials
                .as_deref()
                .is_some_and(|t| t.phase == TrialsPhase::Playing)
            || w.dark_world
                .as_deref()
                .is_some_and(|d| matches!(d.phase, DwPhase::Playing | DwPhase::Exit));
        for (i, d) in self.drivers.iter().enumerate() {
            inputs[i] = match d {
                Driver::Pad(id) => ctx
                    .input
                    .pads
                    .iter()
                    .find(|p| p.id == *id)
                    .map(|p| match self.opts.pads.get(&p.name) {
                        Some(m) => crate::play::pad_input_with(p, m),
                        None => crate::play::pad_input(p),
                    })
                    .unwrap_or_default(),
                Driver::Keyboard(k) => self
                    .keys
                    .layouts
                    .get(*k)
                    .map(|l| crate::play::key_input(l, ctx.input))
                    .unwrap_or_else(|| {
                        crate::play::key_input(&KeyLayout::default_config(), ctx.input)
                    }),
                // The shared bot fights and walks into the exit portal.
                Driver::Bot if live => self.bots[i].input(w, c, i as u8),
                Driver::Bot => PlayerInput::default(),
                Driver::Idle => PlayerInput::default(),
            };
            // Start opens our pause menu; it never reaches the sim.
            inputs[i].buttons &= !PlayerInput::START;
        }
        // No humans: confirm the title card by ourselves.
        let humans = self
            .drivers
            .iter()
            .any(|d| matches!(d, Driver::Pad(_) | Driver::Keyboard(_)));
        if !humans {
            let card = w
                .quest
                .as_deref()
                .is_some_and(|q| q.phase == QuestPhase::Intro && q.phase_t > 150.0)
                || w.trials
                    .as_deref()
                    .is_some_and(|t| t.phase == TrialsPhase::Intro && t.phase_t > 140.0)
                || w.dark_world.as_deref().is_some_and(|d| {
                    (d.phase == DwPhase::Title && d.phase_t > 150.0)
                        || (d.phase == DwPhase::Tips && d.phase_t > 120.0)
                });
            if card && let Some(i) = self.drivers.iter().position(|d| *d == Driver::Bot) {
                inputs[i].buttons |= if frame.is_multiple_of(2) {
                    PlayerInput::JUMP
                } else {
                    0
                };
            }
        }
        if let Some(n) = self.dw_proof.assist {
            super::darkworld::assist(w, c, n, frame);
        }
        w.step(c, &inputs);
        if let Some((complete, at)) = self.opts.end
            && w.frame >= at
        {
            if w.dark_world.is_some() {
                super::darkworld::force_end(w, c, complete);
            } else {
                force_end(w, c, complete);
            }
        }
        self.frame += 1;
        self.shake = self.shake.saturating_sub(1);
        self.wave_counter += 0.01;
        // King's Court: the spotlight is on the King from its transformation.
        if super::art::king_transform(w).is_none() {
            self.king_spot = None;
        } else if self.king_spot.is_none() {
            self.king_spot = Some(self.frame);
        }
        // The King Reaper's intro from the King: remember when it appeared.
        if self.reaper_intro.is_none_or(|(id, _)| !w.exists(id)) {
            use tfp_sim::enemies::{EnemyType, reaper};
            self.reaper_intro = w
                .tagged(tfp_sim::Tag::Enemy)
                .iter()
                .find(|&&e| {
                    matches!(w.kind(e), Some(tfp_sim::entity::Kind::Enemy(en))
                        if en.ty == EnemyType::KingReaper
                            && en.has(reaper::F_INTRO)
                            && en.state == reaper::APPEAR)
                })
                .map(|&e| (e, self.frame));
        }
        let mut bursts = Vec::new();
        self.debris.after_step(w, c, &mut bursts);
        let drops = self.debris.take_sounds();
        bursts.extend(self.bfx.after_step(w, c));
        if let Some(ui) = &mut self.ui {
            ui.after_step(w, None);
            ui.post_step(w, c);
            for (name, n, pos) in bursts {
                if let Some(t) = ui.particles.by_name(name) {
                    ui.particles
                        .emit(t, n, Vec2::new(pos.x, pos.y), Vec2::splat(4.0), None);
                }
            }
        }
        let width = self.size.0 as f32;
        if let Some(h) = &mut self.qhud {
            h.after_step(w, width);
        }
        if let Some(h) = &mut self.thud {
            h.after_step(w);
        }
        if let Some(h) = &mut self.dhud {
            h.after_step(w);
        }
        self.record();
        for (name, x) in drops {
            self.play_sound(ctx.audio, name, x, 0.7);
        }
        self.play_events(ctx.audio);
        ctx.host.rumble.append(&mut self.rumble_out);
        self.update_music(ctx.audio);
        if self.opts.shots.is_some()
            && let Some(tag) = self.shot_due()
            && let Some(dir) = &self.opts.shots
        {
            self.shot_n += 1;
            self.capture = Some(dir.join(format!("{:02}-{tag}.png", self.shot_n)));
        }
        if self.frame.is_multiple_of(600) {
            if let Some(q) = self.quest() {
                println!(
                    "coop: frame {}, quest {:?} wave {}/{} kills {:?} lives {:?}",
                    self.frame,
                    q.phase,
                    q.current_wave + 1,
                    q.total_waves,
                    &q.kills[..2],
                    &q.lives[..2]
                );
                if let Some(w) = &self.world {
                    for id in w.players() {
                        if let Some(pl) = w.player(id) {
                            let p = w.core(id).pos;
                            log::info!(
                                "  player {} at ({:.0},{:.0}) arrows {} dead {} state {:?}",
                                pl.index,
                                p.x,
                                p.y,
                                pl.arrows.len(),
                                pl.dead,
                                pl.state()
                            );
                        }
                    }
                    for &a in w.tagged(tfp_sim::Tag::Arrow) {
                        if let Some(tfp_sim::entity::Kind::Arrow(ar)) = w.kind(a) {
                            let p = w.core(a).pos;
                            log::info!(
                                "  arrow {:?} at ({:.0},{:.0}) collectible {}",
                                ar.state,
                                p.x,
                                p.y,
                                ar.collectible()
                            );
                        }
                    }
                    for &e in w.tagged(tfp_sim::Tag::Enemy) {
                        if let Some(tfp_sim::entity::Kind::Enemy(en)) = w.kind(e) {
                            let p = w.core(e).pos;
                            log::info!(
                                "  enemy {:?} state {} at ({:.0},{:.0})",
                                en.ty,
                                en.state,
                                p.x,
                                p.y
                            );
                        }
                    }
                }
            } else if let Some(t) = self.trial() {
                println!(
                    "coop: frame {}, trial {:?} time {} dummies {}",
                    self.frame, t.phase, t.time, t.drawing
                );
            }
        }
    }

    fn draw(&mut self, ctx: &mut DrawContext<'_>) {
        if let Err(e) = self.ensure_view(ctx.gpu, ctx.renderer) {
            log::warn!("level view: {e:#}");
        }
        // The level canvas for camera transforms (`screen_fx`).
        let size = self.size;
        if self.zoom_canvas.is_none_or(|(_, s)| s != size) {
            if let Some((old, _)) = self.zoom_canvas.take() {
                ctx.renderer.destroy_texture(old);
            }
            self.zoom_canvas = Some((ctx.renderer.create_canvas(ctx.gpu, size), size));
        }
        // Dark World: the camera zooms 1 -> 3 about the exit portal while
        // the level ends (darkworld.md 5.5; 40 ticks CubeInOut from 31).
        let zoom = self.dw_zoom();
        let (Some(v), Some(g), Some(w), Some(c), Some(ui), Some(ua)) = (
            &mut self.view,
            &self.gfx,
            &self.world,
            &self.content,
            &self.ui,
            &self.ua,
        ) else {
            return;
        };
        // The shared match drawing (match_draw.rs) with the co-op art.
        let (art, slot_archers, anims, bfx, debris, dw_art) = (
            &self.art,
            &self.slot_archers,
            &mut self.anims,
            &mut self.bfx,
            &self.debris,
            &self.dw_art,
        );
        let tick = self.frame as u64;
        let d = crate::match_draw::LevelDraw {
            world: w,
            content: c,
            gfx: g,
            ui,
            ui_assets: ua,
            hitboxes: self.opts.hitboxes,
            level_t: tick,
            tick,
            size: self.size,
            hud: None,
        };
        let mut prep = crate::match_draw::prepare(v, &d, &mut |view| {
            let cx = art::Ctx {
                c,
                art,
                slot_archers,
                tick,
            };
            let mut extra = art::apply(view, w, &cx, anims);
            extra.extend(bfx.apply(view, w, c, dw_art));
            debris.draw(view, c, art);
            extra
        });
        // King's Court (quest.md 4.4): the spotlight is on the King from
        // its transformation, then on the King Reaper until 120 ticks after
        // it appears (`CancelSpotlight`); 20-tick fades.
        let spot = match (self.reaper_intro, self.king_spot) {
            (Some((id, f0)), _) if w.exists(id) && self.frame.saturating_sub(f0) < 120 => {
                let left = 120.0 - self.frame.saturating_sub(f0) as f32;
                Some((id, self.king_spot.unwrap_or(f0), left))
            }
            (None, Some(f0)) => super::art::king_transform(w)
                .filter(|(_, t)| {
                    t.is_none_or(|t| t < tfp_sim::entities::scenery::KING_TRANSFORM_REAPER)
                })
                .map(|(k, _)| (k, f0, 20.0)),
            _ => None,
        };
        if let Some((id, f0, left)) = spot {
            let at = w.core(id).pos;
            let t = self.frame.saturating_sub(f0) as f32;
            v.spotlight = true;
            v.alpha_add = (t / 20.0).min(1.0).min(left / 20.0);
            prep.lights = vec![(Vec2::new(at.x, at.y), 100.0)];
        }
        // Camera transforms of the level (`screen_fx`: the Dark World exit
        // zoom, the scroll waves' camera offset): the level is composited
        // into a canvas and drawn back transformed; the Gunn overlay and the
        // HUD stay on top, untransformed.
        let xform = super::screen_fx::LevelXform {
            zoom,
            offset: Vec2::new(prep.view.camera_offset.x, prep.view.camera_offset.y),
        };
        let level_canvas = self.zoom_canvas.map(|(t, _)| t).filter(|_| xform.active());
        v.screen_target = level_canvas;
        v.draw_full(
            ctx.list,
            g,
            tick,
            Some((&prep.view, c)),
            &prep.lights,
            &|dl, at| crate::match_draw::in_level_layer(dl, &d, &prep.view, at),
        );
        if let Some(t) = level_canvas {
            super::screen_fx::draw_level(ctx.list, t, self.size, &xform);
        }
        crate::match_draw::gunn_pass(ctx, &d, &prep.view, &mut self.gunn_canvas);
        ctx.present.wave = if self.screen_wave {
            super::screen_fx::slow_wave(w.time.time_rate, self.wave_counter)
        } else {
            None
        };
        let mut p = Painter {
            list: ctx.list,
            a: ua,
            w: self.size.0 as f32,
        };
        if let Some(h) = &self.qhud {
            h.draw(&mut p, w, c, &self.art, self.frame as u64);
        }
        if let Some(h) = &self.thud {
            h.draw(&mut p, w);
        }
        if let Some(h) = &self.dhud {
            h.draw(&mut p, w, c, self.bfx.screen_flash, self.frame as u64);
        }
        if let Some(ic) = &self.controls {
            ic.draw(&mut p);
        }
        if let Some(t) = &self.tutorial {
            p.rect(0.0, 0.0, p.w, 240.0, Color::BLACK * 0.5);
            t.draw(&mut p);
        }
        if let Some(m) = &self.menu {
            m.draw(&mut p, self.frame as u64);
        }
        let gshake = self.qhud.as_ref().is_some_and(|h| h.shake > 0);
        ctx.present.offset = if self.shake > 0 || ui.shaking() {
            let k = (self.frame as u64).wrapping_mul(2_654_435_761);
            let pick = |bit: u64| if (k >> bit) & 1 == 0 { -2.0 } else { 2.0 };
            Vec2::new(pick(5), pick(11))
        } else if gshake {
            let k = (self.frame as u64).wrapping_mul(2_654_435_761);
            Vec2::new(if (k >> 7) & 1 == 0 { -1.0 } else { 1.0 }, 0.0)
        } else {
            Vec2::ZERO
        };
    }
}

/// Proof aid: drive a running quest to its game over / completion.
fn force_end(w: &mut World, c: &Content, complete: bool) {
    use tfp_sim::quest::Op;
    let Some(q) = w.quest.as_deref() else { return };
    if q.phase != QuestPhase::Playing {
        return;
    }
    if complete {
        let none = tfp_sim::EntityId {
            index: u32::MAX,
            generation: 0,
        };
        tfp_sim::enemies::kill_all(w, c, none);
        tfp_sim::enemies::portal::force_cancel_all(w, c);
        if let Some(q) = w.quest.as_deref_mut()
            && let Some(i) = q.prog.iter().position(|o| *o == Op::LevelEnding)
            && q.pc < i
        {
            q.pc = i;
        }
    } else {
        if let Some(q) = w.quest.as_deref_mut() {
            q.lives = [0; MAXP];
        }
        let ids: Vec<_> = w.players().collect();
        for id in ids {
            tfp_sim::hurt::with_player(w, id, |w, p| {
                if !p.dead {
                    tfp_sim::player::die(w, c, id, p, tfp_sim::player::DeathCause::Enemy, -1);
                }
            });
        }
    }
}

/// `--play --mode quest|trials`: bots on a quest tower (id or part of
/// the theme name) or the trials level of tower `x`, tier `level`.
#[allow(clippy::too_many_arguments)]
pub fn cli_options(
    quest: bool,
    tower: Option<&str>,
    level: usize,
    hardcore: bool,
    players: usize,
    seed: Option<i32>,
    shots: Option<PathBuf>,
    hitboxes: bool,
) -> Result<CoopOptions> {
    let installs = tfp_assets::Installs::discover();
    let inst = installs
        .steam
        .context("no Steam install (set TF_GAME_DIR)")?;
    let index = tfp_assets::towers::TowerIndex::build(&inst)?;
    let norm = |s: &str| s.to_ascii_lowercase().replace([' ', '\'', '-'], "");
    let q = tower.unwrap_or("0");
    let mode = if quest {
        let id = match q.parse::<usize>() {
            Ok(i) => i,
            Err(_) => index
                .quest
                .iter()
                .position(|t| norm(&t.theme).contains(&norm(q)))
                .with_context(|| format!("no quest tower {q:?}"))?,
        };
        if id >= index.quest.len() {
            bail!("no quest tower #{id}");
        }
        CoopMode::Quest {
            tower: id,
            hardcore,
        }
    } else {
        let x = match q.parse::<usize>() {
            Ok(i) => i,
            Err(_) => index
                .trials
                .iter()
                .find(|t| norm(&t.theme).contains(&norm(q)))
                .map(|t| t.index)
                .with_context(|| format!("no trials tower {q:?}"))?,
        };
        let i = index
            .trials
            .iter()
            .position(|t| t.index == x && t.tier == level.min(2))
            .with_context(|| format!("no trial {x}-{level}"))?;
        CoopMode::Trials { level: i }
    };
    let n = if quest { players.clamp(1, 4) } else { 1 };
    Ok(CoopOptions {
        keys: KeyboardSetup::default(),
        rumble: false,
        launch: CoopLaunch {
            mode,
            players: (0..n)
                .map(|i| super::CoopPlayer {
                    slot: i,
                    archer: i,
                    alt: false,
                    owner: None,
                })
                .collect(),
        },
        records: Records::default(),
        keyboard: false,
        shots,
        start_screen: true,
        seed: seed.unwrap_or(0x5EED),
        hitboxes,
        end: None,
        music: true,
        sfx_volume: 1.0,
        pads: BTreeMap::new(),
    })
}

/// `--play --mode darkworld`: bots on Dark World tower `tower` (id 0-4 or
/// part of the theme name) at `difficulty` (0-2) with curses / power-ups.
#[allow(clippy::too_many_arguments)]
pub fn cli_options_dw(
    tower: Option<&str>,
    difficulty: u8,
    players: usize,
    seed: Option<i32>,
    shots: Option<PathBuf>,
    hitboxes: bool,
    set: tfp_sim::variants::VariantSet,
) -> Result<CoopOptions> {
    let installs = tfp_assets::Installs::discover();
    let inst = installs
        .steam
        .context("no Steam install (set TF_GAME_DIR)")?;
    if inst.dark_world.is_none() {
        bail!("Dark World needs the expansion content (DarkWorldContent)");
    }
    let index = tfp_assets::towers::TowerIndex::build(&inst)?;
    let norm = |s: &str| s.to_ascii_lowercase().replace([' ', '\'', '-'], "");
    let q = tower.unwrap_or("0");
    let id = match q.parse::<usize>() {
        Ok(i) => i,
        Err(_) => index
            .dark_world
            .iter()
            .position(|t| norm(&t.theme).contains(&norm(q)) || norm(&t.dir_name).contains(&norm(q)))
            .with_context(|| format!("no Dark World tower {q:?}"))?,
    };
    if id >= index.dark_world.len() {
        bail!("no Dark World tower #{id}");
    }
    let n = players.clamp(1, 4);
    Ok(CoopOptions {
        keys: KeyboardSetup::default(),
        rumble: false,
        launch: CoopLaunch {
            mode: CoopMode::DarkWorld {
                tower: id,
                difficulty: difficulty.min(2),
                variants: set,
            },
            players: (0..n)
                .map(|i| super::CoopPlayer {
                    slot: i,
                    archer: i,
                    alt: false,
                    owner: None,
                })
                .collect(),
        },
        records: Records::default(),
        keyboard: false,
        shots,
        start_screen: true,
        seed: seed.unwrap_or(0x5EED),
        hitboxes,
        end: None,
        music: true,
        sfx_volume: 1.0,
        pads: BTreeMap::new(),
    })
}

/// Dark World music (darkworld.md 5.1-5.5, 9; audio.md 6.2): the tower
/// theme from the first wait of a normal level (also under the title
/// card), `DarkBoss` once a boss level's card is over until the boss dies,
/// silence before the last level and after a boss, `GameOver` when no
/// continue is left, the victory jingle 50 ticks into the completion.
fn dw_music(
    w: &World,
    c: &Content,
    d: &tfp_sim::darkworld::DarkWorld,
    theme: String,
    victory: String,
) -> Option<String> {
    use tfp_sim::darkworld::boss::BossKind;
    use tfp_sim::darkworld::{Op, amaranth, cyclops, dreadwood};
    let boss_level = d.level(c).boss.is_some();
    let boss_dead = d.boss.as_ref().is_some_and(|b| match &b.kind {
        BossKind::Amaranth(s) => s.state == amaranth::DEAD,
        BossKind::Dreadwood(s) => s.phase == dreadwood::DEAD,
        BossKind::Cyclops(s) => s.state == cyclops::DEAD,
        BossKind::Cataclysm(s) => s.heart,
    }) || (boss_level && d.boss_finished);
    let _ = w;
    match d.phase {
        // No music change while the trigger tips slide shows.
        DwPhase::Tips => None,
        DwPhase::Title | DwPhase::Intro if boss_level => None,
        DwPhase::Title | DwPhase::Intro if d.phase_t >= 30.0 => Some(theme),
        DwPhase::Title | DwPhase::Intro => None,
        DwPhase::Playing | DwPhase::Exit if boss_level => {
            let built = d
                .prog
                .iter()
                .position(|o| *o == Op::BuildPortals)
                .is_some_and(|i| d.pc > i);
            if boss_dead {
                Some(String::new())
            } else if built {
                Some("DarkBoss".into())
            } else {
                None
            }
        }
        DwPhase::Playing | DwPhase::Exit => Some(theme),
        DwPhase::Transition => {
            let second_last = d.round_index + 2 == d.levels(c).len();
            (second_last && d.phase_t >= 31.0).then(String::new)
        }
        DwPhase::GameOver if d.continues_remaining() == 0 && d.phase_t >= 122.0 => {
            Some("GameOver".into())
        }
        DwPhase::GameOver => None,
        DwPhase::Complete if d.phase_t < 50.0 => Some(String::new()),
        DwPhase::Complete => Some(victory),
    }
}

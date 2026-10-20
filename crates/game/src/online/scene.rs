//! The online front end and match (`--online`, `--warpzone-host`,
//! `--warpzone-join ROOM|CODE`): host a public/private room, join by code,
//! browse rooms, a lobby where every machine's local players join with
//! their own devices (archer, alt costume, team), the host's settings and
//! start, then the match itself with rollback, and back to the lobby.
//!
//! Controls (any pad or the keyboard, menu bindings): up/down/left/right,
//! confirm, back; in the lobby a device that is not playing yet joins with
//! confirm, then left/right = archer, up = alt costume, down = team (Team
//! Deathmatch), back = leave the roster; start = start the match (host) /
//! ready (guests); F2 or the menu-alt button = match settings (host). In a
//! match, Start or Escape opens the pause menu as a local overlay (the sim
//! keeps running for everybody; this machine's archers idle meanwhile):
//! RESUME, or QUIT = leave the room.

use std::collections::BTreeMap;

use anyhow::Result;
use sdl2::keyboard::Scancode;
use tfp_net::warpzone_core::client::{Event, HostOptions};
use tfp_net::warpzone_core::protocol::api::{Entry, RoomInfo, Visibility};
use tfp_net::{NetConfig, NetEvent, NetSession, SlotRequest};
use tfp_render::Color;
use tfp_render::glam::Vec2;
use tfp_sim::PlayerInput;

use super::adapter::{NetWorld, OnlineSettings, TowerData, online_towers};
use super::present::Presenter;
use super::runner::OnlineMatch;
use crate::app::{DrawContext, LoadContext, Scene, TickContext};
use crate::front::assets::UiAssets;
use crate::front::gfx::{Painter, pal, rgba};
use crate::front::minput::{Dev, KeyboardSetup, MenuInput, b};
use crate::front::pause::Action;
use crate::match_pause::{MatchPause, Panel};
use crate::music::MusicPlayer;
use crate::play::{key_input, pad_input};
use crate::tower::{TowerGfx, TowerView};
use crate::versus_ui::{MusicCue, VersusUi};
use tfp_sim::bot::Bot;
use tfp_sim::versus::HudView;

mod coop_view;
mod lobby;
mod settings;

const MAX_SLOTS: usize = tfp_net::MAX_SLOTS;

#[derive(Clone, Debug, Default)]
pub struct OnlineOptions {
    /// The command line (for `--warpzone-join` / `--warpzone-host`).
    pub args: Vec<String>,
    /// Testing: this many bot players on this machine, ready at once.
    pub bots: usize,
    /// Testing (host): start the match once this many machines are in.
    pub auto_start: Option<usize>,
    /// Testing (host): mode / length / tower (part of the name; for Quest /
    /// Trials the tower / trial number).
    pub mode: Option<u8>,
    /// Testing (host): Versus / Quest / Trials (`--mode quest|trials`).
    pub kind: Option<u8>,
    pub length: Option<u8>,
    pub tower: Option<String>,
    /// Testing: host at once (public room).
    pub host: bool,
    /// Testing: leave (exit) after the first match ended.
    pub exit_after_match: bool,
    /// Inside the menus' game scene: quitting returns to the main menu
    /// instead of closing the application.
    pub embedded: bool,
    /// The front end's keyboard configurations, controller remaps and
    /// effects volume (None: defaults / full volume).
    pub keys: Option<KeyboardSetup>,
    pub pads: std::collections::BTreeMap<String, crate::padmap::PadMap>,
    pub sfx_volume: Option<f32>,
    /// GAMEPAD RUMBLE (None: on).
    pub rumble: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
enum Screen {
    Connecting,
    Menu {
        sel: usize,
    },
    /// Host settings before the room exists, or (lobby) the host editing.
    Settings {
        sel: usize,
        creating: bool,
    },
    /// Host: the machines in the room (ping, kick).
    Machines {
        sel: usize,
        armed: bool,
    },
    /// The variant list (from the settings).
    Variants {
        sel: usize,
        col: usize,
        creating: bool,
    },
    Code {
        text: String,
    },
    Browse {
        sel: usize,
    },
    Lobby,
    Match,
    Error(String),
}

/// One player on this machine.
#[derive(Clone, Debug)]
struct LocalPlayer {
    /// `None` = a bot (testing).
    dev: Option<Dev>,
    archer: u8,
    skin: u8,
    team: Option<u8>,
}

pub struct OnlineScene {
    opts: OnlineOptions,
    net: Option<NetSession<NetWorld>>,
    ui: Option<UiAssets>,
    gfx: Option<TowerGfx>,
    inst: Option<tfp_assets::GameInstall>,
    installs: tfp_assets::Installs,
    /// What this machine has (sent to the lobby).
    info: super::adapter::MachineInfo,
    /// Versus towers of the 8-player build.
    wide_names: Vec<String>,
    /// The host's wide/standard decision and why.
    tower_size: super::adapter::TowerSize,
    /// Tower graphics of the 8-player build (wide matches; loaded on use).
    gfx_wide: Option<TowerGfx>,
    tower_cache: super::adapter::TowerCache,
    /// Quest / Trials content (loaded when the game kind needs it).
    coop: Option<super::coop::CoopData>,
    /// The co-op presentation of the running Quest / Trials match.
    coop_view: Option<coop_view::CoopView>,
    tower_names: Vec<String>,
    tower: Option<TowerData>,
    mi: MenuInput,
    keys: KeyboardSetup,
    screen: Screen,
    status: String,
    logged_in: bool,
    name: String,
    pending_join: Option<String>,
    pending_host: bool,
    public: bool,
    settings: OnlineSettings,
    locals: Vec<LocalPlayer>,
    rooms: Vec<RoomInfo>,
    room_code: Option<String>,
    om: Option<OnlineMatch>,
    presenter: Option<Presenter>,
    /// Slot -> index into `locals`.
    slot_local: BTreeMap<usize, usize>,
    bots: BTreeMap<usize, Bot>,
    /// Enemy / ghost art for the match's content.
    scene_art: Option<crate::coop_ui::art::SceneArt>,
    crack_color: Option<Color>,
    /// A new match (procedural towers: a new layout): rebuild the view.
    view_stale: bool,
    gunn_canvas: crate::match_draw::GunnCanvas,
    /// The level view, keyed by (tower, level).
    view: Option<((usize, usize), TowerView)>,
    view_t0: i32,
    /// The Versus presentation (fed confirmed frames only).
    vui: Option<VersusUi>,
    /// HUD of the newest confirmed frame.
    hud: Option<HudView>,
    variant_icons: Vec<String>,
    /// Index of the tower in the tower graphics / UI tower lists.
    tower_ui: usize,
    /// The match pause, local only: the sim keeps running online.
    pause: Option<MatchPause>,
    /// Keep Start from reaching the sim until released after a resume.
    block_start: bool,
    music: Option<MusicPlayer>,
    theme_music: (String, bool),
    size: (u32, u32),
    tick: u64,
    /// Local slots whose pad is missing (the panel shows once per loss).
    lost_seen: std::collections::BTreeSet<usize>,
    matches_ended: u32,
    /// Host: a menu action to send in slot 0's input (action, frames left).
    pending_action: Option<(u8, u8)>,
    /// Guests: our ready flag. Host: a second Start before this tick forces.
    ready: bool,
    force_until: u64,
    pub quit: bool,
    /// Wall-clock pacing (the headless host ticks as fast as it can; the
    /// network needs real time).
    next_tick: Option<std::time::Instant>,
}

fn sanitize_name(s: &str) -> String {
    let t: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(12)
        .collect::<String>()
        .to_ascii_lowercase();
    if t.len() < 3 { format!("archer{t}") } else { t }
}

/// Typed characters of a join code from keyboard scancodes.
fn typed(sc: Scancode) -> Option<char> {
    let n = sc.name();
    let mut c = n.chars();
    match (c.next(), c.next()) {
        (Some(ch), None) if ch.is_ascii_alphanumeric() => Some(ch.to_ascii_uppercase()),
        _ => None,
    }
}

const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

impl OnlineScene {
    pub fn new(opts: OnlineOptions) -> Self {
        let keys = opts.keys.clone().unwrap_or_default();
        let mut settings = OnlineSettings::default();
        if let Some(m) = opts.mode {
            settings.mode = m;
        }
        if let Some(l) = opts.length {
            settings.length = l;
        }
        if let Some(k) = opts.kind {
            settings.kind = k;
            settings.coop_index = opts
                .tower
                .as_deref()
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
        }
        OnlineScene {
            opts,
            net: None,
            ui: None,
            gfx: None,
            inst: None,
            installs: tfp_assets::Installs::default(),
            info: Default::default(),
            wide_names: Vec::new(),
            tower_size: super::adapter::TowerSize::FewArchers,
            gfx_wide: None,
            tower_cache: Default::default(),
            coop: None,
            coop_view: None,
            tower_names: Vec::new(),
            tower: None,
            mi: MenuInput::default(),
            keys,
            screen: Screen::Connecting,
            status: "CONNECTING TO WARPZONE...".into(),
            logged_in: false,
            name: String::new(),
            pending_join: None,
            pending_host: false,
            public: true,
            settings,
            locals: Vec::new(),
            rooms: Vec::new(),
            room_code: None,
            om: None,
            presenter: None,
            slot_local: BTreeMap::new(),
            bots: BTreeMap::new(),
            view: None,
            scene_art: None,
            crack_color: None,
            view_stale: false,
            gunn_canvas: Default::default(),
            view_t0: 0,
            vui: None,
            hud: None,
            variant_icons: Vec::new(),
            tower_ui: 0,
            pause: None,
            block_start: false,
            music: None,
            theme_music: (String::new(), false),
            size: (320, 240),
            tick: 0,
            lost_seen: Default::default(),
            matches_ended: 0,
            ready: false,
            force_until: 0,
            pending_action: None,
            quit: false,
            next_tick: None,
        }
    }

    fn base_archers(&self) -> usize {
        self.tower
            .as_ref()
            .map(|t| t.base_archers)
            .or(self.ui.as_ref().map(|u| u.archers.len()))
            .unwrap_or(8)
            .max(1)
    }

    /// Load the tower the settings name (no-op if loaded).
    fn ensure_tower(&mut self, dir: &str, wide: bool, key: (i32, bool)) {
        if self
            .tower
            .as_ref()
            .is_some_and(|t| t.dir_name == dir && t.wide == wide && (!t.procedural || t.key == key))
        {
            return;
        }
        match self
            .tower_cache
            .get(&self.installs, dir, wide, key)
            .cloned()
        {
            Ok(t) => {
                println!(
                    "online: loaded tower {} ({} levels, {})",
                    t.dir_name,
                    t.levels.len(),
                    if wide { "wide" } else { "standard" }
                );
                self.tower = Some(t);
            }
            Err(e) => self.status = format!("CANNOT LOAD TOWER: {e:#}").to_ascii_uppercase(),
        }
    }

    /// Load the co-op content of the settings' kind (no-op for Versus).
    pub(super) fn ensure_coop(&mut self) {
        self.ensure_coop_kind(self.settings.kind);
    }

    fn ensure_coop_kind(&mut self, kind: u8) {
        if kind == super::adapter::KIND_VERSUS || self.coop.as_ref().is_some_and(|c| c.kind == kind)
        {
            return;
        }
        let Some(inst) = &self.inst else { return };
        match super::coop::CoopData::load(inst, kind) {
            Ok(c) => self.coop = Some(c),
            Err(e) => self.status = format!("CANNOT LOAD CO-OP DATA: {e:#}").to_ascii_uppercase(),
        }
    }

    /// Every machine in the room (as far as known) has Dark World.
    pub(super) fn all_dark_world(&self) -> bool {
        match &self.net {
            Some(n) if !n.lobby().machines.is_empty() => n.lobby().machines.iter().all(|m| {
                super::adapter::MachineInfo::decode(&m.info).is_some_and(|i| i.dark_world)
            }),
            _ => self.info.dark_world,
        }
    }

    /// Host: wide or standard towers for the room as it is now.
    fn decide_tower_size(&mut self) {
        let Some(n) = &self.net else { return };
        if !n.is_host() {
            return;
        }
        let l = n.lobby();
        let infos: Vec<Option<super::adapter::MachineInfo>> = l
            .machines
            .iter()
            .filter(|m| !m.slots.is_empty() || m.spectate)
            .map(|m| super::adapter::MachineInfo::decode(&m.info))
            .collect();
        let size = super::adapter::tower_size(
            &infos,
            l.preview.len(),
            self.wide_names.contains(&self.settings.tower),
        );
        self.tower_size = size;
        let wide = size == super::adapter::TowerSize::Wide;
        if wide != self.settings.wide {
            self.settings.wide = wide;
            self.apply_settings();
        }
    }

    /// Host: push the settings to the lobby (loads the tower first).
    fn apply_settings(&mut self) {
        if self.settings.tower.is_empty() {
            self.settings.tower = self.tower_names.first().cloned().unwrap_or_default();
        }
        if self.settings.kind != super::adapter::KIND_VERSUS {
            self.ensure_coop();
            if let Some(c) = &self.coop {
                c.apply_to(&mut self.settings);
            }
            let archers = self.base_archers() as u8;
            let ns = self.settings.net_settings(archers);
            if let Some(n) = &mut self.net {
                let _ = n.set_settings(ns);
            }
            return;
        }
        if self.settings.tower.is_empty()
            || self.settings.tower == "QUEST"
            || self.settings.tower == "TRIALS"
        {
            self.settings.tower = self.tower_names.first().cloned().unwrap_or_default();
        }
        if self.settings.wide && !self.wide_names.contains(&self.settings.tower) {
            self.settings.wide = false;
        }
        let (dir, wide) = (self.settings.tower.clone(), self.settings.wide);
        let key = super::adapter::procedural_key(&self.settings);
        self.ensure_tower(&dir, wide, key);
        if let Some(t) = &self.tower {
            t.apply_to(&mut self.settings);
        }
        let archers = self.base_archers() as u8;
        let ns = self.settings.net_settings(archers);
        if let Some(n) = &mut self.net {
            let _ = n.set_settings(ns);
        }
    }

    fn push_slots(&mut self) {
        let reqs: Vec<SlotRequest> = self
            .locals
            .iter()
            .enumerate()
            .map(|(k, l)| SlotRequest {
                name: format!("{} {}", self.name.to_ascii_uppercase(), k + 1),
                archer: l.archer,
                skin: l.skin,
                team: l.team,
            })
            .collect();
        if let Some(n) = &mut self.net {
            n.set_local_slots(reqs);
        }
    }

    fn host_room(&mut self) {
        self.apply_settings();
        let public = self.public;
        if let Some(n) = &mut self.net {
            n.host(HostOptions {
                max_players: 12,
                visibility: if public {
                    Visibility::Public
                } else {
                    Visibility::Unlisted
                },
                entry: Entry::Open,
                preset: Some(
                    match self.settings.kind {
                        super::adapter::KIND_QUEST => "quest",
                        super::adapter::KIND_TRIALS => "trials",
                        super::adapter::KIND_DARKWORLD => "darkworld",
                        _ => ["lms", "headhunters", "teams"][self.settings.mode.min(2) as usize],
                    }
                    .into(),
                ),
                ..Default::default()
            });
        }
        self.status = "OPENING ROOM...".into();
        self.screen = Screen::Connecting;
    }

    // ------------------------------------------------------------ events

    fn handle_events(&mut self, ctx: &mut TickContext<'_>) {
        let Some(net) = &mut self.net else { return };
        net.poll();
        let mut evs = Vec::new();
        while let Some(e) = net.next_event() {
            evs.push(e);
        }
        for e in evs {
            self.on_event(e, ctx);
        }
    }

    fn on_event(&mut self, e: NetEvent, ctx: &mut TickContext<'_>) {
        match e {
            NetEvent::Warpzone(w) => match *w {
                Event::LoggedIn {
                    account: Some(a), ..
                } => {
                    self.logged_in = true;
                    self.name = a.username.clone();
                    self.after_login();
                }
                Event::Registered(a) => {
                    self.logged_in = true;
                    self.name = a.username.clone();
                    self.after_login();
                }
                Event::Error { op, message, .. } => {
                    if op == "register" {
                        let n = format!(
                            "{}{}",
                            &self.name[..self.name.len().min(10)],
                            self.tick % 1000
                        );
                        self.name = n.clone();
                        if let Some(net) = &mut self.net {
                            net.register(&n);
                        }
                    } else {
                        self.status = format!("{}: {}", op.to_ascii_uppercase(), message);
                        if matches!(self.screen, Screen::Connecting) && self.logged_in {
                            self.screen = Screen::Menu { sel: 0 };
                        }
                    }
                }
                Event::RoomHosted { code, room, .. } => {
                    self.room_code = code.clone();
                    println!(
                        "online: hosting room {} code {}",
                        room.id,
                        code.unwrap_or_else(|| "-".into())
                    );
                }
                Event::RoomList(rooms) => {
                    self.rooms = rooms;
                    if let Screen::Browse { sel } = &mut self.screen {
                        *sel = (*sel).min(self.rooms.len().saturating_sub(1));
                    }
                }
                _ => {}
            },
            NetEvent::Welcome { machine } => {
                println!("online: in the room as {machine}");
                self.screen = Screen::Lobby;
                self.status.clear();
                self.push_slots();
                if self.net.as_ref().is_some_and(|n| n.is_host()) {
                    self.apply_settings();
                }
            }
            NetEvent::Rejected { reason } => {
                self.screen = Screen::Error(format!("REJECTED: {reason}"));
            }
            NetEvent::LobbyChanged => {
                // Guests load the host's tower while still in the lobby.
                let tower = self
                    .net
                    .as_ref()
                    .and_then(|n| OnlineSettings::decode(&n.lobby().settings.game))
                    .map(|s| (super::adapter::procedural_key(&s), s.tower, s.wide, s.kind));
                if let Some((_, _, _, kind)) = tower
                    && kind != super::adapter::KIND_VERSUS
                {
                    self.ensure_coop_kind(kind);
                } else if let Some((key, t, wide, _)) = tower
                    && !t.is_empty()
                {
                    self.ensure_tower(&t, wide, key);
                }
                self.decide_tower_size();
            }
            NetEvent::MatchStarted(ms) => self.start_match(ms),
            NetEvent::WaitingForNextMatch { .. } => {
                self.status = "A MATCH IS RUNNING: YOU PLAY IN THE NEXT ONE".into()
            }
            NetEvent::MatchFrozen { .. } => self.status = "AN ARCHER LEFT. RESYNCING...".into(),
            NetEvent::PlayersDropped { slots, .. } => {
                self.status = format!(
                    "{} LEFT THE MATCH",
                    slots
                        .iter()
                        .map(|s| format!("P{}", s + 1))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
            NetEvent::Desync { frame, machine, .. } => {
                log::warn!("desync with {machine} at frame {frame}");
                self.status = format!("DESYNC WITH {machine} (FRAME {frame})");
            }
            NetEvent::MatchEnded { agreed, .. } => {
                println!(
                    "online: match ended, agreed={agreed}, {}",
                    self.om.as_ref().map(|m| m.summary()).unwrap_or_default()
                );
                self.end_match(
                    ctx,
                    if agreed {
                        ""
                    } else {
                        "RESULTS DIFFER BETWEEN MACHINES!"
                    },
                );
            }
            NetEvent::MatchAborted { reason } => self.end_match(
                ctx,
                &format!("MATCH ABORTED: {}", reason.to_ascii_uppercase()),
            ),
            NetEvent::RoomLeft { reason } => {
                self.end_match(ctx, "");
                self.room_code = None;
                self.status = format!("LEFT THE ROOM ({})", reason.to_ascii_uppercase());
                self.screen = Screen::Menu { sel: 0 };
                if self.opts.exit_after_match {
                    self.quit = true;
                }
            }
            _ => {}
        }
    }

    fn after_login(&mut self) {
        println!("online: logged in as {}", self.name);
        if let Some(j) = self.pending_join.take() {
            self.status = "JOINING...".into();
            if let Some(n) = &mut self.net {
                super::join(n, &j);
            }
        } else if self.pending_host {
            self.pending_host = false;
            if self.opts.host {
                self.host_room();
            } else {
                self.screen = Screen::Settings {
                    sel: 0,
                    creating: true,
                };
            }
        } else if self.screen == Screen::Connecting {
            self.screen = Screen::Menu { sel: 0 };
            self.status.clear();
        }
    }

    fn start_match(&mut self, ms: tfp_net::MatchStart) {
        let Some(os) = OnlineSettings::decode(&ms.settings.game) else {
            self.status = "BAD MATCH SETTINGS".into();
            return;
        };
        if os.kind != super::adapter::KIND_VERSUS {
            self.ensure_coop_kind(os.kind);
            let Some(c) = &self.coop else { return };
            match OnlineMatch::new_coop(ms.clone(), c) {
                Ok(mut m) => {
                    m.keep_snapshots = true;
                    self.coop_view = match (&self.gfx, &self.ui, &self.inst) {
                        (Some(g), Some(ua), Some(inst)) => Some(coop_view::CoopView::new(
                            c,
                            &os,
                            &ms,
                            &m.game.world,
                            g,
                            ua,
                            inst,
                        )),
                        _ => None,
                    };
                    // Archer cosmetics (match_draw), art in our content copy.
                    if let (Some(cv), Some(inst)) = (&mut self.coop_view, &self.inst)
                        && let Ok(gd) = tfp_assets::game_data::GameData::load(&inst.content)
                    {
                        let content = std::sync::Arc::make_mut(&mut m.game.content);
                        cv.attach_actors(crate::versus_ui::actors::build_art(content, inst, &gd));
                    }
                    let me = self.net.as_ref().and_then(|n| n.me());
                    self.slot_local = ms
                        .players
                        .iter()
                        .filter(|p| Some(p.machine) == me)
                        .map(|p| (p.slot as usize, p.local_index as usize))
                        .collect();
                    self.bots = self
                        .slot_local
                        .iter()
                        .filter(|(_, li)| self.locals.get(**li).is_some_and(|l| l.dev.is_none()))
                        .map(|(s, _)| (*s, Bot::new(7919 * (*s as i32 + 1) + ms.match_id as i32)))
                        .collect();
                    println!(
                        "online: {} started, local slots {:?}",
                        super::adapter::KIND_NAMES[os.kind.min(3) as usize].to_ascii_lowercase(),
                        self.slot_local.keys().collect::<Vec<_>>()
                    );
                    self.vui = None;
                    self.hud = None;
                    self.pause = None;
                    self.om = Some(m);
                    self.screen = Screen::Match;
                    self.status.clear();
                }
                Err(e) => self.status = format!("{e:#}").to_ascii_uppercase(),
            }
            return;
        }
        self.ensure_tower(&os.tower, os.wide, super::adapter::procedural_key(&os));
        let Some(t) = &self.tower else { return };
        match OnlineMatch::new(ms.clone(), t) {
            Ok(m) => {
                let me = self.net.as_ref().and_then(|n| n.me());
                self.slot_local = ms
                    .players
                    .iter()
                    .filter(|p| Some(p.machine) == me)
                    .map(|p| (p.slot as usize, p.local_index as usize))
                    .collect();
                self.bots = self
                    .slot_local
                    .iter()
                    .filter(|(_, li)| self.locals.get(**li).is_some_and(|l| l.dev.is_none()))
                    .map(|(s, _)| (*s, Bot::new(7919 * (*s as i32 + 1) + ms.match_id as i32)))
                    .collect();
                println!(
                    "online: match {} started, {} archers on {}, local slots {:?}",
                    ms.match_id,
                    ms.players.len(),
                    t.dir_name,
                    self.slot_local.keys().collect::<Vec<_>>()
                );
                let mut m = m;
                m.keep_snapshots = true;
                // Archer looks in the sim's order: base archers, then alts.
                let mut looks = self
                    .ui
                    .as_ref()
                    .map(|u| u.archers.clone())
                    .unwrap_or_default();
                if let Some(u) = &self.ui {
                    looks.extend(u.archers.iter().filter_map(|a| a.alt.as_deref().cloned()));
                }
                let mut vui = VersusUi::new(looks);
                // A machine with only bots confirms the screens itself.
                vui.auto = self
                    .slot_local
                    .values()
                    .all(|li| self.locals.get(*li).is_some_and(|l| l.dev.is_none()));
                // Archer / corpse / arrow cosmetics (match_draw); their art
                // goes into this machine's copy of the content (presentation
                // data only, the sim never reads it).
                if let Some(inst) = &self.inst
                    && let Ok(gd) = tfp_assets::game_data::GameData::load(&inst.content)
                {
                    let content = std::sync::Arc::make_mut(&mut m.game.content);
                    crate::versus_ui::actors::install(&mut vui, content, inst, &gd);
                }
                self.vui = Some(vui);
                self.scene_art = Some(crate::coop_ui::art::SceneArt::from_art(t.art.clone()));
                self.view_stale = true;
                self.hud = None;
                self.variant_icons = m
                    .game
                    .world
                    .versus
                    .as_ref()
                    .map(|v| crate::play::variant_icons(&v.settings.variants))
                    .unwrap_or_default();
                self.tower_ui = if t.wide { &self.gfx_wide } else { &self.gfx }
                    .as_ref()
                    .and_then(|g| g.index.versus.iter().position(|v| v.dir_name == t.dir_name))
                    .unwrap_or(0);
                self.theme_music = if t.wide { &self.gfx_wide } else { &self.gfx }
                    .as_ref()
                    .and_then(|g| g.music(self.tower_ui))
                    .unwrap_or_default();
                if let Some(mp) = &self.music {
                    let (theme, dark) = &self.theme_music;
                    mp.prefetch(&[
                        theme.clone(),
                        if *dark { "DarkBoss" } else { "Boss" }.to_owned(),
                    ]);
                }
                self.pause = None;
                self.om = Some(m);
                self.screen = Screen::Match;
                self.status.clear();
            }
            Err(e) => self.status = format!("{e:#}").to_ascii_uppercase(),
        }
    }

    fn end_match(&mut self, ctx: &mut TickContext<'_>, msg: &str) {
        if let Some(p) = &mut self.presenter {
            p.stop_all(ctx.audio);
        }
        if let Some(m) = &mut self.music {
            m.stop(ctx.audio);
            ctx.audio.set_music_volume(m.volume);
        }
        self.pause = None;
        self.coop_view = None;
        if self.om.take().is_some() {
            self.matches_ended += 1;
            if self.opts.exit_after_match {
                if let Some(n) = &mut self.net {
                    n.leave();
                }
                self.quit = true;
            }
        }
        if self.screen == Screen::Match {
            self.screen = Screen::Lobby;
        }
        if !msg.is_empty() {
            self.status = msg.into();
        }
    }

    // ------------------------------------------------------------ screens

    fn tick_menu(&mut self, sel: usize) {
        const N: usize = 4;
        let mut sel = sel;
        if self.mi.p(b::UP) {
            sel = (sel + N - 1) % N;
        }
        if self.mi.p(b::DOWN) {
            sel = (sel + 1) % N;
        }
        self.screen = Screen::Menu { sel };
        if self.mi.p(b::CONFIRM) || self.mi.p(b::START) {
            match sel {
                0 => {
                    self.screen = Screen::Settings {
                        sel: 0,
                        creating: true,
                    }
                }
                1 => {
                    self.screen = Screen::Code {
                        text: String::new(),
                    }
                }
                2 => {
                    self.rooms.clear();
                    if let Some(n) = &mut self.net {
                        n.list_rooms(false);
                    }
                    self.screen = Screen::Browse { sel: 0 };
                }
                _ => self.quit = true,
            }
        } else if self.mi.p(b::BACK) {
            self.quit = true;
        }
    }

    fn tick_code(&mut self, mut text: String) {
        for &sc in &self.mi.keys_pressed {
            if sc == Scancode::Backspace {
                text.pop();
            } else if let Some(c) = typed(sc)
                && text.len() < 6
            {
                text.push(c);
            }
        }
        let enter = self.mi.keys_pressed.contains(&Scancode::Return)
            || self.mi.keys_pressed.contains(&Scancode::KpEnter);
        let escape = self.mi.keys_pressed.contains(&Scancode::Escape);
        // Pads: up/down change the last letter, right/confirm add one,
        // left/back delete, start joins.
        let pads: Vec<u16> = self
            .mi
            .devs
            .iter()
            .filter(|d| matches!(d.dev, Dev::Pad(_)))
            .map(|d| d.pressed)
            .collect();
        let mut join = enter;
        let mut back = escape;
        for p in pads {
            let has = |bit: u16| p & bit != 0;
            if has(b::UP) || has(b::DOWN) {
                if text.is_empty() {
                    text.push('A');
                }
                let last = text.pop().unwrap_or('A');
                let k = CODE_ALPHABET
                    .iter()
                    .position(|&c| c as char == last)
                    .unwrap_or(0) as i32;
                let d = if has(b::UP) { 1 } else { -1 };
                let m = CODE_ALPHABET.len() as i32;
                text.push(CODE_ALPHABET[(k + d).rem_euclid(m) as usize] as char);
            }
            if (has(b::RIGHT) || has(b::CONFIRM)) && text.len() < 6 {
                text.push('A');
            }
            if has(b::LEFT) {
                text.pop();
            }
            if has(b::BACK) {
                if text.is_empty() {
                    back = true;
                }
                text.pop();
            }
            if has(b::START) {
                join = true;
            }
        }
        if back {
            self.screen = Screen::Menu { sel: 1 };
            return;
        }
        if join && text.len() >= 4 {
            self.status = format!("JOINING {text}...");
            if let Some(n) = &mut self.net {
                n.join_code(&text);
            }
            self.screen = Screen::Connecting;
            return;
        }
        self.screen = Screen::Code { text };
    }

    fn tick_browse(&mut self, sel: usize) {
        let n = self.rooms.len();
        let mut sel = sel;
        if n > 0 {
            if self.mi.p(b::UP) {
                sel = (sel + n - 1) % n;
            }
            if self.mi.p(b::DOWN) {
                sel = (sel + 1) % n;
            }
        }
        self.screen = Screen::Browse { sel };
        if (self.tick.is_multiple_of(300) || self.mi.p(b::ALT))
            && let Some(net) = &mut self.net
        {
            net.list_rooms(false);
        }
        if (self.mi.p(b::CONFIRM) || self.mi.p(b::START))
            && let Some(r) = self.rooms.get(sel)
        {
            self.status = format!("JOINING {}...", r.name.to_ascii_uppercase());
            let id = r.id.clone();
            if let Some(net) = &mut self.net {
                net.join(&id, None, None);
            }
            self.screen = Screen::Connecting;
        } else if self.mi.p(b::BACK) {
            self.screen = Screen::Menu { sel: 2 };
        }
    }

    fn tick_lobby(&mut self) {
        let is_host = self.net.as_ref().is_some_and(|n| n.is_host());
        let archers = self.base_archers() as i32;
        let teams = self.settings_in_lobby().mode == 2;
        let mut changed = false;
        let mut start = false;
        let mut leave = false;
        let mut open_settings = self.mi.keys_pressed.contains(&Scancode::F2);
        let devs: Vec<(Dev, u16)> = self.mi.devs.iter().map(|d| (d.dev, d.pressed)).collect();
        for (dev, p) in devs {
            let has = |bit: u16| p & bit != 0;
            match self.locals.iter().position(|l| l.dev == Some(dev)) {
                None => {
                    if has(b::CONFIRM) && self.locals.len() < MAX_SLOTS {
                        let used: Vec<u8> = self.locals.iter().map(|l| l.archer).collect();
                        let archer = (0..archers as u8).find(|a| !used.contains(a)).unwrap_or(0);
                        self.locals.push(LocalPlayer {
                            dev: Some(dev),
                            archer,
                            skin: 0,
                            team: None,
                        });
                        changed = true;
                    } else if has(b::BACK) && self.locals.iter().all(|l| l.dev.is_none()) {
                        leave = true;
                    }
                    if has(b::START) {
                        start = true;
                    }
                    if has(b::ALT) {
                        open_settings = true;
                    }
                }
                Some(k) => {
                    let l = &mut self.locals[k];
                    let d = has(b::RIGHT) as i32 - has(b::LEFT) as i32;
                    if d != 0 {
                        l.archer = ((l.archer as i32 + d).rem_euclid(archers)) as u8;
                        changed = true;
                    }
                    if has(b::UP) {
                        l.skin = if l.skin == 0 { 1 } else { 0 };
                        changed = true;
                    }
                    if has(b::DOWN) && teams {
                        l.team = match l.team {
                            None => Some(0),
                            Some(0) => Some(1),
                            _ => None,
                        };
                        changed = true;
                    }
                    if has(b::BACK) {
                        self.locals.remove(k);
                        changed = true;
                    } else {
                        if has(b::START) {
                            start = true;
                        }
                        if has(b::ALT) {
                            open_settings = true;
                        }
                    }
                }
            }
        }
        if changed {
            self.push_slots();
        }
        if leave {
            if let Some(n) = &mut self.net {
                n.leave();
            }
            return;
        }
        if is_host && (self.mi.keys_pressed.contains(&Scancode::F3) || self.mi.p(b::ALT2)) {
            self.screen = Screen::Machines {
                sel: 0,
                armed: false,
            };
            return;
        }
        if open_settings && is_host {
            self.screen = Screen::Settings {
                sel: 0,
                creating: false,
            };
            return;
        }
        let auto = is_host
            && self.opts.auto_start.is_some_and(|k| {
                self.net.as_ref().is_some_and(|n| {
                    let l = n.lobby();
                    l.machines.len() >= k
                        && l.preview.len() >= 2
                        && l.machines.iter().all(|m| !m.slots.is_empty() || m.spectate)
                })
            })
            && self.matches_ended == 0;
        let mut host_start = false;
        if (start || auto)
            && let Some(n) = &mut self.net
        {
            if is_host {
                let players = n.lobby().preview.len();
                let not_ready: Vec<String> = n
                    .lobby()
                    .machines
                    .iter()
                    .filter(|m| m.machine.0 != 0 && !m.slots.is_empty() && !m.ready)
                    .map(|m| m.name.to_ascii_uppercase())
                    .collect();
                if players < 2 && self.settings.kind == super::adapter::KIND_VERSUS {
                    self.status = "NEED 2 OR MORE ARCHERS".into();
                } else if self.settings.kind == super::adapter::KIND_QUEST
                    && players > super::coop::QUEST_MAX
                {
                    self.status = format!("QUEST TAKES 1-{} ARCHERS", super::coop::QUEST_MAX);
                } else if self.settings.kind == super::adapter::KIND_DARKWORLD
                    && players > super::coop::DW_MAX
                {
                    self.status = format!("DARK WORLD TAKES 1-{} ARCHERS", super::coop::DW_MAX);
                } else if self.settings.kind == super::adapter::KIND_TRIALS && players != 1 {
                    self.status = "TRIALS TAKES ONE ARCHER (THE OTHERS WATCH)".into();
                } else if self.settings.kind == super::adapter::KIND_VERSUS
                    && self.settings.mode == 2
                    && players < 3
                {
                    self.status = "TEAM DEATHMATCH NEEDS 3 OR MORE ARCHERS".into();
                } else if !not_ready.is_empty() && !auto && self.tick > self.force_until {
                    // Not everybody is ready: a second Start within 3 s forces.
                    self.status = format!(
                        "NOT READY: {} (START AGAIN: START ANYWAY)",
                        not_ready.join(", ")
                    );
                    self.force_until = self.tick + 180;
                } else {
                    self.force_until = 0;
                    host_start = true;
                }
            } else {
                self.ready = !self.ready;
                n.set_ready(self.ready);
                self.status = if self.ready { "READY" } else { "NOT READY" }.into();
            }
        }
        if host_start {
            // A procedural tower gets a fresh layout seed for every match
            // (the content is rebuilt here and in every guest from it).
            if self.tower.as_ref().is_some_and(|t| t.procedural) {
                self.settings.procedural_seed = super::adapter::random_procedural_seed();
                self.apply_settings();
            }
            if let Some(n) = &mut self.net
                && let Err(e) = n.start_match(None)
            {
                self.status = format!("{e}").to_ascii_uppercase();
            }
        }
    }

    fn settings_in_lobby(&self) -> OnlineSettings {
        self.net
            .as_ref()
            .and_then(|n| OnlineSettings::decode(&n.lobby().settings.game))
            .unwrap_or_else(|| self.settings.clone())
    }

    fn tick_match(&mut self, ctx: &mut TickContext<'_>) {
        if self.pause_tick(ctx) {
            if let Some(n) = &mut self.net {
                n.leave();
            }
            return;
        }
        let paused = self.pause.is_some();
        // Dark World game over: the host answers the menu; the choice goes
        // into its slot-0 input (ACTION_*), guests see the menu read-only.
        let is_host = self.net.as_ref().is_some_and(|n| n.is_host());
        if let Some(cv) = &mut self.coop_view
            && let Some(items) = cv.confirmed().and_then(coop_view::menu_items)
            && is_host
        {
            let n = items.len();
            if self.mi.p(b::UP) {
                cv.menu_sel = (cv.menu_sel + n - 1) % n;
            }
            if self.mi.p(b::DOWN) {
                cv.menu_sel = (cv.menu_sel + 1) % n;
            }
            cv.menu_sel = cv.menu_sel.min(n - 1);
            if self.mi.p(b::CONFIRM) {
                match items[cv.menu_sel] {
                    coop_view::MenuItem::Continue(_) => {
                        self.pending_action = Some((super::adapter::ACTION_CONTINUE, 6))
                    }
                    coop_view::MenuItem::Restart => {
                        self.pending_action = Some((super::adapter::ACTION_RESTART, 6))
                    }
                    coop_view::MenuItem::Map => {
                        if let Some(n) = &mut self.net {
                            let _ = n.end_match();
                        }
                        return;
                    }
                    coop_view::MenuItem::Quit => {
                        if let Some(n) = &mut self.net {
                            n.leave();
                        }
                        return;
                    }
                }
                cv.menu_sel = 0;
            }
        }
        if self.block_start && !self.mi.h(b::START) {
            self.block_start = false;
        }
        let (Some(net), Some(om)) = (&mut self.net, &mut self.om) else {
            return;
        };
        let mut inputs = [PlayerInput::default(); MAX_SLOTS];
        let mut human = [false; MAX_SLOTS];
        for (&slot, &li) in &self.slot_local {
            let dev = self.locals.get(li).and_then(|l| l.dev);
            inputs[slot] = match dev {
                Some(Dev::Pad(id)) => ctx
                    .input
                    .pads
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| match self.opts.pads.get(&p.name) {
                        Some(m) => crate::play::pad_input_with(p, m),
                        None => pad_input(p),
                    })
                    .unwrap_or_default(),
                Some(Dev::Keyboard(k)) => self
                    .keys
                    .layouts
                    .get(k as usize)
                    .map(|l| key_input(l, ctx.input))
                    .unwrap_or_default(),
                None => match self.bots.get_mut(&slot) {
                    Some(b) => b.input(&om.game.world, &om.game.content, slot as u8),
                    None => PlayerInput::default(),
                },
            };
            human[slot] = dev.is_some();
            // The local pause menu has the devices; the archer idles (the
            // match goes on for everybody else).
            if dev.is_some() && paused {
                inputs[slot] = PlayerInput::default();
            }
            if dev.is_some() && self.block_start {
                inputs[slot].buttons &= !PlayerInput::START;
            }
        }
        if let Some((action, left)) = self.pending_action.take()
            && self.slot_local.contains_key(&0)
        {
            inputs[0]._pad = action;
            if left > 1 {
                self.pending_action = Some((action, left - 1));
            }
        }
        // The presentation decides when Confirm may reach the sim (no
        // continue while the local replay / results animation runs).
        if let Some(v) = &mut self.vui {
            v.gate_inputs(self.hud.as_ref(), &mut inputs, &human);
        }
        let local: Vec<(usize, PlayerInput)> =
            self.slot_local.keys().map(|&s| (s, inputs[s])).collect();
        let released = om.step(net, &local);
        if let Some(p) = &mut self.presenter {
            // The master pitch follows slow motion (audio.md 3).
            p.sfxp
                .set_time_rate(ctx.audio, om.game.world.time.time_rate);
            p.present(ctx.audio, released);
            // Rumble for this machine's pads only (input.md 2.4).
            let on = self.opts.rumble.unwrap_or(true);
            for (slot, s, f) in std::mem::take(&mut p.rumble) {
                let dev = self
                    .slot_local
                    .get(&(slot as usize))
                    .and_then(|&li| self.locals.get(li))
                    .and_then(|l| l.dev);
                if on && let Some(Dev::Pad(id)) = dev {
                    ctx.host.rumble.push((id, s, f));
                }
            }
        }
        // The Versus presentation runs on the confirmed timeline only.
        let content = om.game.content.clone();
        for w in om.drain_confirmed() {
            if let Some(cv) = &mut self.coop_view {
                cv.after_confirmed(w, &content);
                continue;
            }
            let hud = w.versus.as_ref().map(|m| m.hud(&w));
            if let Some(v) = &mut self.vui {
                v.after_step(&w, hud.as_ref());
                v.post_step(&w, &om.game.content);
            }
            self.hud = hud;
        }
        if let (Some(v), Some(p)) = (&mut self.vui, &mut self.presenter) {
            for (name, x) in std::mem::take(&mut v.cues) {
                p.cue(ctx.audio, name, x);
            }
            for e in std::mem::take(&mut v.replay_sounds) {
                p.replay_sound(ctx.audio, e);
            }
        }
        if let (Some(cv), Some(p)) = (&mut self.coop_view, &mut self.presenter) {
            for (name, x) in cv.take_cues() {
                p.cue(ctx.audio, name, x);
            }
        }
        if let (Some(cv), Some(m)) = (&mut self.coop_view, &mut self.music) {
            cv.music(ctx.audio, m);
        }
        // Tower music and jingles (requested on the confirmed timeline).
        if let (Some(v), Some(m)) = (&mut self.vui, &mut self.music) {
            match v.music.take() {
                Some(MusicCue::Theme { overtime }) => {
                    let (theme, dark) = &self.theme_music;
                    let cue = match (overtime, *dark) {
                        (true, true) => "DarkBoss",
                        (true, false) => "Boss",
                        _ => theme.as_str(),
                    };
                    if !cue.is_empty() {
                        m.play(ctx.audio, cue);
                    }
                }
                Some(MusicCue::Stop) => m.stop(ctx.audio),
                Some(MusicCue::Victory(cue)) => m.play_immediate(ctx.audio, &cue),
                None => {}
            }
        }
    }

    fn ui_sound(&self, audio: &mut crate::audio::Audio, name: &str) {
        if let Some(s) = self.ui.as_ref().and_then(|a| a.sound(name)) {
            let volume = self.opts.sfx_volume.unwrap_or(1.0);
            if volume > 0.0 {
                audio.play(
                    s,
                    crate::audio::PlayParams {
                        volume,
                        ..Default::default()
                    },
                );
            }
        }
    }

    /// The match pause, online: Start (or Escape) opens the Versus pause
    /// menu as a local overlay; the sim is not paused. A local player's
    /// controller going missing shows the disconnect panel the same way.
    /// Returns true when QUIT was chosen (leave the match and the room).
    fn pause_tick(&mut self, ctx: &mut TickContext<'_>) -> bool {
        let lost: Vec<(usize, usize)> = self
            .slot_local
            .iter()
            .filter(|(_, li)| {
                matches!(self.locals.get(**li).and_then(|l| l.dev),
                    Some(Dev::Pad(id)) if !ctx.input.pads.iter().any(|p| p.id == id))
            })
            .map(|(s, li)| (*s, *li))
            .collect();
        let newly: Vec<usize> = lost
            .iter()
            .map(|l| l.0)
            .filter(|s| !self.lost_seen.contains(s))
            .collect();
        self.lost_seen = lost.iter().map(|l| l.0).collect();
        if self.pause.is_none() {
            let open = self.mi.p(b::START) || self.mi.keys_pressed.contains(&Scancode::Escape);
            if open || !newly.is_empty() {
                let mut pz = MatchPause::new();
                if let Some(&slot) = newly.first() {
                    let color = self
                        .om
                        .as_ref()
                        .and_then(|m| m.start.players.get(slot))
                        .and_then(|p| self.ui.as_ref()?.archers.get(p.archer as usize))
                        .map(|a| rgba(a.color_b))
                        .unwrap_or(Color::WHITE);
                    pz.panel = Some(Panel {
                        slot,
                        t: 0,
                        color,
                        badge: String::new(),
                    });
                }
                self.pause = Some(pz);
                self.ui_sound(ctx.audio, "ui_pause");
                if let Some(m) = &self.music {
                    ctx.audio.set_music_volume(m.volume * 0.5);
                }
            }
            return false;
        }
        let mut sounds = Vec::new();
        let action = self.pause.as_mut().and_then(|pz| {
            pz.step(
                &self.mi,
                |slot| !lost.iter().any(|l| l.0 == slot),
                &mut sounds,
            )
        });
        for s in sounds {
            self.ui_sound(ctx.audio, s);
        }
        match action {
            Some(Action::Resume) => {
                self.pause = None;
                self.block_start = true;
                if let Some(m) = &self.music {
                    ctx.audio.set_music_volume(m.volume);
                }
                false
            }
            Some(Action::Quit) => true,
            _ => false,
        }
    }
}

impl OnlineScene {
    /// Load assets and connect (also used when the menus switch to online).
    pub fn init(
        &mut self,
        gpu: &tfp_render::Gpu,
        renderer: &mut tfp_render::Renderer,
    ) -> Result<()> {
        self.ui = Some(UiAssets::load(gpu, renderer, false));
        let installs = tfp_assets::Installs::discover();
        self.inst = installs.steam.clone().or(installs.eight_player.clone());
        self.info = super::adapter::MachineInfo::of(&installs);
        self.wide_names = super::adapter::wide_tower_names(&installs);
        if let Some(i8p) = &installs.eight_player {
            self.gfx_wide = TowerGfx::load_with_main(
                gpu,
                renderer,
                i8p,
                self.ui
                    .as_ref()
                    .and_then(|u| u.main.as_ref().map(|m| m.texture)),
            )
            .map_err(|e| log::warn!("8-player build graphics: {e:#}"))
            .ok();
        }
        self.installs = installs.clone();
        if let Some(inst) = &self.inst {
            self.tower_names = online_towers(inst)?
                .into_iter()
                .map(|t| t.dir_name)
                .collect();
            if let Some(q) = &self.opts.tower {
                let q = q.to_ascii_lowercase();
                if let Some(t) = self
                    .tower_names
                    .iter()
                    .find(|t| t.to_ascii_lowercase().contains(&q))
                {
                    self.settings.tower = t.clone();
                }
            }
            self.gfx = Some(TowerGfx::load_with_main(
                gpu,
                renderer,
                inst,
                self.ui
                    .as_ref()
                    .and_then(|u| u.main.as_ref().map(|m| m.texture)),
            )?);
            let mut p = Presenter::new(inst);
            p.volume = self.opts.sfx_volume.unwrap_or(1.0);
            self.presenter = Some(p);
            match MusicPlayer::open(&inst.content.music_dir()) {
                Ok(m) => self.music = Some(m),
                Err(e) => log::warn!("music: {e:#}"),
            }
        } else {
            self.screen = Screen::Error("GAME DATA NOT FOUND (TF_GAME_DIR)".into());
            return Ok(());
        }
        let user = std::env::var("TFP_PLAYER_NAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "archer".into());
        self.name = sanitize_name(&user);
        let mut cfg = NetConfig::new(&super::net_version());
        cfg.auto_register = Some(self.name.clone());
        match NetSession::new(cfg) {
            Ok(mut n) => {
                n.set_machine_info(self.info.encode());
                self.net = Some(n);
            }
            Err(e) => {
                self.screen = Screen::Error(format!("WARPZONE: {e}"));
                return Ok(());
            }
        }
        // Testing: bot players, announced before joining (in `Hello`).
        let n = self.base_archers();
        let salt = self
            .name
            .bytes()
            .fold(0usize, |a, b| a.wrapping_mul(31) + b as usize);
        for k in 0..self.opts.bots.min(MAX_SLOTS) {
            self.locals.push(LocalPlayer {
                dev: None,
                archer: ((salt + k * 3) % n) as u8,
                skin: 0,
                team: None,
            });
        }
        if !self.locals.is_empty() {
            self.push_slots();
            self.ready = true;
            if let Some(net) = &mut self.net {
                net.set_ready(true);
            }
        }
        let intent = tfp_net::warpzone_core::launch::from_args(&self.opts.args);
        self.pending_join = super::launch_join(&intent);
        self.pending_host = intent.host || self.opts.host;
        Ok(())
    }
}

impl Scene for OnlineScene {
    fn game_size(&self) -> (u32, u32) {
        self.size
    }

    fn load(&mut self, ctx: &mut LoadContext<'_>) -> Result<()> {
        self.init(ctx.gpu, ctx.renderer)
    }

    fn tick(&mut self, ctx: &mut TickContext<'_>) {
        let step = std::time::Duration::from_micros(16_667);
        let now = std::time::Instant::now();
        let due = *self.next_tick.get_or_insert(now);
        if due > now {
            std::thread::sleep((due - now).min(step));
        }
        self.next_tick = Some(if now > due + step * 6 {
            now
        } else {
            due + step
        });
        self.tick += 1;
        self.mi.update(ctx.input, &self.keys);
        self.handle_events(ctx);
        if let Some(p) = &mut self.presenter {
            p.tick();
        }
        match self.screen.clone() {
            Screen::Connecting => {
                if self.mi.p(b::BACK) || self.mi.keys_pressed.contains(&Scancode::Escape) {
                    if let Some(n) = &mut self.net {
                        n.leave();
                    }
                    self.screen = Screen::Menu { sel: 0 };
                }
            }
            Screen::Menu { sel } => self.tick_menu(sel),
            Screen::Settings { sel, creating } => self.tick_settings(sel, creating),
            Screen::Variants { sel, col, creating } => self.tick_variants(sel, col, creating),
            Screen::Machines { sel, armed } => self.tick_machines(sel, armed),
            Screen::Code { text } => self.tick_code(text),
            Screen::Browse { sel } => self.tick_browse(sel),
            Screen::Lobby => self.tick_lobby(),
            Screen::Match => self.tick_match(ctx),
            Screen::Error(_) => {
                if self.mi.p(b::CONFIRM) || self.mi.p(b::BACK) {
                    self.quit = true;
                }
            }
        }
        if self.quit && !self.opts.embedded {
            ctx.host.quit = true;
        }
    }

    fn draw(&mut self, ctx: &mut DrawContext<'_>) {
        if self.screen == Screen::Match {
            self.draw_match(ctx);
            return;
        }
        self.size = (320, 240);
        ctx.present.offset = Vec2::ZERO;
        let Some(ui) = &self.ui else { return };
        let w = 320.0;
        ctx.list.rect(0.0, 0.0, 320.0, 240.0, Color::hex(0x10141c));
        let mut p = Painter {
            list: ctx.list,
            a: ui,
            w,
        };
        p.text_outlined(
            "ONLINE",
            Vec2::new(160.0, 12.0),
            (0.5, 0.0),
            2.0,
            pal::SUBTITLE,
        );
        if self.screen == Screen::Lobby {
            self.draw_lobby(&mut p);
        }
        let lines = self.menu_lines();
        let mut y = 44.0;
        for (text, color) in lines {
            p.text(&text, Vec2::new(160.0, y), (0.5, 0.0), 1.0, color);
            y += 11.0;
        }
        if !self.status.is_empty() {
            p.text(
                &self.status,
                Vec2::new(160.0, 226.0),
                (0.5, 0.0),
                1.0,
                pal::RED,
            );
        }
    }
}

impl OnlineScene {
    /// The text of the current menu screen (one line per entry).
    fn menu_lines(&self) -> Vec<(String, Color)> {
        let sel_col = |on: bool| if on { pal::SELECTED } else { pal::IDLE };
        let mut out: Vec<(String, Color)> = Vec::new();
        match &self.screen {
            Screen::Connecting => out.push((self.status.clone(), pal::YELLOW)),
            Screen::Menu { sel } => {
                out.push((
                    format!("SIGNED IN AS {}", self.name.to_ascii_uppercase()),
                    Color::WHITE,
                ));
                out.push((String::new(), Color::WHITE));
                for (i, t) in ["HOST A ROOM", "JOIN BY CODE", "BROWSE ROOMS", "QUIT"]
                    .iter()
                    .enumerate()
                {
                    out.push((t.to_string(), sel_col(i == *sel)));
                }
            }
            Screen::Settings { sel, creating } => out = self.settings_lines(*sel, *creating),
            Screen::Variants { sel, col, .. } => out = self.variants_lines(*sel, *col),
            Screen::Machines { sel, armed } => out = self.machines_lines(*sel, *armed),
            Screen::Code { text } => {
                out.push(("ENTER THE ROOM CODE".into(), Color::WHITE));
                out.push((String::new(), Color::WHITE));
                let mut shown = text.clone();
                while shown.len() < 6 {
                    shown.push('_');
                }
                out.push((shown, pal::SELECTED));
                out.push((String::new(), Color::WHITE));
                out.push(("KEYBOARD: TYPE, ENTER".into(), pal::IDLE));
                out.push((
                    "PAD: UP/DOWN LETTER, RIGHT NEXT, START JOIN".into(),
                    pal::IDLE,
                ));
            }
            Screen::Browse { sel } => {
                out.push(("ROOMS".into(), Color::WHITE));
                if self.rooms.is_empty() {
                    out.push(("NO ROOMS YET (REFRESHING)".into(), pal::IDLE));
                }
                for (i, r) in self.rooms.iter().enumerate().take(12) {
                    let md = |k: &str| r.metadata.get(k).cloned().unwrap_or_default();
                    out.push((
                        format!(
                            "{}  {} ARCHERS  {}  {}",
                            r.name.to_ascii_uppercase(),
                            md("players"),
                            md("mode"),
                            md("map")
                        ),
                        sel_col(i == *sel),
                    ));
                }
            }
            Screen::Lobby => {}
            Screen::Error(e) => out.push((e.clone(), pal::RED)),
            Screen::Match => {}
        }
        out
    }

    /// Online Quest / Trials: the co-op presentation (see `coop_view`).
    fn draw_coop(&mut self, ctx: &mut DrawContext<'_>) {
        let (Some(cv), Some(g), Some(data), Some(om), Some(ua)) = (
            &mut self.coop_view,
            &mut self.gfx,
            &self.coop,
            &self.om,
            &self.ui,
        ) else {
            return;
        };
        let host = self.net.as_ref().is_some_and(|n| n.is_host());
        cv.draw(
            ctx,
            g,
            data,
            &om.game.world,
            &om.game.content,
            ua,
            om.frame as u64,
            host,
        );
        self.size = cv.size;
        if let Some(pz) = &self.pause {
            let mut p = Painter {
                list: ctx.list,
                a: ua,
                w: self.size.0 as f32,
            };
            pz.draw(&mut p, om.frame as u64);
        }
    }

    fn draw_match(&mut self, ctx: &mut DrawContext<'_>) {
        if let (Some(v), Some(cc)) = (&mut self.vui, self.crack_color) {
            v.effects.crack_color = Some(cc);
        }
        if self
            .om
            .as_ref()
            .is_some_and(|m| m.game.world.versus.is_none())
        {
            self.draw_coop(ctx);
            return;
        }
        let wide = self.tower.as_ref().is_some_and(|t| t.wide);
        let gfx = if wide { &self.gfx_wide } else { &self.gfx };
        let (Some(om), Some(g), Some(vui)) = (&self.om, gfx, &self.vui) else {
            return;
        };
        let live = &om.game.world;
        let c = &om.game.content;
        // The live (predicted) world, or a replay frame from the confirmed
        // recording while the instant replay runs.
        let w = vui.render_world(live);
        let level = w.level;
        let key = (self.tower_ui, level);
        if self.view_stale || self.view.as_ref().is_none_or(|(k, _)| *k != key) {
            self.view_stale = false;
            let look = match self.tower.as_ref().filter(|t| t.procedural) {
                Some(t) => g.look_procedural(self.tower_ui, level, t.key),
                None => g.look(self.tower_ui, level, None),
            };
            match look.and_then(|look| TowerView::new(ctx.gpu, ctx.renderer, g, look)) {
                Ok(v) => {
                    if let Some((_, old)) = self.view.take() {
                        old.destroy(ctx.renderer);
                    }
                    // Cracked blocks in the theme's colour.
                    if let Some(th) = g.la.game_data.theme(&v.look.theme) {
                        let cc = th.cracked_block_color;
                        self.crack_color = Some(Color::rgba(cc[0], cc[1], cc[2], 255));
                    }
                    self.size = v.size;
                    self.view = Some((key, v));
                    self.view_t0 = om.frame;
                }
                Err(e) => log::warn!("level view: {e:#}"),
            }
        }
        let (Some((_, v)), Some(ui)) = (&mut self.view, &self.ui) else {
            return;
        };
        // The shared match drawing (match_draw.rs): the level from the
        // predicted world, the HUD below from the confirmed timeline.
        let scene_art = &mut self.scene_art;
        crate::match_draw::draw_level(
            ctx,
            v,
            &crate::match_draw::LevelDraw {
                world: w,
                content: c,
                gfx: g,
                ui: vui,
                ui_assets: ui,
                hitboxes: false,
                level_t: (om.frame - self.view_t0).max(0) as u64,
                tick: om.frame as u64,
                size: self.size,
                hud: self.hud.as_ref(),
            },
            &mut self.gunn_canvas,
            &mut |view| {
                if let Some(sa) = scene_art.as_mut() {
                    sa.apply(view, w, c);
                }
                Vec::new()
            },
        );
        let width = self.size.0 as f32;
        let shake = self.presenter.as_ref().map_or(0, |p| p.shake);
        let offset = crate::match_draw::shake_offset(om.frame as u64, shake > 0 || vui.shaking());
        // Tower card, P-tags, skulls, scores, round and match results: from
        // the confirmed HUD (never shows a point a rollback takes back);
        // not shaken.
        let (hud, pause, icons) = (&self.hud, &self.pause, &self.variant_icons);
        let tower = ui.towers.get(self.tower_ui);
        crate::match_draw::steady(ctx.list, offset, |list| {
            let mut p = Painter {
                list,
                a: ui,
                w: width,
            };
            if let Some(h) = hud {
                vui.draw_hud(&mut p, h, tower, icons, om.frame as u64);
            }
            if let Some(pz) = pause {
                pz.draw(&mut p, om.frame as u64);
            }
        });
        let mut p = Painter {
            list: ctx.list,
            a: ui,
            w: width,
        };
        ctx.present.wave = vui.wave().or_else(|| {
            // The orbs' eased slow motion only, not the final kill's.
            live.orb.game_rate_ease.then(|| {
                crate::coop_ui::screen_fx::slow_wave(live.time.time_rate, om.frame as f32 * 0.01)
            })?
        });
        ctx.present.offset = offset;
        // Network state.
        if let Some(n) = &self.net {
            let msg = if om.done.is_some() {
                None
            } else if n.is_spectator() {
                Some("SPECTATING".to_string())
            } else if !n.is_running() {
                Some(if self.status.is_empty() {
                    "WAITING FOR THE OTHER MACHINES...".into()
                } else {
                    self.status.clone()
                })
            } else {
                None
            };
            if let Some(m) = msg {
                p.text_outlined(
                    &m,
                    Vec2::new(width / 2.0, 226.0),
                    (0.5, 0.0),
                    1.0,
                    pal::YELLOW,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_typing() {
        assert_eq!(sanitize_name("Ada K."), "adak");
        assert_eq!(sanitize_name("x"), "archerx");
        assert_eq!(typed(Scancode::K), Some('K'));
        assert_eq!(typed(Scancode::Num7), Some('7'));
        assert_eq!(typed(Scancode::Space), None);
    }
}

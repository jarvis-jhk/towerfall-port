//! The application scene: the front end (menus) and, once the menus have
//! a match, the play mode built from their setup; Start in the match goes
//! back to the end-of-match menu.

use anyhow::Result;

use crate::app::{DrawContext, LoadContext, Scene, TickContext};
use crate::coop_ui::scene::{CoopOptions, CoopPlay};
use crate::front::FrontEnd;
use crate::front::ScreenId;
use crate::front::settings::FrontEndSetup;
use crate::online::scene::{OnlineOptions, OnlineScene};
use crate::play::{Play, PlayOptions};

pub struct Game {
    front: FrontEnd,
    play: Option<Play>,
    pending: Option<FrontEndSetup>,
    keyboard: bool,
    /// Finished matches whose GPU textures are freed at the next draw.
    retired: Vec<Play>,
    /// The main menu's ONLINE: the online scene (menus, lobby, matches).
    online: Option<OnlineScene>,
    online_pending: bool,
    /// Quest / Trials (`coop_ui`).
    coop: Option<CoopPlay>,
    pending_coop: Option<crate::coop_ui::CoopLaunch>,
    /// Testing (`flow.rs`): bots play the menus' matches.
    pub test_bots: bool,
}

/// Where the app is (`flow.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    Front(Option<ScreenId>),
    Versus(Option<tfp_sim::versus::Phase>),
    Coop,
    Online,
}

impl Game {
    /// A co-op game's pause menu is open.
    pub fn coop_paused(&self) -> bool {
        self.coop.as_ref().is_some_and(|c| c.paused())
    }

    /// The front end's screen has settled (takes input).
    pub fn front_settled(&self) -> bool {
        self.front.settled()
    }

    pub fn front_options(&self) -> &crate::front::Options {
        self.front.options()
    }

    /// Frees a running match's GPU textures (tests).
    pub fn release_all(&mut self, r: &mut tfp_render::Renderer) {
        for mut p in self.retired.drain(..).chain(self.play.take()) {
            p.release(r);
        }
    }

    /// The app's current place (front-end screen, match phase, ...).
    pub fn whereabouts(&self) -> Where {
        if self.online.is_some() {
            Where::Online
        } else if self.coop.is_some() || self.pending_coop.is_some() {
            Where::Coop
        } else if let Some(p) = &self.play {
            Where::Versus(p.phase())
        } else if self.pending.is_some() {
            Where::Versus(None)
        } else {
            Where::Front(self.front.screen())
        }
    }

    pub fn new(mut front: FrontEnd, keyboard: bool) -> Self {
        // Quest / trials records for the co-op maps come from the save.
        front.coop_session().records = crate::save::coop_records();
        Self {
            front,
            play: None,
            pending: None,
            keyboard,
            retired: Vec::new(),
            test_bots: false,
            online: None,
            online_pending: false,
            coop: None,
            pending_coop: None,
        }
    }
}

impl Scene for Game {
    fn game_size(&self) -> (u32, u32) {
        if let Some(o) = &self.online {
            return o.game_size();
        }
        if let Some(c) = &self.coop {
            return c.game_size();
        }
        match &self.play {
            Some(p) => p.game_size(),
            None => self.front.game_size(),
        }
    }

    fn load(&mut self, ctx: &mut LoadContext<'_>) -> Result<()> {
        self.front.load(ctx)
    }

    fn tick(&mut self, ctx: &mut TickContext<'_>) {
        if let Some(o) = &mut self.online {
            o.tick(ctx);
            if o.quit {
                leave_scene_audio(ctx.audio);
                self.online = None;
            }
            return;
        }
        if let Some(c) = &mut self.coop {
            c.tick(ctx);
            if let Some(exit) = c.exit {
                leave_scene_audio(ctx.audio);
                let records = c.records.clone();
                self.coop = None;
                crate::save::store_coop(&records);
                // Dark World runs went to the store as they ended; the
                // store stays the source of truth for the maps.
                self.front.coop_session().records = crate::save::coop_records();
                let _ = records;
                let mode = self.front.coop_session().mode;
                self.front.return_to(match exit {
                    crate::coop_ui::Exit::Map => match mode {
                        crate::coop_ui::RollcallMode::Quest => ScreenId::QuestMap,
                        crate::coop_ui::RollcallMode::DarkWorld => ScreenId::DarkWorldMap,
                        _ => ScreenId::TrialsMap,
                    },
                    crate::coop_ui::Exit::Quit => ScreenId::Main,
                });
            }
            return;
        }
        if let Some(p) = &mut self.play {
            p.tick(ctx);
            if p.menu_requested {
                leave_scene_audio(ctx.audio);
                self.retired.extend(self.play.take());
                self.front.return_from_match();
            } else if p.quit_requested {
                leave_scene_audio(ctx.audio);
                self.retired.extend(self.play.take());
                self.front.quit_from_match();
            }
            return;
        }
        self.front.tick(ctx);
        if self.front.take_online() {
            self.online_pending = true;
        }
        if let Some(s) = self.front.take_match() {
            println!("{}", crate::front::pause::summary(&s));
            self.pending = Some(s);
        }
        if let Some(l) = self.front.take_coop() {
            println!("co-op: {:?}, {} archers", l.mode, l.players.len());
            self.pending_coop = Some(l);
        }
    }

    fn draw(&mut self, ctx: &mut DrawContext<'_>) {
        for mut p in self.retired.drain(..) {
            p.release(ctx.renderer);
        }
        if std::mem::take(&mut self.online_pending) {
            let mut o = OnlineScene::new(OnlineOptions {
                embedded: true,
                keys: Some(self.front.options().keys.clone()),
                pads: self.front.options().pads.clone(),
                sfx_volume: Some(self.front.options().sounds as f32 / 10.0),
                rumble: Some(self.front.options().rumble),
                ..Default::default()
            });
            match o.init(ctx.gpu, ctx.renderer) {
                Ok(()) => self.online = Some(o),
                Err(e) => log::warn!("online: {e:#}"),
            }
        }
        if let Some(o) = &mut self.online {
            o.draw(ctx);
            return;
        }
        if let Some(launch) = self.pending_coop.take() {
            // The persistent records (save.rs) are the source of truth.
            let records = crate::save::coop_records();
            let mut c = CoopPlay::new(CoopOptions {
                launch,
                records,
                keyboard: self.keyboard,
                shots: None,
                start_screen: true,
                seed: ctx.tick as i32,
                hitboxes: false,
                end: None,
                music: self.front.music_enabled(),
                sfx_volume: self.front.options().sounds as f32 / 10.0,
                pads: self.front.options().pads.clone(),
                keys: self.front.options().keys.clone(),
                rumble: self.front.options().rumble,
            });
            c.show_tips = self.front.options().lessons;
            c.screen_wave = self.front.options().screen_wave;
            match c.init(ctx.gpu, ctx.renderer) {
                Ok(()) => self.coop = Some(c),
                Err(e) => {
                    log::warn!("starting the co-op game: {e:#}");
                    self.front.return_to(ScreenId::Main);
                }
            }
        }
        if let Some(c) = &mut self.coop {
            c.draw(ctx);
            return;
        }
        // Starting a match needs the GPU (level canvases), so it happens here.
        if let Some(setup) = self.pending.take() {
            let mut p = Play::new(PlayOptions {
                keyboard: self.keyboard,
                setup: Some(setup),
                music: self.front.music_enabled(),
                sfx_volume: self.front.options().sounds as f32 / 10.0,
                pads: self.front.options().pads.clone(),
                keys: self.front.options().keys.layouts.clone(),
                bots_for_humans: self.test_bots,
                hold_to_pause: self.front.options().hold_to_pause,
                screen_wave: self.front.options().screen_wave,
                rumble: self.front.options().rumble,
                ..Default::default()
            });
            match p.init(ctx.gpu, ctx.renderer) {
                Ok(()) => self.play = Some(p),
                Err(e) => log::warn!("starting the match: {e:#}"),
            }
        }
        match &mut self.play {
            Some(p) => p.draw(ctx),
            None => self.front.draw(ctx),
        }
    }
}

/// Leaving a match scene (audio.md 3, 6.3): every effect stops; the music
/// fades out over 1 s, so the menu's song crossfades in instead of a cut.
fn leave_scene_audio(audio: &mut crate::audio::Audio) {
    audio.stop_voices();
    audio.stop_music(1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::HostRequests;
    use crate::audio::Audio;
    use crate::front::FrontOptions;
    use crate::input::RawInput;
    use tfp_render::{DrawList, Gpu, PresentSettings, Renderer};

    #[test]
    fn menus_hand_the_match_to_play() {
        let Ok(gpu) = Gpu::headless() else { return };
        if tfp_assets::Installs::discover().steam.is_none() {
            eprintln!("skipping: no game data");
            return;
        }
        let mut renderer = Renderer::new(&gpu);
        let mut audio = Audio::offline(None).unwrap();
        let front = FrontEnd::new(FrontOptions {
            screen: Some(ScreenId::MatchReady),
            players: Some(4),
            music: false,
            ..Default::default()
        });
        let mut g = Game::new(front, false);
        g.load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
        let raw = RawInput::default();
        let mut host = HostRequests::default();
        let (mut list, mut overlay) = (DrawList::new(), DrawList::new());
        let mut present = PresentSettings::default();
        for tick in 0..120 {
            g.tick(&mut TickContext {
                input: &raw,
                audio: &mut audio,
                host: &mut host,
                tick,
            });
            list.clear();
            let size = g.game_size();
            g.draw(&mut DrawContext {
                list: &mut list,
                overlay: &mut overlay,
                present: &mut present,
                game_size: size,
                window: (960, 720),
                gpu: &gpu,
                renderer: &mut renderer,
                tick,
            });
        }
        let p = g.play.as_ref().expect("match started");
        assert_eq!(p.game_size(), (320, 240));
        assert!(!list.is_empty(), "the match draws");
        g.play.as_mut().unwrap().menu_requested = true;
        g.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick: 200,
        });
        assert!(g.play.is_none());
        assert_eq!(g.front.screen(), Some(ScreenId::MatchEnd));
    }
}

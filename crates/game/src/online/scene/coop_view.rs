//! Online Quest / Trials on screen, from the local co-op presentation's
//! parts (`coop_ui`: enemy art, debris, the Quest and Trials HUDs, the
//! Versus effects and particles) — fed the confirmed timeline only, like
//! `versus_ui` in online Versus: the HUD, debris, effects and particles
//! never show a frame a rollback rewrites; the arena and the actors are
//! drawn from the live (predicted) world.

use anyhow::{Context, Result};
use tfp_render::Color;
use tfp_render::glam::Vec2;
use tfp_sim::quest::QuestPhase;
use tfp_sim::trials::TrialsPhase;
use tfp_sim::{Content, World};

use crate::audio::Audio;
use crate::coop_ui::art::{self, Anims, Art};
use crate::coop_ui::darkworld::bosses::{BossFx, DwArt};
use crate::coop_ui::darkworld::hud::{Card, DwHud};
use crate::coop_ui::debris::Debris;
use crate::coop_ui::quest_hud::{self, QuestHud};
use crate::coop_ui::trials_hud::TrialsHud;
use crate::front::assets::UiAssets;
use crate::front::gfx::Painter;
use crate::music::MusicPlayer;
use crate::online::adapter::{KIND_DARKWORLD, KIND_QUEST, OnlineSettings};
use crate::online::coop::CoopData;
use crate::tower::{TowerGfx, TowerView};
use crate::versus_ui::VersusUi;
use tfp_sim::darkworld::DwPhase;

const MAXP: usize = tfp_sim::world::MAX_PLAYERS;

pub(super) struct CoopView {
    kind: u8,
    index: usize,
    art: Art,
    anims: Anims,
    debris: Debris,
    ui: VersusUi,
    qhud: Option<QuestHud>,
    thud: Option<TrialsHud>,
    dhud: Option<DwHud>,
    dw_art: DwArt,
    bfx: BossFx,
    /// Dark World: arena files and themes in `Content::levels` order.
    dw_levels: Vec<(std::path::PathBuf, String)>,
    /// The game-over menu's cursor (the host's; guests only see it).
    pub menu_sel: usize,
    slot_archers: Vec<Option<u16>>,
    view: Option<(usize, TowerView)>,
    pub size: (u32, u32),
    /// The newest confirmed world (the HUD's input).
    confirmed: Option<World>,
    gunn_canvas: crate::match_draw::GunnCanvas,
    theme_cue: String,
    music_now: String,
    boss_loaded: bool,
}

/// Tower title and icon of a quest tower / trial (as the local scene).
fn title(data: &CoopData, g: &TowerGfx, i: usize) -> (String, String) {
    let (theme, tier) = if data.kind == KIND_QUEST {
        (data.index.quest.get(i).map(|q| q.theme.clone()), 0)
    } else if data.kind == KIND_DARKWORLD {
        (data.index.dark_world.get(i).map(|t| t.theme.clone()), 0)
    } else {
        (
            data.index.trials.get(i).map(|t| t.theme.clone()),
            data.index.trials.get(i).map_or(0, |t| t.tier),
        )
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

impl CoopView {
    pub fn new(
        data: &CoopData,
        os: &OnlineSettings,
        ms: &tfp_net::MatchStart,
        world: &World,
        g: &TowerGfx,
        ua: &UiAssets,
        inst: &tfp_assets::GameInstall,
    ) -> CoopView {
        let i = os.coop_index as usize;
        let (name, icon) = title(data, g, i);
        let mut slot_archers = vec![None; MAXP];
        if let Some(q) = &world.quest {
            slot_archers = q.settings.slots.to_vec();
        } else if let Some(t) = &world.trials {
            slot_archers[0] = Some(t.settings.archer);
        }
        let mut dhud = None;
        if data.kind == KIND_DARKWORLD {
            let theme = data
                .index
                .dark_world
                .get(i)
                .map(|t| t.theme.clone())
                .unwrap_or_default();
            let cata =
                g.la.game_data
                    .theme(&theme)
                    .is_some_and(|t| t.tower_type.eq_ignore_ascii_case("Cataclysm"));
            let block = if cata {
                "vortexLevelBlock"
            } else {
                "darkLevelBlock"
            };
            let cards = ms
                .players
                .iter()
                .map(|p| Card {
                    slot: p.slot as usize,
                    archer: p.archer as usize,
                    alt: p.skin == tfp_net::SKIN_ALT,
                })
                .collect();
            let icons = world
                .dark_world
                .as_ref()
                .map(|d| {
                    tfp_sim::variants::Variant::ALL
                        .iter()
                        .filter(|&&v| d.settings.variants.any(v))
                        .map(|&v| crate::coop_ui::darkworld::variant_ui(v).1)
                        .filter(|i| !i.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            dhud = Some(DwHud::new(
                name.clone(),
                block.into(),
                icon.clone(),
                !cata,
                cards,
                icons,
            ));
            if let Some(d) = &world.dark_world {
                slot_archers = d.settings.slots.to_vec();
            }
        }
        let (qhud, thud) = if data.kind == KIND_DARKWORLD {
            (None, None)
        } else if data.kind == KIND_QUEST {
            let gems = ms
                .players
                .iter()
                .map(|p| (p.slot as usize, p.archer as usize))
                .collect();
            let mut h = QuestHud::new(name, icon, gems);
            h.tips = quest_hud::load_tips(inst);
            (Some(h), None)
        } else {
            (None, Some(TrialsHud::new(name, icon, 0, [false; 3])))
        };
        let mut looks = ua.archers.clone();
        looks.extend(ua.archers.iter().filter_map(|a| a.alt.as_deref().cloned()));
        let theme = if data.kind == KIND_QUEST {
            data.index.quest.get(i).map(|q| q.theme.clone())
        } else if data.kind == KIND_DARKWORLD {
            data.index.dark_world.get(i).map(|t| t.theme.clone())
        } else {
            data.index.trials.get(i).map(|t| t.theme.clone())
        };
        CoopView {
            kind: data.kind,
            index: i,
            art: data.art.clone(),
            anims: Anims::default(),
            debris: Debris::default(),
            ui: VersusUi::new(looks),
            qhud,
            thud,
            dhud,
            dw_art: data.dw_art.clone(),
            bfx: BossFx::default(),
            dw_levels: data
                .index
                .dark_world
                .iter()
                .flat_map(|t| t.levels.iter().map(|l| (l.clone(), t.theme.clone())))
                .collect(),
            menu_sel: 0,
            slot_archers,
            view: None,
            size: (320, 240),
            confirmed: None,
            gunn_canvas: Default::default(),
            theme_cue: theme
                .and_then(|t| g.la.game_data.theme(&t))
                .map_or(String::new(), |t| t.music.clone()),
            music_now: String::new(),
            boss_loaded: false,
        }
    }

    /// The newest confirmed world.
    pub fn confirmed(&self) -> Option<&World> {
        self.confirmed.as_ref()
    }

    /// One confirmed frame (in order, each once).
    pub fn after_confirmed(&mut self, w: World, c: &Content) {
        let mut bursts = Vec::new();
        self.debris.after_step(&w, c, &mut bursts);
        if w.dark_world.is_some() {
            bursts.extend(self.bfx.after_step(&w, c));
        }
        self.ui.after_step(&w, None);
        self.ui.post_step(&w, c);
        for (name, n, pos) in bursts {
            if let Some(t) = self.ui.particles.by_name(name) {
                self.ui
                    .particles
                    .emit(t, n, Vec2::new(pos.x, pos.y), Vec2::splat(4.0), None);
            }
        }
        let width = self.size.0 as f32;
        if let Some(h) = &mut self.qhud {
            h.after_step(&w, width);
        }
        if let Some(h) = &mut self.thud {
            h.after_step(&w);
        }
        if let Some(h) = &mut self.dhud {
            h.after_step(&w);
        }
        self.confirmed = Some(w);
    }

    /// The presentation's sound cues (results, replay) since the last call.
    pub fn take_cues(&mut self) -> Vec<crate::versus_ui::Cue> {
        std::mem::take(&mut self.ui.cues)
    }

    /// Tower music by the confirmed phase (theme while playing, the boss
    /// song while the King Reaper is out, the game-over cue).
    pub fn music(&mut self, audio: &mut Audio, m: &mut MusicPlayer) {
        let Some(w) = &self.confirmed else { return };
        let want = if let Some(q) = &w.quest {
            match q.phase {
                QuestPhase::Intro | QuestPhase::Playing | QuestPhase::Ending => {
                    let reaper = w.tagged(tfp_sim::Tag::Enemy).iter().any(|&e| {
                        matches!(w.kind(e), Some(tfp_sim::entity::Kind::Enemy(en))
                            if en.ty == tfp_sim::enemies::EnemyType::KingReaper)
                    });
                    if reaper {
                        "Boss".to_string()
                    } else {
                        self.theme_cue.clone()
                    }
                }
                QuestPhase::GameOver if q.phase_t >= 211.0 => "GameOver".into(),
                _ => String::new(),
            }
        } else if let Some(t) = &w.trials {
            match t.phase {
                TrialsPhase::Intro | TrialsPhase::Playing => self.theme_cue.clone(),
                _ => String::new(),
            }
        } else if let Some(d) = &w.dark_world {
            match d.phase {
                DwPhase::Title | DwPhase::Intro | DwPhase::Playing | DwPhase::Exit
                    if d.boss.is_some() && !d.boss_finished =>
                {
                    "DarkBoss".into()
                }
                DwPhase::Title | DwPhase::Intro | DwPhase::Playing | DwPhase::Exit => {
                    self.theme_cue.clone()
                }
                DwPhase::Transition | DwPhase::Tips => self.music_now.clone(),
                _ => String::new(),
            }
        } else {
            String::new()
        };
        if want != self.music_now {
            if want.is_empty() {
                m.stop(audio);
            } else {
                m.play(audio, &want);
            }
            self.music_now = want;
        }
    }

    fn ensure_view(
        &mut self,
        gpu: &tfp_render::Gpu,
        r: &mut tfp_render::Renderer,
        g: &TowerGfx,
        data: &CoopData,
        level: usize,
        c: &Content,
    ) -> Result<()> {
        if self.view.as_ref().is_some_and(|(l, _)| *l == level) {
            return Ok(());
        }
        let (path, theme) = if self.kind == KIND_QUEST {
            let q = data.index.quest.get(level).context("no quest tower")?;
            (q.level.clone(), q.theme.clone())
        } else if self.kind == KIND_DARKWORLD {
            self.dw_levels
                .get(level)
                .cloned()
                .context("no dark world arena")?
        } else {
            let t = data.index.trials.get(level).context("no trial")?;
            (t.path.clone(), t.theme.clone())
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
            c.level(level).load_seed,
            &tfp_assets::look::LookOptions::default(),
        )?;
        if let Some((_, old)) = self.view.take() {
            old.destroy(r);
        }
        self.size = (look.width, look.height);
        self.view = Some((level, TowerView::new(gpu, r, g, look)?));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    /// Archer / corpse / arrow cosmetics (art registered in the content).
    pub fn attach_actors(&mut self, art: crate::versus_ui::actors::ArcherArt) {
        crate::versus_ui::actors::attach(&mut self.ui, art);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ctx: &mut crate::app::DrawContext<'_>,
        g: &mut TowerGfx,
        data: &CoopData,
        live: &World,
        c: &Content,
        ua: &UiAssets,
        frame: u64,
        host: bool,
    ) {
        if !self.boss_loaded {
            self.boss_loaded = true;
            if let Err(e) = g.load_boss(ctx.gpu, ctx.renderer) {
                log::warn!("boss atlas: {e:#}");
            }
        }
        if let Err(e) = self.ensure_view(ctx.gpu, ctx.renderer, g, data, live.level, c) {
            log::warn!("co-op level view: {e:#}");
            return;
        }
        let _ = self.index;
        let Some((_, v)) = &mut self.view else { return };
        // The shared match drawing (match_draw.rs) with the co-op art (and
        // the Dark World boss art, as in the local scene).
        let (art, slot_archers, anims, debris) =
            (&self.art, &self.slot_archers, &mut self.anims, &self.debris);
        let (bfx, dw_art) = (&mut self.bfx, &self.dw_art);
        let g: &TowerGfx = g;
        crate::match_draw::draw_level(
            ctx,
            v,
            &crate::match_draw::LevelDraw {
                world: live,
                content: c,
                gfx: g,
                ui: &self.ui,
                ui_assets: ua,
                hitboxes: false,
                level_t: frame,
                tick: frame,
                size: self.size,
                hud: None,
            },
            &mut self.gunn_canvas,
            &mut |view| {
                let cx = art::Ctx {
                    c,
                    art,
                    slot_archers,
                    tick: frame,
                };
                let mut extra = art::apply(view, live, &cx, anims);
                if live.dark_world.is_some() {
                    extra.extend(bfx.apply(view, live, c, dw_art));
                }
                debris.draw(view, c, art);
                extra
            },
        );
        let ui = &self.ui;
        let mut p = Painter {
            list: ctx.list,
            a: ua,
            w: self.size.0 as f32,
        };
        let w = self.confirmed.as_ref().unwrap_or(live);
        if let Some(h) = &self.qhud {
            h.draw(&mut p, w, c, &self.art, frame);
        }
        if let Some(h) = &self.thud {
            h.draw(&mut p, w);
        }
        if let Some(h) = &self.dhud {
            h.draw(&mut p, w, c, self.bfx.screen_flash, frame);
        }
        if let Some(items) = menu_items(w) {
            draw_menu(&mut p, &items, self.menu_sel, host, frame);
        }
        ctx.present.offset = if ui.shaking() {
            let k = frame.wrapping_mul(2_654_435_761);
            let pick = |bit: u64| if (k >> bit) & 1 == 0 { -2.0 } else { 2.0 };
            Vec2::new(pick(5), pick(11))
        } else {
            Vec2::ZERO
        };
        let _ = Color::WHITE;
    }
}

/// A Dark World game-over menu row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuItem {
    Continue(i32),
    Restart,
    Map,
    Quit,
}

impl MenuItem {
    fn label(self) -> String {
        match self {
            MenuItem::Continue(n) if n < 0 => "CONTINUE".into(),
            MenuItem::Continue(n) => format!("CONTINUE ({n})"),
            MenuItem::Restart => "RESTART".into(),
            MenuItem::Map => "MAP".into(),
            MenuItem::Quit => "QUIT".into(),
        }
    }
}

/// The Dark World game-over menu of a (confirmed) world, if it is up
/// (ui.md 10.4: [CONTINUE (n)], RESTART, MAP, QUIT). Online the host
/// chooses; everybody else sees it.
pub(super) fn menu_items(w: &World) -> Option<Vec<MenuItem>> {
    let d = w.dark_world.as_ref()?;
    if d.phase != DwPhase::GameOver || !d.menu_ready {
        return None;
    }
    let mut v = vec![MenuItem::Restart, MenuItem::Map, MenuItem::Quit];
    if d.can_continue() {
        v.insert(0, MenuItem::Continue(d.continues_remaining()));
    }
    Some(v)
}

fn draw_menu(p: &mut Painter, items: &[MenuItem], sel: usize, host: bool, tick: u64) {
    use crate::front::gfx::pal;
    let centre = Vec2::new(p.w / 2.0, 200.0);
    let n = items.len() as f32;
    let ph = 10.0 * n + 30.0;
    p.panel(centre.x - 60.0, centre.y - ph / 2.0, 120.0, ph);
    let y0 = centre.y - ((n - 1.0) * 14.0 + 39.0) / 2.0 + 20.0;
    for (i, a) in items.iter().enumerate() {
        let y = y0 + 14.0 * i as f32;
        if host && i == sel {
            let col = if (tick / 4).is_multiple_of(2) {
                pal::PAUSE_A
            } else {
                pal::PAUSE_B
            };
            p.text(
                &format!("> {}", a.label()),
                Vec2::new(centre.x, y),
                (0.5, 0.5),
                1.0,
                col,
            );
        } else {
            p.text(
                &a.label(),
                Vec2::new(centre.x, y),
                (0.5, 0.5),
                1.0,
                pal::PAUSE_IDLE,
            );
        }
    }
    if !host {
        p.text_outlined(
            "THE HOST DECIDES",
            Vec2::new(centre.x, centre.y - ph / 2.0 - 8.0),
            (0.5, 0.5),
            1.0,
            pal::YELLOW,
        );
    }
}

//! Co-op presentation tests (need a GPU adapter and the game data; they
//! skip otherwise): every enemy family, portal, hazard and dummy draws
//! from the atlas (no debug boxes left), with a gallery image
//! `scratch/enemy-gallery.png`; the menus produce a launch; a quest
//! attempt reaches its result screen and the end menu.

use super::scene::{CoopOptions, CoopPlay, cli_options};
use super::*;
use crate::app::{DrawContext, HostRequests, LoadContext, Scene, TickContext};
use crate::audio::Audio;
use crate::input::RawInput;
use tfp_render::{DrawList, Gpu, PresentSettings, Renderer};
use tfp_sim::enemies::{EnemyType, SkelKind};
use tfp_sim::quiver::ArrowType;
use tfp_sim::{Vec2, Visual};

fn env() -> Option<(Gpu, Renderer, Audio)> {
    let gpu = Gpu::headless().ok()?;
    if tfp_assets::Installs::discover().steam.is_none() {
        eprintln!("skipping: no game data");
        return None;
    }
    let renderer = Renderer::new(&gpu);
    let audio = Audio::offline(None).ok()?;
    Some((gpu, renderer, audio))
}

fn skel(arrow: ArrowType, shield: bool, wings: bool, mimic: bool, jester: bool) -> EnemyType {
    EnemyType::Skeleton(SkelKind {
        arrow,
        shield,
        wings,
        mimic,
        jester,
        boss: false,
    })
}

/// Draws one frame of `scene` into a canvas and reads it back.
fn render(
    scene: &mut CoopPlay,
    gpu: &Gpu,
    renderer: &mut Renderer,
    tick: u64,
) -> tfp_render::Image {
    let size = scene.game_size();
    let canvas = renderer.create_canvas(gpu, size);
    let (mut list, mut overlay) = (DrawList::new(), DrawList::new());
    let mut present = PresentSettings::default();
    scene.draw(&mut DrawContext {
        list: &mut list,
        overlay: &mut overlay,
        present: &mut present,
        game_size: size,
        window: (960, 720),
        gpu,
        renderer,
        tick,
    });
    renderer.render(gpu, &list, canvas).unwrap();
    let img = renderer.capture(gpu, canvas).unwrap();
    renderer.destroy_texture(canvas);
    img
}

#[test]
fn every_enemy_draws_from_the_atlas() {
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let mut o = cli_options(true, Some("0"), 0, false, 1, Some(3), None, false).unwrap();
    o.start_screen = false;
    let mut scene = CoopPlay::new(o);
    scene
        .load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
    use EnemyType::*;
    let types = [
        Slime,
        RedSlime,
        BlueSlime,
        Bat,
        BombBat,
        SuperBombBat,
        Crow,
        Ghost,
        GreenGhost,
        Elemental,
        GreenElemental,
        Cultist,
        ScytheCultist,
        BossCultist,
        Birdman,
        DarkBirdman,
        skel(ArrowType::Normal, false, false, false, false),
        skel(ArrowType::Normal, true, false, false, false),
        skel(ArrowType::Bomb, false, true, false, false),
        skel(ArrowType::Normal, false, false, true, false),
        skel(ArrowType::Normal, true, false, false, true),
        Mole,
        Worm,
        FlamingSkull,
        Exploder,
        EvilCrystal,
        BlueCrystal,
        BoltCrystal,
        PrismCrystal,
        TechnoMage,
        KingReaper,
    ];
    let (w, c) = scene.world_content_mut();
    let mut ids = Vec::new();
    for (i, &t) in types.iter().enumerate() {
        let pos = Vec2::new(24.0 + 40.0 * (i % 8) as f32, 40.0 + 50.0 * (i / 8) as f32);
        ids.push((t, tfp_sim::enemies::spawn(w, c, t, pos, 1, &[])));
    }
    let owner = ids.last().unwrap().1;
    let hz = [
        tfp_sim::enemies::hazard::spawn_tornado(
            w,
            Vec2::new(40.0, 225.0),
            Vec2::new(1.0, 0.0),
            None,
        ),
        tfp_sim::enemies::hazard::spawn_missile(w, Vec2::new(80.0, 225.0), Vec2::new(1.0, 0.0)),
        tfp_sim::enemies::hazard::spawn_bomb(
            w,
            Vec2::new(120.0, 225.0),
            Vec2::new(0.0, 0.0),
            owner,
        ),
        tfp_sim::enemies::hazard::spawn_crystal(w, owner, 0.0, false),
    ];
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    let n: u64 = std::env::var("TFP_GALLERY_TICKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    for tick in 0..n {
        scene.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
    }
    scene.world_content_mut().0.set_frozen(true);
    let img = render(&mut scene, &gpu, &mut renderer, 3);
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scratch");
    if out.is_dir() {
        let _ = img.save_png(out.join("enemy-gallery.png"));
    }
    // Every enemy and hazard is drawn with atlas art.
    let view = scene.frame_view();
    for (t, id) in &ids {
        if !scene.world_content_mut().0.exists(*id) {
            continue;
        }
        let items: Vec<_> = view.items.iter().filter(|d| d.entity == *id).collect();
        assert!(!items.is_empty(), "{t:?} draws nothing");
        assert!(
            items
                .iter()
                .all(|d| matches!(d.visual, Visual::Sprite { .. })),
            "{t:?} still a debug box"
        );
    }
    for id in hz {
        if !scene.world_content_mut().0.exists(id) {
            continue;
        }
        assert!(
            view.items
                .iter()
                .filter(|d| d.entity == id)
                .all(|d| matches!(d.visual, Visual::Sprite { .. })),
            "hazard still a debug box"
        );
    }
}

#[test]
fn quest_reaches_game_over_menu() {
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let mut o: CoopOptions =
        cli_options(true, Some("0"), 0, false, 2, Some(5), None, false).unwrap();
    o.end = Some((false, 400));
    let mut scene = CoopPlay::new(o);
    scene
        .load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    let mut tick = 0;
    while scene.exit.is_none() && tick < 3000 {
        scene.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
        tick += 1;
    }
    let q = scene.quest().unwrap();
    assert_eq!(q.phase, tfp_sim::quest::QuestPhase::GameOver);
    assert!(q.ready);
    // Without humans the end menu picks MAP by itself.
    assert_eq!(scene.exit, Some(Exit::Map));
    let rec = scene.records.quest[&0];
    assert_eq!(rec.attempts, 1);
    assert!(!rec.normal);
    let _ = render(&mut scene, &gpu, &mut renderer, tick);
}

#[test]
fn trials_bot_completes_twilight_spire() {
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let o = cli_options(false, Some("1"), 0, false, 1, Some(5), None, false).unwrap();
    let mut scene = CoopPlay::new(o);
    scene
        .load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    let mut tick = 0;
    while scene.exit.is_none() && tick < 4000 {
        scene.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
        tick += 1;
    }
    let level = match scene.mode() {
        CoopMode::Trials { level } => level,
        m => panic!("{m:?}"),
    };
    let rec = scene.records.trials[&level];
    assert!(rec.best > 0, "the bot cleared the trial");
    assert_eq!(scene.exit, Some(Exit::Map));
}

/// Keyboard through the menus: main -> CO-OP -> QUEST -> rollcall (join,
/// start) -> quest map -> HARDCORE -> zoom -> a launch for the game scene;
/// then TRIALS -> rollcall -> trials grid (Up = tier II) -> launch.
#[test]
fn menus_launch_quest_and_trials() {
    use crate::front::{FrontEnd, FrontOptions, ScreenId};
    use crate::input::InputHub;
    use sdl2::keyboard::Scancode;
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let mut front = FrontEnd::new(FrontOptions {
        screen: Some(ScreenId::Main),
        music: false,
        ..Default::default()
    });
    front
        .load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
    let mut hub = InputHub::new();
    let mut raw = RawInput::default();
    let mut host = HostRequests::default();
    let mut tick = 0u64;
    let mut run = |front: &mut FrontEnd, key: Option<Scancode>, wait: u32, hub: &mut InputHub| {
        for k in 0..wait.max(2) {
            if let Some(s) = key {
                hub.key(s as usize, k == 0);
            }
            hub.snapshot(&mut raw);
            front.tick(&mut TickContext {
                input: &raw,
                audio: &mut audio,
                host: &mut host,
                tick,
            });
            tick += 1;
        }
    };
    run(&mut front, None, 30, &mut hub);
    run(&mut front, Some(Scancode::Right), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 70, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::CoOp));
    run(&mut front, None, 25, &mut hub);
    run(&mut front, Some(Scancode::C), 70, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::Rollcall));
    run(&mut front, None, 25, &mut hub);
    run(&mut front, Some(Scancode::C), 10, &mut hub);
    run(&mut front, Some(Scancode::C), 30, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::QuestMap));
    run(&mut front, None, 30, &mut hub);
    run(&mut front, Some(Scancode::C), 5, &mut hub);
    run(&mut front, Some(Scancode::Down), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 90, &mut hub);
    let l = front.take_coop().expect("a quest launch");
    assert_eq!(
        l.mode,
        CoopMode::Quest {
            tower: 0,
            hardcore: true
        }
    );
    assert_eq!(l.players.len(), 1);
    assert!(front.take_coop().is_none(), "taken once");
    // Back to the main menu, then TRIALS.
    front.return_to(ScreenId::Main);
    run(&mut front, None, 30, &mut hub);
    run(&mut front, Some(Scancode::Down), 5, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::Main));
    // CO-OP was selected (main_from); Down = ARCHIVES, Left = TRIALS.
    run(&mut front, Some(Scancode::Left), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 10, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::Rollcall));
    run(&mut front, None, 25, &mut hub);
    run(&mut front, Some(Scancode::C), 60, &mut hub);
    assert_eq!(
        front.screen(),
        Some(ScreenId::TrialsMap),
        "one archer starts by itself"
    );
    run(&mut front, None, 30, &mut hub);
    run(&mut front, Some(Scancode::Up), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 90, &mut hub);
    let l = front.take_coop().expect("a trials launch");
    let CoopMode::Trials { level } = l.mode else {
        panic!("{:?}", l.mode)
    };
    let idx =
        tfp_assets::towers::TowerIndex::build(&tfp_assets::Installs::discover().steam.unwrap())
            .unwrap();
    assert_eq!((idx.trials[level].index, idx.trials[level].tier), (0, 1));
}

/// Trials gems: a found gem goes into the level's settings (its chest
/// starts empty); a collected one is kept in the records and the save
/// store at once.
#[test]
fn trials_gems_feed_and_store() {
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let mut o = cli_options(false, Some("1"), 0, false, 1, Some(5), None, false).unwrap();
    let level = match o.launch.mode {
        CoopMode::Trials { level } => level,
        m => panic!("{m:?}"),
    };
    o.records.trials.insert(
        level,
        TrialRecord {
            gem: true,
            ..Default::default()
        },
    );
    let mut scene = CoopPlay::new(o);
    scene
        .load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
    assert!(
        scene.trial().unwrap().settings.gem_found,
        "fed from the records"
    );
    // Another level: collect its gem.
    let mut o = cli_options(false, Some("2"), 1, false, 1, Some(5), None, false).unwrap();
    o.records = Records::default();
    let level = match o.launch.mode {
        CoopMode::Trials { level } => level,
        m => panic!("{m:?}"),
    };
    let mut scene = CoopPlay::new(o);
    scene
        .load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
    assert!(!scene.trial().unwrap().settings.gem_found);
    scene
        .world_content_mut()
        .0
        .trials
        .as_mut()
        .unwrap()
        .gem_collected = true;
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    for tick in 0..3 {
        scene.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
    }
    assert!(scene.records.trials[&level].gem);
    assert!(crate::save::snapshot().0.trials[&level].gem, "in the store");
    let _ = render(&mut scene, &gpu, &mut renderer, 3);
}

/// Quest floor miasma (Twilight Spire's strips, every group switched on)
/// draws its sprites, not debug boxes (`scratch/quest-floor.png`).
#[test]
fn quest_floor_miasma_draws() {
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let mut o = cli_options(true, Some("1"), 0, false, 1, Some(3), None, false).unwrap();
    o.start_screen = false;
    let mut scene = CoopPlay::new(o);
    scene
        .load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    for tick in 0..200 {
        if tick == 100 {
            tfp_sim::quest::floor::start_groups(scene.world_content_mut().0, &[0, 1, 2, 3]);
        }
        scene.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
    }
    let view = scene.frame_view();
    let w = scene.world_content_mut().0;
    let strips: Vec<_> = w
        .entities()
        .filter(|&e| matches!(w.kind(e), Some(tfp_sim::entity::Kind::FloorMiasma(_))))
        .collect();
    assert!(!strips.is_empty(), "Twilight Spire has floor miasma");
    let mut sprites = 0;
    for e in &strips {
        for d in view.items.iter().filter(|d| d.entity == *e) {
            assert!(matches!(d.visual, Visual::Sprite { .. }), "debug box left");
            sprites += 1;
        }
    }
    assert!(sprites > strips.len(), "one sprite per 10 px");
    scene.world_content_mut().0.set_frozen(true);
    let img = render(&mut scene, &gpu, &mut renderer, 200);
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scratch");
    if out.is_dir() {
        let _ = img.save_png(out.join("quest-floor.png"));
    }
}

/// King's Court: the first Reaper wave transforms the intro King (the
/// NPC is hidden from then on, the spotlight is on the Reaper's intro);
/// the Reaper's death drops the crown and leaves its head
/// (`scratch/quest-reaper-*.png`).
#[test]
fn kings_court_reaper_intro_and_death() {
    use tfp_sim::enemies::EnemyType;
    use tfp_sim::entity::Kind;
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let mut o = cli_options(true, Some("7"), 0, false, 1, Some(3), None, false).unwrap();
    o.start_screen = false;
    let mut scene = CoopPlay::new(o);
    scene
        .load(&mut LoadContext {
            gpu: &gpu,
            renderer: &mut renderer,
            audio: &mut audio,
        })
        .unwrap();
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scratch");
    let shot = |scene: &mut CoopPlay, renderer: &mut Renderer, name: &str, tick: u64| {
        let img = render(scene, &gpu, renderer, tick);
        if out.is_dir() {
            let _ = img.save_png(out.join(format!("quest-reaper-{name}.png")));
        }
    };
    let mut tick = 0u64;
    let mut step = |scene: &mut CoopPlay, audio: &mut Audio, n: u64, tick: &mut u64| {
        for _ in 0..n {
            scene.tick(&mut TickContext {
                input: &raw,
                audio,
                host: &mut host,
                tick: *tick,
            });
            *tick += 1;
        }
    };
    step(&mut scene, &mut audio, 200, &mut tick);
    let king = scene.world_content_mut().0.tower.king;
    if king.is_none() {
        eprintln!("skipping: no intro King in quest tower 7");
        return;
    }
    // Jump the program to the last group with a Reaper (groups expand
    // when they run): the final wave, where its death drops the crown
    // (quest.md §6); the King still transforms (first Reaper here).
    {
        let (w, c) = scene.world_content_mut();
        let q = w.quest.as_deref_mut().unwrap();
        let script = &c.quest_scripts[q.settings.script];
        let at = q
            .prog
            .iter()
            .rposition(|o| match o {
                tfp_sim::quest::Op::Group(wi, gi) => script
                    .normal
                    .get(*wi as usize)
                    .and_then(|wv| wv.groups.get(*gi as usize))
                    .is_some_and(|g| g.reaper >= 0),
                _ => false,
            })
            .expect("a reaper wave");
        q.pc = at;
    }
    for _ in 0..400 {
        step(&mut scene, &mut audio, 1, &mut tick);
        if scene.quest().unwrap().king_transformed() {
            break;
        }
    }
    assert!(scene.quest().unwrap().king_transformed());
    // 60 ticks, then the transform animation: the King stays visible
    // (spotlit) until the Reaper comes out of it at frame 16.
    step(&mut scene, &mut audio, 100, &mut tick);
    let reaper_out = |scene: &mut CoopPlay| {
        let w = scene.world_content_mut().0;
        w.entities()
            .any(|e| matches!(w.kind(e), Some(Kind::Enemy(en)) if en.ty == EnemyType::KingReaper))
    };
    assert!(
        !reaper_out(&mut scene),
        "the Reaper waits for the transform"
    );
    let view = scene.frame_view();
    assert!(
        view.items.iter().any(|d| Some(d.entity) == king),
        "the King's transform frames show"
    );
    shot(&mut scene, &mut renderer, "transform", tick);
    for _ in 0..120 {
        if reaper_out(&mut scene) {
            break;
        }
        step(&mut scene, &mut audio, 1, &mut tick);
    }
    assert!(reaper_out(&mut scene), "the Reaper appears from the King");
    step(&mut scene, &mut audio, 30, &mut tick);
    let view = scene.frame_view();
    assert!(
        !view.items.iter().any(|d| Some(d.entity) == king),
        "the King NPC is hidden once the Reaper is out"
    );
    shot(&mut scene, &mut renderer, "intro", tick);
    step(&mut scene, &mut audio, 300, &mut tick);
    shot(&mut scene, &mut renderer, "fight", tick);
    // Kill it: the death sequence, the crown, the head.
    {
        let (w, c) = scene.world_content_mut();
        let none = tfp_sim::EntityId {
            index: u32::MAX,
            generation: 0,
        };
        tfp_sim::enemies::kill_all(w, c, none);
    }
    let mut gone = false;
    for _ in 0..900 {
        step(&mut scene, &mut audio, 1, &mut tick);
        let w = scene.world_content_mut().0;
        let alive =
            w.tagged(tfp_sim::Tag::Enemy).iter().any(
                |&e| matches!(w.kind(e), Some(Kind::Enemy(en)) if en.ty == EnemyType::KingReaper),
            ) || w.entities().any(
                |e| matches!(w.kind(e), Some(Kind::Enemy(en)) if en.ty == EnemyType::KingReaper),
            );
        if !alive {
            gone = true;
            break;
        }
    }
    assert!(gone, "the Reaper's death sequence ends");
    step(&mut scene, &mut audio, 90, &mut tick);
    let view = scene.frame_view();
    let w = scene.world_content_mut().0;
    assert!(
        w.entities()
            .any(|e| matches!(w.kind(e), Some(Kind::Hat(_)))),
        "the crown dropped"
    );
    let _ = view;
    shot(&mut scene, &mut renderer, "death", tick);
}

#[test]
fn rollcall_limits() {
    assert_eq!(RollcallMode::Trials.limits(4), (1, 1));
    assert_eq!(RollcallMode::Quest.limits(4), (1, 4));
    assert_eq!(RollcallMode::Versus.limits(8), (2, 8));
    use crate::front::settings::Slot;
    let slots: Vec<Slot> = (0..4)
        .map(|i| Slot {
            joined: i != 1,
            archer: i,
            ..Default::default()
        })
        .collect();
    let p = players_from_slots(&slots, 2);
    assert_eq!(p.iter().map(|p| p.slot).collect::<Vec<_>>(), vec![0, 2]);
}

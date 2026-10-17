//! Dark World presentation tests (need a GPU adapter and the Dark World
//! content; they skip otherwise): every boss, part and projectile draws
//! from the atlas (no debug boxes), the menus produce a Dark World launch
//! with the chosen difficulty and power-up, game over offers a continue and
//! a Legendary clear writes the session records.

use crate::app::{DrawContext, HostRequests, LoadContext, Scene, TickContext};
use crate::audio::Audio;
use crate::coop_ui::scene::{CoopPlay, DwProof, cli_options_dw};
use crate::coop_ui::{CoopMode, Exit};
use crate::input::RawInput;
use tfp_render::{DrawList, Gpu, PresentSettings, Renderer};
use tfp_sim::entity::Kind;
use tfp_sim::variants::{Variant, VariantSet};
use tfp_sim::{EntityId, Visual};

fn env() -> Option<(Gpu, Renderer, Audio)> {
    let gpu = Gpu::headless().ok()?;
    match tfp_assets::Installs::discover().steam {
        Some(i) if i.dark_world.is_some() => {}
        _ => {
            eprintln!("skipping: no Dark World content");
            return None;
        }
    }
    let renderer = Renderer::new(&gpu);
    let audio = Audio::offline(None).ok()?;
    Some((gpu, renderer, audio))
}

fn scene(
    tower: usize,
    difficulty: u8,
    players: usize,
    gpu: &Gpu,
    r: &mut Renderer,
    a: &mut Audio,
) -> CoopPlay {
    let mut o = cli_options_dw(
        Some(&tower.to_string()),
        difficulty,
        players,
        Some(7),
        None,
        false,
        VariantSet::default(),
    )
    .unwrap();
    o.music = false;
    o.start_screen = false;
    let mut s = CoopPlay::new(o);
    s.dw_proof = DwProof {
        level: Some(99),
        assist: Some(25),
    };
    s.load(&mut LoadContext {
        gpu,
        renderer: r,
        audio: a,
    })
    .unwrap();
    s
}

fn draw(s: &mut CoopPlay, gpu: &Gpu, renderer: &mut Renderer, tick: u64) {
    let size = s.game_size();
    let (mut list, mut overlay) = (DrawList::new(), DrawList::new());
    let mut present = PresentSettings::default();
    s.draw(&mut DrawContext {
        list: &mut list,
        overlay: &mut overlay,
        present: &mut present,
        game_size: size,
        window: (960, 720),
        gpu,
        renderer,
        tick,
    });
}

/// Each boss fought with the proof assist: every 15 ticks, every Dark
/// World entity (bodies, eyes, tentacles, fists, shots, bullets, blades,
/// orbs, blocks, the bones, the pedestal) is drawn with atlas art only.
#[test]
fn every_boss_draws_from_the_atlas() {
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    for tower in 0..4 {
        let mut s = scene(tower, 0, 1, &gpu, &mut renderer, &mut audio);
        let mut kinds = std::collections::BTreeSet::new();
        let mut phases = std::collections::BTreeSet::new();
        for tick in 0..3600u64 {
            s.tick(&mut TickContext {
                input: &raw,
                audio: &mut audio,
                host: &mut host,
                tick,
            });
            if tick % 15 != 0 {
                continue;
            }
            let view = s.frame_view();
            let (w, _) = s.world_content_mut();
            if let Some(p) = tfp_sim::darkworld::boss_phase(w) {
                phases.insert(p);
            }
            let dw: Vec<(EntityId, String, bool)> = w
                .entities()
                .filter_map(|e| match w.kind(e) {
                    Some(Kind::Dw(p)) => {
                        let name = format!("{p:?}");
                        let name = name.split([' ', '(', '{']).next().unwrap_or("").to_string();
                        Some((e, name, w.core(e).visible))
                    }
                    _ => None,
                })
                .collect();
            for (e, name, visible) in dw {
                let items: Vec<_> = view.items.iter().filter(|d| d.entity == e).collect();
                assert!(
                    items
                        .iter()
                        .all(|d| matches!(d.visual, Visual::Sprite { .. })),
                    "tower {tower}: {name} still a debug box"
                );
                if visible && name != "Tentacle" {
                    kinds.insert(name.clone());
                }
            }
            if tick % 600 == 0 {
                draw(&mut s, &gpu, &mut renderer, tick);
            }
        }
        eprintln!("tower {tower}: parts {kinds:?}, phases {phases:?}");
        assert!(
            kinds.contains("Body") || tower == 1,
            "tower {tower}: the boss"
        );
        assert!(
            phases.len() >= 2,
            "tower {tower}: the fight moved on: {phases:?}"
        );
    }
}

/// The miasma (Versus walls, the Amaranth's Legendary miasma) draws its
/// art: tentacles and surface strips over the body (`scratch/miasma.png`).
#[test]
fn miasma_draws_its_art() {
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    let mut s = scene(0, 0, 1, &gpu, &mut renderer, &mut audio);
    s.dw_proof = DwProof::default();
    let ids = tfp_sim::versus::miasma::spawn(s.world_content_mut().0);
    for tick in 0..400u64 {
        s.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
    }
    let view = s.frame_view();
    for id in ids {
        let items: Vec<_> = view.items.iter().filter(|d| d.entity == id).collect();
        assert!(
            items
                .iter()
                .filter(|d| matches!(d.visual, Visual::Sprite { .. }))
                .count()
                > 10,
            "miasma art"
        );
    }
    let size = s.game_size();
    let canvas = renderer.create_canvas(&gpu, size);
    let (mut list, mut overlay) = (DrawList::new(), DrawList::new());
    let mut present = PresentSettings::default();
    s.draw(&mut DrawContext {
        list: &mut list,
        overlay: &mut overlay,
        present: &mut present,
        game_size: size,
        window: (960, 720),
        gpu: &gpu,
        renderer: &mut renderer,
        tick: 400,
    });
    renderer.render(&gpu, &list, canvas).unwrap();
    let img = renderer.capture(&gpu, canvas).unwrap();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scratch");
    if out.is_dir() {
        let _ = img.save_png(out.join("miasma.png"));
    }
}

/// Game over on level 2 offers CONTINUE; a Legendary clear for two
/// records the tower (flags, eye, time) and the crowns.
#[test]
fn game_over_and_clear_records() {
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let raw = RawInput::default();
    let mut host = HostRequests::default();
    // Game over.
    let mut s = scene(1, 1, 1, &gpu, &mut renderer, &mut audio);
    s.dw_proof = DwProof::default();
    let (w, c) = s.world_content_mut();
    tfp_sim::darkworld::jump_to_level(w, c, 1);
    let mut saw_continue = false;
    for tick in 0..2000u64 {
        if tick == 400 {
            let (w, c) = s.world_content_mut();
            if let Some(d) = w.dark_world.as_deref_mut() {
                d.extra_lives = 0;
            }
            super::force_end(w, c, false);
        }
        s.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
        saw_continue |= s
            .dark_world()
            .is_some_and(|d| d.menu_ready && d.can_continue());
        if s.exit.is_some() {
            break;
        }
    }
    assert!(saw_continue, "the game over offers a continue on level 2");
    assert_eq!(s.exit, Some(Exit::Map), "no humans: MAP by itself");
    assert_eq!(s.records.dark_world[&1].attempts, 1);
    assert!(s.records.dark_world[&1].deaths >= 1);
    // The run went to the save store (in memory in tests).
    let (d, _) = crate::save::snapshot();
    assert!(d.dark_world[&1].attempts >= 1 && d.dark_world[&1].deaths >= 1);
    draw(&mut s, &gpu, &mut renderer, 0);
    // A Legendary clear for two (no continues: the eye).
    let mut s = scene(0, 2, 2, &gpu, &mut renderer, &mut audio);
    s.dw_proof = DwProof::default();
    for tick in 0..3000u64 {
        if tick >= 100 {
            let (w, c) = s.world_content_mut();
            super::force_end(w, c, true);
        }
        s.tick(&mut TickContext {
            input: &raw,
            audio: &mut audio,
            host: &mut host,
            tick,
        });
        if tick % 500 == 0 {
            draw(&mut s, &gpu, &mut renderer, tick);
        }
        if s.exit.is_some() {
            break;
        }
    }
    let r = s.records.dark_world[&0];
    assert!(r.normal && r.hardcore && r.legendary, "{r:?}");
    assert!(r.eye, "no continues on Legendary");
    assert!(r.best[1] > 0, "a Legendary two-player time");
    assert!(s.records.crowns[0] || s.records.crowns[1], "co-op crowns");
    assert_eq!(s.exit, Some(Exit::Map));
    let (d, _) = crate::save::snapshot();
    let t = &d.dark_world[&0];
    assert!(
        t.completed_legendary && t.earned_eye && t.best[1] > 0,
        "{t:?}"
    );
    // The maps read the store back.
    let r = crate::save::coop_records();
    assert!(r.dark_world[&0].legendary && r.dark_world[&0].eye);
}

/// The 8-player layout (420 wide): FIGHT! -> Right: CO-OP under the logo
/// -> DARK WORLD -> the 8-slot rollcall (bind, join, start) -> the Dark
/// World map laid out 420 wide -> NORMAL -> BEGIN! -> a launch.
#[test]
fn wide_menus_launch_dark_world() {
    use crate::front::{FrontEnd, FrontOptions, ScreenId};
    use crate::input::InputHub;
    use sdl2::keyboard::Scancode;
    let Some((gpu, mut renderer, mut audio)) = env() else {
        return;
    };
    let mut front = FrontEnd::new(FrontOptions {
        screen: Some(ScreenId::Main),
        capacity: Some(8),
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
    assert_eq!(front.game_size().0, 420, "laid out for 8");
    run(&mut front, Some(Scancode::Right), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 70, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::CoOp));
    run(&mut front, None, 25, &mut hub);
    run(&mut front, Some(Scancode::Right), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 70, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::Rollcall));
    run(&mut front, None, 25, &mut hub);
    for _ in 0..6 {
        if front.screen() == Some(ScreenId::DarkWorldMap) {
            break;
        }
        run(&mut front, Some(Scancode::C), 30, &mut hub);
    }
    assert_eq!(front.screen(), Some(ScreenId::DarkWorldMap));
    assert_eq!(front.game_size().0, 420);
    run(&mut front, None, 30, &mut hub);
    run(&mut front, Some(Scancode::C), 25, &mut hub);
    run(&mut front, Some(Scancode::C), 25, &mut hub);
    run(&mut front, Some(Scancode::C), 90, &mut hub);
    let l = front
        .take_coop()
        .expect("a Dark World launch from the wide layout");
    assert!(
        matches!(l.mode, CoopMode::DarkWorld { difficulty: 0, .. }),
        "{:?}",
        l.mode
    );
    assert_eq!(l.players.len(), 1);
}

/// Keyboard through the menus: main -> CO-OP -> DARK WORLD -> rollcall
/// (join, start) -> Dark World map -> HARDCORE -> variants (a power-up on,
/// a second one refused by the cap) -> BEGIN! -> a launch.
#[test]
fn menus_launch_dark_world() {
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
    run(&mut front, Some(Scancode::Right), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 70, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::Rollcall));
    run(&mut front, None, 25, &mut hub);
    run(&mut front, Some(Scancode::C), 10, &mut hub);
    run(&mut front, Some(Scancode::C), 30, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::DarkWorldMap));
    run(&mut front, None, 30, &mut hub);
    // Tower select -> difficulty.
    run(&mut front, Some(Scancode::C), 25, &mut hub);
    run(&mut front, Some(Scancode::Right), 5, &mut hub);
    // Difficulty -> variants (BEGIN! selected).
    run(&mut front, Some(Scancode::C), 25, &mut hub);
    // Up: the bottom-right power-up (Super Dodging), on.
    run(&mut front, Some(Scancode::Up), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 5, &mut hub);
    // Left: Double Jumping: refused (Hardcore allows one).
    run(&mut front, Some(Scancode::Left), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 5, &mut hub);
    // A picture of the variants overlay (scratch/dw-variants.png).
    {
        let size = front.game_size();
        let canvas = renderer.create_canvas(&gpu, size);
        let (mut list, mut overlay) = (DrawList::new(), DrawList::new());
        let mut present = PresentSettings::default();
        front.draw(&mut DrawContext {
            list: &mut list,
            overlay: &mut overlay,
            present: &mut present,
            game_size: size,
            window: (960, 720),
            gpu: &gpu,
            renderer: &mut renderer,
            tick: 0,
        });
        renderer.render(&gpu, &list, canvas).unwrap();
        let img = renderer.capture(&gpu, canvas).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scratch");
        if out.is_dir() {
            let _ = img.save_png(out.join("dw-variants.png"));
        }
        renderer.destroy_texture(canvas);
    }
    // Down to BEGIN!, go.
    run(&mut front, Some(Scancode::Down), 5, &mut hub);
    run(&mut front, Some(Scancode::C), 90, &mut hub);
    let l = front.take_coop().expect("a Dark World launch");
    let CoopMode::DarkWorld {
        tower,
        difficulty,
        variants,
    } = l.mode
    else {
        panic!("{:?}", l.mode)
    };
    assert_eq!((tower, difficulty), (0, 1));
    assert!(variants.any(Variant::SuperDodging));
    assert!(!variants.any(Variant::DoubleJumping), "capped at one");
    assert_eq!(l.players.len(), 1);
    // MAP from the game comes back to the Dark World map.
    front.coop_session().mode = crate::coop_ui::RollcallMode::DarkWorld;
    front.return_to(ScreenId::DarkWorldMap);
    run(&mut front, None, 5, &mut hub);
    assert_eq!(front.screen(), Some(ScreenId::DarkWorldMap));
}

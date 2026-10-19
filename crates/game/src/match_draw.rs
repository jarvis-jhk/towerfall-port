//! The one drawing path of a match's level (local play, online, co-op,
//! online co-op): builds the sim's view, lets the scene decorate it (enemy
//! / boss / debris art; extra light sources), applies the archer, corpse and
//! arrow cosmetics (`versus_ui::actors`), gathers the lights (archers, the
//! scene's extras, the sim's entity lights unless the final-kill spotlight
//! runs), the dark orb, then draws the level with the in-level marks
//! (quivers, treasure previews, cooldown stars), explosions / shocks /
//! debris and particles inside the darkened layer, and the Gunn Style
//! overlay on top.
//!
//! What stays with each scene: the HUD (online draws it from the confirmed
//! timeline while the level shows the predicted world, docs/net.md), menus,
//! the screen shake and the wave.
//!
//! Hook points for scenes with their own screen effects (Dark World's
//! camera / wave visuals): `draw_level` is `prepare` (view, decorations,
//! actors, lights, dark orb; set `TowerView` state such as a screen target
//! before or after it) followed by `draw_prepared` (the level with
//! `in_level_layer` inside the darkened layer, then the Gunn overlay). A
//! scene can call `prepare`, draw the level its own way using
//! `in_level_layer` for the in-level layer and `gunn_pass` for the overlay,
//! and add post effects (slow wave) after that.

use tfp_render::glam::Vec2;
use tfp_render::{Color, DrawList, Renderer, TextureId};
use tfp_sim::{Content, View, ViewOptions, World};

use crate::app::DrawContext;
use crate::front::assets::UiAssets;
use crate::tower::{InLevel, TowerGfx, TowerView};
use crate::versus_ui::VersusUi;

/// What `draw_level` needs from the scene.
pub struct LevelDraw<'a> {
    /// The world to draw (online: the predicted one; during an instant
    /// replay the recorded frame, see `VersusUi::render_world`).
    pub world: &'a World,
    pub content: &'a Content,
    pub gfx: &'a TowerGfx,
    pub ui: &'a VersusUi,
    pub ui_assets: &'a UiAssets,
    pub hitboxes: bool,
    /// Clock of the level look's animations (frames since it was built).
    pub level_t: u64,
    /// Clock of the actors' animations (shield, wings, flashes).
    pub tick: u64,
    /// Game canvas size.
    pub size: (u32, u32),
    /// The Versus HUD view (P-tags and death skulls are drawn in the level,
    /// darkened with it).
    pub hud: Option<&'a tfp_sim::versus::HudView>,
}

/// Scratch canvases of the match drawing, kept by the scene between
/// frames: the Gunn Style overlay and the level composite used while the
/// camera is offset (space orb, Offset World, Always Scrolling).
#[derive(Default)]
pub struct GunnCanvas {
    gunn: Option<(TextureId, (u32, u32))>,
    level: Option<(TextureId, (u32, u32))>,
}

fn canvas(
    slot: &mut Option<(TextureId, (u32, u32))>,
    ctx: &mut DrawContext<'_>,
    size: (u32, u32),
) -> TextureId {
    match *slot {
        Some((id, s)) if s == size => id,
        old => {
            if let Some((id, _)) = old {
                ctx.renderer.destroy_texture(id);
            }
            let id = ctx.renderer.create_canvas(ctx.gpu, size);
            *slot = Some((id, size));
            id
        }
    }
}

impl GunnCanvas {
    pub fn release(&mut self, r: &mut Renderer) {
        for (id, _) in [self.gunn.take(), self.level.take()].into_iter().flatten() {
            r.destroy_texture(id);
        }
    }

    fn get(&mut self, ctx: &mut DrawContext<'_>, size: (u32, u32)) -> TextureId {
        canvas(&mut self.gunn, ctx, size)
    }
}

/// The built view and its lights (from `prepare`).
pub struct Prepared {
    pub view: View,
    pub lights: Vec<(Vec2, f32)>,
}

/// View build, scene decorations (`decorate` returns extra light sources),
/// archer cosmetics, lights and the spotlight / dark orb state of `tv`.
pub fn prepare(
    tv: &mut TowerView,
    d: &LevelDraw<'_>,
    decorate: &mut dyn FnMut(&mut View) -> Vec<(tfp_sim::Vec2, f32)>,
) -> Prepared {
    let (w, c, ui) = (d.world, d.content, d.ui);
    let mut view = View::build(
        w,
        c,
        ViewOptions {
            hitboxes: d.hitboxes,
        },
    );
    let extra = decorate(&mut view);
    if let Some(a) = &ui.actors {
        a.apply(&mut view, w, c, d.tick);
    }
    tfp_sim::view::expand_outlines(&mut view.items);
    // Archers are among the sim's light sources (radius 50, `view.lights`).
    let base: Vec<(Vec2, f32)> = extra
        .iter()
        .map(|(p, r)| (Vec2::new(p.x, p.y), *r))
        .collect();
    let (mut lights, spot) = ui.lights(w, &base);
    // The sim's entity lights (arrows, orbs, fire ...), unless the final
    // kill's spotlight restricts the light to victor and victim; coloured
    // ones (rendering.md 7.5) go to the view's light list.
    tv.dyn_lights.clear();
    tv.dyn_rects.clear();
    let el = entity_lights(w, c);
    // Ghosts that were lit last frame and are gone fade out (0.05 / frame).
    let gone: Vec<tfp_assets::look::Light> = tv
        .ghost_seen
        .iter()
        .filter(|(id, _)| !el.ghosts.iter().any(|(g, _)| g == id))
        .map(|(_, l)| l.clone())
        .collect();
    tv.ghost_fading.extend(gone);
    tv.ghost_seen = el.ghosts;
    for l in &mut tv.ghost_fading {
        l.alpha -= 0.05;
    }
    tv.ghost_fading.retain(|l| l.alpha > 0.0);
    if !spot {
        lights.extend(el.black);
        tv.dyn_lights = el.tinted;
        tv.dyn_lights.extend(tv.ghost_fading.iter().cloned());
        tv.dyn_rects = el.rects;
    }
    // Dark orb (rendering.md 7.4): the darkness target goes to 1 and the
    // alpha approaches it by 0.02 per frame; every light still shines. The
    // final-kill spotlight keeps only its own lights.
    let target = if view.dark { 1.0 } else { 0.0 };
    tv.dark_ease = if tv.dark_ease < target {
        (tv.dark_ease + 0.02).min(target)
    } else {
        (tv.dark_ease - 0.02).max(target)
    };
    tv.spotlight = spot;
    tv.alpha_add = tv.dark_ease.max(ui.spot_alpha());
    Prepared { view, lights }
}

/// The sim's light sources (rendering.md 7.5) for one frame.
#[derive(Default)]
pub struct EntityLights {
    /// Plain "remove darkness" lights `(pos, radius)`.
    pub black: Vec<(Vec2, f32)>,
    /// Coloured ones (stored inverted, as the light canvas wants).
    pub tinted: Vec<tfp_assets::look::Light>,
    /// Lit regions (the lava's `DrawLight`): rect, inverted colour, alpha.
    pub rects: Vec<([f32; 4], [u8; 4], f32)>,
    /// Ghosts' coloured lights by entity (they fade out on death).
    pub ghosts: Vec<(tfp_sim::EntityId, tfp_assets::look::Light)>,
}

fn inv(rgb: u32) -> [u8; 4] {
    let c = |s: u32| 255 - ((rgb >> s) & 0xFF) as u8;
    [c(16), c(8), c(0), 255]
}

/// The sim's light sources: radii from `tfp_sim::view::light_radius`,
/// with the colours, alphas and radii of rendering.md 7.5 the sim does not
/// carry (ghosts, crystals, ghost platforms, jump pads, floor miasma, the
/// boss parts, the Dark World portal) and the lava's lit region.
pub fn entity_lights(w: &World, c: &Content) -> EntityLights {
    use tfp_sim::darkworld::Part;
    use tfp_sim::enemies::EnemyType as E;
    use tfp_sim::entities::Tower;
    use tfp_sim::entity::Kind;
    let mut out = EntityLights::default();
    let dark = c.level(w.level).tower.theme.dark;
    let boss = tfp_sim::darkworld::boss_phase(w).map(|b| b.0);
    let width = c.level(w.level).width as f32;
    let light = |at: tfp_sim::Vec2, r: f32, color: [u8; 4], alpha: f32| tfp_assets::look::Light {
        x: at.x,
        y: at.y,
        radius: r,
        color,
        alpha,
        wrap: true,
        kind: tfp_assets::look::LightKind::Decoration,
    };
    for &id in w.tagged(tfp_sim::Tag::LightSource).iter() {
        let core = w.core(id);
        if !core.visible {
            continue;
        }
        let pos = core.pos;
        let centre = w.hitbox_of(id).map_or(pos, |h| {
            pos + tfp_sim::Vec2::new(h.x + h.w / 2.0, h.y + h.h / 2.0)
        });
        let r = tfp_sim::view::light_radius(w, id);
        // (radius, inverted colour or None = black, alpha, at)
        let spec: Option<(f32, Option<[u8; 4]>, f32, tfp_sim::Vec2)> = match w.kind(id) {
            Some(Kind::Enemy(e)) => match e.ty {
                E::Ghost => Some((r, Some(inv(0x2277FF)), 1.0, pos)),
                E::GreenGhost => Some((r, Some(inv(0xB1FF9E)), 1.0, pos)),
                E::EvilCrystal => Some((40.0, Some(inv(0xFF0000)), 1.0, pos)),
                E::BlueCrystal => Some((40.0, Some(inv(0x00C4C7)), 1.0, pos)),
                E::BoltCrystal => Some((40.0, Some(inv(0x7AFF99)), 1.0, pos)),
                E::PrismCrystal => Some((40.0, Some(inv(0xFFBDF0)), 1.0, pos)),
                _ => None,
            },
            Some(Kind::Tower(Tower::GhostPlatform(_))) => {
                Some((40.0, Some(inv(0xC1FFF3)), 1.0, centre))
            }
            Some(Kind::Tower(Tower::JumpPad(j))) => {
                Some((40.0, Some(inv(0x95F07F)), j.light, centre))
            }
            Some(Kind::FloorMiasma(f)) => Some((40.0, Some(inv(0x9ED1FF)), f.light, centre)),
            Some(Kind::Tower(Tower::Lava(l))) => {
                lava_rects(
                    &mut out.rects,
                    l.side,
                    pos,
                    width,
                    if dark { 0x00E605 } else { 0xF83800 },
                );
                continue;
            }
            Some(Kind::Dw(p)) => {
                let r = match p {
                    Part::ExitPortal { .. } => 80.0,
                    Part::Eye(_) => 100.0,
                    Part::Tentacle(_) => 40.0,
                    Part::Fist(_) => 50.0,
                    // The boss bodies: Amaranth 90, Darkfang 50, Cataclysm's eye 120.
                    Part::Body => match boss {
                        Some(0) => 90.0,
                        Some(2) => 50.0,
                        Some(3) => 120.0,
                        _ => r,
                    },
                    _ => r,
                };
                Some((r, None, 1.0, pos))
            }
            _ => None,
        };
        match spec {
            Some((r, Some(color), alpha, at)) if r > 0.0 && alpha > 0.0 => {
                let l = light(at, r, color, alpha);
                if matches!(w.kind(id), Some(Kind::Enemy(e)) if matches!(e.ty, E::Ghost | E::GreenGhost))
                {
                    out.ghosts.push((id, l.clone()));
                }
                out.tinted.push(l);
            }
            Some((r, None, _, at)) if r > 0.0 => out.black.push((Vec2::new(at.x, at.y), r)),
            Some(_) => {}
            None if r > 0.0 => out.black.push((Vec2::new(pos.x, pos.y), r)),
            None => {}
        }
    }
    out
}

/// The lava's lit region (rendering.md 7.5): the lava body plus four 5 px
/// bands towards the screen at 0.8 / 0.6 / 0.4 / 0.2.
fn lava_rects(
    out: &mut Vec<([f32; 4], [u8; 4], f32)>,
    side: u8,
    at: tfp_sim::Vec2,
    w: f32,
    body: u32,
) {
    use tfp_sim::entities::hazards::{BOTTOM, LEFT, TOP};
    let c = inv(body);
    let band = 5.0;
    let alphas = [0.8, 0.6, 0.4, 0.2];
    match side {
        BOTTOM => {
            let mut y = (at.y - 10.0).trunc() - band;
            out.push(([0.0, y, w, 100.0], c, 1.0));
            for a in alphas {
                y -= band;
                out.push(([0.0, y, w, band], c, a));
            }
        }
        TOP => {
            let mut y = (at.y + 10.0).trunc() + band;
            out.push(([0.0, 0.0, w, y], c, 1.0));
            for a in alphas {
                out.push(([0.0, y, w, band], c, a));
                y += band;
            }
        }
        LEFT => {
            let mut x = (at.x + 10.0).trunc() + band;
            out.push(([0.0, 0.0, x, 240.0], c, 1.0));
            for a in alphas {
                out.push(([x, 0.0, band, 240.0], c, a));
                x += band;
            }
        }
        _ => {
            let mut x = (at.x - 10.0).trunc() - band;
            out.push(([x, 0.0, 100.0, 240.0], c, 1.0));
            for a in alphas {
                x -= band;
                out.push(([x, 0.0, band, 240.0], c, a));
            }
        }
    }
}

/// The in-level layer drawn inside the darkened gameplay layer: quivers,
/// treasure previews and cooldown stars, explosions / shocks / debris,
/// particles.
pub fn in_level_layer(dl: &mut DrawList, d: &LevelDraw<'_>, view: &View, at: InLevel) {
    use crate::versus_ui::particles::PLayer;
    let main = d.ui_assets.main.as_ref();
    let sprite = |name: &str| main.and_then(|m| m.get(name));
    match at {
        InLevel::Back => d.ui.draw_particles(dl, &sprite, PLayer::Bg),
        InLevel::Depth(-40) => d.ui.draw_effects(dl, d.gfx),
        InLevel::Depth(-80) => d.ui.draw_particles(dl, &sprite, PLayer::Mid),
        InLevel::Depth(-490) => d.ui.draw_particles(dl, &sprite, PLayer::Fg),
        // Death skulls and P-tags: layer 0, depth -1000 (rendering.md 4.3).
        InLevel::Depth(-1000) => {
            if let Some(h) = d.hud {
                let mut p = crate::front::gfx::Painter {
                    list: dl,
                    a: d.ui_assets,
                    w: d.size.0 as f32,
                };
                d.ui.draw_world_marks(&mut p, h, d.world, d.tick);
            }
        }
        InLevel::Depth(_) => {}
        // Quivers and markers over everything of the level.
        InLevel::Top => crate::versus_ui::marks::draw_marks(dl, view, d.gfx, d.level_t),
    }
}

/// The level with the in-level layer, then the Gunn overlay.
pub fn draw_prepared(
    ctx: &mut DrawContext<'_>,
    tv: &mut TowerView,
    d: &LevelDraw<'_>,
    p: &Prepared,
    gunn: &mut GunnCanvas,
) {
    let (c, g) = (d.content, d.gfx);
    // The camera offset (rendering.md 2, 8.3): the level is composited
    // into a canvas and drawn back shifted, wrapping around the edges.
    let xform = crate::coop_ui::screen_fx::LevelXform {
        zoom: None,
        offset: Vec2::new(p.view.camera_offset.x, p.view.camera_offset.y),
    };
    let level = xform.active().then(|| canvas(&mut gunn.level, ctx, d.size));
    tv.screen_target = level;
    tv.draw_full(
        ctx.list,
        g,
        d.level_t,
        Some((&p.view, c)),
        &p.lights,
        &|dl, at| in_level_layer(dl, d, &p.view, at),
    );
    tv.screen_target = None;
    if let Some(t) = level {
        crate::coop_ui::screen_fx::draw_level(ctx.list, t, d.size, &xform);
    }
    gunn_pass(ctx, d, &p.view, gunn);
}

/// Draws the HUD with the screen shake undone: the present offset moves
/// the whole canvas, the HUD layer is never shaken (rendering.md 8.2).
pub fn steady(list: &mut DrawList, shake: Vec2, f: impl FnOnce(&mut DrawList)) {
    let old = list.transform();
    list.set_transform(tfp_render::glam::Affine2::from_translation(-shake) * old);
    f(list);
    list.set_transform(old);
}

/// The Gunn Style overlay, when the view has one.
pub fn gunn_pass(ctx: &mut DrawContext<'_>, d: &LevelDraw<'_>, view: &View, gunn: &mut GunnCanvas) {
    if let Some(gv) = &view.gunn {
        let canvas = gunn.get(ctx, d.size);
        gunn_overlay(ctx.list, d.gfx, d.world, gv, canvas, d.size);
    }
}

/// Draws the level of a match (`prepare` + `draw_prepared`). `decorate`
/// runs on the built view (scene art) and returns extra light sources.
pub fn draw_level(
    ctx: &mut DrawContext<'_>,
    tv: &mut TowerView,
    d: &LevelDraw<'_>,
    gunn: &mut GunnCanvas,
    decorate: &mut dyn FnMut(&mut View) -> Vec<(tfp_sim::Vec2, f32)>,
) {
    let p = prepare(tv, d, decorate);
    draw_prepared(ctx, tv, d, &p, gunn);
}

/// Gunn Style: black over everything with holes at the victim and at every
/// living archer, wrapped near the edges (rendering.md 9, versus.md 13.2).
fn gunn_overlay(
    dl: &mut DrawList,
    g: &TowerGfx,
    w: &World,
    gv: &tfp_sim::variant_fx::GunnView,
    canvas: TextureId,
    size: (u32, u32),
) {
    dl.begin_pass(Some(canvas), Some(Color::BLACK));
    dl.set_blend(tfp_render::Blend::Erase);
    let (ww, hh) = (size.0 as f32, size.1 as f32);
    let mut hole = |name: &str, p: Vec2| {
        if let Some(s) = g.main_sub(name) {
            let half = Vec2::new(s.rect.w as f32, s.rect.h as f32) / 2.0;
            let mut spots = vec![p];
            for (near, d) in [(p.x < 10.0, ww), (p.x > ww - 10.0, -ww)] {
                if near {
                    spots.push(p + Vec2::new(d, 0.0));
                }
            }
            for (near, d) in [(p.y < 10.0, hh), (p.y > hh - 10.0, -hh)] {
                if near {
                    spots.push(p + Vec2::new(0.0, d));
                }
            }
            for q in spots {
                dl.sprite(s, &tfp_render::SpriteParams::at(q).origin(half));
            }
        }
    };
    hole("gunnStyle", Vec2::new(gv.at.x, gv.at.y));
    for id in w.players() {
        if let Some(p) = w.player(id)
            && p.index != gv.victim
            && !p.dead
        {
            let at = w.core(id).pos;
            hole("gunnStyleB", Vec2::new(at.x, at.y));
        }
    }
    dl.begin_pass(None, None);
    let full = tfp_render::SubTexture::new(
        canvas,
        tfp_render::Rect::new(0, 0, size.0 as i32, size.1 as i32),
    );
    dl.sprite(
        full,
        &tfp_render::SpriteParams::at(Vec2::ZERO).color(Color::WHITE * gv.alpha),
    );
}

/// The screen shake offset every match scene uses (+-2 px, wrapped).
pub fn shake_offset(frame: u64, shaking: bool) -> Vec2 {
    if !shaking {
        return Vec2::ZERO;
    }
    let k = frame.wrapping_mul(2_654_435_761);
    let pick = |bit: u64| if (k >> bit) & 1 == 0 { -2.0 } else { 2.0 };
    Vec2::new(pick(5), pick(11))
}

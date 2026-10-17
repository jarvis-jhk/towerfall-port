//! Dropped hats and crowns (player.md §15.4): small actors that float
//! (default hat) or tumble (wooden/steel hats, crown) and can be picked up.

use crate::actor;
use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox};
use crate::components::SineWave;
use crate::content::{Content, HatMaterial};
use crate::entity::{Core, EntityId, Kind, Tag, depth};
use crate::math::{DEG_TO_RAD, PI, Vec2, approach, clamp, min, shorter_angle};
use crate::player::HatState;
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hat {
    pub hat: HatState,
    pub archer: u16,
    pub material: HatMaterial,
    pub owner: u8,
    pub speed: Vec2,
    pub rotation: f32,
    pub spin: f32,
    pub sine: SineWave,
    pub flipped: bool,
}

impl StableHash for Hat {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u32(self.hat as u32);
        h.u32(self.archer as u32);
        h.u8(self.owner);
        h.vec2(self.speed);
        // `rotation` / `spin` are cosmetic (drawn only; drawn from the
        // cosmetic generator) and stay out of the checksum.
        h.f32(self.sine.counter);
        h.bool(self.flipped);
    }
}

impl Hat {
    fn tumbles(&self) -> bool {
        self.hat == HatState::Crown || self.material != HatMaterial::Default
    }
}

/// `Hat.CreateHat` (§15.3).
#[allow(clippy::too_many_arguments)]
pub fn create(
    w: &mut World,
    archer: u16,
    state: HatState,
    material: HatMaterial,
    pos: Vec2,
    arrow_speed: Option<Vec2>,
    flipped: bool,
    owner: u8,
) -> EntityId {
    let crown = state == HatState::Crown;
    let material = if crown {
        HatMaterial::Default
    } else {
        material
    };
    let hb = if crown {
        Hitbox::wrapping(2.0, 2.0, -1.0, -1.0)
    } else {
        Hitbox::wrapping(2.0, 2.0, -1.0, 0.0)
    };
    let mut core = Core::new(pos, Collider::Box(hb), depth::HAT, &[Tag::Hat, Tag::Actor]);
    core.screen_wrap = true;
    let rng = &mut w.rng;
    let speed = match arrow_speed {
        Some(s) => {
            let mut v = s + Vec2::new(0.0, -1.0 - 1.2 * rng.next_float());
            if !crown {
                v.y = clamp(v.y, -2.0, 2.0);
            }
            v
        }
        None if crown => {
            let x = [-1.0, 0.0, 1.0][rng.choose_index(3)];
            Vec2::new(x, -3.0 + 3.0 * rng.next_float())
        }
        None if material == HatMaterial::Default => Vec2::new(0.0, -1.0 - 1.2 * rng.next_float()),
        None => {
            let s = if rng.next_max(2) == 0 { -1.0 } else { 1.0 };
            let x = s * (0.8 + 0.8 * rng.next_float());
            Vec2::new(x, -1.0 - 1.2 * rng.next_float())
        }
    };
    let mut sine = SineWave::frames(rng.range_i(50, 30) as f32);
    sine.counter = rng.next_float() * PI * 2.0;
    // The spin is cosmetic (player.md §23): its own generator.
    let crng = &mut w.cosmetic_rng;
    let spin_sign = if crng.next_max(2) == 0 { -1.0 } else { 1.0 };
    let spin = spin_sign * (4.0 + 12.0 * crng.next_float()) * DEG_TO_RAD;
    let h = Hat {
        hat: state,
        archer,
        material,
        owner,
        speed,
        rotation: 0.0,
        spin,
        sine,
        flipped,
    };
    w.add(core, Kind::Hat(h))
}

pub fn update(w: &mut World, c: &Content, id: EntityId, h: &mut Hat) {
    let t = w.time.time_mult;
    h.sine.update(t);
    let below = actor::check_below(w, c, id);
    if !h.tumbles() {
        if below {
            h.rotation = approach(h.rotation, 0.0, 5.0 * DEG_TO_RAD * t);
            h.speed.x = approach(h.speed.x, 0.0, 0.1 * t);
        } else {
            h.speed.x = approach(h.speed.x, h.sine.value, 1.0 * t);
            h.rotation = h.speed.x * -40.0 * DEG_TO_RAD;
            h.speed.y = min(h.speed.y + 0.08 * t, 1.2);
        }
    } else if below {
        h.speed.x = approach(h.speed.x, 0.0, 0.2 * t);
        let target = shorter_angle(h.rotation, 0.0, PI);
        h.rotation = crate::math::angle_approach(h.rotation, target, 6.0 * DEG_TO_RAD * t);
    } else {
        let k = if h.speed.y.abs() <= 0.5 { 0.5 } else { 1.0 };
        h.speed.y = min(h.speed.y + 0.3 * k * t, 3.0);
        h.rotation += h.spin * t;
    }
    if actor::move_h(w, c, id, h.speed.x * t).is_some() {
        if h.tumbles() {
            h.speed.x *= -0.5;
            h.spin = -h.spin.signum() * (4.0 + 12.0 * w.cosmetic_rng.next_float()) * DEG_TO_RAD;
        } else {
            h.speed.x = 0.0;
        }
    }
    if actor::move_v(w, c, id, h.speed.y * t).is_some() {
        let x = w.core(id).pos.x;
        if h.tumbles() {
            let s = if w.cosmetic_rng.next_max(2) == 0 {
                -1.0
            } else {
                1.0
            };
            h.spin = s * (4.0 + 12.0 * w.cosmetic_rng.next_float()) * DEG_TO_RAD;
            if h.speed.y >= 2.0 {
                h.speed.x += w.rng.next_float() * 2.0 - 1.0;
                h.speed.y *= -0.5;
            } else {
                h.speed.y = 0.0;
            }
            let name = match (h.hat, h.material) {
                (HatState::Crown, _) => "char_crownFall",
                (_, HatMaterial::Wood) => "sfx_endropScyHandle",
                _ => "sfx_endropStaffHead",
            };
            w.sound(name, x);
        } else {
            h.speed.y = 0.0;
            w.sound("char_hatFall", x);
        }
    }
    actor::enforce_screen_wrap(w, c, id);
}

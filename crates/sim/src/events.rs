//! Events the sim emits for the presentation layer (sounds, particles,
//! rumble, shake). They never feed back into the simulation. Events from
//! one step are in [`crate::World::events`]; the presentation keys them by
//! `(frame, index)` to fire each once across rollbacks.

use crate::math::Vec2;

/// Per-archer voice/effect cues (`CharacterSounds`, player.md §19.9). The
/// presentation maps them to `<PREFIX>_<NAME>.wav` by the archer's SFX id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharSound {
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
}

/// Particle bursts (presentation simulates them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleKind {
    Dust,
    Dash,
    Hyper,
    JumpPadTrail,
    ShieldBreak,
    WingsBurn,
    CatchShine,
    DeathSkull,
    LightFade,
    // M5 (player = u8::MAX when not tied to an archer)
    Explosion,
    SuperExplosion,
    ShockCircle,
    SmallShock,
    DrillDig,
    PrismDebris,
    PrismVanish,
    SpeedBoots,
    ChestOpen,
    FeatherTrail,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A shared sound effect (`Content/SFX/<name>.wav`), positional at `x`.
    Sound {
        name: &'static str,
        x: f32,
    },
    /// One entry of an array effect (`sfx_enemyKill[n]`,
    /// `char_fireLaserDeflect[n]`; `index` is 0-based).
    SoundIndexed {
        name: &'static str,
        index: u8,
        x: f32,
    },
    /// An archer cue. `player` = player index, `archer` = content archer index.
    CharSound {
        sound: CharSound,
        player: u8,
        archer: u16,
        x: f32,
    },
    /// Start/stop a looped sound owned by a player (wall slide, sleep).
    Loop {
        name: &'static str,
        player: u8,
        on: bool,
        x: f32,
    },
    Particles {
        kind: ParticleKind,
        player: u8,
        pos: Vec2,
        count: u8,
        dir: f32,
    },
    Rumble {
        player: u8,
        strength: f32,
        frames: u16,
    },
    ScreenShake {
        frames: u16,
    },
    /// Start/stop a looped sound owned by an entity (bomb chest, lava).
    EntityLoop {
        name: &'static str,
        entity: crate::entity::EntityId,
        on: bool,
        x: f32,
    },
    /// Floating "<NAME> ARROWS" text of an arrow pick-up (`pickup` =
    /// `Pickups` index; names and colours in `arrows::NAMES`/`COLOR_A`).
    PickupText {
        pickup: u8,
        pos: Vec2,
    },
    /// Match flow (round start/end, points, crowns, final kill, match end).
    Versus(crate::versus::VersusEvent),
    /// Tower entities: named particles, sounds with volume, entity loops.
    Env(crate::entities::EnvEvent),
    /// A player died (kill feed / HUD): `killer` = player index or -1.
    Death {
        player: u8,
        killer: i8,
        cause: u8,
    },
}

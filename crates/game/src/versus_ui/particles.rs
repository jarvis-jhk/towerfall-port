//! Particles (rendering.md 9): the original's particle types as data, a
//! ring-buffer system with the original's update order, and the mapping
//! from the sim's particle events. Cosmetic: its own RNG, never fed back.

use std::f32::consts::PI;

use tfp_render::glam::Vec2;
use tfp_render::{Color, DrawList, SpriteParams, SubTexture};

/// The level's three particle systems (rendering.md 4.1, 9.1):
/// `ParticlesBG` (layer -3, over the back decorations), `Particles`
/// (layer 0, depth -80) and `ParticlesFG` (layer 0, depth -490).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PLayer {
    Bg,
    Mid,
    Fg,
}

/// Which system a particle type is emitted into (rendering.md 9.1).
pub fn layer_of(name: &str) -> PLayer {
    const BG: [&str; 12] = [
        "BGTorch",
        "WingsDie",
        "PurpleAmbience",
        "BramblesGrow",
        "EnLichHornBreak",
        "EnBrambleSpore",
        "EnBirdDie",
        "EnDarkBirdman",
        "DummyDie",
        "Dash",
        "CrackedBlockCrumble",
        "LaserArrowTrail",
    ];
    const FG: [&str; 16] = [
        "GhostPlatformPlayerPop",
        "YellowSleeping",
        "WhiteArcherDisappear",
        "SnowClumpDisperse",
        "SensorBlockDust",
        "PrismVanish",
        "PrismCorpse",
        "PickupSparkle",
        "NextLevelPortalEnter",
        "MudPlayerRun",
        "LoopPlatformWall",
        "LavaHeat",
        "Fire",
        "ExplodingOrbGlow",
        "EnPortalSpawn",
        "EnDreadTentacle",
    ];
    // Per-archer / per-team variants keep their base name as a suffix.
    let base = name.trim_start_matches("Team").trim_start_matches("Enemy");
    if BG.iter().any(|b| base.starts_with(b)) {
        PLayer::Bg
    } else if FG.iter().any(|f| base.starts_with(f)) {
        PLayer::Fg
    } else {
        PLayer::Mid
    }
}

/// A `ParticleType`.
#[derive(Clone, Debug, PartialEq)]
pub struct PType {
    pub name: &'static str,
    /// Main-atlas image; `None` = a flat square.
    pub source: Option<&'static str>,
    pub c1: Color,
    pub c2: Color,
    pub switch: u32,
    pub switch_loop: bool,
    pub speed: f32,
    pub speed_range: f32,
    pub speed_mult: f32,
    pub accel: Vec2,
    pub dir: f32,
    pub dir_range: f32,
    pub life: u32,
    pub life_range: u32,
    pub size: f32,
    pub size_range: f32,
    pub rotated: bool,
    pub random_rotate: bool,
    pub scale_out: bool,
}

const BASE: PType = PType {
    name: "",
    source: None,
    c1: Color::WHITE,
    c2: Color::WHITE,
    switch: 0,
    switch_loop: true,
    speed: 0.0,
    speed_range: 0.0,
    speed_mult: 1.0,
    accel: Vec2::ZERO,
    dir: 0.0,
    dir_range: 0.0,
    life: 0,
    life_range: 0,
    size: 2.0,
    size_range: 0.0,
    rotated: true,
    random_rotate: false,
    scale_out: false,
};

fn hex(h: u32) -> Color {
    Color::hex(h)
}

/// The fixed (not per-archer) types of rendering.md 9.4.
pub fn catalogue() -> Vec<PType> {
    let t = |name: &'static str| PType { name, ..BASE };
    let mut v = Vec::new();
    v.push(PType {
        c1: hex(0x8EE8FF),
        size: 1.0,
        size_range: 2.0,
        speed: 0.5,
        speed_range: 0.6,
        accel: Vec2::new(0.0, 0.03),
        dir_range: PI / 3.0,
        life: 12,
        life_range: 8,
        ..t("DrillArrowDig")
    });
    v.push(PType {
        c1: hex(0xF6FF00),
        c2: hex(0xFFD400),
        switch: 3,
        size: 1.0,
        size_range: 2.0,
        speed: 0.05,
        speed_range: 0.01,
        accel: Vec2::new(0.0, -0.01),
        dir_range: PI * 2.0,
        life: 12,
        life_range: 4,
        ..t("TreasureChestGlow")
    });
    v.push(PType {
        c1: hex(0xD800CC),
        c2: hex(0xF8B8F8),
        switch: 4,
        size: 2.0,
        size_range: 3.0,
        speed: 1.0,
        speed_range: 0.7,
        speed_mult: 0.91,
        dir_range: PI * 2.0,
        life: 26,
        life_range: 12,
        ..t("OrbShatter")
    });
    let orb_glow = PType {
        c1: hex(0xF8A4C0),
        size: 1.0,
        size_range: 2.0,
        speed: 0.1,
        speed_range: 0.03,
        accel: Vec2::new(0.0, -0.01),
        dir_range: PI * 2.0,
        life: 18,
        life_range: 8,
        ..t("OrbGlow")
    };
    v.push(PType {
        name: "ExplodingOrbGlow",
        c1: hex(0xAFEB00),
        ..orb_glow.clone()
    });
    v.push(orb_glow);
    v.push(PType {
        c1: hex(0xB8F8B8),
        c2: hex(0xFFFFFF),
        switch: 6,
        size: 1.0,
        size_range: 2.0,
        speed: 0.1,
        speed_range: 0.2,
        dir_range: PI * 2.0,
        life: 22,
        life_range: 8,
        ..t("JumpPadTrail")
    });
    v.push(PType {
        c1: hex(0x00E8D8),
        size: 1.0,
        size_range: 2.0,
        speed: 0.02,
        speed_range: 0.04,
        accel: Vec2::new(0.0, 0.06),
        dir: PI / 2.0,
        dir_range: PI / 15.0,
        life: 12,
        life_range: 18,
        ..t("CrackedBlockCrumble")
    });
    v.push(PType {
        c1: hex(0x00E8D8),
        size: 1.0,
        size_range: 2.0,
        speed: 1.0,
        speed_range: 0.5,
        accel: Vec2::new(0.0, 0.06),
        dir_range: PI * 2.0,
        life: 8,
        life_range: 3,
        ..t("CrackedBlockBreak")
    });
    let fire = PType {
        source: Some("fireParticle"),
        c1: hex(0xFCF353),
        c2: hex(0xF83800),
        switch: 12,
        switch_loop: false,
        size: 0.25,
        size_range: 0.25,
        speed: 0.14,
        speed_range: 0.1,
        dir: -PI / 2.0,
        dir_range: PI / 6.0,
        life: 28,
        life_range: 10,
        scale_out: true,
        ..t("Fire")
    };
    v.push(PType {
        name: "BGTorch",
        ..fire.clone()
    });
    v.push(fire);
    let collect = PType {
        c1: hex(0x9878F8),
        c2: hex(0xB8B8F8),
        switch: 4,
        size: 2.0,
        size_range: 3.0,
        speed: 1.0,
        speed_range: 0.7,
        speed_mult: 0.91,
        dir_range: PI * 2.0,
        life: 26,
        life_range: 12,
        ..t("DarkOrbCollect")
    };
    for (n, a, b) in [
        ("TimeOrbCollect", 0xF8F8F8, 0xFFFFFF),
        ("LavaOrbCollect", 0xF83800, 0xF8B800),
        ("SpaceOrbCollect", 0xF8F8F8, 0xFFFFFF),
        ("PrismOrbCollect", 0xFF52FF, 0xFFD9FF),
    ] {
        v.push(PType {
            name: n,
            c1: hex(a),
            c2: hex(b),
            ..collect.clone()
        });
    }
    v.push(collect);
    v.push(PType {
        c1: hex(0xF83800),
        c2: hex(0xF8D878),
        switch: 2,
        size: 1.0,
        size_range: 1.0,
        speed: 1.2,
        speed_range: 0.4,
        dir_range: PI * 2.0,
        life: 12,
        life_range: 5,
        ..t("BramblesGrow")
    });
    let wings = PType {
        source: Some("featherParticle"),
        c1: Color::WHITE,
        c2: Color::WHITE * 0.5,
        switch: 50,
        size: 1.0 / 3.0,
        size_range: 2.0 / 3.0,
        speed: 0.6,
        speed_range: 0.2,
        speed_mult: 0.98,
        accel: Vec2::new(0.0, 0.02),
        dir: -PI / 2.0,
        dir_range: PI * 8.0 / 9.0,
        life: 60,
        life_range: 12,
        random_rotate: true,
        ..t("WingsDie")
    };
    v.push(PType {
        name: "DummyDie",
        c1: hex(0xFFDE8C),
        c2: hex(0xB59C63) * 0.4,
        ..wings.clone()
    });
    v.push(PType {
        name: "EnBirdDie",
        c1: Color::BLACK,
        c2: Color::BLACK * 0.5,
        ..wings.clone()
    });
    v.push(PType {
        name: "EnDarkBirdman",
        c1: hex(0x86A867),
        c2: hex(0x86A867) * 0.5,
        ..wings.clone()
    });
    v.push(wings);
    let lava = PType {
        c1: hex(0xF8B800),
        size: 1.0,
        size_range: 1.0,
        speed: 1.2,
        speed_range: 0.4,
        accel: Vec2::new(0.0, 0.08),
        dir: -PI / 2.0,
        dir_range: PI / 9.0,
        life: 18,
        life_range: 4,
        ..t("LavaHeatBottom")
    };
    v.push(PType {
        name: "LavaHeatTop",
        accel: Vec2::new(0.0, -0.08),
        dir: PI / 2.0,
        ..lava.clone()
    });
    v.push(PType {
        name: "LavaHeatLeft",
        accel: Vec2::new(-0.08, 0.0),
        dir: 0.0,
        ..lava.clone()
    });
    v.push(PType {
        name: "LavaHeatRight",
        accel: Vec2::new(0.08, 0.0),
        dir: PI,
        ..lava.clone()
    });
    v.push(lava);
    v.push(PType {
        c1: hex(0xD7E3EA),
        size: 1.0,
        size_range: 2.0,
        speed: 0.6,
        speed_range: 0.4,
        accel: Vec2::new(0.0, 0.04),
        dir: -PI / 2.0,
        dir_range: PI / 3.0,
        life: 20,
        life_range: 10,
        ..t("SnowClumpDisperse")
    });
    v.push(PType {
        c1: hex(0xB5E7F2),
        size: 1.0,
        size_range: 2.0,
        speed: 1.0,
        speed_range: 1.0,
        accel: Vec2::new(0.0, 0.05),
        dir_range: PI * 2.0,
        life: 16,
        life_range: 8,
        ..t("IcicleBreak")
    });
    v.push(PType {
        c1: hex(0xB5E7F2),
        size: 1.0,
        size_range: 1.0,
        speed: 0.4,
        speed_range: 0.6,
        accel: Vec2::new(0.0, 0.05),
        dir: PI / 2.0,
        dir_range: PI * 2.0 / 9.0,
        life: 10,
        life_range: 6,
        ..t("IcicleShift")
    });
    v.push(PType {
        c1: hex(0xCCC7BD),
        size: 1.0,
        size_range: 1.0,
        speed: 0.2,
        speed_range: 0.1,
        accel: Vec2::new(0.0, 0.05),
        dir: -PI / 2.0,
        dir_range: 1.745_329_3,
        life: 10,
        life_range: 6,
        ..t("BGSkeletonBreak")
    });
    let glass = PType {
        source: Some("glassParticle"),
        c1: Color::WHITE,
        c2: Color::WHITE * 0.5,
        switch: 18,
        switch_loop: false,
        size: 0.6,
        size_range: 0.6,
        speed: 0.6,
        speed_range: 0.2,
        accel: Vec2::new(0.0, 0.02),
        dir_range: PI * 2.0,
        life: 20,
        life_range: 10,
        ..t("MoonglassShatter")
    };
    v.push(glass);
    let portal = PType {
        source: Some("circleParticle"),
        c1: hex(0xFF2EFC),
        c2: hex(0xFF2EFC) * 0.4,
        switch: 16,
        switch_loop: false,
        size: 0.5,
        size_range: 0.5,
        speed: 0.2,
        speed_range: 0.1,
        dir_range: PI * 2.0,
        life: 20,
        life_range: 10,
        scale_out: true,
        ..t("EnPortalSpawn")
    };
    v.push(PType {
        name: "NextLevelPortalEnter",
        c1: hex(0x5CF5FF),
        c2: hex(0x5CF5FF) * 0.4,
        ..portal.clone()
    });
    v.push(portal);
    let mage = PType {
        source: Some("hollowCircleParticle"),
        c1: hex(0x85DEFF),
        c2: hex(0x85DEFF) * 0.4,
        switch: 18,
        switch_loop: false,
        size: 0.5,
        size_range: 0.5,
        speed: 0.4,
        speed_range: 0.1,
        speed_mult: 0.98,
        dir: PI / 2.0,
        dir_range: PI / 6.0,
        life: 24,
        life_range: 6,
        scale_out: true,
        ..t("EnTechnoMageFly")
    };
    v.push(PType {
        name: "EnTechnoMissile",
        speed: 0.1,
        speed_range: 0.04,
        speed_mult: 1.0,
        dir_range: PI * 2.0,
        ..mage.clone()
    });
    v.push(PType {
        name: "EnTechnoMageDead",
        c1: hex(0xEB1717),
        c2: hex(0xEB1717) * 0.4,
        ..mage.clone()
    });
    v.push(PType {
        name: "EnExploderChase",
        c1: hex(0xFFE07C),
        c2: hex(0xFFE07C) * 0.4,
        ..mage.clone()
    });
    v.push(mage);
    let sensor = PType {
        source: Some("circleParticle"),
        c1: hex(0x7D8C8A),
        c2: hex(0x7D8C8A) * 0.5,
        switch: 14,
        switch_loop: false,
        size: 0.625,
        size_range: 0.375,
        speed: 0.4,
        speed_range: 0.2,
        speed_mult: 0.96,
        dir: -PI / 2.0,
        dir_range: PI / 2.0,
        life: 18,
        life_range: 5,
        random_rotate: true,
        scale_out: true,
        ..t("SensorBlockDust")
    };
    v.push(PType {
        name: "SensorBlockDust2",
        switch: 60,
        size: 0.5,
        size_range: 0.25,
        speed: 0.02,
        speed_range: 0.01,
        speed_mult: 1.02,
        life: 90,
        life_range: 0,
        ..sensor.clone()
    });
    v.push(sensor);
    v.push(PType {
        c1: hex(0x7D8E39),
        c2: hex(0x7D8E39) * 0.5,
        switch: 10,
        switch_loop: false,
        size: 1.0,
        speed: 1.2,
        speed_range: 0.1,
        accel: Vec2::new(0.0, 0.05),
        dir: PI / 2.0,
        life: 14,
        life_range: 2,
        ..t("SensorBlockDust3")
    });
    v.push(PType {
        c1: hex(0xFFEB2E),
        c2: hex(0xFFF9C9),
        switch: 2,
        size: 1.0,
        size_range: 1.0,
        speed: 0.05,
        speed_range: 0.01,
        accel: Vec2::new(0.0, -0.01),
        dir_range: PI * 2.0,
        life: 12,
        life_range: 4,
        ..t("PickupSparkle")
    });
    let loop_wall = PType {
        c1: hex(0xDBFFF9),
        c2: hex(0x9BDFD4),
        switch: 12,
        switch_loop: false,
        size: 1.0,
        size_range: 1.0,
        speed: 0.1,
        speed_range: 0.1,
        speed_mult: 1.02,
        dir: 0.0,
        dir_range: 0.872_664_6,
        life: 20,
        life_range: 5,
        ..t("LoopPlatformWallLeft")
    };
    v.push(PType {
        name: "LoopPlatformWallRight",
        dir: PI,
        ..loop_wall.clone()
    });
    v.push(PType {
        name: "LoopPlatformWallUp",
        dir: PI / 2.0,
        ..loop_wall.clone()
    });
    let down = PType {
        name: "LoopPlatformWallDown",
        dir: -PI / 2.0,
        ..loop_wall.clone()
    };
    let pop_a = PType {
        name: "GhostPlatformPlayerPopA",
        life: 12,
        life_range: 4,
        ..down.clone()
    };
    v.push(PType {
        name: "GhostPlatformPlayerPopB",
        speed: 0.5,
        speed_mult: 0.98,
        life: 24,
        life_range: 6,
        ..pop_a.clone()
    });
    v.push(pop_a);
    v.push(down);
    v.push(loop_wall);
    v.push(PType {
        c1: hex(0x49C56D),
        size: 1.0,
        size_range: 1.0,
        speed: 0.6,
        speed_range: 0.8,
        accel: Vec2::new(0.0, 0.15),
        dir: -PI / 2.0,
        dir_range: PI * 2.0 / 9.0,
        life: 10,
        life_range: 2,
        ..t("MudPlayerRun")
    });
    v.push(PType {
        c1: hex(0xFF52FF),
        c2: hex(0xFFD9FF),
        switch: 3,
        size: 1.0,
        speed: 0.05,
        speed_range: 0.05,
        dir_range: PI * 2.0,
        life: 14,
        life_range: 6,
        ..t("PrismCorpse")
    });
    v.push(PType {
        c1: hex(0xFF52FF),
        c2: hex(0xFFD9FF),
        switch: 3,
        size: 1.0,
        speed: 1.0,
        speed_range: 2.0,
        speed_mult: 0.9,
        accel: Vec2::new(0.0, 0.1),
        dir: -PI / 2.0,
        dir_range: PI * 2.0 / 3.0,
        life: 24,
        life_range: 6,
        ..t("PrismVanish")
    });
    v.push(PType {
        c1: hex(0xFFEB2E),
        c2: hex(0xFFF9E1),
        switch: 3,
        size: 1.0,
        size_range: 2.0,
        speed: 1.5,
        speed_range: 0.8,
        speed_mult: 0.9,
        accel: Vec2::new(0.0, 0.05),
        life: 24,
        life_range: 6,
        ..t("SpeedBootsPickup")
    });
    v.push(PType {
        c1: hex(0xFFEB2E),
        c2: hex(0xFFF9E1),
        switch: 3,
        size: 1.0,
        size_range: 2.0,
        speed: 2.0,
        speed_range: 1.0,
        dir_range: PI * 2.0,
        life: 6,
        life_range: 2,
        ..t("SpeedBootsPickup2")
    });
    v.push(PType {
        c1: hex(0x667580),
        size: 1.0,
        size_range: 2.0,
        speed: 1.2,
        speed_mult: 0.95,
        dir_range: PI * 2.0,
        life: 20,
        life_range: 12,
        ..t("EnLichHornBreak")
    });
    v.push(PType {
        c1: hex(0xE55137),
        c2: hex(0xC2280B),
        switch: 4,
        size: 1.0,
        speed: 0.1,
        speed_range: 0.1,
        dir_range: PI * 2.0,
        life: 40,
        life_range: 8,
        ..t("EnBrambleSpore")
    });
    v.push(PType {
        c1: hex(0x7D8C8A),
        size: 2.0,
        size_range: 1.0,
        speed: 0.6,
        speed_range: 0.3,
        accel: Vec2::new(0.0, 0.06),
        dir: -PI / 2.0,
        dir_range: PI * 2.0 / 9.0,
        life: 20,
        life_range: 4,
        ..t("EnDreadTentacle")
    });
    // Ours (the sim's ShieldBreak / CatchShine have no named type in 9.4).
    v.push(PType {
        c1: hex(0xFFFFFF),
        c2: hex(0xA4E4FC),
        switch: 3,
        size: 1.0,
        size_range: 1.0,
        speed: 1.0,
        speed_range: 0.7,
        speed_mult: 0.91,
        dir_range: PI * 2.0,
        life: 20,
        life_range: 8,
        ..t("ShieldBreak")
    });
    v
}

/// Per-archer types (rendering.md 9.3): `kind` = Dash, HyperJump,
/// LaserArrowGlow, LaserArrowTrail, PlayerDust, PlayerFeathers.
pub fn archer_type(kind: &str, a: Color, b: Color) -> Option<PType> {
    let p = match kind {
        "Dash" => PType {
            name: "Dash",
            c1: b,
            c2: b,
            switch: 4,
            size: 1.0,
            size_range: 1.0,
            speed: 0.12,
            speed_range: 0.08,
            accel: Vec2::new(0.0, 0.015),
            dir_range: PI,
            life: 30,
            life_range: 15,
            ..BASE
        },
        "HyperJump" => PType {
            name: "HyperJump",
            source: Some("circleParticle"),
            c1: b * 0.8,
            c2: b * 0.8,
            switch: 4,
            switch_loop: false,
            size: 0.5,
            size_range: 0.375,
            speed: 0.12,
            speed_range: 0.08,
            accel: Vec2::new(0.0, 0.015),
            dir_range: PI,
            life: 8,
            life_range: 4,
            scale_out: true,
            ..BASE
        },
        "LaserArrowGlow" => PType {
            name: "LaserArrowGlow",
            c1: b,
            c2: b,
            switch: 4,
            size: 1.0,
            size_range: 1.0,
            life: 10,
            life_range: 4,
            ..BASE
        },
        "LaserArrowTrail" => PType {
            name: "LaserArrowTrail",
            source: Some("arrows/laserArrow"),
            c1: b,
            c2: b,
            switch: 2,
            size: 1.0,
            life: 8,
            life_range: 4,
            ..BASE
        },
        "PlayerDust" => PType {
            name: "PlayerDust",
            source: Some("circleParticle"),
            c1: b,
            c2: a * 0.5,
            switch: 14,
            switch_loop: false,
            size: 0.5,
            size_range: 0.375,
            speed: 0.4,
            speed_range: 0.2,
            speed_mult: 0.96,
            dir: -PI / 2.0,
            dir_range: PI / 2.0,
            life: 18,
            life_range: 5,
            scale_out: true,
            ..BASE
        },
        "PlayerFeathers" => PType {
            name: "PlayerFeathers",
            source: Some("featherParticle"),
            c1: b,
            c2: a * 0.5,
            switch: 50,
            size: 1.0 / 3.0,
            size_range: 2.0 / 3.0,
            speed: 0.6,
            speed_range: 0.2,
            speed_mult: 0.98,
            accel: Vec2::new(0.0, 0.02),
            dir: -PI / 2.0,
            dir_range: PI * 8.0 / 9.0,
            life: 60,
            life_range: 12,
            random_rotate: true,
            ..BASE
        },
        _ => return None,
    };
    Some(p)
}

/// The sim's particle kinds (by `Debug` name) -> (type, per-archer?, spread).
/// Unknown kinds (new sim features) fall back to a type of the same name in
/// the catalogue, else they are skipped (logged once).
pub fn sim_kind(kind: &str) -> Option<(&'static str, bool, Vec2)> {
    Some(match kind {
        "Dust" => ("PlayerDust", true, Vec2::new(4.0, 1.0)),
        "Dash" => ("Dash", true, Vec2::new(3.0, 3.0)),
        "Hyper" => ("HyperJump", true, Vec2::new(3.0, 3.0)),
        "JumpPadTrail" => ("JumpPadTrail", false, Vec2::new(3.0, 2.0)),
        "ShieldBreak" => ("ShieldBreak", false, Vec2::new(5.0, 6.0)),
        "WingsBurn" => ("PlayerFeathers", true, Vec2::new(6.0, 4.0)),
        "CatchShine" => ("PickupSparkle", false, Vec2::new(4.0, 4.0)),
        "DrillDig" => ("DrillArrowDig", false, Vec2::new(3.0, 3.0)),
        "PrismDebris" => ("PrismCorpse", false, Vec2::new(4.0, 4.0)),
        "PrismVanish" => ("PrismVanish", false, Vec2::new(4.0, 4.0)),
        "SpeedBoots" => ("SpeedBootsPickup", false, Vec2::new(4.0, 4.0)),
        "ChestOpen" => ("TreasureChestGlow", false, Vec2::new(8.0, 6.0)),
        "FeatherTrail" => ("PlayerFeathers", true, Vec2::new(2.0, 2.0)),
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    ty: u16,
    pos: Vec2,
    vel: Vec2,
    color: Color,
    alt: bool,
    switch_t: u32,
    life: f32,
    size: f32,
    size_change: f32,
    rot: f32,
}

/// A ring buffer of particles (`ParticleSystem`).
pub struct Particles {
    pub types: Vec<PType>,
    buf: Vec<Option<Particle>>,
    /// Per system (BG, normal, FG): its slice of `buf` (start, length) and
    /// the next slot in it; one shared ring when not `layered`.
    rings: [(usize, usize); 3],
    next: [usize; 3],
    layered: bool,
    rng: u64,
}

impl Particles {
    /// One ring of `capacity` for every particle type.
    pub fn new(capacity: usize) -> Self {
        Self {
            types: catalogue(),
            buf: vec![None; capacity],
            rings: [(0, capacity); 3],
            next: [0; 3],
            layered: false,
            rng: 0x0BAD_5EED,
        }
    }

    /// The level's three systems with their own capacities
    /// (rendering.md 4.3): `ParticlesBG` 600, `Particles` 500,
    /// `ParticlesFG` 400; a full system overwrites its oldest particle.
    pub fn level() -> Self {
        let mut p = Self::new(1500);
        p.rings = [(0, 600), (600, 500), (1100, 400)];
        p.layered = true;
        p
    }

    fn ring_of(&self, ty: usize) -> usize {
        if !self.layered {
            return 0;
        }
        match layer_of(self.types[ty].name) {
            PLayer::Bg => 0,
            PLayer::Mid => 1,
            PLayer::Fg => 2,
        }
    }

    fn rand(&mut self) -> f32 {
        (crate::front::variants::next_rand(&mut self.rng) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Index of a type (registered on first use).
    pub fn type_index(&mut self, t: PType) -> usize {
        if let Some(i) = self.types.iter().position(|x| *x == t) {
            return i;
        }
        self.types.push(t);
        self.types.len() - 1
    }

    pub fn by_name(&self, name: &str) -> Option<usize> {
        self.types.iter().position(|t| t.name == name)
    }

    /// `Emit(type, amount, pos, range, direction?)`.
    pub fn emit(&mut self, ty: usize, amount: usize, pos: Vec2, range: Vec2, dir: Option<f32>) {
        for _ in 0..amount {
            let p = pos
                + Vec2::new(
                    (self.rand() - 0.5) * 2.0 * range.x,
                    (self.rand() - 0.5) * 2.0 * range.y,
                );
            let t = self.types[ty].clone();
            let speed = t.speed + self.rand() * t.speed_range;
            let a = dir.unwrap_or(t.dir) - t.dir_range / 2.0 + self.rand() * t.dir_range;
            let life = t.life as f32
                + if t.life_range > 0 {
                    (self.rand() * t.life_range as f32).floor()
                } else {
                    0.0
                };
            let mut size = t.size + self.rand() * t.size_range;
            if t.source.is_none() {
                size = size.trunc().max(1.0);
            }
            let rot = if t.random_rotate {
                self.rand() * PI * 2.0
            } else if t.rotated {
                a
            } else {
                0.0
            };
            let part = Particle {
                ty: ty as u16,
                pos: p,
                vel: Vec2::new(a.cos(), a.sin()) * speed,
                color: t.c1,
                alt: false,
                switch_t: t.switch,
                life,
                size,
                size_change: if t.scale_out {
                    -size / (2.0 * life.max(1.0))
                } else {
                    0.0
                },
                rot,
            };
            let r = self.ring_of(ty);
            let (start, len) = self.rings[r];
            self.buf[start + self.next[r]] = Some(part);
            self.next[r] = (self.next[r] + 1) % len;
        }
    }

    /// One frame (`time_mult` = the sim's TimeMult).
    pub fn update(&mut self, time_mult: f32) {
        for slot in &mut self.buf {
            let Some(p) = slot else { continue };
            p.life -= time_mult;
            if p.life <= 0.0 {
                *slot = None;
                continue;
            }
            let t = &self.types[p.ty as usize];
            if p.switch_t > 0 {
                p.switch_t -= 1;
                if p.switch_t == 0 {
                    p.alt = !p.alt;
                    p.color = if p.alt { t.c2 } else { t.c1 };
                    if t.switch_loop {
                        p.switch_t = t.switch;
                    }
                }
            }
            p.pos += p.vel * time_mult;
            p.vel += t.accel * time_mult;
            if t.speed_mult != 1.0 {
                p.vel *= t.speed_mult.powf(time_mult);
            }
            p.size += p.size_change * time_mult;
        }
    }

    #[allow(dead_code)]
    pub fn active(&self) -> usize {
        self.buf.iter().filter(|p| p.is_some()).count()
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|p| *p = None);
    }

    /// The live particles (a replay frame keeps them).
    pub fn snapshot(&self) -> Vec<Particle> {
        self.buf.iter().flatten().copied().collect()
    }

    /// Draws the particles of one of the level's three systems in slot
    /// order; `sprite` resolves a main-atlas name (else a flat square).
    pub fn draw_layer(
        &self,
        dl: &mut DrawList,
        sprite: &dyn Fn(&str) -> Option<SubTexture>,
        layer: PLayer,
    ) {
        self.draw_some(self.buf.iter().flatten(), dl, sprite, layer);
    }

    /// Draws `list` (live or a replay frame's) of one layer.
    pub fn draw_some<'a>(
        &self,
        list: impl Iterator<Item = &'a Particle>,
        dl: &mut DrawList,
        sprite: &dyn Fn(&str) -> Option<SubTexture>,
        layer: PLayer,
    ) {
        let layers: Vec<PLayer> = self.types.iter().map(|t| layer_of(t.name)).collect();
        for p in list {
            if layers[p.ty as usize] != layer {
                continue;
            }
            let t = &self.types[p.ty as usize];
            let pos = p.pos.floor();
            match t.source.and_then(sprite) {
                Some(s) => dl.sprite(
                    s,
                    &SpriteParams::at(pos)
                        .origin(s.rect.size() / 2.0)
                        .scale(Vec2::splat(p.size))
                        .rotation(p.rot)
                        .color(p.color),
                ),
                None => {
                    let s = p.size.round().max(1.0);
                    dl.rect(
                        pos.x - (s / 2.0).floor(),
                        pos.y - (s / 2.0).floor(),
                        s,
                        s,
                        p.color,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_systems_have_their_own_rings() {
        let mut ps = Particles::level();
        let bg = ps.by_name("BGTorch").unwrap();
        let fg = ps.by_name("Fire").unwrap();
        let mid = ps.by_name("IcicleBreak").unwrap();
        ps.emit(fg, 10, Vec2::ZERO, Vec2::ZERO, None);
        // Filling the BG system past its 600 leaves the others alone.
        ps.emit(bg, 700, Vec2::ZERO, Vec2::ZERO, None);
        ps.emit(mid, 5, Vec2::ZERO, Vec2::ZERO, None);
        let snap = ps.snapshot();
        let count = |t: usize| snap.iter().filter(|p| p.ty as usize == t).count();
        assert_eq!((count(bg), count(mid), count(fg)), (600, 5, 10));
    }

    #[test]
    fn ring_buffer_and_life() {
        let mut ps = Particles::new(8);
        let i = ps.by_name("CrackedBlockBreak").unwrap();
        ps.emit(i, 12, Vec2::new(10.0, 10.0), Vec2::ZERO, None);
        assert_eq!(ps.active(), 8, "the oldest are overwritten");
        for _ in 0..12 {
            ps.update(1.0);
        }
        assert_eq!(ps.active(), 0, "life 8 + Next(3) frames");
        assert!(ps.by_name("ExplodingOrbGlow").is_some());
        assert!(catalogue().len() > 50);
        assert!(archer_type("PlayerDust", Color::RED, Color::BLUE).is_some());
        assert!(sim_kind("Dust").is_some() && sim_kind("Nope").is_none());
    }

    #[test]
    fn scale_out_halves() {
        let mut ps = Particles::new(4);
        let t = PType {
            life: 10,
            life_range: 0,
            size: 2.0,
            scale_out: true,
            source: Some("x"),
            ..BASE
        };
        let i = ps.type_index(t);
        ps.emit(i, 1, Vec2::ZERO, Vec2::ZERO, None);
        for _ in 0..9 {
            ps.update(1.0);
        }
        let p = ps.buf.iter().flatten().next().unwrap();
        assert!((p.size - 1.1).abs() < 1e-4, "{}", p.size);
    }
}

#![allow(dead_code)]
//! Test harness: ASCII arenas, scripted inputs, player probes.

use tfp_sim::entity::{Core, EntityId, Kind, Tag};
use tfp_sim::player::{PState, Player};
use tfp_sim::{Content, MatchSetup, PlayerInput, PlayerSetup, Vec2, World};

/// 32x24 cells: walls left/right, floor top at y = 220, a block at
/// cols 10-14 / rows 13-14 (top y = 130), a jump-thru at cols 26-30 y = 180.
pub const ARENA: [&str; 24] = [
    "################################",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#.........#####................#",
    "#.........#####................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#.........................-----#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "################################",
    "################################",
];

/// Open on both sides at the floor rows (for wrap tests).
pub const OPEN: [&str; 24] = [
    "################################",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "################################",
    "################################",
];

/// Feet on the arena floor.
pub const FLOOR_Y: f32 = 212.0;

pub struct Sim {
    pub c: Content,
    pub w: World,
}

impl Sim {
    pub fn new(map: &[&str], spawns: &[Vec2]) -> Sim {
        Sim::with(map, spawns, |_| {})
    }

    pub fn with(map: &[&str], spawns: &[Vec2], f: impl FnOnce(&mut MatchSetup)) -> Sim {
        let c = Content::from_ascii(map);
        let mut setup = MatchSetup::default();
        for (i, &s) in spawns.iter().enumerate() {
            setup.players[i] = Some(PlayerSetup {
                archer: i as u16,
                spawn: Some(s),
                ..Default::default()
            });
        }
        f(&mut setup);
        let w = World::new(&c, &setup);
        let mut s = Sim { c, w };
        // Let the archers enter the scene and settle one frame.
        s.step(&[]);
        s
    }

    pub fn step(&mut self, inputs: &[PlayerInput]) {
        self.w.step(&self.c, inputs);
    }

    pub fn step1(&mut self, i: PlayerInput) {
        self.step(&[i]);
    }

    pub fn steps(&mut self, n: usize, i: PlayerInput) {
        for _ in 0..n {
            self.step1(i);
        }
    }

    pub fn pid(&self, index: u8) -> Option<EntityId> {
        self.w.player_by_index(index)
    }

    pub fn p(&self, index: u8) -> Player {
        *self
            .w
            .player(self.pid(index).expect("player alive"))
            .unwrap()
    }

    pub fn core(&self, index: u8) -> Core {
        *self.w.core(self.pid(index).expect("player alive"))
    }

    pub fn pos(&self, index: u8) -> Vec2 {
        self.core(index).pos
    }

    pub fn state(&self, index: u8) -> PState {
        self.p(index).state()
    }

    pub fn arrows(&self) -> Vec<(EntityId, tfp_sim::arrow::Arrow)> {
        self.w
            .tagged(Tag::Arrow)
            .iter()
            .filter_map(|&id| match self.w.kind(id) {
                Some(Kind::Arrow(a)) => Some((id, *a)),
                _ => None,
            })
            .collect()
    }

    pub fn corpses(&self) -> Vec<EntityId> {
        self.w.tagged(Tag::Corpse).to_vec()
    }
}

pub const NONE: PlayerInput = PlayerInput {
    aim_x: 0,
    aim_y: 0,
    buttons: 0,
    _pad: 0,
};

pub fn inp(x: i8, y: i8, buttons: u8) -> PlayerInput {
    PlayerInput::dpad(x, y, buttons)
}

pub const JUMP: u8 = PlayerInput::JUMP;
pub const SHOOT: u8 = PlayerInput::SHOOT;
pub const DODGE: u8 = PlayerInput::DODGE;

//! # tfp-sim
//!
//! The deterministic, rollback-able simulation of towerfall-port: everything
//! inside a match, as a pure function `(World, inputs) -> World`. No I/O, no
//! rendering, no wall clock, no threads, no `HashMap` iteration; f32 math
//! like the original with `libm` transcendentals. Behaviour comes from
//! `docs/spec/*.md`; the shape is described in `docs/sim-design.md`.
//!
//! | Module | What |
//! |---|---|
//! | [`world`] | [`World`] (entity arena, Monocle list semantics, tick), [`MatchSetup`] |
//! | [`content`] | [`Content`]: immutable level geometry, sprite tables, archers |
//! | [`time`] | `TimeRate`/`TimeMult`, `OnInterval`, slow motion |
//! | [`components`] | Counter, Alarm, Tween/Ease, Scheduler, SineWave, Wiggler, StateMachine, Coroutine, sprite clock |
//! | [`rng`] | the .NET `System.Random` generator |
//! | [`collide`] | colliders and every collision test, wrap copies |
//! | [`actor`] | `MoveH`/`MoveV`/`MoveExact*`, riding, squish dispatch, wrap helpers |
//! | [`platform`] | `Solid.Move`, `JumpThru.Move`, static and moving platforms |
//! | [`player`] | the archer |
//! | [`arrow`], [`quiver`] | the base arrow, the quiver |
//! | [`arrows`] | arrow types: bomb, laser, bramble, drill, bolt, toy, feather, trigger, prism |
//! | [`explosion`], [`hurt`] | explosions, shocks, the archer's damage entry points |
//! | [`pickups`], [`treasure`] | items, orbs, lava, chests, treasure selection |
//! | [`corpse`], [`hat`] | bodies and dropped hats |
//! | [`entities`] | tower entities: platforms, blocks, surfaces, hazards, scenery |
//! | [`view`] | flat drawable list for renderers |
//! | [`events`] | sounds, particles, rumble for the presentation layer |

pub mod actor;
pub mod arrow;
pub mod arrows;
pub mod bot;
pub mod checksum;
pub mod collide;
pub mod components;
pub mod content;
pub mod coop;
pub mod corpse;
pub mod darkworld;
pub mod enemies;
pub mod entities;
pub mod entity;
pub mod events;
pub mod explosion;
pub mod hat;
pub mod hurt;
pub mod input;
pub mod loader;
pub mod math;
pub mod pickups;
pub mod platform;
pub mod player;
pub mod quest;
pub mod quiver;
pub mod revive;
pub mod rng;
pub mod time;
pub mod treasure;
pub mod trials;
pub mod variant_fx;
pub mod variants;
pub mod versus;
pub mod view;
pub mod view_items;
pub mod world;

pub use content::Content;
pub use entity::{EntityId, Tag};
pub use events::Event;
pub use input::PlayerInput;
pub use math::Vec2;
pub use view::{Drawable, View, ViewOptions, Visual};
pub use world::{MatchSetup, PlayerSetup, World};

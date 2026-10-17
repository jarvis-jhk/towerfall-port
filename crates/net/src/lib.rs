//! # tfp-net
//!
//! Online play for towerfall-port: GGRS rollback sessions whose packets
//! travel over Warpzone's encrypted peer channels, and a small
//! host-authoritative lobby/match protocol on the reliable channel. Generic
//! over the simulation ([`NetGame`]); TowerFall's input is [`TfInput`]. See
//! `docs/net.md`.
//!
//! | Module | What |
//! |---|---|
//! | [`session`] | [`NetSession`]: rooms, lobby, match start/end, `advance()` / `step()` |
//! | [`game`] | [`NetGame`] (the sim as tfp-net sees it), [`Request`], [`fulfil`] |
//! | `matchrun` | GGRS sessions of a match, prediction limit, freeze/resume, spectator stream |
//! | [`input`] | [`TfInput`] and the `Pod` → GGRS input adapter |
//! | [`wire`] | channels, message types, framing |
//! | [`lobby`] | host-side slot/archer/team assignment |
//! | [`transport`] | GGRS socket over Warpzone, host forwarding, link simulation |
//! | [`backend`] | Warpzone client or in-process [`MemHub`] rooms |
//! | [`toy`] | a deterministic toy game, bots, a test driver |

pub mod backend;
pub mod game;
pub mod input;
pub mod lobby;
mod matchrun;
pub mod session;
pub mod toy;
pub mod transport;
pub mod wire;

pub use backend::{MemEndpoint, MemHub};
pub use game::{NetGame, Request, StateCell, Stepped, fulfil};
pub use input::{NetInput, TfInput};
pub use session::{
    DesyncPolicy, GAME_ID, InputStatus, LaunchAction, MachineStats, NetConfig, NetError, NetEvent,
    NetSession, NetStats, NetworkStats,
};
pub use transport::{LinkSim, TransportStats};
pub use warpzone_core;
pub use wire::{
    DesyncReport, DoneReport, GgrsParams, LobbyState, MAX_SLOTS, MachineId, MachineView,
    MatchSettings, MatchStart, Phase, PlayerSlot, SKIN_ALT, SKIN_NORMAL, SKIN_SECRET, SlotRequest,
    TeamMode,
};

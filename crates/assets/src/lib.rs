//! Readers for the original game's data (atlases, sprite data, levels, audio, fonts).
//!
//! Everything here reads the player's own copy of the game (read-only) and turns
//! it into plain Rust data. Format notes live in `docs/formats/*.md`.
//!
//! Entry point: [`install::GameInstall::discover`] finds the installs, then the
//! individual readers ([`atlas`], [`sprite_data`], [`game_data`], [`level`],
//! [`towers`], [`workshop`], [`audio`], [`xnb`]) load what is needed.

pub mod atlas;
pub mod audio;
pub mod autotile;
pub mod dotnet_random;
pub mod error;
pub mod game_data;
pub mod image;
pub mod install;
pub mod level;
pub mod level_tiles;
pub mod look;
pub mod procedural;
pub mod quest_data;
pub mod savedata;
pub mod sprite_data;
pub mod towers;
pub mod workshop;
pub mod xml;
pub mod xnb;

pub use error::{Error, Result};
pub use install::{ContentSet, GameInstall, Installs};

#[cfg(test)]
pub(crate) mod testutil {
    use crate::install::Installs;
    use std::sync::OnceLock;

    /// The installs found on this machine (env overrides honoured). Tests that
    /// need real data call this and return early when the data is absent.
    pub fn installs() -> &'static Installs {
        static I: OnceLock<Installs> = OnceLock::new();
        I.get_or_init(Installs::discover)
    }

    /// Skip helper: prints a note and evaluates to `None` if data is missing.
    macro_rules! require {
        ($opt:expr, $what:expr) => {
            match $opt {
                Some(v) => v,
                None => {
                    eprintln!("skipping: {} not found", $what);
                    return;
                }
            }
        };
    }
    pub(crate) use require;
}

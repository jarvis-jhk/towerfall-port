//! Online play (M8): tfp-net's rollback sessions over Warpzone wired to the
//! real simulation and front end. See docs/net.md, "The game side".
//!
//! | Module | What |
//! |---|---|
//! | [`adapter`] | `NetWorld` (`tfp_net::NetGame` for `tfp_sim::World`), `OnlineSettings`, tower loading, the sim's Versus settings from a `MatchStart` |
//! | [`coop`] | online Quest / Trials: content, settings, the starting world |
//! | [`gate`] | `EventGate`: sim events under rollback (first-seen / confirmed / loops) |
//! | [`runner`] | `OnlineMatch`: one match without graphics (requests, gate, match end) |
//! | [`present`] | sounds for gated events, screen shake |
//! | [`scene`] | `OnlineScene`: host/join/browser/lobby menus and the match on screen |
//! | [`bot`] | input bot for tests |
//! | [`harness`] | `--net-harness` / `--net-peer`: headless multi-process tests |
//! | [`soak`] | `--net-soak`: many random matches (towers, modes, players, variants, link) |

pub mod adapter;
pub mod coop;
pub mod gate;
pub mod harness;
pub mod present;
pub mod runner;
pub mod scene;
pub mod soak;

use tfp_net::NetSession;

use adapter::NetWorld;

/// Machines only play together with the same game and net protocol.
pub fn net_version() -> String {
    format!(
        "towerfall-port {} net{}",
        env!("CARGO_PKG_VERSION"),
        tfp_net::wire::PROTOCOL
    )
}

/// A join code (`K7QM3X`, typed by players) rather than a room id
/// (`v7thncejf4`, from the launcher or an invite).
pub fn is_code(s: &str) -> bool {
    (4..=8).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Join by code or room id / invite link (`--warpzone-join VALUE`).
pub fn join(net: &mut NetSession<NetWorld>, value: &str) {
    let v = value.trim();
    if is_code(v) {
        net.join_code(&v.to_ascii_uppercase());
    } else {
        net.handle_launch(&["--warpzone-join", v]);
    }
}

/// What the launcher asked us to join, as a value [`join`] takes: a private
/// game's code (`--warpzone-code`, which also finds a listed room that
/// wants one), else the room with its invite secret, else the room.
pub fn launch_join(intent: &tfp_net::warpzone_core::launch::Intent) -> Option<String> {
    if let Some(code) = &intent.code {
        return Some(code.clone());
    }
    let room = intent.join.as_ref()?;
    Some(match &intent.secret {
        Some(s) => format!("warpzone://join/{room}#{s}"),
        None => room.clone(),
    })
}

#[cfg(test)]
mod tests {
    use tfp_net::warpzone_core::launch::from_args;

    #[test]
    fn launch_targets() {
        let j = |a: &[&str]| super::launch_join(&from_args(a));
        assert_eq!(j(&[]), None);
        assert_eq!(
            j(&["--warpzone-join", "v7thncejf4"]).as_deref(),
            Some("v7thncejf4")
        );
        assert_eq!(
            j(&["warpzone://join/v7thncejf4#s3cret"]).as_deref(),
            Some("warpzone://join/v7thncejf4#s3cret")
        );
        assert_eq!(j(&["--warpzone-code", "k7qm3x"]).as_deref(), Some("K7QM3X"));
        assert_eq!(
            j(&["--warpzone-join", "v7thncejf4", "--warpzone-code=K7QM3X"]).as_deref(),
            Some("K7QM3X")
        );
        // What the session makes of an invite value again.
        let i = from_args(&["--warpzone-join", "warpzone://join/v7thncejf4#s3cret"]);
        assert_eq!(i.join.as_deref(), Some("v7thncejf4"));
        assert_eq!(i.secret.as_deref(), Some("s3cret"));
    }

    #[test]
    fn codes_and_rooms() {
        assert!(super::is_code("K7QM3X"));
        assert!(super::is_code("k7qm3x"));
        assert!(!super::is_code("v7thncejf4"));
        assert!(!super::is_code("warpzone://join/ab12cd"));
    }
}

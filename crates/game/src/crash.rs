//! Crash reports: a panic in a windowed run is written to
//! `<data dir>/crash-report.txt`; the next start sends it to the Warpzone
//! server's feedback endpoint (`WARPZONE_SERVER`, else the public instance)
//! from a background thread and removes the file once it went through.
//! Only panics count (a missing game folder is not a fault). Nothing
//! personal goes out: the backtrace keeps our own frames, and warpzone-core
//! replaces the home directory and login name. Sending is opt-in: OPTIONS >
//! SEND CRASH REPORTS (`crash_reports` in options.cfg), or
//! `TFP_CRASH_REPORTS=1` / `=0` to override; the file is written either way.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tfp_net::warpzone_core;

pub const FILE: &str = "crash-report.txt";
const GAME: &str = "towerfall-port";
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A report as stored on disk: the first line is the title (stable for the
/// same fault, so the server counts repeats), the rest the detail.
#[derive(Debug, PartialEq)]
pub struct Report {
    pub title: String,
    pub detail: String,
}

impl Report {
    pub fn to_text(&self) -> String {
        format!("{}\n{}", self.title, self.detail)
    }

    pub fn parse(text: &str) -> Option<Report> {
        let (title, detail) = text.split_once('\n').unwrap_or((text, ""));
        let title = title.trim();
        (!title.is_empty()).then(|| Report {
            title: title.to_string(),
            detail: detail.to_string(),
        })
    }
}

/// Title and detail for a panic at `location` with `message`; `backtrace`
/// is cut to the frames of our own crates.
pub fn panic_report(location: &str, message: &str, backtrace: &str) -> Report {
    let ours: Vec<&str> = backtrace
        .lines()
        .filter(|l| l.contains("tfp_") || l.contains("towerfall_port") || l.contains("crates/"))
        .take(40)
        .collect();
    Report {
        title: format!("panic at {location}"),
        detail: format!(
            "version {VERSION} ({} {})\n{message}\n\n{}\n",
            std::env::consts::OS,
            std::env::consts::ARCH,
            ours.join("\n")
        ),
    }
}

fn path() -> Option<PathBuf> {
    crate::options_store::data_dir().map(|d| d.join(FILE))
}

/// Chain a panic hook that writes the report (the default hook still
/// prints the panic).
pub fn install() {
    let Some(file) = path() else { return };
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        prev(info);
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "?".into());
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let report = panic_report(&location, &message, &bt);
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&file, report.to_text()).is_ok() {
            eprintln!(
                "crash report saved to {}; it is sent on the next start",
                file.display()
            );
        }
    }));
}

/// Whether reports go out: the option, unless `TFP_CRASH_REPORTS` says.
pub fn enabled(option: bool) -> bool {
    match std::env::var("TFP_CRASH_REPORTS").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => option,
    }
}

/// Send a report left by an earlier crash, fire and forget.
pub fn send_pending() {
    let Some(file) = path() else { return };
    let Ok(text) = std::fs::read_to_string(&file) else {
        return;
    };
    let Some(report) = Report::parse(&text) else {
        let _ = std::fs::remove_file(&file);
        return;
    };
    let _ = std::thread::Builder::new()
        .name("crash-report".into())
        .spawn(move || send(&file, &report));
}

fn send(file: &Path, report: &Report) {
    let api = warpzone_core::api::Api::new(
        &warpzone_core::server_url(),
        Arc::new(warpzone_core::identity::Identity::generate()),
    );
    let req = warpzone_core::feedback::request(
        GAME,
        "crash",
        &report.title,
        &report.detail,
        Some(VERSION),
        &warpzone_core::feedback::client_name(GAME, VERSION),
    );
    match warpzone_core::feedback::send(&api, &req) {
        Ok(_) => {
            let _ = std::fs::remove_file(file);
            log::info!("crash report from the last run sent");
        }
        Err(e) => log::warn!("crash report not sent (kept for the next start): {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_round_trips() {
        let r = panic_report(
            "crates/sim/src/world.rs:12",
            "index out of bounds",
            "   0: std::panicking\n   7: tfp_sim::world::World::step\n  at ./crates/sim/src/world.rs:12\n   9: main",
        );
        assert_eq!(r.title, "panic at crates/sim/src/world.rs:12");
        assert!(r.detail.contains("index out of bounds"));
        assert!(r.detail.contains("tfp_sim::world::World::step"));
        assert!(!r.detail.contains("std::panicking"));
        assert_eq!(Report::parse(&r.to_text()), Some(r));
    }

    /// Against a running server: `WARPZONE_SERVER=… cargo test -p
    /// towerfall-port crash -- --ignored` (scripts/net/dev-server.sh).
    #[test]
    #[ignore]
    fn sends_to_the_server() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("../../scratch/crash-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(FILE);
        let r = panic_report("crates/game/src/crash.rs:1", "test report", "");
        std::fs::write(&file, r.to_text()).unwrap();
        send(&file, &r);
        assert!(!file.exists(), "not sent");
    }

    #[test]
    fn empty_file_is_no_report() {
        assert_eq!(Report::parse(""), None);
        assert_eq!(Report::parse("  \nx"), None);
    }
}

//! Crash safety net.
//!
//! The chemistry core is written and tested to never panic, but as a second
//! line of defence every lab edit runs inside `catch_unwind` (see
//! `Chem::edit`): if a bug ever panics, the lab is rolled back to its
//! previous state and the game keeps running. Panics are also appended to a
//! crash log so they can be reported.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

/// Per-platform log location:
/// - Windows: `%LOCALAPPDATA%\Valence\crash.log`
/// - macOS:   `~/Library/Logs/Valence/crash.log`
/// - Linux:   `$XDG_STATE_HOME/valence/crash.log` (or `~/.local/state/...`)
pub fn log_path() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let dir = if cfg!(target_os = "windows") {
        env("LOCALAPPDATA")
            .or_else(|| env("APPDATA"))
            .map(|d| d.join("Valence"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Logs/Valence"))
    } else {
        env("XDG_STATE_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".local/state")))
            .map(|d| d.join("valence"))
    };
    dir.unwrap_or_else(std::env::temp_dir).join("crash.log")
}

/// Install a panic hook that records panics to the crash log, then defers to
/// the default hook (which prints to stderr).
pub fn install_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let when = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backtrace = std::backtrace::Backtrace::force_capture();
        let entry = format!(
            "---- panic at unix time {when} (Valence {}, {} {})\n{info}\n{backtrace}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        );
        let path = log_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(entry.as_bytes());
        }
        default_hook(info);
    }));
}

/// Run `f`, turning a panic into `None` instead of unwinding further.
pub fn guard<R>(what: &str, f: impl FnOnce() -> R) -> Option<R> {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => Some(r),
        Err(_) => {
            bevy::log::error!(
                "recovered from a panic in {what}; see {}",
                log_path().display()
            );
            None
        }
    }
}

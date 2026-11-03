//! Tiny persistent settings file (`key = value` lines), stored in the
//! platform's config folder:
//! - Windows: `%APPDATA%\Valence\settings.txt`
//! - macOS:   `~/Library/Application Support/Valence/settings.txt`
//! - Linux:   `$XDG_CONFIG_HOME/valence/settings.txt` (or `~/.config/valence/...`)
//!
//! Reading never fails (missing or garbled files give defaults) and writing
//! is best-effort, so a read-only or odd filesystem can't break the game.

use std::{fs, path::PathBuf};

pub fn path() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let dir = if cfg!(target_os = "windows") {
        env("APPDATA").map(|d| d.join("Valence"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support/Valence"))
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|d| d.join("valence"))
    };
    dir.unwrap_or_else(std::env::temp_dir).join("settings.txt")
}

/// Value for `key`, if present.
pub fn get(key: &str) -> Option<String> {
    let text = fs::read_to_string(path()).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// Set `key = value`, keeping other lines. Returns false if it couldn't be saved.
pub fn set(key: &str, value: &str) -> bool {
    let path = path();
    let mut lines: Vec<String> = fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.split_once('=').is_none_or(|(k, _)| k.trim() != key))
        .map(str::to_string)
        .collect();
    lines.push(format!("{key} = {value}"));
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    fs::write(&path, lines.join("\n") + "\n").is_ok()
}

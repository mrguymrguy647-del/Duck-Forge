//! The Studio's settings: which folder the games are in, the theme, what
//! was showing last time. Saved as a small JSON file in the user's own
//! settings folder, never in the games folder.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The folder the games are in: a Duck Forge folder. `None` means
    /// "look for one next to the Studio".
    pub games_folder: Option<PathBuf>,
    /// "dark", "light" or "system".
    pub theme: Option<String>,
    /// Where Duck Quill is, once the Studio has been told.
    pub duck_quill: Option<PathBuf>,
    /// Show the Timing line under the game.
    pub show_timing: bool,
    /// Show the Console panel.
    pub show_console: bool,
    /// How many parts each game's last build had, for the progress bar.
    pub build_parts: BTreeMap<String, u32>,
}

/// Where the settings are saved. `DUCK_FORGE_STUDIO_SETTINGS` names
/// another file (tests use it, so they never touch a person's settings).
pub fn settings_file() -> Option<PathBuf> {
    if let Some(file) = std::env::var_os("DUCK_FORGE_STUDIO_SETTINGS") {
        return Some(PathBuf::from(file));
    }
    let folder = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?).join("Duck Forge Studio")
    } else if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config).join("duck-forge-studio")
    } else {
        PathBuf::from(std::env::var_os("HOME")?)
            .join(".config")
            .join("duck-forge-studio")
    };
    Some(folder.join("settings.json"))
}

impl Settings {
    /// The saved settings, or fresh ones if there are none (or they can't
    /// be read: a broken settings file never stops the Studio).
    pub fn load() -> Settings {
        settings_file()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Saves them. Failing is fine: the Studio works without.
    pub fn save(&self) {
        let Some(file) = settings_file() else {
            return;
        };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(file, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_being_written_and_read_and_tolerate_old_files() {
        let mut settings = Settings {
            games_folder: Some(PathBuf::from("C:/Duck Forge")),
            show_timing: true,
            ..Settings::default()
        };
        settings.build_parts.insert(String::from("dodge"), 14);
        let text = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), settings);
        // A file from an older Studio, with only some of the fields.
        let old: Settings = serde_json::from_str(r#"{"theme": "light"}"#).unwrap();
        assert_eq!(old.theme.as_deref(), Some("light"));
        assert!(!old.show_console && old.games_folder.is_none());
    }
}

//! The Studio's settings: which folder the games are in, the theme, what
//! was showing last time. Saved as a small JSON file in the user's own
//! settings folder, never in the games folder.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The file these settings are saved in: the person's own settings
    /// file for [`Settings::load`], nowhere for [`Settings::default`] (so a
    /// test never touches a person's settings).
    #[serde(skip)]
    pub file: Option<PathBuf>,
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
    /// Look for a newer Duck Forge Studio when it starts (Settings, "Tell
    /// me about new versions"). On unless the person turns it off.
    pub check_for_updates: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            file: None,
            games_folder: None,
            theme: None,
            duck_quill: None,
            show_timing: false,
            show_console: false,
            build_parts: BTreeMap::new(),
            check_for_updates: true,
        }
    }
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
        let file = settings_file();
        let mut settings: Settings = file
            .as_ref()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        settings.file = file;
        settings
    }

    /// Saves them in their file, if they have one. Failing is fine: the
    /// Studio works without.
    pub fn save(&self) {
        let Some(file) = &self.file else {
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
        assert!(old.check_for_updates, "on unless it's turned off");
        let off: Settings = serde_json::from_str(r#"{"check_for_updates": false}"#).unwrap();
        assert!(!off.check_for_updates);
    }

    #[test]
    fn settings_are_saved_in_their_own_file_and_nowhere_else() {
        let folder = crate::test_folder::TestFolder::new("settings");
        Settings::default().save(); // has no file: saves nothing
        let mut settings = folder.settings();
        settings.show_timing = true;
        settings.save();
        let text = std::fs::read_to_string(folder.join("settings.json")).unwrap();
        let read: Settings = serde_json::from_str(&text).unwrap();
        assert!(read.show_timing);
        assert!(
            !text.contains("\"file\""),
            "where it's saved isn't saved: {text}"
        );
    }
}

//! How the app looks on this machine (decision #96): the text size and theme
//! picked in Settings. Kept in app-data `ui.json`, per machine; never synced, since
//! each screen and pair of eyes differs.

use std::path::Path;

use serde::{Deserialize, Serialize};

const UI_FILE: &str = "ui.json";

/// The text sizes Settings offers, as zoom factors. Modest on purpose: the
/// layout stays readable at each without growing past the window.
pub const TEXT_SIZES: [f64; 4] = [0.9, 1.0, 1.1, 1.2];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// One of [`TEXT_SIZES`]; 1.0 is the default.
    pub text_size: f64,
    pub theme: Theme,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            text_size: 1.0,
            theme: Theme::System,
        }
    }
}

/// Light or dark, or whichever the OS is set to (the default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// A missing or unreadable file, or a size Settings doesn't offer, is the default.
pub fn load(app_dir: &Path) -> UiPrefs {
    let prefs: UiPrefs = std::fs::read_to_string(app_dir.join(UI_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    UiPrefs {
        text_size: nearest(prefs.text_size),
        ..prefs
    }
}

/// Save the text size, snapped to the nearest one Settings offers.
pub fn set_text_size(app_dir: &Path, size: f64) -> Result<UiPrefs, String> {
    save(
        app_dir,
        UiPrefs {
            text_size: nearest(size),
            ..load(app_dir)
        },
    )
}

pub fn set_theme(app_dir: &Path, theme: Theme) -> Result<UiPrefs, String> {
    save(
        app_dir,
        UiPrefs {
            theme,
            ..load(app_dir)
        },
    )
}

fn save(app_dir: &Path, prefs: UiPrefs) -> Result<UiPrefs, String> {
    let path = app_dir.join(UI_FILE);
    let tmp = app_dir.join(format!("{UI_FILE}.tmp"));
    let json = serde_json::to_string_pretty(&prefs).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app_dir)
        .and_then(|()| std::fs::write(&tmp, json))
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(prefs)
}

fn nearest(size: f64) -> f64 {
    if !size.is_finite() {
        return 1.0;
    }
    TEXT_SIZES
        .into_iter()
        .min_by(|a, b| (a - size).abs().total_cmp(&(b - size).abs()))
        .unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_when_missing_or_broken() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path()).text_size, 1.0);
        std::fs::write(tmp.path().join(UI_FILE), "not json").unwrap();
        assert_eq!(load(tmp.path()).text_size, 1.0);
    }

    #[test]
    fn saves_and_snaps_to_an_offered_size() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(set_text_size(tmp.path(), 1.1).unwrap().text_size, 1.1);
        assert_eq!(load(tmp.path()).text_size, 1.1);
        assert_eq!(set_text_size(tmp.path(), 3.0).unwrap().text_size, 1.2);
        assert_eq!(set_text_size(tmp.path(), f64::NAN).unwrap().text_size, 1.0);
        std::fs::write(tmp.path().join(UI_FILE), r#"{"text_size":0.2}"#).unwrap();
        assert_eq!(load(tmp.path()).text_size, 0.9);
    }

    #[test]
    fn theme_keeps_the_text_size() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path()).theme, Theme::System);
        set_text_size(tmp.path(), 1.2).unwrap();
        let prefs = set_theme(tmp.path(), Theme::Dark).unwrap();
        assert_eq!((prefs.theme, prefs.text_size), (Theme::Dark, 1.2));
        assert_eq!(load(tmp.path()), prefs);
    }
}

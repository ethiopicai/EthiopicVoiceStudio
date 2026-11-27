//! Portable shortcut preferences; native activation is qualified per platform.
use serde::{Deserialize, Serialize};

pub const CTRL: u8 = 1;
pub const ALT: u8 = 2;
pub const SHIFT: u8 = 4;
pub const META: u8 = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shortcut {
    pub modifiers: u8,
    pub key: Option<String>,
}
impl Shortcut {
    pub fn default_for(platform: &str) -> Self {
        if platform == "windows" {
            Self {
                modifiers: CTRL | META,
                key: None,
            }
        } else {
            Self {
                modifiers: 0,
                key: Some("Fn".into()),
            }
        }
    }
    pub fn validate(&self, platform: &str) -> Result<(), String> {
        if !matches!(platform, "macos" | "windows") {
            return Err("Dictation shortcuts are available on macOS and Windows.".into());
        }
        if self.modifiers & !(CTRL | ALT | SHIFT | META) != 0 {
            return Err("Choose a supported modifier key.".into());
        }
        match self.key.as_deref() {
            Some("Fn") if platform == "macos" && self.modifiers == 0 => return Ok(()),
            None if self.modifiers.count_ones() >= 2 => return Ok(()),
            Some(key) if self.modifiers & (CTRL | ALT | META) != 0 && supported_key(key) => {},
            _ => return Err("Hold at least two modifier keys, or add a letter, number, Space or function key to Ctrl, Alt/Option or Win/Command.".into()),
        }
        let key = self.key.as_deref().unwrap_or_default();
        if (platform == "macos"
            && self.modifiers == META
            && matches!(key, "KeyQ" | "KeyW" | "KeyH" | "KeyM" | "Space"))
            || (platform == "windows"
                && self.modifiers == META
                && matches!(key, "KeyL" | "KeyD" | "KeyE" | "KeyR" | "Space"))
            || (self.modifiers == ALT && key == "F4")
        {
            return Err(
                "That combination is used by the operating system. Choose another shortcut.".into(),
            );
        }
        Ok(())
    }
    pub fn label(&self, platform: &str) -> String {
        let mut parts = vec![];
        for (flag, name) in [
            (CTRL, "Ctrl"),
            (ALT, if platform == "macos" { "Option" } else { "Alt" }),
            (SHIFT, "Shift"),
            (
                META,
                if platform == "macos" {
                    "Command"
                } else {
                    "Win"
                },
            ),
        ] {
            if self.modifiers & flag != 0 {
                parts.push(name.to_owned());
            }
        }
        if let Some(key) = &self.key {
            parts.push(
                key.strip_prefix("Key")
                    .or_else(|| key.strip_prefix("Digit"))
                    .unwrap_or(key)
                    .to_owned(),
            );
        }
        parts.join(" + ")
    }
    #[cfg(any(target_os = "macos", test))]
    pub fn mac_key_code(&self) -> Option<i32> {
        let Some(key) = self.key.as_deref() else {
            return Some(-1);
        };
        const KEYS: &[(&str, i32)] = &[
            ("Fn", 63),
            ("Space", 49),
            ("KeyA", 0),
            ("KeyS", 1),
            ("KeyD", 2),
            ("KeyF", 3),
            ("KeyH", 4),
            ("KeyG", 5),
            ("KeyZ", 6),
            ("KeyX", 7),
            ("KeyC", 8),
            ("KeyV", 9),
            ("KeyB", 11),
            ("KeyQ", 12),
            ("KeyW", 13),
            ("KeyE", 14),
            ("KeyR", 15),
            ("KeyY", 16),
            ("KeyT", 17),
            ("Digit1", 18),
            ("Digit2", 19),
            ("Digit3", 20),
            ("Digit4", 21),
            ("Digit6", 22),
            ("Digit5", 23),
            ("Digit9", 25),
            ("Digit7", 26),
            ("Digit8", 28),
            ("Digit0", 29),
            ("KeyO", 31),
            ("KeyU", 32),
            ("KeyI", 34),
            ("KeyP", 35),
            ("KeyL", 37),
            ("KeyJ", 38),
            ("KeyK", 40),
            ("KeyN", 45),
            ("KeyM", 46),
            ("F1", 122),
            ("F2", 120),
            ("F3", 99),
            ("F4", 118),
            ("F5", 96),
            ("F6", 97),
            ("F7", 98),
            ("F8", 100),
            ("F9", 101),
            ("F10", 109),
            ("F11", 103),
            ("F12", 111),
        ];
        KEYS.iter()
            .find_map(|(name, code)| (*name == key).then_some(*code))
    }
}
fn supported_key(key: &str) -> bool {
    key == "Space"
        || key
            .strip_prefix("Key")
            .is_some_and(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_uppercase())
        || key
            .strip_prefix("Digit")
            .is_some_and(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_digit())
        || key.strip_prefix('F').is_some_and(|s| {
            s.parse::<u8>()
                .ok()
                .is_some_and(|n| (1..=12).contains(&n) && n.to_string() == s)
        })
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedShortcut {
    pub schema_version: u8,
    pub platform: String,
    pub shortcut: Shortcut,
}
impl SavedShortcut {
    pub fn load(bytes: &[u8], platform: &str) -> Option<Shortcut> {
        let saved: Self = serde_json::from_slice(bytes).ok()?;
        if saved.schema_version != 1
            || saved.platform != platform
            || saved.shortcut.validate(platform).is_err()
        {
            return None;
        }
        Some(saved.shortcut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_defaults_are_fn_and_ctrl_win() {
        let mac = Shortcut::default_for("macos");
        let win = Shortcut::default_for("windows");
        assert_eq!(mac.label("macos"), "Fn");
        assert_eq!(mac.mac_key_code(), Some(63));
        assert_eq!(
            win,
            Shortcut {
                modifiers: CTRL | META,
                key: None
            }
        );
        assert_eq!(win.label("windows"), "Ctrl + Win");
        mac.validate("macos").unwrap();
        win.validate("windows").unwrap();
    }
    #[test]
    fn plain_typing_and_reserved_system_shortcuts_are_rejected() {
        for shortcut in [
            Shortcut {
                modifiers: CTRL,
                key: None,
            },
            Shortcut {
                modifiers: 0,
                key: Some("KeyD".into()),
            },
            Shortcut {
                modifiers: SHIFT,
                key: Some("KeyA".into()),
            },
            Shortcut {
                modifiers: META,
                key: Some("KeyQ".into()),
            },
            Shortcut {
                modifiers: ALT,
                key: Some("F4".into()),
            },
        ] {
            assert!(shortcut.validate("macos").is_err());
        }
        assert!(Shortcut::default_for("macos").validate("windows").is_err());
        assert!(Shortcut {
            modifiers: META,
            key: Some("KeyL".into())
        }
        .validate("windows")
        .is_err());
    }
    #[test]
    fn custom_shortcuts_round_trip_only_for_their_saved_platform() {
        let shortcut = Shortcut {
            modifiers: CTRL | SHIFT,
            key: Some("KeyD".into()),
        };
        let bytes = serde_json::to_vec(&SavedShortcut {
            schema_version: 1,
            platform: "windows".into(),
            shortcut: shortcut.clone(),
        })
        .unwrap();
        assert_eq!(SavedShortcut::load(&bytes, "windows"), Some(shortcut));
        assert!(SavedShortcut::load(&bytes, "macos").is_none());
        assert!(SavedShortcut::load(b"{}", "windows").is_none());
    }
    #[test]
    fn every_allowed_primary_key_has_a_native_mac_mapping() {
        for key in ('A'..='Z')
            .map(|c| format!("Key{c}"))
            .chain((0..=9).map(|n| format!("Digit{n}")))
            .chain((1..=12).map(|n| format!("F{n}")))
            .chain(["Space".into()])
        {
            let shortcut = Shortcut {
                modifiers: CTRL | SHIFT,
                key: Some(key),
            };
            shortcut.validate("macos").unwrap();
            assert!(shortcut.mac_key_code().is_some());
        }
    }
}

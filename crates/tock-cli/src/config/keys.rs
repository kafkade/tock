//! TUI keybinding resolution.
//!
//! [`Keymap`] translates the `[keys]` config table into a lookup from a
//! pressed [`KeyCode`] to a semantic [`Action`]. Unmapped keys fall through to
//! the built-in navigational defaults handled by the caller (arrows, number
//! keys, Ctrl-C).

use crossterm::event::KeyCode;

use super::Keys;

/// A semantic TUI action, decoupled from the physical key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Quit the TUI.
    Quit,
    /// Focus the next pane.
    NextPane,
    /// Focus the previous pane.
    PrevPane,
    /// Move the selection down.
    Down,
    /// Move the selection up.
    Up,
    /// Jump to the top of the list.
    Top,
    /// Jump to the bottom of the list.
    Bottom,
    /// Activate the selected item.
    Activate,
    /// Complete the selected task.
    Complete,
    /// Delete the selected task.
    Delete,
    /// Reload tasks.
    Reload,
    /// Toggle the help overlay.
    Help,
}

/// Resolved keybindings for the TUI.
#[derive(Clone, Debug)]
pub struct Keymap {
    bindings: Vec<(KeyCode, Action)>,
}

impl Keymap {
    /// Build a keymap from the `[keys]` config table. Values that cannot be
    /// parsed are skipped, leaving the built-in fallback in place.
    #[must_use]
    pub fn from_config(keys: &Keys) -> Self {
        let mut bindings = Vec::new();
        let mut push = |raw: &str, action: Action| {
            if let Some(code) = parse_key(raw) {
                bindings.push((code, action));
            }
        };
        push(&keys.quit, Action::Quit);
        push(&keys.next_pane, Action::NextPane);
        push(&keys.prev_pane, Action::PrevPane);
        push(&keys.down, Action::Down);
        push(&keys.up, Action::Up);
        push(&keys.top, Action::Top);
        push(&keys.bottom, Action::Bottom);
        push(&keys.activate, Action::Activate);
        push(&keys.complete, Action::Complete);
        push(&keys.delete, Action::Delete);
        push(&keys.reload, Action::Reload);
        push(&keys.help, Action::Help);
        Self { bindings }
    }

    /// Look up the action bound to a key, if any.
    #[must_use]
    pub fn action(&self, code: KeyCode) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(bound, _)| *bound == code)
            .map(|(_, action)| *action)
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::from_config(&Keys::default())
    }
}

/// Parse a config key string into a [`KeyCode`].
fn parse_key(raw: &str) -> Option<KeyCode> {
    let trimmed = raw.trim();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        // Single character binding.
        return Some(KeyCode::Char(first));
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "tab" => Some(KeyCode::Tab),
        "backtab" | "shift-tab" | "shifttab" => Some(KeyCode::BackTab),
        "enter" | "return" => Some(KeyCode::Enter),
        "esc" | "escape" => Some(KeyCode::Esc),
        "up" => Some(KeyCode::Up),
        "down" => Some(KeyCode::Down),
        "left" => Some(KeyCode::Left),
        "right" => Some(KeyCode::Right),
        "space" => Some(KeyCode::Char(' ')),
        "backspace" => Some(KeyCode::Backspace),
        "delete" | "del" => Some(KeyCode::Delete),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Keymap, parse_key};
    use crate::config::Keys;
    use crossterm::event::KeyCode;

    #[test]
    fn parses_single_chars_and_named_keys() {
        assert_eq!(parse_key("j"), Some(KeyCode::Char('j')));
        assert_eq!(parse_key("G"), Some(KeyCode::Char('G')));
        assert_eq!(parse_key("tab"), Some(KeyCode::Tab));
        assert_eq!(parse_key("Enter"), Some(KeyCode::Enter));
        assert_eq!(parse_key("nonsense"), None);
    }

    #[test]
    fn default_keymap_matches_documented_bindings() {
        let map = Keymap::default();
        assert_eq!(map.action(KeyCode::Char('q')), Some(Action::Quit));
        assert_eq!(map.action(KeyCode::Tab), Some(Action::NextPane));
        assert_eq!(map.action(KeyCode::Char('d')), Some(Action::Complete));
        assert_eq!(map.action(KeyCode::Enter), Some(Action::Activate));
    }

    #[test]
    fn config_overrides_bindings() {
        let keys = Keys {
            complete: String::from("c"),
            ..Keys::default()
        };
        let map = Keymap::from_config(&keys);
        assert_eq!(map.action(KeyCode::Char('c')), Some(Action::Complete));
        // The old default is no longer bound to complete.
        assert_ne!(map.action(KeyCode::Char('d')), Some(Action::Complete));
    }
}

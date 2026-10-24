//! Startup-only INI parsing and fixed-size, edge-triggered menu shortcuts.
use crate::ButtonConfig;

pub const DEFAULT_OPEN_MENU_CONFIG: ButtonConfig = ButtonConfig { B: 1, DPAD_UP: 1, ..ButtonConfig::empty() };

pub const DEFAULT_INI: &str = "# B + DPad Up is always available. Restart the game after editing.\n[menu]\nshortcut_1=Minus\nshortcut_2=disabled\n";
pub const DEFAULT_SHORTCUTS: MenuShortcuts = MenuShortcuts {
    additional: [ButtonConfig::MINUS, ButtonConfig::empty()],
};

// The same logical buttons as ButtonConfig/button_mapping; never allocate a Vec in input hooks.
pub const BUTTONS: [ButtonConfig; 16] = ButtonConfig::ALL_CONSTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuShortcuts {
    additional: [ButtonConfig; 2],
}

fn parse_chord(value: &str) -> Option<ButtonConfig> {
    if value.eq_ignore_ascii_case("disabled") || value.eq_ignore_ascii_case("none") {
        return Some(ButtonConfig::empty());
    }
    let mut chord = ButtonConfig::empty();
    for token in value.split('+').map(str::trim) {
        // Reuse byteflags' declared identifiers and display labels. Trigger
        // labels expose Pro/GCC alternatives separated by ';'.
        let index = ButtonConfig::ALL_FIELDS.iter().zip(ButtonConfig::ALL_NAMES.iter())
            .position(|(field, label)| token.eq_ignore_ascii_case(field)
                || label.split(';').any(|name| token.eq_ignore_ascii_case(name.trim())))?;
        let button = BUTTONS[index];
        if chord.contains(&button) {
            return None;
        }
        chord = chord.union(button);
    }
    Some(chord)
}

impl MenuShortcuts {
    /// Unknown keys/sections are ignored. Missing keys use defaults; malformed
    /// or duplicate recognised keys fall back independently to that slot's default.
    pub fn parse(contents: &str) -> Self {
        let mut result = DEFAULT_SHORTCUTS;
        let mut in_menu = false;
        let mut seen = [false; 2];
        let mut invalid = [false; 2];
        for raw in contents.lines() {
            let line = raw.split([';', '#']).next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                in_menu = line.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
                    .is_some_and(|section| section.trim().eq_ignore_ascii_case("menu"));
                continue;
            }
            if !in_menu {
                continue;
            }
            let (key, value) = line.split_once('=').unwrap_or((line, ""));
            let index = if key.trim().eq_ignore_ascii_case("shortcut_1") {
                0
            } else if key.trim().eq_ignore_ascii_case("shortcut_2") {
                1
            } else {
                continue;
            };
            if seen[index] {
                invalid[index] = true;
            }
            seen[index] = true;
            match parse_chord(value.trim()) {
                Some(chord) if !invalid[index] => result.additional[index] = chord,
                _ => invalid[index] = true,
            }
            if invalid[index] {
                result.additional[index] = DEFAULT_SHORTCUTS.additional[index];
            }
        }
        result
    }

    pub fn description(self) -> String {
        [self.additional[0], self.additional[1], DEFAULT_OPEN_MENU_CONFIG]
            .iter().copied().filter(|chord| *chord != ButtonConfig::empty())
            .map(|chord| chord.to_string()).collect::<Vec<_>>().join(" or ")
    }

    /// Controller mapping stays with the input owner. This only combines logical
    /// button predicates and does not allocate or retain controller state.
    pub fn passes(self, mut held: impl FnMut(ButtonConfig) -> bool,
        mut pressed: impl FnMut(ButtonConfig) -> bool) -> bool {
        [DEFAULT_OPEN_MENU_CONFIG, self.additional[0], self.additional[1]]
            .iter().copied().any(|chord| {
                let mut edge = false;
                for button in BUTTONS.iter().filter(|button| chord.contains(button)) {
                    if !held(*button) { return false; }
                    edge |= pressed(*button);
                }
                edge
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn passes(config: MenuShortcuts, held: ButtonConfig, pressed: ButtonConfig) -> bool {
        config.passes(|button| held.contains(&button), |button| pressed.contains(&button))
    }

    #[test]
    fn defaults_and_missing_keys() {
        assert_eq!(MenuShortcuts::parse(""), DEFAULT_SHORTCUTS);
        assert_eq!(MenuShortcuts::parse(DEFAULT_INI), DEFAULT_SHORTCUTS);
        assert_eq!(MenuShortcuts::parse("[other]\nshortcut_1=Plus"), DEFAULT_SHORTCUTS);
        let config = MenuShortcuts::parse("[menu]\nshortcut_2=ZL + R");
        assert_eq!(config.additional, [ButtonConfig::MINUS, ButtonConfig::ZL.union(ButtonConfig::R)]);
    }

    #[test]
    fn supported_names_comments_and_disabled_slots() {
        let config = MenuShortcuts::parse("[ MeNu ]\nSHORTCUT_1= disabled ; off\nshortcut_2= Left Stick Press + DPAD_RIGHT # chord");
        assert_eq!(config.additional, [ButtonConfig::empty(), ButtonConfig::LSTICK.union(ButtonConfig::DPAD_RIGHT)]);
        assert_eq!(MenuShortcuts::parse("[menu]\nshortcut_1=none").additional[0], ButtonConfig::empty());
    }

    #[test]
    fn invalid_or_duplicate_slot_falls_back_independently() {
        for value in ["", "garbage", "B +", "+ B", "B + B", "disabled + B"] {
            let config = MenuShortcuts::parse(&format!("[menu]\nshortcut_1={value}\nshortcut_2=X"));
            assert_eq!(config.additional, [ButtonConfig::MINUS, ButtonConfig::X]);
        }
        let config = MenuShortcuts::parse("[menu]\nshortcut_1=X\nshortcut_1=Y\nshortcut_2=Plus");
        assert_eq!(config.additional, [ButtonConfig::MINUS, ButtonConfig::PLUS]);
        assert_eq!(MenuShortcuts::parse("[menu]\nshortcut_2=X\nshortcut_2=Y").additional[1], ButtonConfig::empty());
    }

    #[test]
    fn permanent_chord_survives_disabled_extras_and_uses_press_edges() {
        let config = MenuShortcuts::parse("[menu]\nshortcut_1=disabled\nshortcut_2=disabled");
        let held = ButtonConfig::B.union(ButtonConfig::DPAD_UP);
        assert!(passes(config, held, ButtonConfig::B));
        assert!(passes(config, held, ButtonConfig::DPAD_UP));
        assert!(!passes(config, held, ButtonConfig::empty()));
        assert!(!passes(config, ButtonConfig::B, ButtonConfig::B));
    }

    #[test]
    fn minus_is_edge_triggered_and_plus_is_not_an_implicit_opener() {
        let minus = ButtonConfig::MINUS;
        assert!(passes(DEFAULT_SHORTCUTS, minus, minus));
        assert!(!passes(DEFAULT_SHORTCUTS, minus, ButtonConfig::empty()));
        let plus = ButtonConfig::PLUS;
        assert!(!passes(DEFAULT_SHORTCUTS, plus, plus));
        assert!(!passes(DEFAULT_SHORTCUTS, plus, ButtonConfig::empty()));
        assert!(passes(MenuShortcuts::parse("[menu]\nshortcut_2=Plus"), plus, plus));
    }

    #[test]
    fn custom_chord_requires_every_held_button_and_one_new_edge() {
        let config = MenuShortcuts::parse("[menu]\nshortcut_1=disabled\nshortcut_2=R + ZL");
        let both = ButtonConfig::R.union(ButtonConfig::ZL);
        assert!(passes(config, both, ButtonConfig::R));
        assert!(passes(config, both, ButtonConfig::ZL));
        assert!(!passes(config, ButtonConfig::R, ButtonConfig::R));
        assert!(!passes(config, both, ButtonConfig::empty()));
    }
}

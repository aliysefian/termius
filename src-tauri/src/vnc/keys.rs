//! Browser key names (`KeyboardEvent.code`) to the X11 key symbols a VNC server expects. A VNC key event
//! names the symbol, not the key, so these are the US layout: the unshifted symbol for each key, with the
//! Shift key sent as a key of its own (a server applies it, as at its own keyboard).

/// `(code, keysym)`.
const KEYS: &[(&str, u32)] = &[
    ("Digit1", 0x31), ("Digit2", 0x32), ("Digit3", 0x33), ("Digit4", 0x34), ("Digit5", 0x35),
    ("Digit6", 0x36), ("Digit7", 0x37), ("Digit8", 0x38), ("Digit9", 0x39), ("Digit0", 0x30),
    ("Minus", 0x2d), ("Equal", 0x3d), ("BracketLeft", 0x5b), ("BracketRight", 0x5d), ("Backslash", 0x5c),
    ("Semicolon", 0x3b), ("Quote", 0x27), ("Backquote", 0x60), ("Comma", 0x2c), ("Period", 0x2e), ("Slash", 0x2f),
    ("IntlBackslash", 0x5c), ("Space", 0x20),
    ("Backspace", 0xff08), ("Tab", 0xff09), ("Enter", 0xff0d), ("NumpadEnter", 0xff8d), ("Escape", 0xff1b),
    ("Home", 0xff50), ("ArrowLeft", 0xff51), ("ArrowUp", 0xff52), ("ArrowRight", 0xff53), ("ArrowDown", 0xff54),
    ("PageUp", 0xff55), ("PageDown", 0xff56), ("End", 0xff57), ("Insert", 0xff63), ("Delete", 0xffff),
    ("ContextMenu", 0xff67), ("PrintScreen", 0xff61), ("ScrollLock", 0xff14), ("NumLock", 0xff7f), ("CapsLock", 0xffe5),
    ("ShiftLeft", 0xffe1), ("ShiftRight", 0xffe2), ("ControlLeft", 0xffe3), ("ControlRight", 0xffe4),
    ("AltLeft", 0xffe9), ("AltRight", 0xffea), ("MetaLeft", 0xffeb), ("MetaRight", 0xffec),
    ("Numpad0", 0xffb0), ("Numpad1", 0xffb1), ("Numpad2", 0xffb2), ("Numpad3", 0xffb3), ("Numpad4", 0xffb4),
    ("Numpad5", 0xffb5), ("Numpad6", 0xffb6), ("Numpad7", 0xffb7), ("Numpad8", 0xffb8), ("Numpad9", 0xffb9),
    ("NumpadMultiply", 0xffaa), ("NumpadAdd", 0xffab), ("NumpadSubtract", 0xffad), ("NumpadDecimal", 0xffae), ("NumpadDivide", 0xffaf),
];

/// The key symbol for a `KeyboardEvent.code`, or `None` for a key there is no symbol for.
pub fn keysym(code: &str) -> Option<u32> {
    if let Some(letter) = code.strip_prefix("Key") {
        let mut chars = letter.chars();
        if let (Some(c @ 'A'..='Z'), None) = (chars.next(), chars.next()) {
            return Some(c.to_ascii_lowercase() as u32);
        }
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=12).contains(&n) {
            return Some(0xffbd + n);
        }
    }
    KEYS.iter().find(|(c, _)| *c == code).map(|&(_, k)| k)
}

pub const CONTROL_LEFT: u32 = 0xffe3;
pub const ALT_LEFT: u32 = 0xffe9;
pub const DELETE: u32 = 0xffff;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_key_has_its_own_name_and_symbol() {
        let names: HashSet<_> = KEYS.iter().map(|k| k.0).collect();
        assert_eq!(names.len(), KEYS.len(), "a key name appears twice");
        // IntlBackslash shares the backslash symbol on purpose; nothing else may.
        let symbols: HashSet<_> = KEYS.iter().map(|k| k.1).collect();
        assert_eq!(symbols.len(), KEYS.len() - 1);
    }

    #[test]
    fn known_keys() {
        assert_eq!(keysym("KeyA"), Some(0x61), "letters are the unshifted symbol");
        assert_eq!(keysym("KeyZ"), Some(0x7a));
        assert_eq!(keysym("Digit0"), Some(0x30));
        assert_eq!(keysym("Enter"), Some(0xff0d));
        assert_eq!(keysym("F1"), Some(0xffbe));
        assert_eq!(keysym("F12"), Some(0xffc9));
        assert_eq!(keysym("Delete"), Some(0xffff));
        assert_eq!(keysym("ArrowLeft"), Some(0xff51));
        assert_eq!(keysym("ShiftLeft"), Some(0xffe1));
    }

    #[test]
    fn unknown_keys_have_no_symbol() {
        for c in ["", "NoSuchKey", "Key", "KeyAB", "Keya", "F0", "F13", "F", "Fx"] {
            assert_eq!(keysym(c), None, "{c:?}");
        }
    }

    #[test]
    fn every_letter_digit_and_function_key_is_there() {
        for c in 'A'..='Z' {
            assert!(keysym(&format!("Key{c}")).is_some());
        }
        for d in 0..=9 {
            assert!(keysym(&format!("Digit{d}")).is_some());
        }
        for f in 1..=12 {
            assert!(keysym(&format!("F{f}")).is_some());
        }
    }

    #[test]
    fn the_ctrl_alt_del_symbols_match_the_table() {
        assert_eq!(keysym("ControlLeft"), Some(CONTROL_LEFT));
        assert_eq!(keysym("AltLeft"), Some(ALT_LEFT));
        assert_eq!(keysym("Delete"), Some(DELETE));
    }
}

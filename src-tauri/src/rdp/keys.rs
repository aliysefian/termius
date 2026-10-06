//! Browser key names (`KeyboardEvent.code`) to the PC scan codes an RDP server
//! expects. Scan codes name the physical key, so the server applies its own
//! keyboard layout, the same as sitting at its keyboard.

use ironrdp_input::Scancode;

/// `(code, extended, scan code)`, in the order of a PC keyboard.
const KEYS: &[(&str, bool, u8)] = &[
    ("Escape", false, 0x01),
    ("Digit1", false, 0x02), ("Digit2", false, 0x03), ("Digit3", false, 0x04), ("Digit4", false, 0x05), ("Digit5", false, 0x06),
    ("Digit6", false, 0x07), ("Digit7", false, 0x08), ("Digit8", false, 0x09), ("Digit9", false, 0x0A), ("Digit0", false, 0x0B),
    ("Minus", false, 0x0C), ("Equal", false, 0x0D), ("Backspace", false, 0x0E), ("Tab", false, 0x0F),
    ("KeyQ", false, 0x10), ("KeyW", false, 0x11), ("KeyE", false, 0x12), ("KeyR", false, 0x13), ("KeyT", false, 0x14),
    ("KeyY", false, 0x15), ("KeyU", false, 0x16), ("KeyI", false, 0x17), ("KeyO", false, 0x18), ("KeyP", false, 0x19),
    ("BracketLeft", false, 0x1A), ("BracketRight", false, 0x1B), ("Enter", false, 0x1C), ("ControlLeft", false, 0x1D),
    ("KeyA", false, 0x1E), ("KeyS", false, 0x1F), ("KeyD", false, 0x20), ("KeyF", false, 0x21), ("KeyG", false, 0x22),
    ("KeyH", false, 0x23), ("KeyJ", false, 0x24), ("KeyK", false, 0x25), ("KeyL", false, 0x26),
    ("Semicolon", false, 0x27), ("Quote", false, 0x28), ("Backquote", false, 0x29), ("ShiftLeft", false, 0x2A), ("Backslash", false, 0x2B),
    ("KeyZ", false, 0x2C), ("KeyX", false, 0x2D), ("KeyC", false, 0x2E), ("KeyV", false, 0x2F), ("KeyB", false, 0x30),
    ("KeyN", false, 0x31), ("KeyM", false, 0x32), ("Comma", false, 0x33), ("Period", false, 0x34), ("Slash", false, 0x35),
    ("ShiftRight", false, 0x36), ("NumpadMultiply", false, 0x37), ("AltLeft", false, 0x38), ("Space", false, 0x39), ("CapsLock", false, 0x3A),
    ("F1", false, 0x3B), ("F2", false, 0x3C), ("F3", false, 0x3D), ("F4", false, 0x3E), ("F5", false, 0x3F),
    ("F6", false, 0x40), ("F7", false, 0x41), ("F8", false, 0x42), ("F9", false, 0x43), ("F10", false, 0x44),
    ("NumLock", false, 0x45), ("ScrollLock", false, 0x46),
    ("Numpad7", false, 0x47), ("Numpad8", false, 0x48), ("Numpad9", false, 0x49), ("NumpadSubtract", false, 0x4A),
    ("Numpad4", false, 0x4B), ("Numpad5", false, 0x4C), ("Numpad6", false, 0x4D), ("NumpadAdd", false, 0x4E),
    ("Numpad1", false, 0x4F), ("Numpad2", false, 0x50), ("Numpad3", false, 0x51), ("Numpad0", false, 0x52), ("NumpadDecimal", false, 0x53),
    ("IntlBackslash", false, 0x56), ("F11", false, 0x57), ("F12", false, 0x58), ("IntlRo", false, 0x73), ("IntlYen", false, 0x7D),
    // Keys that send an E0 prefix.
    ("NumpadEnter", true, 0x1C), ("ControlRight", true, 0x1D), ("NumpadDivide", true, 0x35), ("PrintScreen", true, 0x37),
    ("AltRight", true, 0x38), ("Home", true, 0x47), ("ArrowUp", true, 0x48), ("PageUp", true, 0x49),
    ("ArrowLeft", true, 0x4B), ("ArrowRight", true, 0x4D), ("End", true, 0x4F), ("ArrowDown", true, 0x50),
    ("PageDown", true, 0x51), ("Insert", true, 0x52), ("Delete", true, 0x53),
    ("MetaLeft", true, 0x5B), ("MetaRight", true, 0x5C), ("ContextMenu", true, 0x5D),
];

/// The scan code for a `KeyboardEvent.code`, or `None` for a key RDP has no code for.
pub fn scancode(code: &str) -> Option<Scancode> {
    KEYS.iter().find(|(c, _, _)| *c == code).map(|&(_, extended, sc)| Scancode::from_u8(extended, sc))
}

pub const CONTROL_LEFT: Scancode = Scancode::from_u8(false, 0x1D);
pub const ALT_LEFT: Scancode = Scancode::from_u8(false, 0x38);
pub const DELETE: Scancode = Scancode::from_u8(true, 0x53);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_key_has_its_own_scan_code_and_name() {
        let codes: HashSet<_> = KEYS.iter().map(|k| k.0).collect();
        assert_eq!(codes.len(), KEYS.len(), "a key name appears twice");
        let scans: HashSet<_> = KEYS.iter().map(|k| (k.1, k.2)).collect();
        assert_eq!(scans.len(), KEYS.len(), "two keys share a scan code");
    }

    #[test]
    fn known_keys() {
        let sc = |c: &str| scancode(c).map(|s| s.as_u8());
        assert_eq!(sc("KeyA"), Some((false, 0x1E)));
        assert_eq!(sc("KeyQ"), Some((false, 0x10)), "the physical key, whatever letter the layout puts on it");
        assert_eq!(sc("Enter"), Some((false, 0x1C)));
        assert_eq!(sc("NumpadEnter"), Some((true, 0x1C)), "the keypad's Enter is the extended one");
        assert_eq!(sc("ArrowLeft"), Some((true, 0x4B)));
        assert_eq!(sc("Numpad4"), Some((false, 0x4B)), "same code as the arrow, but not extended");
        assert_eq!(sc("Delete"), Some((true, 0x53)));
        assert_eq!(sc("F12"), Some((false, 0x58)));
        assert_eq!(sc("ControlRight"), Some((true, 0x1D)));
        assert_eq!(sc("NoSuchKey"), None);
        assert_eq!(sc(""), None);
    }

    #[test]
    fn letters_and_digits_are_all_present() {
        for c in 'A'..='Z' {
            assert!(scancode(&format!("Key{c}")).is_some(), "Key{c}");
        }
        for d in 0..=9 {
            assert!(scancode(&format!("Digit{d}")).is_some(), "Digit{d}");
        }
        for f in 1..=12 {
            assert!(scancode(&format!("F{f}")).is_some(), "F{f}");
        }
    }

    #[test]
    fn the_ctrl_alt_del_keys_match_the_table() {
        assert_eq!(scancode("ControlLeft"), Some(CONTROL_LEFT));
        assert_eq!(scancode("AltLeft"), Some(ALT_LEFT));
        assert_eq!(scancode("Delete"), Some(DELETE));
    }
}

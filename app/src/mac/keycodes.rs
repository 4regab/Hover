//! The shortcut's keys for macOS: a WPF Key (what settings.json keeps) as the ANSI
//! virtual key code `RegisterEventHotKey` takes (Carbon's kVK_ANSI_* and friends), and
//! Hover's modifiers as Carbon's mask. Option is Hover's Alt (Option-N opens the office),
//! Command is its Windows key, and Control and Shift are themselves.

use hover_core::shortcut::{Key, Modifiers};

/// Carbon's modifier bits (Events.h): cmdKey, shiftKey, optionKey, controlKey.
pub const CMD: u32 = 1 << 8;
pub const SHIFT: u32 = 1 << 9;
pub const OPTION: u32 = 1 << 11;
pub const CONTROL: u32 = 1 << 12;

/// The chord's modifiers as Carbon's mask.
pub fn carbon_mask(m: Modifiers) -> u32 {
    let mut mask = 0;
    if m.has(Modifiers::ALT) { mask |= OPTION; }
    if m.has(Modifiers::CONTROL) { mask |= CONTROL; }
    if m.has(Modifiers::SHIFT) { mask |= SHIFT; }
    if m.has(Modifiers::WINDOWS) { mask |= CMD; }
    mask
}

pub const KVK_SPACE: u32 = 0x31;
pub const KVK_ESCAPE: u32 = 0x35;

/// The ANSI key codes of A to Z, in the order the letters run.
const LETTERS: [u32; 26] = [
    0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, 0x2D, 0x1F, 0x23, 0x0C, 0x0F, 0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
];
/// 0 to 9.
const DIGITS: [u32; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];
/// F1 to F20.
const FUNCTION: [u32; 20] = [
    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A, 0x40, 0x4F, 0x50, 0x5A,
];
/// The keypad's 0 to 9.
const KEYPAD: [u32; 10] = [0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5B, 0x5C];

/// The virtual key code of a WPF Key, for the keys a shortcut can hold; None has no key
/// on a Mac (F21 to F24, the Windows-only ones), and registering it says so.
pub fn keycode(k: Key) -> Option<u32> {
    let v = k.0 as u32;
    Some(match v {
        2 => 0x33,                       // Back: Delete
        3 => 0x30,                       // Tab
        6 => 0x24,                       // Return
        13 => KVK_ESCAPE,
        18 => KVK_SPACE,
        19 => 0x74,                      // PageUp
        20 => 0x79,                      // PageDown
        21 => 0x77,                      // End
        22 => 0x73,                      // Home
        23 => 0x7B, 24 => 0x7E, 25 => 0x7C, 26 => 0x7D, // Left, Up, Right, Down
        32 => 0x75,                      // Delete: forward delete
        34..=43 => DIGITS[(v - 34) as usize],
        44..=69 => LETTERS[(v - 44) as usize],
        74..=83 => KEYPAD[(v - 74) as usize],
        84 => 0x43, 85 => 0x45, 87 => 0x4E, 88 => 0x41, 89 => 0x4B, // keypad * + - . /
        90..=109 => FUNCTION[(v - 90) as usize],
        140 => 0x29,                     // ;
        141 => 0x18,                     // =
        142 => 0x2B,                     // ,
        143 => 0x1B,                     // -
        144 => 0x2F,                     // .
        145 => 0x2C,                     // /
        146 => 0x32,                     // `
        149 => 0x21,                     // [
        150 => 0x2A,                     // \
        151 => 0x1E,                     // ]
        152 => 0x27,                     // '
        _ => return None,
    })
}

/// NSEvent.modifierFlags bits (NSEventModifierFlagShift and friends, in the high word),
/// and the chord they make, for the local key monitor that catches the shortcut while a
/// Hover window holds the keyboard.
pub const NS_SHIFT: usize = 1 << 17;
pub const NS_CONTROL: usize = 1 << 18;
pub const NS_OPTION: usize = 1 << 19;
pub const NS_COMMAND: usize = 1 << 20;

/// A chord's modifiers as NSEvent reports them (only the four that count).
pub fn ns_mask(m: Modifiers) -> usize {
    let mut mask = 0;
    if m.has(Modifiers::ALT) { mask |= NS_OPTION; }
    if m.has(Modifiers::CONTROL) { mask |= NS_CONTROL; }
    if m.has(Modifiers::SHIFT) { mask |= NS_SHIFT; }
    if m.has(Modifiers::WINDOWS) { mask |= NS_COMMAND; }
    mask
}

/// The four modifier bits of an event's flags, nothing else (caps lock, function and the
/// device bits are not part of the chord).
pub fn ns_chord(flags: usize) -> usize { flags & (NS_SHIFT | NS_CONTROL | NS_OPTION | NS_COMMAND) }

#[cfg(test)]
mod tests {
    use super::*;
    use hover_core::shortcut::Shortcut;

    #[test]
    fn the_default_chords_are_option_n_and_control_option_space() {
        assert_eq!(keycode(Shortcut::DEFAULT.key), Some(0x2D));
        assert_eq!(carbon_mask(Shortcut::DEFAULT.modifiers), OPTION);
        // Voice's default: Ctrl+Alt+Space.
        let m = Modifiers(Modifiers::CONTROL.0 | Modifiers::ALT.0);
        assert_eq!((keycode(Key(18)), carbon_mask(m)), (Some(KVK_SPACE), CONTROL | OPTION));
        assert_eq!(ns_mask(m), NS_CONTROL | NS_OPTION);
    }

    /// Carbon's kVK_ANSI_* for every letter, digit and function key a chord can use.
    #[test]
    fn keys_map_to_the_keyboards_codes() {
        let letter = |c: char| keycode(Key::from_name(&c.to_string()).unwrap());
        for (c, code) in [('A', 0x00), ('S', 0x01), ('N', 0x2D), ('Z', 0x06), ('Q', 0x0C), ('M', 0x2E), ('P', 0x23)] { assert_eq!(letter(c), Some(code), "{c}"); }
        let digit = |d: &str| keycode(Key::from_name(d).unwrap());
        for (d, code) in [("D0", 0x1D), ("D1", 0x12), ("D5", 0x17), ("D6", 0x16), ("D9", 0x19)] { assert_eq!(digit(d), Some(code), "{d}"); }
        let f = |n: &str| keycode(Key::from_name(n).unwrap());
        assert_eq!((f("F1"), f("F3"), f("F10"), f("F12"), f("F20")), (Some(0x7A), Some(0x63), Some(0x6D), Some(0x6F), Some(0x5A)));
        assert_eq!((f("Left"), f("Up"), f("Right"), f("Down")), (Some(0x7B), Some(0x7E), Some(0x7C), Some(0x7D)));
        assert_eq!(f("Oem3"), Some(0x32), "the backtick");
        assert_eq!(f("F21"), None);
        assert_eq!(f("LWin"), None);
        // Every letter has its own code.
        let mut seen: Vec<u32> = ('A'..='Z').map(|c| letter(c).unwrap()).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 26);
    }

    #[test]
    fn an_events_flags_are_reduced_to_the_chord() {
        let caps = 1 << 16;
        assert_eq!(ns_chord(NS_OPTION | caps | 0x20), NS_OPTION);
        assert_eq!(ns_mask(Modifiers::WINDOWS), NS_COMMAND);
        assert_eq!(carbon_mask(Modifiers::NONE), 0);
    }
}

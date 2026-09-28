//! Keys between the three worlds a shortcut passes through: what Slint reports when a
//! chord is pressed in Settings (its key text), the WPF Key the settings keep (so
//! settings.json stays the C#'s), and what the system grabs: a virtual-key code for
//! RegisterHotKey (KeyInterop.VirtualKeyFromKey) or an X keysym for XGrabKey.

use hover_core::shortcut::{Key, Modifiers, Shortcut};

// WPF Key values (System.Windows.Input.Key).
const D0: u16 = 34;
const A: u16 = 44;
const F1: u16 = 90;

/// A key Slint reported as the Key it is in WPF: letters and digits by their character
/// (upper or lower case), the named keys by Slint's private-use codes, and the US
/// layout's punctuation as its Oem key (shifted or not). Modifier keys alone are
/// None: the recorder waits for the key that goes with them.
pub fn from_slint(text: &str) -> Option<Key> {
    let mut cs = text.chars();
    let c = cs.next()?;
    if cs.next().is_some() { return None; }
    let v: u16 = match c {
        'a'..='z' => A + (c as u16 - 'a' as u16),
        'A'..='Z' => A + (c as u16 - 'A' as u16),
        '0'..='9' => D0 + (c as u16 - '0' as u16),
        // Shifted digits on a US layout: the key is the digit's.
        ')' => D0, '!' => D0 + 1, '@' => D0 + 2, '#' => D0 + 3, '$' => D0 + 4, '%' => D0 + 5, '^' => D0 + 6, '&' => D0 + 7, '*' => D0 + 8, '(' => D0 + 9,
        '\u{8}' => 2, '\t' | '\u{19}' => 3, '\n' | '\r' => 6, '\u{1b}' => 13, ' ' => 18, '\u{7f}' => 32,
        '\u{F700}' => 24, '\u{F701}' => 26, '\u{F702}' => 23, '\u{F703}' => 25,
        '\u{F704}'..='\u{F71B}' => F1 + (c as u16 - 0xF704),
        '\u{F727}' => 31, '\u{F729}' => 22, '\u{F72B}' => 21, '\u{F72C}' => 19, '\u{F72D}' => 20,
        '\u{F72F}' => 115, '\u{F730}' => 7, '\u{F731}' => 30, '\u{F735}' => 72,
        ';' | ':' => 140, '=' | '+' => 141, ',' | '<' => 142, '-' | '_' => 143, '.' | '>' => 144, '/' | '?' => 145,
        '`' | '~' => 146, '[' | '{' => 149, '\\' | '|' => 150, ']' | '}' => 151, '\'' | '"' => 152,
        _ => return None,
    };
    Some(Key(v))
}

/// Slint's own modifier keys (Shift, Control, Alt, AltGr, Meta and their right-hand
/// twins, Caps Lock): pressed alone they start a chord, not end one.
pub fn is_modifier(text: &str) -> bool {
    matches!(text, "\u{10}" | "\u{11}" | "\u{12}" | "\u{13}" | "\u{14}" | "\u{15}" | "\u{16}" | "\u{17}" | "\u{18}")
}

pub fn modifiers(alt: bool, control: bool, shift: bool, meta: bool) -> Modifiers {
    let mut m = Modifiers::NONE;
    if alt { m = m | Modifiers::ALT; }
    if control { m = m | Modifiers::CONTROL; }
    if shift { m = m | Modifiers::SHIFT; }
    if meta { m = m | Modifiers::WINDOWS; }
    m
}

/// The recorder's step (Pages.ShortcutField's PreviewKeyDown): Esc stops, a modifier
/// alone waits, a key without a modifier asks for one, anything else is the chord.
#[derive(Debug, PartialEq)]
pub enum Recorded { Stop, Wait, NeedModifier, Chord(Shortcut) }

pub fn record(text: &str, m: Modifiers) -> Recorded {
    if text == "\u{1b}" { return Recorded::Stop; }
    if is_modifier(text) { return Recorded::Wait; }
    let Some(key) = from_slint(text) else { return Recorded::Wait };
    // A global shortcut without a modifier would take an ordinary typing key away
    // from every application on the desktop.
    if m == Modifiers::NONE { return Recorded::NeedModifier; }
    Recorded::Chord(Shortcut { key, modifiers: m })
}

/// KeyInterop.VirtualKeyFromKey for the keys a shortcut can hold; None has no mapping
/// (HotKeys.Register then says so and fails).
pub fn vk(k: Key) -> Option<u16> {
    let v = k.0;
    Some(match v {
        2 => 0x08, 3 => 0x09, 5 => 0x0C, 6 => 0x0D, 7 => 0x13, 8 => 0x14, 13 => 0x1B, 18 => 0x20,
        19 => 0x21, 20 => 0x22, 21 => 0x23, 22 => 0x24, 23 => 0x25, 24 => 0x26, 25 => 0x27, 26 => 0x28,
        27 => 0x29, 28 => 0x2A, 29 => 0x2B, 30 => 0x2C, 31 => 0x2D, 32 => 0x2E, 33 => 0x2F,
        34..=43 => 0x30 + (v - 34),
        44..=69 => 0x41 + (v - 44),
        70 => 0x5B, 71 => 0x5C, 72 => 0x5D, 73 => 0x5F,
        74..=83 => 0x60 + (v - 74),
        84 => 0x6A, 85 => 0x6B, 86 => 0x6C, 87 => 0x6D, 88 => 0x6E, 89 => 0x6F,
        90..=113 => 0x70 + (v - 90),
        114 => 0x90, 115 => 0x91,
        140 => 0xBA, 141 => 0xBB, 142 => 0xBC, 143 => 0xBD, 144 => 0xBE, 145 => 0xBF, 146 => 0xC0,
        149 => 0xDB, 150 => 0xDC, 151 => 0xDD, 152 => 0xDE, 153 => 0xDF, 154 => 0xE2,
        _ => return None,
    })
}

/// The X keysym of the key, as the US layout names it.
pub fn keysym(k: Key) -> Option<u32> {
    let v = k.0 as u32;
    Some(match v {
        2 => 0xff08, 3 => 0xff09, 6 => 0xff0d, 7 => 0xff13, 13 => 0xff1b, 18 => 0x20,
        19 => 0xff55, 20 => 0xff56, 21 => 0xff57, 22 => 0xff50, 23 => 0xff51, 24 => 0xff52, 25 => 0xff53, 26 => 0xff54,
        30 => 0xff61, 31 => 0xff63, 32 => 0xffff,
        34..=43 => 0x30 + (v - 34),
        44..=69 => 0x61 + (v - 44),
        72 => 0xff67,
        74..=83 => 0xffb0 + (v - 74),
        84 => 0xffaa, 85 => 0xffab, 87 => 0xffad, 88 => 0xffae, 89 => 0xffaf,
        90..=113 => 0xffbe + (v - 90),
        140 => 0x3b, 141 => 0x3d, 142 => 0x2c, 143 => 0x2d, 144 => 0x2e, 145 => 0x2f, 146 => 0x60,
        149 => 0x5b, 150 => 0x5c, 151 => 0x5d, 152 => 0x27,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_is_recorded_as_pages_records_it() {
        let alt = modifiers(true, false, false, false);
        assert_eq!(record("n", alt), Recorded::Chord(Shortcut::DEFAULT));
        assert_eq!(record("N", alt), Recorded::Chord(Shortcut::DEFAULT));
        assert_eq!(record("\u{1b}", alt), Recorded::Stop);
        assert_eq!(record("\u{12}", alt), Recorded::Wait);
        assert_eq!(record("k", Modifiers::NONE), Recorded::NeedModifier);
        let c = record("\u{F70D}", modifiers(false, true, true, false));
        let Recorded::Chord(s) = c else { panic!() };
        assert_eq!(s.label(), "Ctrl+Shift+F10");
        assert_eq!(from_slint("!"), Some(Key(35)));
        assert_eq!(from_slint("ab"), None);
    }

    /// VirtualKeyFromKey's table (winuser.h's VK_ values) and the X keysyms (keysymdef.h).
    #[test]
    fn keys_map_to_the_systems_codes() {
        assert_eq!(vk(Key::N), Some(0x4E));
        assert_eq!(vk(Key(34)), Some(0x30));
        assert_eq!(vk(Key(90)), Some(0x70));
        assert_eq!(vk(Key(144)), Some(0xBE));
        assert_eq!(vk(Key(160)), None);
        assert_eq!(keysym(Key::N), Some(0x6e));
        assert_eq!(keysym(Key(90)), Some(0xffbe));
        assert_eq!(keysym(Key(18)), Some(0x20));
    }
}

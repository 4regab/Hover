//! Core/Shortcut.cs: a key and its modifiers, stored under WPF's own enum names so
//! settings.json stays the same file on both builds and both platforms.

use crate::json::{Json, JsonError, Result};

/// System.Windows.Input.Key, by value. Where WPF gives a value two names (Return and
/// Enter, Prior and PageUp...) the first declared is the one written; which name
/// .NET's enum formatting picks for such a pair is not settled from the source alone,
/// so reading takes both (ALIASES).
const KEYS: [&str; 173] = [
    "None", "Cancel", "Back", "Tab", "LineFeed", "Clear", "Return", "Pause", "Capital", "KanaMode", "JunjaMode", "FinalMode",
    "HanjaMode", "Escape", "ImeConvert", "ImeNonConvert", "ImeAccept", "ImeModeChange", "Space", "Prior", "Next", "End", "Home",
    "Left", "Up", "Right", "Down", "Select", "Print", "Execute", "Snapshot", "Insert", "Delete", "Help",
    "D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "D8", "D9",
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "LWin", "RWin", "Apps", "Sleep",
    "NumPad0", "NumPad1", "NumPad2", "NumPad3", "NumPad4", "NumPad5", "NumPad6", "NumPad7", "NumPad8", "NumPad9",
    "Multiply", "Add", "Separator", "Subtract", "Decimal", "Divide",
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15", "F16", "F17", "F18", "F19",
    "F20", "F21", "F22", "F23", "F24",
    "NumLock", "Scroll", "LeftShift", "RightShift", "LeftCtrl", "RightCtrl", "LeftAlt", "RightAlt",
    "BrowserBack", "BrowserForward", "BrowserRefresh", "BrowserStop", "BrowserSearch", "BrowserFavorites", "BrowserHome",
    "VolumeMute", "VolumeDown", "VolumeUp", "MediaNextTrack", "MediaPreviousTrack", "MediaStop", "MediaPlayPause",
    "LaunchMail", "SelectMedia", "LaunchApplication1", "LaunchApplication2",
    "Oem1", "OemPlus", "OemComma", "OemMinus", "OemPeriod", "Oem2", "Oem3", "AbntC1", "AbntC2", "Oem4", "Oem5", "Oem6", "Oem7",
    "Oem8", "Oem102", "ImeProcessed", "System", "OemAttn", "OemFinish", "OemCopy", "OemAuto", "OemEnlw", "OemBackTab", "Attn",
    "CrSel", "ExSel", "EraseEof", "Play", "Zoom", "NoName", "Pa1", "OemClear", "DeadCharProcessed",
];

const ALIASES: [(&str, u16); 29] = [
    ("Enter", 6), ("CapsLock", 8), ("HangulMode", 9), ("KanjiMode", 12), ("PageUp", 19), ("PageDown", 20), ("PrintScreen", 30),
    ("OemSemicolon", 140), ("OemQuestion", 145), ("OemTilde", 146), ("OemOpenBrackets", 149), ("OemPipe", 150),
    ("OemCloseBrackets", 151), ("OemQuotes", 152), ("OemBackslash", 154), ("DbeAlphanumeric", 157), ("DbeKatakana", 158),
    ("DbeHiragana", 159), ("DbeSbcsChar", 160), ("DbeDbcsChar", 161), ("DbeRoman", 162), ("DbeNoRoman", 163),
    ("DbeEnterWordRegisterMode", 164), ("DbeEnterImeConfigureMode", 165), ("DbeFlushString", 166), ("DbeCodeInput", 167),
    ("DbeNoCodeInput", 168), ("DbeDetermineString", 169), ("DbeEnterDialogConversionMode", 170),
];

/// A WPF Key value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Key(pub u16);

impl Key {
    pub const NONE: Key = Key(0);
    pub const BACK: Key = Key(2);
    pub const ESCAPE: Key = Key(13);
    pub const N: Key = Key(57);
    pub const SYSTEM: Key = Key(156);

    pub fn name(self) -> Option<&'static str> { KEYS.get(self.0 as usize).copied() }

    pub fn from_name(s: &str) -> Option<Key> {
        let s = s.trim();
        KEYS.iter().position(|n| n.eq_ignore_ascii_case(s)).map(|i| Key(i as u16))
            .or_else(|| ALIASES.iter().find(|(n, _)| n.eq_ignore_ascii_case(s)).map(|&(_, v)| Key(v)))
    }

    /// A letter A–Z.
    pub fn letter(c: char) -> Option<Key> { c.is_ascii_alphabetic().then(|| Key(44 + (c.to_ascii_uppercase() as u16 - b'A' as u16))) }
    pub fn digit(d: u8) -> Option<Key> { (d < 10).then_some(Key(34 + d as u16)) }
    pub fn function(n: u8) -> Option<Key> { (1..=24).contains(&n).then(|| Key(89 + n as u16)) }
}

/// System.Windows.Input.ModifierKeys, a flags enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(pub u8);

impl Modifiers {
    pub const NONE: Modifiers = Modifiers(0);
    pub const ALT: Modifiers = Modifiers(1);
    pub const CONTROL: Modifiers = Modifiers(2);
    pub const SHIFT: Modifiers = Modifiers(4);
    pub const WINDOWS: Modifiers = Modifiers(8);
    const NAMES: [(&'static str, u8); 4] = [("Alt", 1), ("Control", 2), ("Shift", 4), ("Windows", 8)];

    pub fn has(self, m: Modifiers) -> bool { self.0 & m.0 == m.0 && m.0 != 0 }

    /// The enum's text as JsonStringEnumConverter writes a flags value: the names in
    /// ascending value order, ", " between them; None for zero; a number when some bit
    /// has no name.
    pub fn to_json(self) -> Json {
        if self.0 == 0 { return Json::str("None"); }
        if self.0 & !15 != 0 { return Json::int(self.0 as i64); }
        Json::str(Self::NAMES.iter().filter(|(_, v)| self.0 & v != 0).map(|(n, _)| *n).collect::<Vec<_>>().join(", "))
    }

    pub fn from_json(v: &Json) -> Result<Modifiers> {
        match v {
            Json::Num(_) => { let n = v.i32()?; u8::try_from(n).map(Modifiers).map_err(|_| JsonError("not ModifierKeys".into())) }
            Json::Str(s) => {
                let mut m = 0u8;
                for part in s.split(',') {
                    let p = part.trim();
                    if p.eq_ignore_ascii_case("None") { continue; }
                    m |= Self::NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(p)).map(|&(_, v)| v).ok_or_else(|| JsonError(format!("{p} is not a modifier")))?;
                }
                Ok(Modifiers(m))
            }
            _ => Err(JsonError("expected ModifierKeys".into())),
        }
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Modifiers;
    fn bitor(self, o: Modifiers) -> Modifiers { Modifiers(self.0 | o.0) }
}

/// Core.Shortcut: {"Key": ..., "Modifiers": ...}.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Shortcut { pub key: Key, pub modifiers: Modifiers }

impl Shortcut {
    /// Settings' default: Alt+N (Option-N on a Mac).
    pub const DEFAULT: Shortcut = Shortcut { key: Key::N, modifiers: Modifiers::ALT };

    pub fn is_set(&self) -> bool { self.key != Key::NONE }

    pub fn to_json(&self) -> Json {
        let key = match self.key.name() { Some(n) => Json::str(n), None => Json::int(self.key.0 as i64) };
        Json::obj(vec![("Key", key), ("Modifiers", self.modifiers.to_json())])
    }

    pub fn from_json(v: &Json) -> Result<Shortcut> {
        let mut s = Shortcut::default();
        for (k, x) in v.props()? {
            match k.as_str() {
                "Key" => s.key = match x {
                    Json::Str(n) => Key::from_name(n).ok_or_else(|| JsonError(format!("{n} is not a Key")))?,
                    _ => Key(u16::try_from(x.i32()?).map_err(|_| JsonError("not a Key".into()))?),
                },
                "Modifiers" => s.modifiers = Modifiers::from_json(x)?,
                _ => {}
            }
        }
        Ok(s)
    }

    /// Shortcut.ToString: "Ctrl+Alt+Shift+Win+Key", or an em dash when unset.
    pub fn label(&self) -> String {
        if !self.is_set() { return "—".into(); }
        let mut parts = vec![];
        if self.modifiers.has(Modifiers::CONTROL) { parts.push("Ctrl".to_string()); }
        if self.modifiers.has(Modifiers::ALT) { parts.push("Alt".into()); }
        if self.modifiers.has(Modifiers::SHIFT) { parts.push("Shift".into()); }
        if self.modifiers.has(Modifiers::WINDOWS) { parts.push("Win".into()); }
        parts.push(match self.key.name() {
            Some("Back") => "Backspace".into(),
            Some("Escape") => "Esc".into(),
            Some("OemPeriod") => ".".into(),
            Some("OemComma") => ",".into(),
            Some("OemPlus") => "+".into(),
            Some("OemMinus") => "−".into(),
            Some("Add") => "Num +".into(),
            Some("Subtract") => "Num −".into(),
            Some(n) => n.into(),
            None => self.key.0.to_string(),
        });
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SettingsTests expects "Key": "H" for Ctrl+Shift+H; the flags text follows
    /// Enum formatting (ascending values, ", ").
    #[test]
    fn written_under_wpf_names() {
        let s = Shortcut { key: Key::letter('h').unwrap(), modifiers: Modifiers::CONTROL | Modifiers::SHIFT };
        assert_eq!(s.to_json().indented("\n"), "{\n  \"Key\": \"H\",\n  \"Modifiers\": \"Control, Shift\"\n}");
        assert_eq!(Shortcut::DEFAULT.to_json().compact(), r#"{"Key":"N","Modifiers":"Alt"}"#);
        assert_eq!(Shortcut::default().to_json().compact(), r#"{"Key":"None","Modifiers":"None"}"#);
        assert_eq!(Shortcut::from_json(&s.to_json()).unwrap(), s);
        let r = Shortcut::from_json(&crate::json::parse(r#"{"Key":"enter","Modifiers":"shift, alt"}"#).unwrap()).unwrap();
        assert_eq!((r.key, r.modifiers), (Key(6), Modifiers(5)));
        assert_eq!(Key::function(24).unwrap().name(), Some("F24"));
        assert_eq!(Key::digit(9).unwrap().name(), Some("D9"));
        assert_eq!(Key::from_name("OemClear"), Some(Key(171)));
    }

    #[test]
    fn labels_as_shortcut_to_string() {
        assert_eq!(Shortcut::DEFAULT.label(), "Alt+N");
        assert_eq!(Shortcut { key: Key::BACK, modifiers: Modifiers::WINDOWS | Modifiers::CONTROL }.label(), "Ctrl+Win+Backspace");
        assert_eq!(Shortcut::default().label(), "—");
    }
}

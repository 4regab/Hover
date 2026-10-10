package core

// shortcut.rs: a key and its modifiers, stored under WPF's own enum names so
// settings.json stays the same file on both builds and both platforms.

import (
	"errors"
	"fmt"
	"runtime"
	"strconv"
	"strings"
)

// keyNames is System.Windows.Input.Key, by value. Where WPF gives a value two names
// (Return and Enter, Prior and PageUp...) the first declared is the one written; reading
// takes both (keyAliases).
var keyNames = [173]string{
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
}

var keyAliases = []struct {
	name string
	key  uint16
}{
	{"Enter", 6}, {"CapsLock", 8}, {"HangulMode", 9}, {"KanjiMode", 12}, {"PageUp", 19}, {"PageDown", 20}, {"PrintScreen", 30},
	{"OemSemicolon", 140}, {"OemQuestion", 145}, {"OemTilde", 146}, {"OemOpenBrackets", 149}, {"OemPipe", 150},
	{"OemCloseBrackets", 151}, {"OemQuotes", 152}, {"OemBackslash", 154}, {"DbeAlphanumeric", 157}, {"DbeKatakana", 158},
	{"DbeHiragana", 159}, {"DbeSbcsChar", 160}, {"DbeDbcsChar", 161}, {"DbeRoman", 162}, {"DbeNoRoman", 163},
	{"DbeEnterWordRegisterMode", 164}, {"DbeEnterImeConfigureMode", 165}, {"DbeFlushString", 166}, {"DbeCodeInput", 167},
	{"DbeNoCodeInput", 168}, {"DbeDetermineString", 169}, {"DbeEnterDialogConversionMode", 170},
}

// Key is a WPF Key value.
type Key uint16

const (
	KeyNone   Key = 0
	KeyBack   Key = 2
	KeyEscape Key = 13
	KeyN      Key = 57
	KeySystem Key = 156
)

func (k Key) Name() (string, bool) {
	if int(k) < len(keyNames) {
		return keyNames[k], true
	}
	return "", false
}

func KeyFromName(s string) (Key, bool) {
	s = strings.TrimSpace(s)
	for i, n := range keyNames {
		if asciiEqualFold(n, s) {
			return Key(i), true
		}
	}
	for _, a := range keyAliases {
		if asciiEqualFold(a.name, s) {
			return Key(a.key), true
		}
	}
	return 0, false
}

// KeyLetter is a letter A–Z.
func KeyLetter(c rune) (Key, bool) {
	switch {
	case c >= 'a' && c <= 'z':
		return Key(44 + c - 'a'), true
	case c >= 'A' && c <= 'Z':
		return Key(44 + c - 'A'), true
	}
	return 0, false
}

func KeyDigit(d uint8) (Key, bool)    { return Key(34 + uint16(d)), d < 10 }
func KeyFunction(n uint8) (Key, bool) { return Key(89 + uint16(n)), n >= 1 && n <= 24 }

// Modifiers is System.Windows.Input.ModifierKeys, a flags enum.
type Modifiers uint8

const (
	ModNone    Modifiers = 0
	ModAlt     Modifiers = 1
	ModControl Modifiers = 2
	ModShift   Modifiers = 4
	ModWindows Modifiers = 8
)

var modNames = []struct {
	name string
	v    Modifiers
}{{"Alt", 1}, {"Control", 2}, {"Shift", 4}, {"Windows", 8}}

func (m Modifiers) Has(x Modifiers) bool { return m&x == x && x != 0 }

// ToJSON is the enum's text as JsonStringEnumConverter writes a flags value: the names in
// ascending value order, ", " between them; None for zero; a number when some bit has no
// name.
func (m Modifiers) ToJSON() JSON {
	if m == 0 {
		return JStr("None")
	}
	if m&^15 != 0 {
		return JInt(int64(m))
	}
	var parts []string
	for _, n := range modNames {
		if m&n.v != 0 {
			parts = append(parts, n.name)
		}
	}
	return JStr(strings.Join(parts, ", "))
}

func ModifiersFromJSON(v JSON) (Modifiers, error) {
	switch v.Kind() {
	case NumKind:
		n, err := v.I32()
		if err != nil {
			return 0, err
		}
		if n < 0 || n > 255 {
			return 0, errors.New("not ModifierKeys")
		}
		return Modifiers(n), nil
	case StrKind:
		s, _ := v.AsStr()
		var m Modifiers
		for _, part := range strings.Split(s, ",") {
			p := strings.TrimSpace(part)
			if asciiEqualFold(p, "None") {
				continue
			}
			found := false
			for _, n := range modNames {
				if asciiEqualFold(n.name, p) {
					m |= n.v
					found = true
					break
				}
			}
			if !found {
				return 0, fmt.Errorf("%s is not a modifier", p)
			}
		}
		return m, nil
	}
	return 0, errors.New("expected ModifierKeys")
}

// Shortcut is Core.Shortcut: {"Key": ..., "Modifiers": ...}.
type Shortcut struct {
	Key       Key
	Modifiers Modifiers
}

// DefaultShortcut is Settings' default: Alt+N (Option-N on a Mac).
var DefaultShortcut = Shortcut{KeyN, ModAlt}

func (s Shortcut) IsSet() bool { return s.Key != KeyNone }

func (s Shortcut) ToJSON() JSON {
	key := JInt(int64(s.Key))
	if n, ok := s.Key.Name(); ok {
		key = JStr(n)
	}
	return JObj(P("Key", key), P("Modifiers", s.Modifiers.ToJSON()))
}

func ShortcutFromJSON(v JSON) (Shortcut, error) {
	props, err := v.Props()
	if err != nil {
		return Shortcut{}, err
	}
	var s Shortcut
	for _, p := range props {
		switch p.Key {
		case "Key":
			if n, ok := p.Val.AsStr(); ok {
				k, ok := KeyFromName(n)
				if !ok {
					return Shortcut{}, fmt.Errorf("%s is not a Key", n)
				}
				s.Key = k
			} else {
				n, err := p.Val.I32()
				if err != nil {
					return Shortcut{}, err
				}
				if n < 0 || n > 65535 {
					return Shortcut{}, errors.New("not a Key")
				}
				s.Key = Key(n)
			}
		case "Modifiers":
			m, err := ModifiersFromJSON(p.Val)
			if err != nil {
				return Shortcut{}, err
			}
			s.Modifiers = m
		}
	}
	return s, nil
}

// Label is Shortcut.ToString: "Ctrl+Alt+Shift+Win+Key", or an em dash when unset. On a
// Mac the same bits are Control, Option, Shift and Command (⌃⌥⇧⌘), written as macOS does.
func (s Shortcut) Label() string { return s.LabelFor(runtime.GOOS == "darwin") }

func (s Shortcut) LabelFor(mac bool) string {
	if !s.IsSet() {
		return "—"
	}
	var parts []string
	for _, m := range []struct {
		m        Modifiers
		win, mac string
	}{{ModControl, "Ctrl", "⌃"}, {ModAlt, "Alt", "⌥"}, {ModShift, "Shift", "⇧"}, {ModWindows, "Win", "⌘"}} {
		if s.Modifiers.Has(m.m) {
			if mac {
				parts = append(parts, m.mac)
			} else {
				parts = append(parts, m.win)
			}
		}
	}
	name, ok := s.Key.Name()
	switch {
	case !ok:
		name = strconv.Itoa(int(s.Key))
	case name == "Back":
		name = "Backspace"
	case name == "Escape":
		name = "Esc"
	case name == "OemPeriod":
		name = "."
	case name == "OemComma":
		name = ","
	case name == "OemPlus":
		name = "+"
	case name == "OemMinus":
		name = "−"
	case name == "Add":
		name = "Num +"
	case name == "Subtract":
		name = "Num −"
	}
	parts = append(parts, name)
	if mac {
		return strings.Join(parts, "")
	}
	return strings.Join(parts, "+")
}

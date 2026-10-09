package app

import (
	"strconv"
	"strings"

	"github.com/4regab/Hover/go/internal/core"
)

// keys.rs: keys between the three worlds a shortcut passes through: what the UI reports
// when a chord is pressed in Settings (Gio's key names here, Slint's key text in Rust),
// the WPF Key the settings keep (so settings.json stays the C#'s), and what the system
// grabs: a virtual-key code for RegisterHotKey (KeyInterop.VirtualKeyFromKey) or an X
// keysym for XGrabKey.

// WPF Key values (System.Windows.Input.Key).
const (
	wpfD0 = 34
	wpfA  = 44
	wpfF1 = 90
)

// KeyFromName is a key the UI reported as the Key it is in WPF: letters and digits by
// their character (upper or lower case), the named keys by Gio's names (and the window's
// own for those Gio has none for: Insert, Pause, PrintScreen, Apps, F13 to F24), and the
// US layout's punctuation as its Oem key (shifted or not). Modifier keys alone are false:
// the recorder waits for the key that goes with them.
func KeyFromName(name string) (core.Key, bool) {
	r := []rune(name)
	if len(r) == 1 {
		c := r[0]
		var v uint16
		switch {
		case c >= 'a' && c <= 'z':
			v = wpfA + uint16(c-'a')
		case c >= 'A' && c <= 'Z':
			v = wpfA + uint16(c-'A')
		case c >= '0' && c <= '9':
			v = wpfD0 + uint16(c-'0')
		default:
			// Shifted digits on a US layout: the key is the digit's.
			if k := strings.IndexRune(")!@#$%^&*(", c); k >= 0 {
				v = wpfD0 + uint16(k)
				break
			}
			switch c {
			case ';', ':':
				v = 140
			case '=', '+':
				v = 141
			case ',', '<':
				v = 142
			case '-', '_':
				v = 143
			case '.', '>':
				v = 144
			case '/', '?':
				v = 145
			case '`', '~':
				v = 146
			case '[', '{':
				v = 149
			case '\\', '|':
				v = 150
			case ']', '}':
				v = 151
			case '\'', '"':
				v = 152
			case '←':
				v = 23
			case '↑':
				v = 24
			case '→':
				v = 25
			case '↓':
				v = 26
			case '⏎', '⌤':
				v = 6
			case '⎋':
				v = 13
			case '⇱':
				v = 22
			case '⇲':
				v = 21
			case '⌫':
				v = 2
			case '⌦':
				v = 32
			case '⇞':
				v = 19
			case '⇟':
				v = 20
			default:
				return 0, false
			}
		}
		return core.Key(v), true
	}
	switch name {
	case "Space":
		return 18, true
	case "Tab":
		return 3, true
	case "Insert":
		return 31, true
	case "Pause":
		return 7, true
	case "PrintScreen":
		return 30, true
	case "Apps":
		return 72, true
	case "ScrollLock":
		return 115, true
	}
	if n, err := strconv.Atoi(strings.TrimPrefix(name, "F")); err == nil && strings.HasPrefix(name, "F") && n >= 1 && n <= 24 {
		return core.Key(wpfF1 + n - 1), true
	}
	return 0, false
}

// IsModifier: the modifier keys (Shift, Ctrl, Alt, Super, Command, Caps Lock): pressed
// alone they start a chord, not end one.
func IsModifier(name string) bool {
	switch name {
	case "Shift", "Ctrl", "Alt", "Super", "⌘", "CapsLock", "AltGr":
		return true
	}
	return false
}

// Mods is the UI's modifiers as the settings keep them.
func Mods(alt, control, shift, meta bool) core.Modifiers {
	m := core.ModNone
	if alt {
		m |= core.ModAlt
	}
	if control {
		m |= core.ModControl
	}
	if shift {
		m |= core.ModShift
	}
	if meta {
		m |= core.ModWindows
	}
	return m
}

// Recorded is the recorder's step (Pages.ShortcutField's PreviewKeyDown): Esc stops, a
// modifier alone waits, a key without a modifier asks for one, anything else is the chord.
type Recorded struct {
	Kind  RecordedKind
	Chord core.Shortcut
}

type RecordedKind int

const (
	RecStop RecordedKind = iota
	RecWait
	RecNeedModifier
	RecChord
)

func Record(name string, m core.Modifiers) Recorded {
	if name == "⎋" {
		return Recorded{Kind: RecStop}
	}
	if IsModifier(name) {
		return Recorded{Kind: RecWait}
	}
	key, ok := KeyFromName(name)
	if !ok {
		return Recorded{Kind: RecWait}
	}
	// A global shortcut without a modifier would take an ordinary typing key away from
	// every application on the desktop.
	if m == core.ModNone {
		return Recorded{Kind: RecNeedModifier}
	}
	return Recorded{Kind: RecChord, Chord: core.Shortcut{Key: key, Modifiers: m}}
}

// VK is KeyInterop.VirtualKeyFromKey for the keys a shortcut can hold; false has no
// mapping (HotKeys.Register then says so and fails).
func VK(k core.Key) (uint16, bool) {
	v := uint16(k)
	switch {
	case v >= 34 && v <= 43:
		return 0x30 + (v - 34), true
	case v >= 44 && v <= 69:
		return 0x41 + (v - 44), true
	case v >= 74 && v <= 83:
		return 0x60 + (v - 74), true
	case v >= 90 && v <= 113:
		return 0x70 + (v - 90), true
	}
	m := map[uint16]uint16{
		2: 0x08, 3: 0x09, 5: 0x0C, 6: 0x0D, 7: 0x13, 8: 0x14, 13: 0x1B, 18: 0x20,
		19: 0x21, 20: 0x22, 21: 0x23, 22: 0x24, 23: 0x25, 24: 0x26, 25: 0x27, 26: 0x28,
		27: 0x29, 28: 0x2A, 29: 0x2B, 30: 0x2C, 31: 0x2D, 32: 0x2E, 33: 0x2F,
		70: 0x5B, 71: 0x5C, 72: 0x5D, 73: 0x5F,
		84: 0x6A, 85: 0x6B, 86: 0x6C, 87: 0x6D, 88: 0x6E, 89: 0x6F,
		114: 0x90, 115: 0x91,
		140: 0xBA, 141: 0xBB, 142: 0xBC, 143: 0xBD, 144: 0xBE, 145: 0xBF, 146: 0xC0,
		149: 0xDB, 150: 0xDC, 151: 0xDD, 152: 0xDE, 153: 0xDF, 154: 0xE2,
	}
	c, ok := m[v]
	return c, ok
}

// Keysym is the X keysym of the key, as the US layout names it.
func Keysym(k core.Key) (uint32, bool) {
	v := uint32(k)
	switch {
	case v >= 34 && v <= 43:
		return 0x30 + (v - 34), true
	case v >= 44 && v <= 69:
		return 0x61 + (v - 44), true
	case v >= 74 && v <= 83:
		return 0xffb0 + (v - 74), true
	case v >= 90 && v <= 113:
		return 0xffbe + (v - 90), true
	}
	m := map[uint32]uint32{
		2: 0xff08, 3: 0xff09, 6: 0xff0d, 7: 0xff13, 13: 0xff1b, 18: 0x20,
		19: 0xff55, 20: 0xff56, 21: 0xff57, 22: 0xff50, 23: 0xff51, 24: 0xff52, 25: 0xff53, 26: 0xff54,
		30: 0xff61, 31: 0xff63, 32: 0xffff, 72: 0xff67,
		84: 0xffaa, 85: 0xffab, 87: 0xffad, 88: 0xffae, 89: 0xffaf,
		140: 0x3b, 141: 0x3d, 142: 0x2c, 143: 0x2d, 144: 0x2e, 145: 0x2f, 146: 0x60,
		149: 0x5b, 150: 0x5c, 151: 0x5d, 152: 0x27,
	}
	c, ok := m[v]
	return c, ok
}

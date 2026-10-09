//go:build linux

package wayland

import (
	"fmt"
	"sync"
	"unicode"
	"unicode/utf8"

	"gioui.org/io/key"
	"github.com/ebitengine/purego"
)

// libxkbcommon, called through purego: the compositor sends the keyboard layout as text,
// and only xkbcommon turns a key and the modifiers held into the letter it types.

var (
	xkbOnce sync.Once
	xkbErr  error

	xkbContextNew           func(flags int32) uintptr
	xkbKeymapNewFromString  func(ctx uintptr, s string, format int32, flags int32) uintptr
	xkbKeymapUnref          func(km uintptr)
	xkbKeymapKeyRepeats     func(km uintptr, key uint32) int32
	xkbStateNew             func(km uintptr) uintptr
	xkbStateUnref           func(st uintptr)
	xkbStateUpdateMask      func(st uintptr, dep, lat, lock, depL, latL, locL uint32) uint32
	xkbStateKeyGetOneSym    func(st uintptr, key uint32) uint32
	xkbStateKeyGetUTF8      func(st uintptr, key uint32, buf *byte, size uintptr) int32
	xkbStateModNameIsActive func(st uintptr, name string, typ int32) int32
	xkbCtx                  uintptr
)

func loadXkb() error {
	xkbOnce.Do(func() {
		var h uintptr
		var err error
		for _, n := range []string{"libxkbcommon.so.0", "libxkbcommon.so"} {
			if h, err = purego.Dlopen(n, purego.RTLD_NOW|purego.RTLD_GLOBAL); err == nil {
				break
			}
		}
		if err != nil {
			xkbErr = fmt.Errorf("libxkbcommon is not installed (%v)", err)
			return
		}
		purego.RegisterLibFunc(&xkbContextNew, h, "xkb_context_new")
		purego.RegisterLibFunc(&xkbKeymapNewFromString, h, "xkb_keymap_new_from_string")
		purego.RegisterLibFunc(&xkbKeymapUnref, h, "xkb_keymap_unref")
		purego.RegisterLibFunc(&xkbKeymapKeyRepeats, h, "xkb_keymap_key_repeats")
		purego.RegisterLibFunc(&xkbStateNew, h, "xkb_state_new")
		purego.RegisterLibFunc(&xkbStateUnref, h, "xkb_state_unref")
		purego.RegisterLibFunc(&xkbStateUpdateMask, h, "xkb_state_update_mask")
		purego.RegisterLibFunc(&xkbStateKeyGetOneSym, h, "xkb_state_key_get_one_sym")
		purego.RegisterLibFunc(&xkbStateKeyGetUTF8, h, "xkb_state_key_get_utf8")
		purego.RegisterLibFunc(&xkbStateModNameIsActive, h, "xkb_state_mod_name_is_active")
		if xkbCtx = xkbContextNew(0); xkbCtx == 0 {
			xkbErr = fmt.Errorf("xkb_context_new failed")
		}
	})
	return xkbErr
}

// keyLayout is a keymap with two states: one that follows the modifiers held (for the text a
// key types and the modifiers), and one that never changes (for the key's name, which is
// the key as printed: Shift+1 is still "1", as the Windows window names it).
type keyLayout struct {
	km, state, base uintptr
}

func newKeyLayout(text string) (*keyLayout, error) {
	if err := loadXkb(); err != nil {
		return nil, err
	}
	km := xkbKeymapNewFromString(xkbCtx, text, 1, 0) // XKB_KEYMAP_FORMAT_TEXT_V1
	if km == 0 {
		return nil, fmt.Errorf("the keyboard layout could not be read")
	}
	return &keyLayout{km: km, state: xkbStateNew(km), base: xkbStateNew(km)}, nil
}

func (l *keyLayout) close() {
	if l == nil {
		return
	}
	xkbStateUnref(l.state)
	xkbStateUnref(l.base)
	xkbKeymapUnref(l.km)
}

func (l *keyLayout) update(dep, lat, lock, group uint32) {
	xkbStateUpdateMask(l.state, dep, lat, lock, 0, 0, group)
}

func (l *keyLayout) repeats(code uint32) bool { return xkbKeymapKeyRepeats(l.km, code+8) != 0 }

func (l *keyLayout) mods() key.Modifiers {
	var m key.Modifiers
	on := func(n string) bool { return xkbStateModNameIsActive(l.state, n, 8) > 0 } // effective
	if on("Shift") {
		m |= key.ModShift
	}
	if on("Control") {
		m |= key.ModCtrl
	}
	if on("Mod1") {
		m |= key.ModAlt
	}
	if on("Mod4") {
		m |= key.ModSuper
	}
	return m
}

// text is what the key types with the modifiers held now.
func (l *keyLayout) text(code uint32) string {
	var buf [16]byte
	n := xkbStateKeyGetUTF8(l.state, code+8, &buf[0], uintptr(len(buf)))
	if n <= 0 {
		return ""
	}
	return string(buf[:min(int(n), len(buf)-1)])
}

// name is Gio's name for the key (evdev code), as the Windows window names them.
func (l *keyLayout) name(code uint32) (key.Name, bool) {
	return keysymName(xkbStateKeyGetOneSym(l.base, code+8))
}

func keysymName(sym uint32) (key.Name, bool) {
	switch {
	case sym >= 'a' && sym <= 'z':
		return key.Name(rune(sym - 32)), true
	case sym >= 'A' && sym <= 'Z', sym >= '0' && sym <= '9':
		return key.Name(rune(sym)), true
	case sym >= 0xffb0 && sym <= 0xffb9: // keypad digits
		return key.Name(rune('0' + sym - 0xffb0)), true
	case sym >= 0xffbe && sym <= 0xffbe+34: // F1 and on
		return key.Name(fmt.Sprintf("F%d", sym-0xffbe+1)), true
	}
	if n, ok := keysymNames[sym]; ok {
		return n, true
	}
	if sym >= 0x20 && sym < 0x7f {
		return key.Name(rune(sym)), true
	}
	if sym > 0xff && sym < 0x110000 {
		if r := rune(sym); utf8.ValidRune(r) {
			return key.Name(string(unicode.ToUpper(r))), true
		}
	}
	return "", false
}

var keysymNames = map[uint32]key.Name{
	0x20: "Space", 0xff0d: "⏎", 0xff8d: "⏎", 0xff1b: "⎋", 0xff09: "Tab", 0xfe20: "Tab", 0xff08: "⌫", 0xffff: "⌦",
	0xff51: "←", 0xff52: "↑", 0xff53: "→", 0xff54: "↓", 0xff55: "⇞", 0xff56: "⇟", 0xff50: "⇱", 0xff57: "⇲", 0xff63: "Insert",
	0xffe1: "Shift", 0xffe2: "Shift", 0xffe3: "Ctrl", 0xffe4: "Ctrl", 0xffe9: "Alt", 0xffea: "Alt", 0xffeb: "Super", 0xffec: "Super",
	0xffe5: "CapsLock", 0xff13: "Pause", 0xff61: "PrintScreen", 0xff67: "Apps", 0xff14: "ScrollLock",
	0xffab: "+", 0xffad: "-", 0xffaa: "*", 0xffaf: "/", 0xffae: ".",
}

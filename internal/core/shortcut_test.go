package core

import (
	"runtime"
	"testing"
)

// The tests of shortcut.rs, one for one.

// SettingsTests expects "Key": "H" for Ctrl+Shift+H; the flags text follows Enum
// formatting (ascending values, ", ").
func TestWrittenUnderWPFNames(t *testing.T) {
	h, _ := KeyLetter('h')
	s := Shortcut{h, ModControl | ModShift}
	if got := s.ToJSON().Indented("\n"); got != "{\n  \"Key\": \"H\",\n  \"Modifiers\": \"Control, Shift\"\n}" {
		t.Fatal(got)
	}
	if got := DefaultShortcut.ToJSON().Compact(); got != `{"Key":"N","Modifiers":"Alt"}` {
		t.Fatal(got)
	}
	if got := (Shortcut{}).ToJSON().Compact(); got != `{"Key":"None","Modifiers":"None"}` {
		t.Fatal(got)
	}
	if back, err := ShortcutFromJSON(s.ToJSON()); err != nil || back != s {
		t.Fatal(back, err)
	}
	v, _ := ParseJSON(`{"Key":"enter","Modifiers":"shift, alt"}`)
	if r, err := ShortcutFromJSON(v); err != nil || r.Key != 6 || r.Modifiers != 5 {
		t.Fatal(r, err)
	}
	if k, _ := KeyFunction(24); func() string { n, _ := k.Name(); return n }() != "F24" {
		t.Fatal(k)
	}
	if k, _ := KeyDigit(9); func() string { n, _ := k.Name(); return n }() != "D9" {
		t.Fatal(k)
	}
	if k, ok := KeyFromName("OemClear"); !ok || k != 171 {
		t.Fatal(k)
	}
}

func TestLabelsAsShortcutToString(t *testing.T) {
	if got := DefaultShortcut.LabelFor(false); got != "Alt+N" {
		t.Fatal(got)
	}
	if got := (Shortcut{KeyBack, ModWindows | ModControl}).LabelFor(false); got != "Ctrl+Win+Backspace" {
		t.Fatal(got)
	}
	if got := (Shortcut{}).Label(); got != "—" {
		t.Fatal(got)
	}
	// Off a Mac, Label() is the Windows text.
	if runtime.GOOS != "darwin" && DefaultShortcut.Label() != "Alt+N" {
		t.Fatal(DefaultShortcut.Label())
	}
}

// The same bits on a Mac: Alt is Option, Windows is Command.
func TestLabelsInMacOSSymbols(t *testing.T) {
	if got := DefaultShortcut.LabelFor(true); got != "⌥N" {
		t.Fatal(got)
	}
	space, _ := KeyFromName("Space")
	voice := Shortcut{space, ModControl | ModAlt}
	if voice.LabelFor(true) != "⌃⌥Space" || voice.LabelFor(false) != "Ctrl+Alt+Space" {
		t.Fatal(voice.LabelFor(true), voice.LabelFor(false))
	}
	all := Shortcut{KeyEscape, ModWindows | ModShift | ModAlt | ModControl}
	if got := all.LabelFor(true); got != "⌃⌥⇧⌘Esc" {
		t.Fatal(got)
	}
	if got := (Shortcut{}).LabelFor(true); got != "—" {
		t.Fatal(got)
	}
}

package backend

import (
	"errors"
	"fmt"
	"strings"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// The Windows host's settings commands (docs/development/windows-csharp.md, B2 rows 7 and 8).
// `settings` answers with the whole model as settings.json holds it; `setSetting` changes one
// setting by the name that file gives it and answers with the whole model again. The backend
// is the only writer of settings.json, so the bytes stay the ones Go writes. Every value is
// checked here: the host's line is untrusted.

const (
	// settingTextMax is the longest text a setting takes (a folder path).
	settingTextMax = 4096
	// themeNameMax, themeColorsMax and themeColorMax bound a saved theme: a VS Code theme
	// keeps a few dozen colours, each a short text.
	themeNameMax   = 200
	themeColorsMax = 512
	themeColorMax  = 128
)

// sendSettings is `settings`: {type: "settings", model}, the model as settings.json holds it.
func (b *Backend) sendSettings() {
	b.link.out.Send(core.JObj(core.P("type", jst("settings")), core.P("model", b.settings.Model().ToJSON())))
}

// setSetting is `setSetting` {key, value}. The keys are the names in settings.json that the
// Windows pages set: Appearance, Theme, WorkspaceSize, ScWorkspace, ChatView, LastEditor,
// KiroFolder and KiroRetryBusy. A value that does not fit changes nothing and is a toast.
// Like the Go setters it calls, it does one thing: the Settings page that picks an
// appearance also clears the theme, which the host does by sending Theme null.
func (b *Backend) setSetting(m core.JSON) error {
	key, ok := strOf(m, "key")
	if !ok {
		return errors.New("Choose which setting to change.")
	}
	v, has := m.Get("value")
	if !has {
		return errors.New("Give the setting a value.")
	}
	s := b.settings
	switch key {
	case "Appearance":
		i, err := enumIn(v, core.AppearanceNames)
		if err != nil {
			return fmt.Errorf("Appearance is %s.", strings.Join(core.AppearanceNames, ", "))
		}
		s.SetAppearance(core.Appearance(i))
	case "WorkspaceSize":
		i, err := enumIn(v, core.WorkspaceSizeNames)
		if err != nil {
			return fmt.Errorf("WorkspaceSize is %s.", strings.Join(core.WorkspaceSizeNames, ", "))
		}
		s.SetWorkspaceSize(core.WorkspaceSize(i))
	case "Theme":
		t, err := themeIn(v)
		if err != nil {
			return err
		}
		s.SetTheme(t)
	case "ScWorkspace":
		if v.Kind() != core.ObjKind {
			return errors.New("ScWorkspace is a shortcut: {Key, Modifiers}.")
		}
		sc, err := core.ShortcutFromJSON(v)
		if err != nil {
			return errors.New("ScWorkspace isn't a shortcut Hover knows.")
		}
		s.SetScWorkspace(sc)
	case "ChatView", "KiroRetryBusy":
		if v.Kind() != core.BoolKind {
			return fmt.Errorf("%s is true or false.", key)
		}
		on, _ := v.Bool()
		if key == "ChatView" {
			s.SetChatView(on)
		} else {
			s.SetKiroRetryBusy(on)
		}
	case "LastEditor":
		id, ok := v.AsStr()
		if !ok || !knownEditor(id) {
			return errors.New("LastEditor is an editor Hover knows, or fm for the file manager.")
		}
		s.SetLastEditor(id)
	case "KiroFolder":
		if v.IsNull() {
			s.SetKiroFolder(nil)
			break
		}
		p, ok := v.AsStr()
		if !ok || units(p) > settingTextMax || strings.ContainsFunc(p, func(r rune) bool { return r < 0x20 || r == 0x7f }) {
			return errors.New("KiroFolder is a folder path.")
		}
		// As picked, even when the folder has gone; but a path that names no place on
		// this computer from any working folder is not one a picker gives.
		if strings.TrimSpace(p) != "" && !agents.FullyQualified(p) {
			return errors.New("KiroFolder is a full folder path.")
		}
		s.SetKiroFolder(&p)
	case "Voice":
		vs, err := voiceIn(v, s.Voice())
		if err != nil {
			return err
		}
		s.SetVoice(vs)
		// What the voice flow reads (the shortcut, whether it is on) is read at the next press.
	case "ToolAgent":
		if err := b.setToolAgent(v); err != nil {
			return err
		}
	default:
		return fmt.Errorf("%s can't be changed this way.", clip(key, 40))
	}
	b.sendSettings()
	b.link.push()
	return nil
}

// enumIn is an enum's index: its name in any case, or its number in range.
func enumIn(v core.JSON, names []string) (int, error) {
	i, ok, err := v.EnumOf(names)
	if err != nil || !ok {
		return 0, errors.New("not a member")
	}
	return i, nil
}

// knownEditor: an editor in the Open in list, or the file manager.
func knownEditor(id string) bool {
	if id == agents.FileManager {
		return true
	}
	for _, e := range agents.EditorsBuiltin {
		if e[0] == id {
			return true
		}
	}
	return false
}

// themeIn is a saved theme {Name, Dark, Colors{key: text}}, or none for null.
func themeIn(v core.JSON) (*core.SavedTheme, error) {
	if v.IsNull() {
		return nil, nil
	}
	bad := errors.New("Theme is a saved theme: {Name, Dark, Colors}, or null.")
	if v.Kind() != core.ObjKind {
		return nil, bad
	}
	t, err := core.SavedThemeFromJSON(v)
	if err != nil {
		return nil, bad
	}
	if units(t.Name) > themeNameMax || len(t.Colors) > themeColorsMax {
		return nil, errors.New("That theme is too big.")
	}
	for _, c := range t.Colors {
		if units(c.Key) > themeColorMax || units(c.Val) > themeColorMax {
			return nil, errors.New("That theme is too big.")
		}
	}
	return &t, nil
}

// voiceIn is the voice settings a host sends ({Enabled, Shortcut, Microphone, Speech, Model,
// Cleanup, CleanupProvider, CleanupModel, CleanupBase, Agent, Countdown, AuraColor,
// HoldToTalk}, as the model has them): read the way settings.json is read, then bounded. The
// local model is Phonon's to say, so it stays as it is.
func voiceIn(v core.JSON, current core.VoiceSettings) (core.VoiceSettings, error) {
	bad := errors.New("Voice is the voice settings: {Enabled, Shortcut, Microphone, Speech, Model, Cleanup, ...}.")
	if v.Kind() != core.ObjKind {
		return current, bad
	}
	// settings.json is read leniently (a value of the wrong kind is its default); a host's
	// line is not, or a typo would quietly reset a setting.
	props, _ := v.Props()
	for _, p := range props {
		x := p.Val
		okKind := true
		switch p.Key {
		case "Enabled", "Cleanup", "HoldToTalk":
			okKind = x.Kind() == core.BoolKind
		case "Microphone", "Model", "CleanupModel", "CleanupBase", "Agent", "AuraColor":
			okKind = x.Kind() == core.StrKind || x.IsNull()
		case "Speech", "CleanupProvider":
			okKind = x.Kind() == core.StrKind || x.Kind() == core.NumKind
		case "Countdown":
			n, err := x.I32()
			okKind = x.Kind() == core.NumKind && err == nil && n >= 0 && n <= 60
		case "Shortcut":
			okKind = x.Kind() == core.ObjKind
		}
		if !okKind {
			return current, bad
		}
	}
	vs, err := core.VoiceSettingsFromJSON(v)
	if err != nil {
		return current, bad
	}
	long := func(p *string, max int) bool { return p != nil && units(*p) > max }
	if long(vs.Microphone, 100) || long(vs.CleanupModel, 200) || long(vs.CleanupBase, 500) {
		return current, errors.New("A voice setting is too long.")
	}
	if vs.CleanupBase != nil {
		b := strings.TrimRight(*vs.CleanupBase, "/")
		if !strings.HasPrefix(b, "https://") && !strings.HasPrefix(b, "http://") || strings.ContainsFunc(b, func(r rune) bool { return r <= ' ' || r == 0x7f }) {
			return current, errors.New("Use the full address, starting with https://.")
		}
		vs.CleanupBase = &b
	}
	vs.Local = current.Local
	return vs, nil
}

// setToolAgent is `setSetting` ToolAgent {tool, agent}: the agent a tool runs as (Kiro's
// --agent, OpenCode's mode), or none for null.
func (b *Backend) setToolAgent(v core.JSON) error {
	bad := errors.New("ToolAgent is {tool, agent}.")
	if v.Kind() != core.ObjKind {
		return bad
	}
	tool, ok := core.ParseTool(optStrOf(v, "tool"))
	if !ok {
		return bad
	}
	o := b.settings.AgentOptions(tool)
	o.Agent = nil
	if x, has := v.Get("agent"); has && !x.IsNull() {
		a, isStr := x.AsStr()
		if !isStr || units(a) > 100 || strings.ContainsFunc(a, func(r rune) bool { return r < 0x20 || r == 0x7f }) {
			return bad
		}
		if a = strings.TrimSpace(a); a != "" {
			o.Agent = &a
		}
	}
	b.settings.SetAgentOptions(tool, o)
	return nil
}

package backend

import (
	"regexp"
	"strconv"
	"strings"
	"unicode/utf16"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// DeskInfo.ScreenAction and DeskInfo.Number: a computer-use step in words, for the Screen
// panel's activity ("Clicked", "Typed" and what it was on). The rest of what the Screen
// panel needs from the backend (`testing`, `apps`, and which steps are computer use) is
// internal/agents' desk.go.

func itoa(n int64) string { return strconv.FormatInt(n, 10) }

// screenVerbs are the verbs a computer-use title can name, in the order DeskInfo.ScreenVerb
// tries them at each place in the title (so "double_click" wins over "click" where both
// start).
var screenVerbs = []string{
	"double_click", "right_click", "left_click", "click", "type_text", "press_key", "hotkey", "scroll", "drag", "move_mouse", "move_cursor",
	"launch_app", "open_app", "screenshot", "get_window_state", "get_desktop_state", "list_apps", "list_windows", "set_value", "zoom", "invoke_menu",
}

// verbOf is the leftmost verb in the title, any case.
func verbOf(title string) string {
	t := strings.ToLower(title)
	for i := range t {
		for _, v := range screenVerbs {
			if strings.HasPrefix(t[i:], v) {
				return v
			}
		}
	}
	return ""
}

// number is the first of these whole-number fields in a step's input JSON (DeskInfo.Number).
func number(input *string, names []string) (int64, bool) {
	if input == nil || !strings.HasPrefix(*input, "{") {
		return 0, false
	}
	v, err := core.ParseJSON(*input)
	if err != nil {
		return 0, false
	}
	for _, n := range names {
		if x, ok := v.Get(n); ok && x.Kind() == core.NumKind {
			if k, err := x.I64(); err == nil {
				return k, true
			}
		}
	}
	return 0, false
}

// cut60 is t[..59] + "…" past 60 UTF-16 units.
func cut60(t string) string {
	if len(utf16.Encode([]rune(t))) <= 60 {
		return t
	}
	return cutUnits(t, 59) + "…"
}

var (
	keysRe      = regexp.MustCompile(`"keys"\s*:\s*\[([^\]]*)\]`)
	spaceToolRe = regexp.MustCompile(`computer_(?:type|key|launch|hotkey|move_cursor|get_window|get_accessibility_tree)\b`)
)

// driverTitle: a Cua Space's tools are computer_* (computer_type, computer_key,
// computer_launch…); Cua Driver's on the user's desktop are type_text, press_key,
// launch_app. The title with the first kind named as the second.
func driverTitle(title string) string {
	return spaceToolRe.ReplaceAllStringFunc(title, func(m string) string {
		switch m[len("computer_"):] {
		case "type":
			return "type_text"
		case "key":
			return "press_key"
		case "launch":
			return "launch_app"
		case "get_window", "get_accessibility_tree":
			return "get_window_state"
		}
		return m[len("computer_"):]
	})
}

func some(s string) *string { return &s }

// screenAction is a computer-use step in words, as the screen panel's activity lists it:
// what was done ("Clicked", "Typed") and on what (the text typed, the key, the app).
func screenAction(x *agents.DeskStep) (string, *string) {
	input := x.Input
	app := agents.DeskField(input, []string{"app_name", "appName", "name", "app", "bundle_id"})
	did := ""
	var on *string
	switch verbOf(driverTitle(x.Title)) {
	case "click", "left_click":
		did = "Clicked"
		if t := agents.DeskField(input, []string{"label", "element_label", "text"}); t != nil {
			on = some(cut60(*t))
		} else if cx, ok1 := number(input, []string{"x"}); ok1 {
			if cy, ok2 := number(input, []string{"y"}); ok2 {
				on = some("at " + itoa(cx) + ", " + itoa(cy))
			} else {
				on = app
			}
		} else {
			on = app
		}
	case "double_click":
		did, on = "Double-clicked", app
	case "right_click":
		did, on = "Right-clicked", app
	case "type_text", "set_value":
		did = "Typed"
		if t := agents.DeskField(input, []string{"text", "value"}); t != nil {
			on = some(cut60("“" + *t + "”"))
		}
	case "press_key":
		did, on = "Pressed", agents.DeskField(input, []string{"key"})
	case "hotkey":
		did = "Pressed"
		if input != nil && strings.Contains(*input, "keys") {
			keys := ""
			if m := keysRe.FindStringSubmatch(*input); m != nil {
				keys = m[1]
			}
			on = some(cut60(strings.ReplaceAll(strings.ReplaceAll(keys, `"`, ""), ",", "+")))
		}
	case "scroll":
		did, on = "Scrolled", agents.DeskField(input, []string{"direction"})
	case "drag":
		did, on = "Dragged", app
	case "move_mouse", "move_cursor":
		did = "Moved the cursor"
	case "launch_app", "open_app":
		did, on = "Opened", app
		if on == nil {
			on = agents.DeskField(input, []string{"bundle_id"})
		}
	case "screenshot", "get_desktop_state", "zoom":
		did, on = "Looked at the screen", app
	case "get_window_state":
		did, on = "Read the window", app
	case "list_apps", "list_windows":
		did = "Listed the apps"
	case "invoke_menu":
		did = "Used a menu"
		if p := agents.DeskField(input, []string{"path"}); p != nil {
			on = some(cut60(*p))
		}
	default:
		did, on = "Used the computer", some(cut60(x.Title))
	}
	return did, on
}

//go:build linux

package linux

import (
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/godbus/dbus/v5"
)

// MARK: Pickers (x11.rs's pick, for Wayland)

type filterPattern struct {
	Kind    uint32
	Pattern string
}

type filter struct {
	Name     string
	Patterns []filterPattern
}

// portalPick asks the desktop portal's FileChooser. done is false when the portal isn't
// there (so another picker can be tried); path is empty when it was cancelled.
func portalPick(title string, folder bool, filters []filter) (path string, done bool) {
	conn, err := Connect("")
	if err != nil {
		return "", false
	}
	defer conn.Close()
	resp, err := request(conn, 30*time.Minute, func(tok string) *dbus.Call {
		opts := map[string]dbus.Variant{"handle_token": dbus.MakeVariant(tok)}
		if folder {
			opts["directory"] = dbus.MakeVariant(true)
		}
		if len(filters) > 0 {
			opts["filters"] = dbus.MakeVariant(filters)
		}
		return conn.Object(portalBus, portalPath).Call("org.freedesktop.portal.FileChooser.OpenFile", 0, "", title, opts)
	})
	if err != nil {
		return "", false
	}
	if resp.Code != 0 {
		return "", true
	}
	if uris, ok := resp.Results["uris"].Value().([]string); ok && len(uris) > 0 {
		if p, ok := fileURI(uris[0]); ok {
			return p, true
		}
	}
	return "", true
}

// pick is the portal's, else zenity (GNOME) or kdialog (KDE), in that order. A cancel is
// not asked of the next.
func pick(title string, folder bool, filters []filter, zenity, kdialog []string) (string, bool) {
	if p, done := portalPick(title, folder, filters); done {
		return p, p != ""
	}
	for _, t := range []struct {
		exe  string
		args []string
	}{{"zenity", zenity}, {"kdialog", kdialog}} {
		out, err := exec.Command(t.exe, t.args...).Output()
		if err == nil {
			p := strings.TrimSpace(string(out))
			return p, p != ""
		}
		if _, notFound := err.(*exec.Error); notFound {
			continue
		}
		return "", false // cancelled
	}
	return "", false
}

func home() string { h, _ := os.UserHomeDir(); return h }

// PickFolder is the agents' folder.
func PickFolder() (string, bool) {
	return pick("Choose the agents' folder", true, nil,
		[]string{"--file-selection", "--directory", "--title=Choose the agents' folder"},
		[]string{"--getexistingdirectory", home()})
}

// PickThemeFile is a VS Code theme file.
func PickThemeFile() (string, bool) {
	return pick("Import a VS Code theme file", false,
		[]filter{{"VS Code colour theme (*.json)", []filterPattern{{0, "*.json"}}}, {"All files", []filterPattern{{0, "*"}}}},
		[]string{"--file-selection", "--title=Import a VS Code theme file", "--file-filter=VS Code colour theme (*.json) | *.json", "--file-filter=All files | *"},
		[]string{"--getopenfilename", home(), "VS Code colour theme (*.json)"})
}

// PickImage is an image to attach.
func PickImage() (string, bool) {
	return pick("Attach an image", false,
		[]filter{{"Images", []filterPattern{{0, "*.png"}, {0, "*.jpg"}, {0, "*.jpeg"}, {0, "*.gif"}, {0, "*.webp"}}}},
		[]string{"--file-selection", "--title=Attach an image", "--file-filter=Images | *.png *.jpg *.jpeg *.gif *.webp"},
		[]string{"--getopenfilename", home(), "Images (*.png *.jpg *.jpeg *.gif *.webp)"})
}

// OpenURL opens a link in the desktop's browser.
func OpenURL(u string) error { return exec.Command("xdg-open", u).Start() }

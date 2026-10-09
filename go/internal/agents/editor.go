package agents

// editor.rs: Open in editor, a desk's folder, or a file in it, in VS Code, Zed, Cursor or
// Kiro IDE. Editors are found by the places their installers use and by PATH; nothing is
// installed. The folder is the session's own. Every argument goes to the program as it is
// (no shell), so a path with spaces, Unicode or shell characters is one argument. The
// editor is started and let go: it is no child of any agent and ends with nobody.
//
// Launch forms, from each editor's own docs: VS Code and its forks (Cursor, Kiro IDE) take
// `<folder> --goto <file>:<line>:<column>`; Zed takes `<folder> <file>:<line>:<column>`.
// Kiro IDE's command is `kiro`, which is not kiro-cli (the agent Hover runs): a `kiro` that
// turns out to be kiro-cli's own file is refused.

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/4regab/Hover/go/internal/core"
)

// EditorsBuiltin are the editors Hover knows: id and the name shown.
var EditorsBuiltin = [][2]string{{"vscode", "VS Code"}, {"zed", "Zed"}, {"cursor", "Cursor"}, {"kiro", "Kiro IDE"}}

func EditorName(id string) string {
	for _, b := range EditorsBuiltin {
		if b[0] == id {
			return b[1]
		}
	}
	return id
}

// EditorTarget is what to open: the folder, and perhaps a file in it (relative to it) with
// a line and column.
type EditorTarget struct {
	Folder       string
	File         *string
	Line, Column *uint32
}

func EditorFolder(folder string) EditorTarget { return EditorTarget{Folder: folder} }

// EditorFile is the folder with a file in it. A file outside the folder (or a link out of
// it) is dropped and the folder opens alone.
func EditorFile(folder, rel string, line, column *uint32) EditorTarget {
	if Inside(folder, rel) == "" {
		return EditorTarget{Folder: folder}
	}
	return EditorTarget{Folder: folder, File: &rel, Line: line, Column: column}
}

// CheckFolder: the folder an open can use, or why not. cloud is a Kiro Web session: its
// work is in Kiro's cloud, so there is nothing here to open (and nothing is cloned to make
// one).
func CheckFolder(folder string, cloud bool) error {
	if cloud {
		return fmt.Errorf("This task runs in Kiro's cloud, so there is no local folder to open.")
	}
	if !UsableFolder(folder) {
		return fmt.Errorf("The folder isn’t there any more: %s", folder)
	}
	return nil
}

// MARK: Finding

func editorPlaces(id string) []string {
	switch runtime.GOOS {
	case "windows":
		local, pf := os.Getenv("LOCALAPPDATA"), os.Getenv("ProgramFiles")
		under := func(root, rest string) []string {
			if root == "" {
				return nil
			}
			return []string{filepath.Join(root, rest)}
		}
		switch id {
		case "vscode":
			return append(under(local, `Programs\Microsoft VS Code\Code.exe`), under(pf, `Microsoft VS Code\Code.exe`)...)
		case "cursor":
			return append(under(local, `Programs\cursor\Cursor.exe`), under(local, `Programs\Cursor\Cursor.exe`)...)
		case "kiro":
			return append(under(local, `Programs\Kiro\Kiro.exe`), under(pf, `Kiro\Kiro.exe`)...)
		case "zed":
			return append(under(local, `Programs\Zed\zed.exe`), under(pf, `Zed\zed.exe`)...)
		}
	case "darwin":
		app := func(a, rest string) []string {
			return []string{filepath.Join("/Applications", a, rest), filepath.Join(Home(), "Applications", a, rest)}
		}
		switch id {
		case "vscode":
			return app("Visual Studio Code.app", "Contents/Resources/app/bin/code")
		case "cursor":
			return app("Cursor.app", "Contents/Resources/app/bin/cursor")
		case "kiro":
			return app("Kiro.app", "Contents/Resources/app/bin/kiro")
		case "zed":
			return app("Zed.app", "Contents/MacOS/cli")
		}
	default:
		h := Home()
		switch id {
		case "vscode":
			return []string{"/usr/bin/code", "/usr/share/code/bin/code", "/snap/bin/code", "/opt/visual-studio-code/bin/code"}
		case "cursor":
			return []string{"/usr/bin/cursor", "/opt/Cursor/cursor", filepath.Join(h, ".local/bin/cursor")}
		case "kiro":
			return []string{"/usr/bin/kiro", "/opt/Kiro/bin/kiro", "/usr/share/kiro/bin/kiro", filepath.Join(h, ".local/bin/kiro")}
		case "zed":
			return []string{filepath.Join(h, ".local/bin/zed"), "/usr/bin/zed", "/usr/bin/zeditor", "/usr/bin/zed-editor", "/usr/local/bin/zed"}
		}
	}
	return nil
}

// editorCommands are the names an editor answers to on PATH.
func editorCommands(id string) []string {
	switch id {
	case "vscode":
		return []string{"code"}
	case "cursor":
		return []string{"cursor"}
	case "kiro":
		return []string{"kiro"}
	case "zed":
		return []string{"zed", "zeditor", "zed-editor"}
	}
	return nil
}

// FindEditor is the program that opens the editor, or "" when it isn't found. The places
// its installer uses come first (a real program over a PATH shim), then PATH.
func FindEditor(id string) string {
	found := ""
	for _, p := range editorPlaces(id) {
		if isFile(p) {
			found = p
			break
		}
	}
	if found == "" {
		for _, c := range editorCommands(id) {
			if found = OnPath(c); found != "" {
				break
			}
		}
	}
	if found != "" && id == "kiro" && isKiroCli(found) {
		return ""
	}
	return found
}

// isKiroCli: `kiro` on PATH can be kiro-cli's own file (Hover's agent), which opens no window.
func isKiroCli(p string) bool {
	real := func(p string) string {
		if r, err := filepath.EvalSymlinks(p); err == nil {
			if a, err := filepath.Abs(r); err == nil {
				return a
			}
		}
		return p
	}
	me := real(p)
	if strings.HasPrefix(strings.ToLower(fileStem(me)), "kiro-cli") {
		return true
	}
	cli := Exe(core.Kiro)
	return cli != "" && real(cli) == me
}

// FoundEditor is an editor Hover found: its id, the name shown and its program.
type FoundEditor struct{ ID, Name, Exe string }

// AvailableEditors are the editors that can be opened now: the four Hover knows that were
// found, in the order of EditorsBuiltin. Looks at the disk and PATH, so call it off the
// UI goroutine.
func AvailableEditors() []FoundEditor {
	var out []FoundEditor
	for _, b := range EditorsBuiltin {
		if exe := FindEditor(b[0]); exe != "" {
			out = append(out, FoundEditor{b[0], b[1], exe})
		}
	}
	return out
}

// PickEditor is which editor a click opens: the one named, else the first one found. The
// error says what is missing, so the message can tell the user what to do.
func PickEditor(choice *string, found []FoundEditor) (FoundEditor, error) {
	if choice != nil && *choice != "" {
		for _, f := range found {
			if f.ID == *choice {
				return f, nil
			}
		}
		return FoundEditor{}, fmt.Errorf("%s wasn’t found on this computer. Install it, or pick another editor.", EditorName(*choice))
	}
	if len(found) == 0 {
		return FoundEditor{}, fmt.Errorf("No editor was found. Install VS Code, Zed, Cursor or Kiro IDE.")
	}
	return found[0], nil
}

// MARK: Arguments

// EditorArgs are the arguments for id. A file is opened at its line and column.
func EditorArgs(id string, t EditorTarget) []string {
	place := ""
	if t.File != nil {
		if p := Inside(t.Folder, *t.File); p != "" {
			place = p
			if t.Line != nil {
				place += fmt.Sprintf(":%d", *t.Line)
				if t.Column != nil {
					place += fmt.Sprintf(":%d", *t.Column)
				}
			}
		}
	}
	switch {
	case id == "zed" && place != "":
		return []string{t.Folder, place}
	case id == "zed", place == "":
		return []string{t.Folder}
	}
	return []string{t.Folder, "--goto", place}
}

// MARK: Launching

// shimSafe: Windows runs .cmd and .bat through cmd.exe, which reads & | < > ^ % " as its
// own. An argument with one can't be passed as it is, so it is refused rather than changed.
func shimSafe(exe string, args []string) error {
	e := strings.ToLower(extension(exe))
	if runtime.GOOS == "windows" && (e == "cmd" || e == "bat") {
		for _, a := range args {
			if strings.ContainsAny(a, `&|<>^%"`) {
				return fmt.Errorf("Windows can't pass that path to %s safely.", exe)
			}
		}
	}
	return nil
}

// LaunchEditor starts the editor and lets go of it. No shell; stdio closed; its own
// process group, so Hover's signals and the agents' stops never reach it. A goroutine
// reaps it when the launcher exits.
func LaunchEditor(exe string, args []string) error {
	if err := shimSafe(exe, args); err != nil {
		return err
	}
	cmd := exec.Command(exe, args...)
	// A .cmd or .bat as Rust's Command runs one (shimSafe refused what cmd would read).
	if e := strings.ToLower(extension(exe)); e == "cmd" || e == "bat" {
		cmd = batCommand(exe, args)
	}
	cmd.Stdin, cmd.Stdout, cmd.Stderr = nil, nil, nil
	var env []string
	for _, kv := range os.Environ() {
		if !strings.HasPrefix(kv, "GIT_DIR=") && !strings.HasPrefix(kv, "GIT_WORK_TREE=") && !strings.HasPrefix(kv, "GIT_INDEX_FILE=") {
			env = append(env, kv)
		}
	}
	cmd.Env = env
	detach(cmd)
	if err := cmd.Start(); err != nil {
		return fmt.Errorf("%s didn’t start: %v", exe, err)
	}
	go cmd.Wait()
	return nil
}

// OpenEditor opens t in the editor: the one named, else the first found. What to tell the
// user: where it opened. Blocks briefly (it looks at the disk): call it off the UI goroutine.
func OpenEditor(choice *string, t EditorTarget, cloud bool) (string, error) {
	if err := CheckFolder(t.Folder, cloud); err != nil {
		return "", err
	}
	f, err := PickEditor(choice, AvailableEditors())
	if err != nil {
		return "", err
	}
	if err := LaunchEditor(f.Exe, EditorArgs(f.ID, t)); err != nil {
		return "", err
	}
	core.Logf("editor: %s opened %s", f.Name, t.Folder)
	return "Opened in " + f.Name + ".", nil
}

// EditorChoice is what the Open in picker lists: an editor, or the computer's file manager.
type EditorChoice struct {
	ID, Name string
	Last     bool
}

// FileManager is the id the file manager has in the picker.
const FileManager = "fm"

// FileManagerName is what the file manager is called here.
func FileManagerName() string {
	switch runtime.GOOS {
	case "windows":
		return "File Explorer"
	case "darwin":
		return "Finder"
	}
	return "Files"
}

// EditorChoices are the picker's rows: the last one used first and marked, then the
// editors found (in EditorsBuiltin order), then the file manager. Only editors that were
// found are listed; a last that is gone is forgotten.
func EditorChoices(found []FoundEditor, last *string) []EditorChoice {
	var all []EditorChoice
	for _, f := range found {
		all = append(all, EditorChoice{ID: f.ID, Name: f.Name})
	}
	all = append(all, EditorChoice{ID: FileManager, Name: FileManagerName()})
	if last != nil {
		for i, c := range all {
			if c.ID == *last {
				c.Last = true
				all = append(all[:i], all[i+1:]...)
				all = append([]EditorChoice{c}, all...)
				break
			}
		}
	}
	return all
}

// OpenFileManager opens t in the file manager: the folder, or the folder a file is in
// (selected, where the file manager can). No shell: the path is one argument.
func OpenFileManager(t EditorTarget, cloud bool) (string, error) {
	if err := CheckFolder(t.Folder, cloud); err != nil {
		return "", err
	}
	file := ""
	if t.File != nil {
		file = Inside(t.Folder, *t.File)
	}
	var exe string
	var args []string
	switch runtime.GOOS {
	case "windows":
		exe, args = "explorer.exe", []string{t.Folder}
		if file != "" {
			args = []string{"/select," + file}
		}
	default:
		exe = "xdg-open"
		if runtime.GOOS == "darwin" {
			exe = "open"
		}
		folder := t.Folder
		if file != "" {
			folder = filepath.Dir(file)
		}
		args = []string{folder}
	}
	if err := LaunchEditor(exe, args); err != nil {
		return "", err
	}
	return "Opened in " + FileManagerName() + ".", nil
}

// OpenEditorOrFirst is OpenEditor, but an editor that isn't found (the last one used, since
// uninstalled) gives way to the first that is. The id of the one used, and what to tell
// the user.
func OpenEditorOrFirst(choice *string, t EditorTarget, cloud bool) (string, string, error) {
	if err := CheckFolder(t.Folder, cloud); err != nil {
		return "", "", err
	}
	found := AvailableEditors()
	f, err := PickEditor(choice, found)
	if err != nil {
		first, err2 := PickEditor(nil, found)
		if err2 != nil {
			return "", "", err
		}
		f = first
	}
	if err := LaunchEditor(f.Exe, EditorArgs(f.ID, t)); err != nil {
		return "", "", err
	}
	core.Logf("editor: %s opened %s", f.Name, t.Folder)
	return f.ID, "Opened in " + f.Name + ".", nil
}

// OpenIn opens t in the one picked: an editor's id, or FileManager. Off the UI goroutine.
func OpenIn(choice string, t EditorTarget, cloud bool) (string, error) {
	if choice == FileManager {
		return OpenFileManager(t, cloud)
	}
	return OpenEditor(&choice, t, cloud)
}

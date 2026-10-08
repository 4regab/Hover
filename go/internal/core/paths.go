package core

// paths.rs: everything Hover owns lives in one folder. On Windows that is %APPDATA%\Hover;
// on Linux $XDG_DATA_HOME/Hover (~/.local/share/Hover); on macOS ~/Library/Application
// Support/Hover. HOVER_DATA_DIR overrides them all, and a test without it gets a
// temporary folder of its own. And log.rs: one line per event, to hover.log and stderr.

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
)

// ResolveDataDir is Paths.Init, with the platform's base folder given, so it can be tested
// anywhere. An empty or blank override is none.
func ResolveDataDir(overridden string, appData string) (string, error) {
	if strings.TrimSpace(overridden) != "" {
		forced := FullPath(overridden)
		return forced, os.MkdirAll(forced, 0o755)
	}
	if appData == "" {
		return "", errors.New("no application data folder")
	}
	dir := filepath.Join(appData, "Hover")
	// The app used to be called Noty. An existing install's settings and key come across
	// the first time the renamed build runs, only when there is an old folder and no new
	// one yet. There was never a Linux Noty, so there it never fires.
	legacy := filepath.Join(appData, "Noty")
	if !isDir(dir) && isDir(legacy) {
		// Not the log: logging needs this very folder.
		if err := os.Rename(legacy, dir); err != nil {
			fmt.Fprintf(os.Stderr, "hover: data migration failed — %v\n", err)
		}
	}
	return dir, os.MkdirAll(dir, 0o755)
}

func isDir(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.IsDir()
}

var support = sync.OnceValue(func() string {
	overridden := os.Getenv("HOVER_DATA_DIR")
	// A test's own data folder, one per process, so no test writes the user's log,
	// settings or key. (Rust told its test binaries apart by where cargo puts them.)
	if overridden == "" && testing.Testing() {
		overridden = filepath.Join(os.TempDir(), fmt.Sprintf("hover-test-data-%d", os.Getpid()))
	}
	dir, err := ResolveDataDir(overridden, AppData())
	if err != nil {
		// A folder that can't be made stops the app, as the C# static initialiser does.
		panic("Hover couldn't make its data folder: " + err.Error())
	}
	return dir
})

// Support is Paths.Support, found once.
func Support() string { return support() }

func KeyFile() string      { return filepath.Join(Support(), "note.key") }
func SettingsFile() string { return filepath.Join(Support(), "settings.json") }
func LogFile() string      { return filepath.Join(Support(), "hover.log") }
func AgentsDir() string    { return filepath.Join(Support(), "agents") }

// LexicalFullPath is Path.GetFullPath on Unix: rooted at the working folder and with "."
// and ".." taken out by the text alone (links are not followed), a trailing separator
// kept. filepath.Abs drops the trailing separator, so it isn't the same.
func LexicalFullPath(p, cwd string) string {
	joined := p
	if !strings.HasPrefix(p, "/") {
		joined = cwd + "/" + p
	}
	var parts []string
	for _, c := range strings.Split(joined, "/") {
		switch c {
		case "", ".":
		case "..":
			if len(parts) > 0 {
				parts = parts[:len(parts)-1]
			}
		default:
			parts = append(parts, c)
		}
	}
	out := "/" + strings.Join(parts, "/")
	if (strings.HasSuffix(p, "/") || strings.HasSuffix(p, "/.")) && out != "/" {
		return out + "/"
	}
	return out
}

// DropPlanner: the old planner went with the workspace in 2.0 and the user chose to have
// it deleted (OwlApp.DropPlanner). The key stays: the history is sealed with it.
func DropPlanner(dir string) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return
	}
	for _, e := range entries {
		name := e.Name()
		if strings.HasPrefix(name, "planner.dat") && e.Type().IsRegular() {
			if err := os.Remove(filepath.Join(dir, name)); err != nil {
				Logf("couldn't remove the old planner - %v", err)
			} else {
				Logf("removed %s (the workspace is gone)", name)
			}
		}
	}
}

var logGate sync.Mutex

// Logf is Log.cs: one line, to stderr and to hover.log in the data folder. Logging must
// never take the app down.
func Logf(format string, a ...any) {
	stamp := LocalClock() + " hover: " + fmt.Sprintf(format, a...)
	fmt.Fprintln(os.Stderr, stamp)
	logGate.Lock()
	defer logGate.Unlock()
	if f, err := os.OpenFile(LogFile(), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644); err == nil {
		f.WriteString(stamp + NewLine)
		f.Close()
	}
}

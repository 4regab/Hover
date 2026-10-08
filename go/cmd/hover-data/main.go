// Command hover-data is crates/hover-core/src/bin/hover-data.rs in Go: a data folder
// written and read by the port, for the Rust↔Go round trips.
//
//	hover-data write <data> <project> <turns> <answer.md>   settings, key, one sealed session; prints its key
//	hover-data dump <data>                                   what the folder holds, read by the port
//	hover-data steps <data> [n]                              the newest n sessions' steps (read only)
//	hover-data where                                         the data folder Hover would use
package main

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"

	"github.com/4regab/Hover/go/internal/core"
)

func key(data string) *core.Crypto {
	return core.LoadOrCreateCrypto(filepath.Join(data, "note.key"), core.SystemKeyGuard{})
}

func die(format string, a ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", a...)
	os.Exit(1)
}

func main() {
	a := os.Args
	cmd := ""
	if len(a) > 1 {
		cmd = a[1]
	}
	switch {
	case cmd == "write" && len(a) >= 6:
		data, project := a[2], a[3]
		turns, err := strconv.Atoi(a[4])
		if err != nil {
			die("a number of turns")
		}
		answer, err := os.ReadFile(a[5])
		if err != nil {
			die("the answer file: %v", err)
		}
		os.MkdirAll(data, 0o755)
		os.MkdirAll(project, 0o755)
		settings := core.LoadSettings(filepath.Join(data, "settings.json"))
		settings.SetKiroFolder(&project)
		settings.Flush()
		crypto := key(data)
		if crypto == nil {
			die("a key for the history")
		}
		h := core.NewAgentHistory(filepath.Join(data, "agents"), crypto)
		k := core.GUIDN()
		now := core.Now()
		t := func(i int) core.Stamp {
			return core.Stamp{Ticks: now.Ticks - int64(turns-i)*600_000_000, Kind: now.Kind}
		}
		text := string(answer)
		completed := core.Completed
		s := core.SavedSession{Key: k, Tool: core.Kiro, Folder: project, Title: "A long rich conversation", Context: ptr(42.0), Updated: now}
		for i := 0; i < turns; i++ {
			woke, ended := t(i), t(i).AddSecs(30)
			target := fmt.Sprintf("src/file%d.rs", i)
			s.Turns = append(s.Turns, core.SavedTurn{
				Prompt: fmt.Sprintf("Question %d about the rich fixture", i+1), Images: []string{},
				Steps: []core.KiroStep{core.NewStep(fmt.Sprintf("t%d", i), "read", fmt.Sprintf("Read src/file%d.rs", i), &target, "completed")},
				State: &completed, Text: &text, StartedAt: t(i), WokeAt: &woke, EndedAt: &ended,
			})
		}
		h.Save(s)
		h.Flush()
		fmt.Println(k)
	case cmd == "dump" && len(a) >= 3:
		data := a[2]
		crypto := key(data)
		if crypto == nil {
			fmt.Println("no key: history unreadable this run")
			return
		}
		h := core.NewAgentHistory(filepath.Join(data, "agents"), crypto)
		e := h.Entries()
		fmt.Printf("history: %d sessions\n", len(e))
		for _, x := range e {
			n, bytes := 0, 0
			if s, ok := h.Load(x.Key); ok {
				n = len(s.Turns)
				for _, t := range s.Turns {
					if t.Text != nil {
						bytes += len(*t.Text)
					}
				}
			}
			fmt.Printf("  %s… %s %s %q %d turns, %d answer bytes, updated %s\n", x.Key[:min(8, len(x.Key))], x.Tool.Name(), x.State.Name(), x.Title, n, bytes, x.Updated.ISO())
		}
	case cmd == "where":
		fmt.Println(core.Support())
	case cmd == "check" && len(a) >= 3:
		// Go only: whether this build would save a data folder's files with the same
		// bytes it read them as, settings.json and every sealed history file. Read only:
		// nothing is written, no key is made.
		os.Exit(check(a[2]))
	case cmd == "steps" && len(a) >= 3:
		// What the tools reported as steps, newest sessions first: kind, title, target and
		// status only (never prompts or answers). Read only: no key is made and no index
		// rebuilt.
		data := a[2]
		stored, err := os.ReadFile(filepath.Join(data, "note.key"))
		if err != nil {
			die("note.key: %v", err)
		}
		raw, kerr := core.SystemKeyGuard{}.Unwrap(stored)
		if kerr != nil || len(raw) != 32 {
			die("note.key unwraps to a 32-byte key")
		}
		crypto := core.CryptoWithKey([32]byte(raw))
		n := 5
		if len(a) > 3 {
			if x, err := strconv.Atoi(a[3]); err == nil {
				n = x
			}
		}
		entries, err := os.ReadDir(filepath.Join(data, "agents"))
		if err != nil {
			die("the agents folder: %v", err)
		}
		var all []core.SavedSession
		for _, e := range entries {
			if filepath.Ext(e.Name()) != ".dat" || e.Name() == "index.dat" {
				continue
			}
			b, err := os.ReadFile(filepath.Join(data, "agents", e.Name()))
			if err != nil {
				continue
			}
			v, err := core.ParseJSON(crypto.Open(b))
			if err != nil {
				continue
			}
			if s, err := core.SavedSessionFromJSON(v); err == nil {
				all = append(all, s)
			}
		}
		sort.SliceStable(all, func(i, j int) bool { return all[i].Updated.Compare(all[j].Updated) > 0 })
		for i, s := range all {
			if i >= n {
				break
			}
			fmt.Println(s.Tool.Name(), s.Updated.ISO())
			for _, t := range s.Turns {
				for _, st := range t.Steps {
					target := "None"
					if st.Target != nil {
						r := []rune(*st.Target)
						target = fmt.Sprintf("Some(%q)", string(r[:min(60, len(r))]))
					}
					fmt.Printf("  [%s] %q target=%s %s\n", st.Kind, st.Title, target, st.Status)
				}
			}
		}
	default:
		fmt.Fprintln(os.Stderr, "hover-data write <data> <project> <turns> <answer.md> | dump <data> | steps <data> [sessions] | check <data> | where")
		os.Exit(2)
	}
}

// check prints one line per file and returns how many would be saved differently.
func check(data string) int {
	bad := 0
	report := func(name string, size int, same bool, err error) {
		switch {
		case err != nil:
			fmt.Printf("%-44s unreadable: %v\n", name, err)
			bad++
		case same:
			fmt.Printf("%-44s %7d bytes, saved again the same\n", name, size)
		default:
			fmt.Printf("%-44s %7d bytes, saved again DIFFERENTLY\n", name, size)
			bad++
		}
	}
	if b, err := os.ReadFile(filepath.Join(data, "settings.json")); err == nil {
		text := core.TextOf(b)
		v, err := core.ParseJSON(text)
		var m core.Model
		if err == nil {
			m, err = core.ModelFromJSON(v)
		}
		report("settings.json", len(b), err == nil && m.Text() == string(b), err)
	}
	stored, err := os.ReadFile(filepath.Join(data, "note.key"))
	if err != nil {
		fmt.Println("note.key:", err)
		return bad + 1
	}
	raw, kerr := core.SystemKeyGuard{}.Unwrap(stored)
	if kerr != nil || len(raw) != 32 {
		fmt.Println("note.key doesn't unwrap to a 32-byte key here:", kerr)
		return bad + 1
	}
	c := core.CryptoWithKey([32]byte(raw))
	entries, _ := os.ReadDir(filepath.Join(data, "agents"))
	for _, e := range entries {
		if filepath.Ext(e.Name()) != ".dat" {
			continue
		}
		b, err := os.ReadFile(filepath.Join(data, "agents", e.Name()))
		if err != nil {
			report(e.Name(), 0, false, err)
			continue
		}
		plain := c.Open(b)
		v, err := core.ParseJSON(plain)
		var again string
		if err == nil && e.Name() == "index.dat" {
			var items []core.JSON
			if items, err = v.Items(); err == nil {
				var out []core.JSON
				for _, x := range items {
					var h core.HistoryEntry
					if h, err = core.HistoryEntryFromJSON(x); err != nil {
						break
					}
					out = append(out, h.ToJSON())
				}
				again = core.JArr(out...).Compact()
			}
		} else if err == nil {
			var s core.SavedSession
			if s, err = core.SavedSessionFromJSON(v); err == nil {
				again = s.ToJSON().Compact()
			}
		}
		report("agents/"+e.Name(), len(plain), err == nil && again == plain, err)
	}
	return bad
}

func ptr[T any](v T) *T { return &v }

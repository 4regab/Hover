package core

import (
	"bytes"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

// The tests of history.rs, one for one.

func at(t *testing.T, s string) Stamp { return mustStamp(t, s) }

func testSession(t *testing.T, key, updated string) SavedSession {
	woke := at(t, "2026-09-28T16:44:09.1234567Z")
	return SavedSession{
		Key: key, Tool: Codex, Folder: `C:\hover`, Title: "Fix the secret thing", AcpID: ptr("acp-1"), Context: ptr(3.37),
		Turns: []SavedTurn{{
			Prompt: "Fix the secret thing", Images: []string{},
			Steps: []KiroStep{NewStep("r0", "read", "Read File", ptr(`C:\hover\src\a.ts`), "completed")},
			State: ptr(Completed), Text: ptr("answer <b> & 'c'"), StartedAt: at(t, "2026-09-28T16:44:07.1234567Z"),
			WokeAt: &woke, Credits: ptr(0.087),
		}},
		Updated: at(t, updated),
	}
}

func dirFor(t *testing.T) string { return t.TempDir() }

func key3() *Crypto { return CryptoWithKey([32]byte(bytes.Repeat([]byte{3}, 32))) }

func keysOf(h *AgentHistory) []string {
	var k []string
	for _, e := range h.Entries() {
		k = append(k, e.Key)
	}
	return k
}

func sortedKeys(h *AgentHistory) []string {
	k := keysOf(h)
	sort.Strings(k)
	return k
}

// A turn's checkpoints are written only when taken, and read back.
func TestATurnsCheckpointsAreKeptAndATurnWithoutThemWritesAsBefore(t *testing.T) {
	s := testSession(t, "cccc", "2026-09-28T16:45:00Z")
	if strings.Contains(s.ToJSON().Compact(), "Checkpoint") {
		t.Fatal("none taken: the bytes earlier versions wrote")
	}
	b, a := strings.Repeat("1", 40), strings.Repeat("a", 40)
	s.Turns[0].Before, s.Turns[0].After = &b, &a
	text := s.ToJSON().Compact()
	if !strings.Contains(text, `"CheckpointBefore":"`+b+`","CheckpointAfter":"`+a+`"`) {
		t.Fatal(text)
	}
	if back := roundTrip(t, s.ToJSON(), SavedSessionFromJSON); !reflect.DeepEqual(back, s) {
		t.Fatalf("%+v", back)
	}
}

// The session file's JSON, as AgentHistory's options (compact, string enums) write
// SavedSession; derived from the records' declaration order.
func TestASessionWritesAsTheSerializerWritesTheRecord(t *testing.T) {
	s := testSession(t, "aaaa", "2026-09-28T16:45:00Z")
	want := `{"Key":"aaaa","Tool":"Codex","Folder":"C:\\hover","Title":"Fix the secret thing","AcpId":"acp-1","Context":3.37,` +
		`"Turns":[{"Prompt":"Fix the secret thing","Images":[],"Steps":[{"Id":"r0","Kind":"read","Title":"Read File","Target":"C:\\hover\\src\\a.ts","Status":"completed","Added":0,"Removed":0,"Diff":null,"Output":null,"Exit":null,"Ms":null}],` +
		`"State":"Completed","Text":"answer \u003Cb\u003E \u0026 \u0027c\u0027","StartedAt":"2026-09-28T16:44:07.1234567Z","WokeAt":"2026-09-28T16:44:09.1234567Z","EndedAt":null,"Credits":0.087}],` +
		`"Updated":"2026-09-28T16:45:00Z","Access":null}`
	if got := s.ToJSON().Compact(); got != want {
		t.Fatalf("%s\nwant %s", got, want)
	}
	if back := roundTrip(t, s.ToJSON(), SavedSessionFromJSON); !reflect.DeepEqual(back, s) {
		t.Fatalf("%+v", back)
	}
}

// AgentHistoryTests, ported: sealed on disk, whole again, newest first, deleted.
func TestSessionsAreSealedComeBackWholeAndGoWhenDeleted(t *testing.T) {
	d := dirFor(t)
	c := key3()
	h := NewAgentHistory(d, c)
	var n atomic.Int32
	h.OnChanged(func() { n.Add(1) })
	h.Save(testSession(t, "aaaa", "2026-09-28T16:45:00Z"))
	b := testSession(t, "bbbb", "2026-09-28T17:00:00+02:00")
	b.Turns[0].State = nil
	h.Save(b)
	h.Save(testSession(t, "aaaa", "2026-09-28T16:45:00Z"))
	h.Flush()
	entries, _ := os.ReadDir(d)
	for _, e := range entries {
		raw, _ := os.ReadFile(filepath.Join(d, e.Name()))
		if bytes.Contains(raw, []byte("secret")) {
			t.Fatal("sealed, not plain text")
		}
	}
	again := NewAgentHistory(d, c)
	e := again.Entries()
	if len(e) != 2 || e[0].Key != "aaaa" || e[1].Key != "bbbb" {
		t.Fatal(keysOf(again))
	}
	if e[0].State != Completed || e[1].State != Running || e[0].Turns != 1 {
		t.Fatalf("%+v", e)
	}
	if s, ok := again.Load("aaaa"); !ok || !reflect.DeepEqual(s, testSession(t, "aaaa", "2026-09-28T16:45:00Z")) {
		t.Fatalf("%+v", s)
	}
	if s, _ := again.Load("bbbb"); s.Updated.Kind != Local {
		t.Fatal(s.Updated.Kind)
	}
	again.Delete("aaaa")
	again.Flush()
	if _, ok := again.Load("aaaa"); ok {
		t.Fatal("deleted, still there")
	}
	if l := len(NewAgentHistory(d, c).Entries()); l != 1 {
		t.Fatal(l)
	}
	if n.Load() != 3 {
		t.Fatal(n.Load())
	}
	// Another key opens nothing, and says so in the log rather than failing.
	other := NewAgentHistory(d, CryptoWithKey([32]byte(bytes.Repeat([]byte{4}, 32))))
	if l := len(other.Entries()); l != 0 {
		t.Fatal(l)
	}
	// It set the index it couldn't open aside and writes a new one in the background;
	// wait for that, or the temp folder's cleanup races the write.
	other.Flush()
}

// A damaged index loses nothing: it is set aside and made again from the session files,
// and the next save keeps every session, not only the new one.
func TestAnUnreadableIndexIsRebuiltFromTheSessionFiles(t *testing.T) {
	d := dirFor(t)
	c := key3()
	h := NewAgentHistory(d, c)
	h.Save(testSession(t, "aaaa", "2026-09-28T16:45:00Z"))
	h.Save(testSession(t, "bbbb", "2026-09-28T17:45:00Z"))
	h.Flush()
	os.WriteFile(filepath.Join(d, "index.dat"), []byte("not sealed at all, a torn write"), 0o644)
	again := NewAgentHistory(d, c)
	if k := sortedKeys(again); !reflect.DeepEqual(k, []string{"aaaa", "bbbb"}) {
		t.Fatal(k)
	}
	again.Save(testSession(t, "cccc", "2026-09-28T18:45:00Z"))
	again.Flush()
	if k := sortedKeys(NewAgentHistory(d, c)); !reflect.DeepEqual(k, []string{"aaaa", "bbbb", "cccc"}) {
		t.Fatal(k)
	}
	if s, _ := NewAgentHistory(d, c).Load("bbbb"); !reflect.DeepEqual(s, testSession(t, "bbbb", "2026-09-28T17:45:00Z")) {
		t.Fatalf("%+v", s)
	}
	// The damaged index is kept beside it, not thrown away.
	found := false
	entries, _ := os.ReadDir(d)
	for _, e := range entries {
		found = found || strings.HasSuffix(e.Name(), ".bad")
	}
	if !found {
		t.Fatal("no .bad")
	}
}

// Hover ended after a session's file was written but before its index line: the session
// is still listed next time.
func TestASessionFileTheIndexMissedIsListedAgain(t *testing.T) {
	d := dirFor(t)
	c := key3()
	h := NewAgentHistory(d, c)
	h.Save(testSession(t, "aaaa", "2026-09-28T16:45:00Z"))
	h.Flush()
	time.Sleep(50 * time.Millisecond)
	s := testSession(t, "bbbb", "2026-09-28T17:45:00Z")
	if err := seal(c, filepath.Join(d, "bbbb.dat"), s.ToJSON().Compact()); err != nil {
		t.Fatal(err)
	}
	// A file of another key, and one that isn't a session, are left alone.
	os.WriteFile(filepath.Join(d, "cccc.dat"), CryptoWithKey([32]byte(bytes.Repeat([]byte{9}, 32))).Seal("{}"), 0o644)
	os.WriteFile(filepath.Join(d, "notes.txt"), []byte("x"), 0o644)
	again := NewAgentHistory(d, c)
	e := again.Entries()
	if k := keysOf(again); !reflect.DeepEqual(k, []string{"bbbb", "aaaa"}) {
		t.Fatal(k)
	}
	if e[0].State != Completed || e[0].Turns != 1 || e[0].Title != "Fix the secret thing" {
		t.Fatalf("%+v", e[0])
	}
	again.Flush()
	// Written back, so the next start reads it from the index.
	if l := len(NewAgentHistory(d, c).Entries()); l != 2 {
		t.Fatal(l)
	}
	if _, err := os.Stat(filepath.Join(d, "cccc.dat")); err != nil {
		t.Fatal("cccc.dat went")
	}
}

// A turn's last save can come just after its session was deleted; it mustn't bring it
// back (Arz's AgentHistory._deleted). Keys are never used again.
func TestASaveThatArrivesAfterTheDeleteDoesNotBringTheSessionBack(t *testing.T) {
	d := dirFor(t)
	c := key3()
	h := NewAgentHistory(d, c)
	var n atomic.Int32
	h.OnChanged(func() { n.Add(1) })
	h.Save(testSession(t, "aaaa", "2026-09-28T16:45:00Z"))
	h.Save(testSession(t, "bbbb", "2026-09-28T17:45:00Z"))
	h.Delete("aaaa")
	// The turn that was ending saves its session once more, after the delete.
	h.Save(testSession(t, "aaaa", "2026-09-28T18:45:00Z"))
	h.Flush()
	if k := keysOf(h); !reflect.DeepEqual(k, []string{"bbbb"}) {
		t.Fatal(k)
	}
	if _, ok := h.Load("aaaa"); ok {
		t.Fatal("no file for it either")
	}
	if _, err := os.Stat(filepath.Join(d, "aaaa.dat")); err == nil {
		t.Fatal("aaaa.dat")
	}
	// Two saves and one delete raised the change; the late save raised nothing.
	if n.Load() != 3 {
		t.Fatal(n.Load())
	}
	// Nor does it come back on the next start; the others are untouched.
	again := NewAgentHistory(d, c)
	if k := keysOf(again); !reflect.DeepEqual(k, []string{"bbbb"}) {
		t.Fatal(k)
	}
	if _, ok := again.Load("bbbb"); !ok {
		t.Fatal("bbbb went")
	}
	// A delete of a key that was never saved still keeps it out.
	h.Delete("cccc")
	h.Save(testSession(t, "cccc", "2026-09-28T19:45:00Z"))
	h.Flush()
	if k := keysOf(h); !reflect.DeepEqual(k, []string{"bbbb"}) {
		t.Fatal(k)
	}
}

type keyCredits struct {
	key     string
	credits *float64
}

func creditsList(h *AgentHistory) []keyCredits {
	var out []keyCredits
	for _, e := range h.Entries() {
		out = append(out, keyCredits{e.Key, e.Credits})
	}
	return out
}

// History shows what a session cost in all: its turns' credits added up, none when no
// turn says. An index written before it kept credits gets them once from the files.
func TestASessionsCreditsAreItsTurnsAddedUpAndOldIndexLinesGetThem(t *testing.T) {
	d := dirFor(t)
	c := key3()
	h := NewAgentHistory(d, c)
	s := testSession(t, "aaaa", "2026-09-28T16:45:00Z")
	s.Tool = Kiro
	turn := func(credits *float64) SavedTurn { x := s.Turns[0]; x.Credits = credits; return x }
	s.Turns = []SavedTurn{turn(ptr(0.25)), turn(nil), turn(ptr(1.5))}
	h.Save(s)
	none := testSession(t, "bbbb", "2026-09-28T17:45:00Z")
	none.Turns = []SavedTurn{turn(nil)}
	h.Save(none)
	h.Flush()
	want := []keyCredits{{"bbbb", nil}, {"aaaa", ptr(1.75)}}
	if got := creditsList(h); !reflect.DeepEqual(got, want) {
		t.Fatalf("%+v", got)
	}
	if got := creditsList(NewAgentHistory(d, c)); !reflect.DeepEqual(got, want) {
		t.Fatal("kept in the index")
	}
	// An index from before: no Credits on its lines. The Kiro session's are read from its
	// file once.
	var old []JSON
	for _, e := range h.Entries() {
		props, _ := e.ToJSON().Props()
		var kept []Prop
		for _, p := range props {
			if p.Key != "Credits" {
				kept = append(kept, p)
			}
		}
		old = append(old, JObj(kept...))
	}
	seal(c, filepath.Join(d, "index.dat"), JArr(old...).Compact())
	again := NewAgentHistory(d, c)
	if got := creditsList(again); !reflect.DeepEqual(got, want) {
		t.Fatalf("%+v", got)
	}
	again.Flush()
}

func TestAKeyThatIsntHoversNeverBecomesAPath(t *testing.T) {
	h := NewAgentHistory(dirFor(t), key3())
	if _, ok := h.Load(`..\..\x`); ok {
		t.Fatal("loaded")
	}
	h.Delete(`..\x`)
	if PlainKey("") || PlainKey(strings.Repeat("a", 65)) || !PlainKey(strings.Repeat("a", 64)) || PlainKey("ab-c") || PlainKey("é") {
		t.Fatal("PlainKey")
	}
}

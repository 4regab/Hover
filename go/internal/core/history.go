package core

// history.rs (Owl/AgentHistory.cs): every agent session there has been, kept until the
// user deletes it. An index of entries (the only part held in memory) and one file per
// session, each sealed with Hover's key. Writes go to a temporary file and then replace
// the old one, off the caller's goroutine, in order.

import (
	"errors"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"
)

// SavedTurn is SavedTurn(Prompt, Images, Steps, State, Text, StartedAt, WokeAt, EndedAt,
// Credits). Credits is nil for turns from before Hover kept it.
type SavedTurn struct {
	Prompt    string
	Images    []string
	Steps     []KiroStep
	State     *KiroState
	Text      *string
	StartedAt Stamp
	WokeAt    *Stamp
	EndedAt   *Stamp
	Credits   *float64
	// Before and After are the project folder's checkpoints (hover-agents' checkpoint
	// store) before the turn ran and after it: a tree id each. nil for turns from before
	// Hover kept them, or where none could be taken.
	Before, After *string
	// Ext is a reply still waiting, its chips and the like; written only when it holds
	// something.
	Ext TurnExt
}

// SavedSession is SavedSession(Key, Tool, Folder, Title, AcpId, Context, Turns, Updated,
// Access). Access is the tool access picked when the session started.
type SavedSession struct {
	Key     string
	Tool    AgentTool
	Folder  string
	Title   string
	AcpID   *string
	Context *float64
	Turns   []SavedTurn
	Updated Stamp
	Access  *string
	// Cloud: run in Kiro's cloud, the GitHub repos it was given ("owner/name"), empty
	// for an empty workspace. nil for a session on this computer.
	Cloud []string
	// Ext is the workspace binding and the links orchestration adds; written only when
	// it holds something.
	Ext SessionExt
}

// HistoryEntry is HistoryEntry(Key, Tool, Title, Folder, Updated, State, Turns, Credits).
type HistoryEntry struct {
	Key     string
	Tool    AgentTool
	Title   string
	Folder  string
	Updated Stamp
	State   KiroState
	Turns   int32
	// Credits is what the whole session cost, as the sum of its turns' credits. nil when
	// no turn says (only Kiro reports credits).
	Credits *float64
}

func optStamp(t *Stamp) JSON {
	if t == nil {
		return JNull
	}
	return t.ToJSON()
}

func (t SavedTurn) ToJSON() JSON {
	images := make([]JSON, len(t.Images))
	for i, x := range t.Images {
		images[i] = JStr(x)
	}
	steps := make([]JSON, len(t.Steps))
	for i, s := range t.Steps {
		steps[i] = s.ToJSON()
	}
	state := JNull
	if t.State != nil {
		state = t.State.ToJSON()
	}
	props := []Prop{P("Prompt", JStr(t.Prompt)), P("Images", JArr(images...)), P("Steps", JArr(steps...)), P("State", state),
		P("Text", optStr(t.Text)), P("StartedAt", t.StartedAt.ToJSON()), P("WokeAt", optStamp(t.WokeAt)), P("EndedAt", optStamp(t.EndedAt)),
		P("Credits", JOptDouble(t.Credits))}
	// Written only when taken, so a turn without them is the bytes 2.x and 3.0 wrote.
	if t.Before != nil {
		props = append(props, P("CheckpointBefore", JStr(*t.Before)))
	}
	if t.After != nil {
		props = append(props, P("CheckpointAfter", JStr(*t.After)))
	}
	if !t.Ext.IsEmpty() {
		props = append(props, P("Ext", t.Ext.ToJSON()))
	}
	return JObj(props...)
}

// stampOr reads a non-nullable DateTime: missing is d, null is an error.
func (r *reader) stampOr(k string, d Stamp) Stamp {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return d
	}
	t, err := StampFromJSON(x)
	r.fail(err)
	return t
}

func (r *reader) optStamp(k string) *Stamp {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return nil
	}
	t, err := OptStampFromJSON(x)
	r.fail(err)
	return t
}

func (r *reader) toolOr(k string, d AgentTool) AgentTool {
	x, ok := r.get(k)
	if !ok || r.err != nil {
		return d
	}
	t, err := ToolFromJSON(x)
	r.fail(err)
	return t
}

// SavedTurnFromJSON reads as the record's constructor gets it: a missing property is its
// default (a missing list is empty here, where C# would hand on a null).
func SavedTurnFromJSON(v JSON) (SavedTurn, error) {
	r := readObj(v)
	t := SavedTurn{Prompt: r.text("Prompt"), Images: r.strings("Images"), Steps: listOf(r, "Steps", StepFromJSON)}
	if t.Steps == nil {
		t.Steps = []KiroStep{}
	}
	if x, ok := r.get("State"); ok && r.err == nil {
		s, err := OptStateFromJSON(x)
		r.fail(err)
		t.State = s
	}
	t.Text = r.optText("Text")
	t.StartedAt = r.stampOr("StartedAt", Stamp{})
	t.WokeAt = r.optStamp("WokeAt")
	t.EndedAt = r.optStamp("EndedAt")
	t.Credits = r.optF64("Credits")
	t.Before = r.optText("CheckpointBefore")
	t.After = r.optText("CheckpointAfter")
	if x, ok := r.some("Ext"); ok && r.err == nil {
		e, err := TurnExtFromJSON(x)
		r.fail(err)
		t.Ext = e
	}
	return t, r.err
}

func (s SavedSession) ToJSON() JSON {
	turns := make([]JSON, len(s.Turns))
	for i, t := range s.Turns {
		turns[i] = t.ToJSON()
	}
	props := []Prop{P("Key", JStr(s.Key)), P("Tool", s.Tool.ToJSON()), P("Folder", JStr(s.Folder)), P("Title", JStr(s.Title)),
		P("AcpId", optStr(s.AcpID)), P("Context", JOptDouble(s.Context)), P("Turns", JArr(turns...)), P("Updated", s.Updated.ToJSON()),
		P("Access", optStr(s.Access))}
	// Only for a cloud session, so every other session is written byte for byte as before.
	if s.Cloud != nil {
		repos := make([]JSON, len(s.Cloud))
		for i, r := range s.Cloud {
			repos[i] = JStr(r)
		}
		props = append(props, P("Cloud", JArr(repos...)))
	}
	if !s.Ext.IsEmpty() {
		props = append(props, P("Ext", s.Ext.ToJSON()))
	}
	return JObj(props...)
}

func SavedSessionFromJSON(v JSON) (SavedSession, error) {
	r := readObj(v)
	s := SavedSession{Key: r.text("Key"), Tool: r.toolOr("Tool", Kiro), Folder: r.text("Folder"), Title: r.text("Title"),
		AcpID: r.optText("AcpId"), Context: r.optF64("Context"), Turns: listOf(r, "Turns", SavedTurnFromJSON)}
	if s.Turns == nil {
		s.Turns = []SavedTurn{}
	}
	s.Updated = r.stampOr("Updated", Stamp{})
	s.Access = r.optText("Access")
	s.Cloud = listOf(r, "Cloud", itemText)
	if x, ok := r.some("Ext"); ok && r.err == nil {
		e, err := SessionExtFromJSON(x)
		r.fail(err)
		s.Ext = e
	}
	return s, r.err
}

func (e HistoryEntry) ToJSON() JSON {
	return JObj(P("Key", JStr(e.Key)), P("Tool", e.Tool.ToJSON()), P("Title", JStr(e.Title)), P("Folder", JStr(e.Folder)),
		P("Updated", e.Updated.ToJSON()), P("State", e.State.ToJSON()), P("Turns", JInt(int64(e.Turns))), P("Credits", JOptDouble(e.Credits)))
}

func HistoryEntryFromJSON(v JSON) (HistoryEntry, error) {
	r := readObj(v)
	e := HistoryEntry{Key: r.text("Key"), Tool: r.toolOr("Tool", Kiro), Title: r.text("Title"), Folder: r.text("Folder"),
		Updated: r.stampOr("Updated", Stamp{}), State: Idle, Turns: r.i32Or("Turns", 0), Credits: r.optF64("Credits")}
	if x, ok := r.get("State"); ok && r.err == nil {
		s, err := StateFromJSON(x)
		r.fail(err)
		e.State = s
	}
	return e, r.err
}

type AgentHistory struct {
	dir    string
	crypto *Crypto

	// mu is the index's lock; gone is checked and changed under it too.
	mu     sync.Mutex
	index  []HistoryEntry
	loaded bool
	// gone is the keys deleted this run. A turn that ends as its session is deleted
	// saves it after the delete, which put it back in the history; keys are never used
	// again.
	gone map[string]bool

	// ponytail: a buffered queue of 256 writes, not Rust's unbounded channel. A burst
	// larger than that makes the saver wait for the disk; it never drops a write.
	jobs    chan func()
	pendMu  sync.Mutex
	pending int
	idle    chan struct{} // closed while nothing is pending

	changedMu sync.Mutex
	changed   []func()
}

func NewAgentHistory(dir string, c *Crypto) *AgentHistory {
	h := &AgentHistory{dir: dir, crypto: c, gone: map[string]bool{}, jobs: make(chan func(), 256), idle: make(chan struct{})}
	close(h.idle)
	go func() {
		for job := range h.jobs {
			job()
			h.pendMu.Lock()
			if h.pending--; h.pending == 0 {
				close(h.idle)
			}
			h.pendMu.Unlock()
		}
	}()
	return h
}

func (h *AgentHistory) indexFile() string        { return filepath.Join(h.dir, "index.dat") }
func (h *AgentHistory) fileOf(key string) string { return filepath.Join(h.dir, key+".dat") }

// OnChanged is raised when an entry is added, changes or goes. From any goroutine.
func (h *AgentHistory) OnChanged(f func()) {
	h.changedMu.Lock()
	h.changed = append(h.changed, f)
	h.changedMu.Unlock()
}

func (h *AgentHistory) raise() {
	h.changedMu.Lock()
	fs := append([]func(){}, h.changed...)
	h.changedMu.Unlock()
	for _, f := range fs {
		f()
	}
}

func indexJSON(list []HistoryEntry) string {
	a := make([]JSON, len(list))
	for i, e := range list {
		a[i] = e.ToJSON()
	}
	return JArr(a...).Compact()
}

// withIndex runs f on the index, read once, under the index's lock.
func (h *AgentHistory) withIndex(f func(list *[]HistoryEntry)) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.loaded {
		h.index = h.loadIndex()
		h.loaded = true
	}
	f(&h.index)
}

func (h *AgentHistory) loadIndex() []HistoryEntry {
	list := []HistoryEntry{}
	whole := true
	if _, err := os.Stat(h.indexFile()); err == nil {
		l, old, err := h.readIndex()
		if err != nil {
			// The index is only a summary of the session files: read them instead. The
			// unreadable one is set aside, not written over.
			Logf("agent history: index unreadable - %v; rebuilding it from the session files", err)
			os.Rename(h.indexFile(), filepath.Join(h.dir, "index-"+GUIDN()+".bad"))
			whole = false
		} else {
			list = l
			// Lines written before the index kept credits get them once, from their Kiro
			// sessions' files (no other tool reports credits); then it is written back.
			// ponytail: reads each old Kiro session's file once, on the first read of the
			// index; a very long history pays that one time.
			if len(old) > 0 {
				for i := range list {
					if list[i].Tool == Kiro && old[list[i].Key] {
						if s, ok := h.read(list[i].Key); ok {
							list[i].Credits = creditsOf(s)
						}
					}
				}
				whole = false
			}
		}
	}
	// A session whose file was written but not yet its line in the index (Hover ended
	// between the two) is found again from its file: one newer than the index, or any
	// when there was no index to go by.
	var since *time.Time
	if whole {
		if st, err := os.Stat(h.indexFile()); err == nil {
			t := st.ModTime()
			since = &t
		}
	}
	if found := h.missingFrom(list, since); len(found) > 0 {
		Logf("agent history: %d session(s) found that the index didn't list", len(found))
		list = append(list, found...)
		whole = false
	}
	if !whole {
		index, idx := indexJSON(list), h.indexFile()
		h.write(func() error { return seal(h.crypto, idx, index) })
	}
	return list
}

// readIndex is the index's entries, and the keys of its lines without Credits.
func (h *AgentHistory) readIndex() ([]HistoryEntry, map[string]bool, error) {
	b, err := os.ReadFile(h.indexFile())
	if err != nil {
		return nil, nil, err
	}
	v, err := ParseJSON(h.crypto.Open(b))
	if err != nil {
		return nil, nil, err
	}
	old := map[string]bool{}
	if items, err := v.Items(); err == nil {
		for _, x := range items {
			if _, has := x.Get("Credits"); !has {
				if k, ok := x.Get("Key"); ok {
					if s, ok := k.AsStr(); ok {
						old[s] = true
					}
				}
			}
		}
	}
	l, _, err := OptList(v, HistoryEntryFromJSON)
	if err != nil {
		return nil, nil, err
	}
	if l == nil {
		l = []HistoryEntry{}
	}
	return l, old, nil
}

// missingFrom is the entries of the session files that list doesn't have, of those
// changed after since (files that can't be opened with this key are left alone).
func (h *AgentHistory) missingFrom(list []HistoryEntry, since *time.Time) []HistoryEntry {
	entries, err := os.ReadDir(h.dir)
	if err != nil {
		return nil
	}
	var out []HistoryEntry
	for _, e := range entries {
		key, ok := strings.CutSuffix(e.Name(), ".dat")
		if !ok || key == "index" || !PlainKey(key) || hasKey(list, key) {
			continue
		}
		if since != nil {
			if info, err := e.Info(); err == nil && !info.ModTime().After(*since) {
				continue
			}
		}
		b, err := os.ReadFile(filepath.Join(h.dir, e.Name()))
		if err != nil {
			continue
		}
		v, err := ParseJSON(h.crypto.Open(b))
		if err != nil {
			continue
		}
		if s, err := SavedSessionFromJSON(v); err == nil && s.Key == key {
			out = append(out, entryOf(s))
		}
	}
	return out
}

func hasKey(list []HistoryEntry, key string) bool {
	for _, x := range list {
		if x.Key == key {
			return true
		}
	}
	return false
}

func without(list []HistoryEntry, key string) []HistoryEntry {
	out := list[:0]
	for _, x := range list {
		if x.Key != key {
			out = append(out, x)
		}
	}
	return out
}

// Entries are newest first (a stable sort, as OrderByDescending is).
func (h *AgentHistory) Entries() []HistoryEntry {
	var l []HistoryEntry
	h.withIndex(func(list *[]HistoryEntry) { l = append([]HistoryEntry{}, *list...) })
	sort.SliceStable(l, func(i, j int) bool { return l[i].Updated.Compare(l[j].Updated) > 0 })
	return l
}

// Save saves a session: its file, and its line in the index. The entry's state is the
// last turn's that has one, else Running.
func (h *AgentHistory) Save(s SavedSession) {
	entry, body, file := entryOf(s), s.ToJSON().Compact(), h.fileOf(s.Key)
	saved := false
	h.withIndex(func(list *[]HistoryEntry) {
		// Checked under the index's lock, which Delete takes too.
		if h.gone[s.Key] {
			return
		}
		*list = append(without(*list, s.Key), entry)
		index, idx := indexJSON(*list), h.indexFile()
		h.write(func() error {
			if err := seal(h.crypto, file, body); err != nil {
				return err
			}
			return seal(h.crypto, idx, index)
		})
		saved = true
	})
	if saved {
		h.raise()
	}
}

// Load is a session's whole record, or none when it is gone or can't be read.
func (h *AgentHistory) Load(key string) (SavedSession, bool) {
	if !PlainKey(key) {
		return SavedSession{}, false
	}
	h.Flush()
	return h.read(key)
}

// read is a session's file as it is on disk now (no wait for writes under way).
func (h *AgentHistory) read(key string) (SavedSession, bool) {
	if !PlainKey(key) {
		return SavedSession{}, false
	}
	b, err := os.ReadFile(h.fileOf(key))
	if errors.Is(err, os.ErrNotExist) {
		return SavedSession{}, false
	}
	if err == nil {
		var v JSON
		if v, err = ParseJSON(h.crypto.Open(b)); err == nil {
			if v.IsNull() {
				return SavedSession{}, false
			}
			var s SavedSession
			if s, err = SavedSessionFromJSON(v); err == nil {
				return s, true
			}
		}
	}
	Logf("agent history: %s unreadable - %v", key, err)
	return SavedSession{}, false
}

func (h *AgentHistory) Delete(key string) {
	if !PlainKey(key) {
		return
	}
	file := h.fileOf(key)
	went := false
	h.withIndex(func(list *[]HistoryEntry) {
		h.gone[key] = true
		before := len(*list)
		*list = without(*list, key)
		if _, err := os.Stat(file); before == len(*list) && err != nil {
			return
		}
		index, idx := indexJSON(*list), h.indexFile()
		h.write(func() error {
			if err := os.Remove(file); err != nil && !errors.Is(err, os.ErrNotExist) {
				return err
			}
			return seal(h.crypto, idx, index)
		})
		went = true
	})
	if went {
		h.raise()
	}
}

// Flush waits (up to 10 s) for the writes under way; Hover calls it on the way out.
func (h *AgentHistory) Flush() {
	h.pendMu.Lock()
	idle := h.idle
	h.pendMu.Unlock()
	select {
	case <-idle:
	case <-time.After(10 * time.Second):
	}
}

func (h *AgentHistory) write(a func() error) {
	h.pendMu.Lock()
	if h.pending++; h.pending == 1 {
		h.idle = make(chan struct{})
	}
	h.pendMu.Unlock()
	dir := h.dir
	h.jobs <- func() {
		err := os.MkdirAll(dir, 0o755)
		if err == nil {
			err = a()
		}
		if err != nil {
			Logf("agent history: save failed - %v", err)
		}
	}
}

// entryOf is a session's line in the index. Its state is the last turn's that has one,
// else Running.
func entryOf(s SavedSession) HistoryEntry {
	state := Running
	for i := len(s.Turns) - 1; i >= 0; i-- {
		if s.Turns[i].State != nil {
			state = *s.Turns[i].State
			break
		}
	}
	return HistoryEntry{Key: s.Key, Tool: s.Tool, Title: s.Title, Folder: s.Folder, Updated: s.Updated, State: state,
		Turns: int32(len(s.Turns)), Credits: creditsOf(s)}
}

// creditsOf is the session's credits: its turns' added up, or nil when none of them says.
func creditsOf(s SavedSession) *float64 {
	var sum *float64
	for _, t := range s.Turns {
		if t.Credits != nil {
			if sum == nil {
				sum = ptr(*t.Credits)
			} else {
				*sum += *t.Credits
			}
		}
	}
	return sum
}

func seal(c *Crypto, file, json string) error {
	tmp := file + ".tmp"
	if err := os.WriteFile(tmp, c.Seal(json), 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, file)
}

// PlainKey: keys are Hover's own GUIDs; anything else never becomes a path.
func PlainKey(key string) bool {
	if n := utf8.RuneCountInString(key); n < 1 || n > 64 {
		return false
	}
	for i := 0; i < len(key); i++ {
		c := key[i]
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z') {
			return false
		}
	}
	return true
}

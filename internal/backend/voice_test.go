package backend

// Rows 41 to 43 of docs/development/windows-csharp.md B2: the voice flow with a microphone
// the host owns, its `voice` event, Phonon and the Groq key check. The speech engine is a
// fake Groq on this computer's loopback (HOVER_GROQ_BASE), which sees the recording the host
// sent.

import (
	"encoding/base64"
	"encoding/binary"
	"io"
	"math"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/4regab/Hover/internal/core"
)

// tone is a second of a 440 Hz tone, as the host would send it: 16-bit little-endian PCM.
func tone(seconds float64) []byte {
	n := int(16000 * seconds)
	b := make([]byte, 2*n)
	for i := 0; i < n; i++ {
		v := int16(8000 * math.Sin(2*math.Pi*440*float64(i)/16000))
		binary.LittleEndian.PutUint16(b[2*i:], uint16(v))
	}
	return b
}

// fakeGroq answers every transcription with the text, and keeps what it was sent.
type fakeGroq struct {
	*httptest.Server
	mu       sync.Mutex
	auth     string
	wavBytes int
	riff     bool
}

func newFakeGroq(t *testing.T, text string) *fakeGroq {
	g := &fakeGroq{}
	g.Server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.HasSuffix(r.URL.Path, "/audio/transcriptions") {
			http.NotFound(w, r)
			return
		}
		if err := r.ParseMultipartForm(32 << 20); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		f, _, err := r.FormFile("file")
		if err != nil {
			http.Error(w, "no file", 400)
			return
		}
		raw, _ := io.ReadAll(f)
		g.mu.Lock()
		g.auth, g.wavBytes, g.riff = r.Header.Get("Authorization"), len(raw), len(raw) > 12 && string(raw[:4]) == "RIFF" && string(raw[8:12]) == "WAVE"
		g.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"text":"`+text+`","language":"en","duration":1.0}`)
	}))
	t.Cleanup(g.Close)
	return g
}

func voiceStage(stage string) func(msg) bool {
	return func(m msg) bool { return m["type"] == "voice" && m["stage"] == stage }
}

func TestVoiceTakesTheHostsAudioAndStartsATask(t *testing.T) {
	g := newFakeGroq(t, "fix the build")
	ws := t.TempDir()
	dir := t.TempDir()
	noticed(t, dir, func(s *core.Settings) {
		w := core.DefaultWorkspaceSetting()
		w.Folder = &ws
		s.SetDefaultWorkspace(w)
		v := s.Voice()
		v.Countdown = 0
		s.SetVoice(v)
	})
	h := startHost(t, dir, "HOVER_FAKE_RUN=echo", "HOVER_GROQ_BASE="+g.URL)
	h.start()
	// The key goes in through the same store voice reads, and never comes back.
	if k := h.ask(msg{"type": "secret", "op": "set", "name": core.GroqSecret, "value": "gsk_test_key"}, "secret"); k["has"] != true {
		t.Fatalf("secret: %v", k)
	}
	// Press, and the host sends what it records: a second of tone, in two blocks.
	h.send(msg{"type": "voicePress"})
	rec := h.until("recording", voiceStage("recording"))
	if rec["preview"] != nil || rec["canStart"] != false {
		t.Fatalf("recording: %v", rec)
	}
	pcm := tone(1)
	half := len(pcm) / 2
	h.send(msg{"type": "voiceAudio", "pcm": base64.StdEncoding.EncodeToString(pcm[:half])})
	h.send(msg{"type": "voiceAudio", "pcm": base64.StdEncoding.EncodeToString(pcm[half:])})
	loud := h.until("a level", func(m msg) bool { return m["type"] == "voice" && m["stage"] == "recording" && num(m, "secs") >= 0.9 })
	if num(loud, "level") <= 0 {
		t.Fatalf("no level from the audio: %v", loud["level"])
	}
	h.send(msg{"type": "voiceRelease"})
	ed := h.until("the card", voiceStage("editing"))
	p := obj(ed, "preview")
	if p["task"] != "fix the build" || p["heard"] != "fix the build" || !core.SameFolder(str(p, "folder"), ws) || p["tool"] != "kiro" || p["trial"] != false {
		t.Fatalf("the preview: %v", p)
	}
	g.mu.Lock()
	auth, size, riff := g.auth, g.wavBytes, g.riff
	g.mu.Unlock()
	if auth != "Bearer gsk_test_key" || !riff || size != 44+len(pcm) {
		t.Fatalf("what Groq was sent: auth %q riff %v bytes %d (want %d)", auth, riff, size, 44+len(pcm))
	}
	if ed["canStart"] != true {
		t.Fatalf("canStart: %v", ed["canStart"])
	}
	// Edit the task, then start it.
	h.send(msg{"type": "voiceEdit", "task": "fix the build and run the tests"})
	h.until("the edit", func(m msg) bool {
		return m["type"] == "voice" && m["stage"] == "editing" && obj(m, "preview")["task"] == "fix the build and run the tests" && m["canStart"] == true
	})
	h.send(msg{"type": "voiceStartNow"})
	started := h.until("started", voiceStage("started"))
	if num(started, "session") <= 0 || started["folder"] == "" {
		t.Fatalf("started: %v", started)
	}
	st := h.state()
	if s := sessionByID(st, num(started, "session")); s == nil || arr(s, "turns")[0].(map[string]any)["prompt"] != "fix the build and run the tests" {
		t.Fatalf("the task at a desk: %v", s)
	}
	h.send(msg{"type": "voiceDismiss"})
	h.until("idle", voiceStage("idle"))
	// The key is in no message the backend sent.
	for _, line := range h.allLines() {
		if strings.Contains(line, "gsk_test_key") {
			t.Fatalf("the key was sent to the host: %s", line)
		}
	}
}

func TestVoiceDictationCancelAndAFailedMicrophone(t *testing.T) {
	g := newFakeGroq(t, "words for the reply box")
	dir := t.TempDir()
	noticed(t, dir)
	h := startHost(t, dir, "HOVER_FAKE_RUN=echo", "HOVER_GROQ_BASE="+g.URL)
	h.start()
	h.ask(msg{"type": "secret", "op": "set", "name": core.GroqSecret, "value": "k"}, "secret")
	// Dictation: the words come back for the reply box, and nothing is routed or started.
	h.send(msg{"type": "voicePress", "dictate": true})
	h.until("recording", voiceStage("recording"))
	h.send(msg{"type": "voiceAudio", "pcm": base64.StdEncoding.EncodeToString(tone(0.5))})
	h.send(msg{"type": "voiceRelease"})
	d := h.until("dictated", voiceStage("dictated"))
	if d["text"] != "words for the reply box" {
		t.Fatalf("dictated: %v", d)
	}
	h.send(msg{"type": "voiceDismiss"})
	h.until("idle", voiceStage("idle"))
	// Cancel drops the interaction.
	h.send(msg{"type": "voicePress"})
	h.until("recording", voiceStage("recording"))
	h.send(msg{"type": "voiceCancel"})
	h.until("cancelled", voiceStage("cancelled"))
	h.send(msg{"type": "voiceDismiss"})
	h.until("idle", voiceStage("idle"))
	// A microphone that fails ends the recording with its words.
	h.send(msg{"type": "voicePress"})
	h.until("recording", voiceStage("recording"))
	h.send(msg{"type": "voiceAudio", "error": "The microphone was unplugged."})
	e := h.until("error", voiceStage("error"))
	if e["message"] != "The microphone was unplugged." || e["retry"] != true {
		t.Fatalf("error: %v", e)
	}
	// Silence is nothing heard.
	h.send(msg{"type": "voiceRetry"})
	h.until("idle", voiceStage("idle"))
	h.send(msg{"type": "voicePress"})
	h.until("recording", voiceStage("recording"))
	h.send(msg{"type": "voiceAudio", "pcm": base64.StdEncoding.EncodeToString(make([]byte, 16000))})
	h.send(msg{"type": "voiceRelease"})
	if e := h.until("nothing heard", voiceStage("error")); !strings.Contains(str(e, "message"), "Nothing was heard") {
		t.Fatalf("silence: %v", e["message"])
	}
}

func TestVoiceRefusesBadInput(t *testing.T) {
	h := taskHost(t, "echo")
	huge := strings.Repeat("A", base64.StdEncoding.EncodedLen(audioBlockMax)+4)
	for name, c := range map[string]msg{
		"audio: pcm a number":      {"type": "voiceAudio", "pcm": 5},
		"audio: pcm missing":       {"type": "voiceAudio"},
		"audio: not base64":        {"type": "voiceAudio", "pcm": "!!!"},
		"audio: an odd byte count": {"type": "voiceAudio", "pcm": base64.StdEncoding.EncodeToString([]byte{1, 2, 3})},
		"audio: a block too big":   {"type": "voiceAudio", "pcm": huge},
		"audio: error a number":    {"type": "voiceAudio", "error": 5},
		"audio: error empty":       {"type": "voiceAudio", "error": ""},
		"audio: error huge":        {"type": "voiceAudio", "error": strings.Repeat("e", 301)},
		"press: project huge":      {"type": "voicePress", "activeProject": strings.Repeat("p", 65)},
		"edit: task a number":      {"type": "voiceEdit", "task": 5},
		"edit: task huge":          {"type": "voiceEdit", "task": strings.Repeat("t", voiceTaskMax+1)},
		"edit: no task":            {"type": "voiceEdit"},
		"chooseAgent: unknown":     {"type": "voiceChooseAgent", "tool": "hal"},
		"changeAgent: a number":    {"type": "voiceChangeAgent", "tool": 5},
		"changeAgent: none":        {"type": "voiceChangeAgent"},
		"folder: a number":         {"type": "voiceChangeFolder", "project": 5},
		"folder: huge":             {"type": "voiceChangeFolder", "project": strings.Repeat("p", 65)},
		"folder: empty":            {"type": "voiceChangeFolder", "project": ""},
		"repo: unknown kind":       {"type": "voiceChangeRepo", "kind": "moon"},
		"repo: named, no name":     {"type": "voiceChangeRepo", "kind": "named"},
		"repo: named, a path":      {"type": "voiceChangeRepo", "kind": "named", "name": "../../x"},
		"unattach: negative":       {"type": "voiceUnattach", "index": -1},
		"unattach: a string":       {"type": "voiceUnattach", "index": "0"},
		"unattach: huge":           {"type": "voiceUnattach", "index": 100000},
	} {
		if why := h.refused(c, ""); why == "" {
			t.Fatal(name)
		}
	}
	// Audio with no recording is dropped without a word, and the commands that need a card
	// do nothing when there is none.
	for _, c := range []msg{
		{"type": "voiceAudio", "pcm": base64.StdEncoding.EncodeToString(tone(0.1))},
		{"type": "voiceRelease"}, {"type": "voiceStartNow"}, {"type": "voiceRetry"}, {"type": "voiceToggleCloud"}, {"type": "voiceHold"},
		{"type": "voiceChangeFolder"}, {"type": "voiceChangeRepo", "kind": "empty"}, {"type": "voiceUnattach", "index": 0},
		{"type": "voiceChangeAgent", "tool": "codex"}, {"type": "voiceChooseAgent", "tool": "codex"}, {"type": "voiceEdit", "task": "x"},
	} {
		h.send(c)
	}
	if v := h.ask(msg{"type": "voice"}, "voice"); v["stage"] != "idle" {
		t.Fatalf("voice: %v", v["stage"])
	}
}

func TestPhononAndTheGroqCheck(t *testing.T) {
	h := taskHost(t, "echo")
	p := h.ask(msg{"type": "phonon"}, "phonon")
	state := obj(p, "state")
	if p["ok"] != true || (state["kind"] != "notInstalled" && state["kind"] != "unsupported") || p["ready"] != false ||
		!strings.HasPrefix(str(obj(p, "facts"), "folder"), h.dir) || num(obj(p, "facts"), "downloadBytes") <= 0 {
		t.Fatalf("phonon: %v", p)
	}
	// Remove of what is not installed is fine; cancel has nothing to stop.
	if r := h.ask(msg{"type": "phonon", "op": "remove"}, "phonon"); r["ok"] != true {
		t.Fatalf("remove: %v", r)
	}
	if r := h.ask(msg{"type": "phonon", "op": "cancel"}, "phonon"); r["ok"] != true {
		t.Fatalf("cancel: %v", r)
	}
	for name, c := range map[string]msg{
		"unknown op":  {"type": "phonon", "op": "format"},
		"op a number": {"type": "phonon", "op": 4},
	} {
		if why := h.refused(c, ""); why == "" {
			t.Fatal(name)
		}
	}
	// With no key there is nothing to check, and the answer says so.
	g := h.ask(msg{"type": "groqCheck"}, "groqCheck")
	if g["ok"] != false || g["said"] != "Add your Groq key first." {
		t.Fatalf("groqCheck: %v", g)
	}
}

// The voice settings go through setSetting, bounded; the local model stays Phonon's.
func TestVoiceSettingsThroughSetSetting(t *testing.T) {
	h := taskHost(t, "echo")
	voiceModel := func() msg { return obj(obj(h.ask(msg{"type": "settings"}, "settings"), "model"), "Voice") }
	good := msg{"Enabled": true, "Shortcut": msg{"Key": "Space", "Modifiers": "Control, Alt"}, "Speech": "Cloud", "Model": "whisper-large-v3",
		"Cleanup": true, "CleanupProvider": "Custom", "CleanupModel": "m", "CleanupBase": "https://example.com/v1/", "Countdown": 5, "AuraColor": "#abc", "HoldToTalk": true}
	m := obj(h.ask(msg{"type": "setSetting", "key": "Voice", "value": good}, "settings"), "model")
	v := obj(m, "Voice")
	if v["Enabled"] != true || v["Model"] != "whisper-large-v3" || v["CleanupBase"] != "https://example.com/v1" || v["AuraColor"] != "#AABBCC" || v["HoldToTalk"] != true {
		t.Fatalf("voice settings: %v", v)
	}
	before := voiceModel()
	for name, val := range map[string]any{
		"a string":      "on",
		"a list":        []int{1},
		"enabled a str": msg{"Enabled": "yes"},
		"base not http": msg{"CleanupBase": "ftp://x/v1"},
		"base spaces":   msg{"CleanupBase": "https://exa mple.com"},
		"base huge":     msg{"CleanupBase": "https://" + strings.Repeat("a", 500)},
		"mic huge":      msg{"Microphone": strings.Repeat("m", 101)},
		"model huge":    msg{"CleanupModel": strings.Repeat("m", 201)},
		"shortcut key":  msg{"Shortcut": msg{"Key": "Nope"}},
	} {
		if why := h.refused(msg{"type": "setSetting", "key": "Voice", "value": val}, ""); why == "" {
			t.Fatal(name)
		}
	}
	if after := voiceModel(); !equalJSON(before, after) {
		t.Fatalf("a refused voice setting changed the settings:\n%v\n%v", before, after)
	}
	// ToolAgent
	if m := obj(h.ask(msg{"type": "setSetting", "key": "ToolAgent", "value": msg{"tool": "kiro", "agent": "reviewer"}}, "settings"), "model"); m["KiroAgent"] != "reviewer" {
		t.Fatalf("agent: %v", m["KiroAgent"])
	}
	if m := obj(h.ask(msg{"type": "setSetting", "key": "ToolAgent", "value": msg{"tool": "kiro", "agent": nil}}, "settings"), "model"); m["KiroAgent"] != nil {
		t.Fatalf("agent: %v", m["KiroAgent"])
	}
	for name, val := range map[string]any{
		"unknown tool": msg{"tool": "hal", "agent": "x"},
		"agent number": msg{"tool": "kiro", "agent": 5},
		"agent huge":   msg{"tool": "kiro", "agent": strings.Repeat("a", 101)},
		"a string":     "kiro",
	} {
		if why := h.refused(msg{"type": "setSetting", "key": "ToolAgent", "value": val}, ""); why == "" {
			t.Fatal(name)
		}
	}
}

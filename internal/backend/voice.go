package backend

import (
	"encoding/base64"
	"errors"
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/voice"
)

// Voice for the Windows host (docs/development/windows-csharp.md, B2 rows 41 to 43). The
// transcribing, cleaning up, routing and starting stay in internal/voice; the host owns the
// microphone and the shortcut. The host sends the audio it records as 16 kHz mono 16-bit
// blocks (`voiceAudio`), and the backend answers with a `voice` event whenever the flow
// moves. Nothing here is made until the host first uses voice, so the Mac host is untouched.

const (
	// audioBlockMax is the most one `voiceAudio` block holds: 128 KiB, four seconds.
	audioBlockMax = 128 << 10
	voiceTaskMax  = 20_000 // an edited task, UTF-16 units
)

// voiceKit is what voice needs of the backend, made with the first voice command.
type voiceKit struct {
	once   sync.Once
	v      *voice.Voice
	phonon *voice.Phonon

	mu     sync.Mutex
	mic    *voice.Remote
	active *string

	pending       atomic.Bool
	phononPending atomic.Bool
}

// kit is voice, made on first use: Phonon reads the disk, Voice has the app's hooks.
func (b *Backend) kit() *voiceKit {
	k := &b.voiceKit
	k.once.Do(func() {
		k.phonon = voice.NewPhonon(b.settings)
		hooks := voice.Hooks{
			Router: func(t core.AgentTool) agents.RunTask {
				if testRunner != nil {
					return testRunner(t)
				}
				for _, r := range b.runtimes {
					if r.Tool() == t {
						return r.Runner()
					}
				}
				return nil
			},
			Start: b.voiceStart,
			// Installed and signed in, by the tool's own status command (kept five minutes).
			Available: func(t core.AgentTool) bool { return agents.Check(t, false).OK() },
			ActiveProject: func() *string {
				k.mu.Lock()
				defer k.mu.Unlock()
				return k.active
			},
		}
		open := func(string, int) (voice.Source, error) {
			k.mu.Lock()
			defer k.mu.Unlock()
			if k.mic == nil {
				return nil, errors.New("The microphone isn’t sending audio.")
			}
			return k.mic, nil
		}
		k.v = voice.NewWith(b.settings, b.secretStore(), k.phonon, hooks, open)
		k.v.OnChange(func() {
			if k.pending.CompareAndSwap(false, true) {
				b.link.with(func(b *Backend) { k.pending.Store(false); b.sendVoice() })
			}
		})
		k.phonon.OnChange(func() {
			if k.phononPending.CompareAndSwap(false, true) {
				b.link.with(func(b *Backend) { k.phononPending.Store(false); b.sendPhonon(nil) })
			}
		})
	})
	return k
}

// shutdownVoice ends a recording or transcription and Phonon's helper (Hover is quitting).
func (b *Backend) shutdownVoice() {
	k := &b.voiceKit
	if k.v != nil {
		k.v.Cancel()
		k.phonon.Shutdown()
	}
}

// voiceStart is a new chat for a voice task, through the same start the new-task box uses
// (internal/shell/voice.go's voiceStart). Ok only once the tool took it. Called on Voice's
// worker goroutine, so it may wait.
func (b *Backend) voiceStart(tool core.AgentTool, folder, prompt, access string, cloud *voice.Repo, shots []string) (int32, error) {
	var cl []string
	if cloud != nil {
		access = "full"
		cl = []string{}
		switch cloud.Kind {
		case voice.RepoFolder:
			if r := agents.SharedDesk().GithubRepo(folder); r != nil {
				cl = append(cl, *r)
			}
		case voice.RepoNamed:
			cl = append(cl, cloud.Name)
		}
	}
	// with_access("read") on a tool with no read only mode would run it with its own
	// setting: more than the target allows.
	if access == "read" && !agents.ReadOnlyWorks(tool) {
		return 0, fmt.Errorf("%s has no read only mode on this computer, so Hover won’t start it here. Change the target’s access in Settings → Projects, or pick another agent.", tool.Name())
	}
	acc := access
	var cloudArg []string
	if cloud != nil {
		cloudArg = cl
	}
	sess, ok := b.sessions.StartIn(tool, folder, prompt, shots, &acc, cloudArg)
	if !ok {
		if b.sessions.CanStart() {
			return 0, errors.New("Hover couldn’t start the task.")
		}
		return 0, errors.New("Three tasks are running already. Start this one when one of them ends.")
	}
	core.Logf("voice: run %d started (%s, access %s)", sess.ID, strings.ToLower(tool.Name()), access)
	t0 := time.Now()
	for {
		x, ok := b.sessions.Get(sess.ID)
		switch {
		case !ok:
			return 0, errors.New("The task’s chat was closed before the agent took it.")
		case x.KiroID != nil:
			return sess.ID, nil
		case !x.Busy():
			if r := x.Result(); r != nil && r.State == core.Completed {
				return sess.ID, nil
			} else if r != nil {
				for _, l := range strings.Split(agents.Plain(r.Text), "\n") {
					if l = strings.TrimSpace(l); l != "" {
						return 0, errors.New(l)
					}
				}
			}
			return 0, fmt.Errorf("%s couldn’t start the task.", tool.Name())
		}
		// ponytail: a tool that neither answers nor fails for ten minutes is treated as
		// having taken it; its chat in the office says how it is going.
		if time.Since(t0) > 600*time.Second || b.link.closed() {
			return sess.ID, nil
		}
		time.Sleep(50 * time.Millisecond)
	}
}

// MARK: The event

var stageNames = [...]string{"idle", "recording", "loading", "transcribing", "cleaning", "resolving", "chooseAgent",
	"preview", "editing", "starting", "started", "dictated", "cancelled", "error"}

func previewJSON(p *voice.Preview) core.JSON {
	if p == nil {
		return core.JNull
	}
	repoKind := [...]string{"folder", "empty", "named"}[p.Repo.Kind]
	return core.JObj(
		core.P("id", jint(int64(p.ID))), core.P("heard", jst(p.Heard)), core.P("cleanupNote", jst(p.CleanupNote)), core.P("task", jst(p.Task)),
		core.P("folder", jst(p.Folder)), core.P("targetName", jst(p.TargetName)), core.P("note", jst(p.Note)),
		core.P("tool", jst(p.Tool.ID())), core.P("model", jst(p.Model)), core.P("access", jst(p.Access)),
		core.P("countdown", core.JDouble(float64(p.Countdown))), core.P("counting", jbool(p.Counting)), core.P("trial", jbool(p.Trial)),
		core.P("cloud", jbool(p.Cloud)), core.P("repo", core.JObj(core.P("kind", jst(repoKind)), core.P("name", jst(p.Repo.Name)))),
	)
}

// voiceEvent is the `voice` event: the stage and what goes with it, and what the card
// lists. Nothing in it blocks.
func (b *Backend) voiceEvent() core.JSON {
	v := b.voiceKit.v
	st := v.Stage()
	var pending core.JSON = core.JNull
	if st.Pending != nil {
		tools := make([]core.JSON, len(st.Pending.Tools))
		for i, t := range st.Pending.Tools {
			tools[i] = jst(t.ID())
		}
		pending = core.JObj(core.P("id", jint(int64(st.Pending.ID))), core.P("text", jst(st.Pending.Text)), core.P("tools", core.JArr(tools...)))
	}
	choices := v.FolderChoices()
	folders := make([]core.JSON, len(choices))
	for i, p := range choices {
		folders[i] = core.JObj(core.P("id", jst(p.ID)), core.P("name", jst(p.Name)), core.P("folder", jst(p.Folder)))
	}
	// Ready by the last check of each tool (kept five minutes); `refresh` makes a new one.
	var ready []core.JSON
	for _, t := range core.AllTools {
		if r, known := agents.Known(t); known && r.OK() {
			ready = append(ready, jst(t.ID()))
		}
	}
	shots := v.Shots()
	pics := make([]core.JSON, len(shots))
	for i, s := range shots {
		pics[i] = jst(s)
	}
	shotN, shotMsg := v.Shot()
	tool := core.JNull
	if t, ok := v.Tool(); ok {
		tool = jst(t.ID())
	}
	return core.JObj(
		core.P("type", jst("voice")), core.P("stage", jst(stageNames[st.Kind])),
		core.P("level", core.JDouble(float64(st.Level))), core.P("secs", core.JDouble(float64(st.Secs))),
		core.P("pending", pending), core.P("preview", previewJSON(st.Preview)),
		core.P("session", jint(int64(st.Session))), core.P("folder", jst(st.Folder)), core.P("text", jst(st.Text)),
		core.P("message", jst(st.Message)), core.P("retry", jbool(st.Retry)), core.P("transcript", jopt(st.Transcript)),
		core.P("countdownTotal", core.JDouble(v.CountdownTotal().Seconds())), core.P("canStart", jbool(v.CanStart())),
		core.P("tool", tool), core.P("folderChoices", core.JArr(folders...)), core.P("readyTools", core.JArr(ready...)),
		core.P("shots", core.JArr(pics...)), core.P("shotCount", jint(int64(shotN))), core.P("shotMessage", jst(shotMsg)),
	)
}

func (b *Backend) sendVoice() {
	if b.voiceKit.v != nil && !b.link.closed() {
		b.link.out.Send(b.voiceEvent())
	}
}

// MARK: Commands

// voiceCommand is every command that starts with "voice" (and `voice` itself, which asks
// for the event). The audio and the commands are untrusted: types, sizes and ids are checked.
func (b *Backend) voiceCommand(typ string, m core.JSON) error {
	k := b.kit()
	v := k.v
	switch typ {
	case "voice":
		b.sendVoice()
	case "voicePress":
		trial, _ := boolOf(m, "trial")
		dictate, _ := boolOf(m, "dictate")
		active := optStrOf(m, "activeProject")
		if active != nil && units(*active) > 64 {
			return errors.New("activeProject is a project id.")
		}
		// The microphone for this recording is made before the press, so the first blocks
		// the host sends have somewhere to go.
		k.mu.Lock()
		k.mic, k.active = voice.NewRemote(voice.MaxSamples), active
		k.mu.Unlock()
		if dictate {
			v.Dictate()
		} else {
			v.Press(trial)
		}
	case "voiceAudio":
		return b.voiceAudio(m)
	case "voiceRelease":
		v.Release()
	case "voiceCancel":
		v.Cancel()
	case "voiceDismiss":
		v.Dismiss()
	case "voiceStartNow":
		v.StartNow()
	case "voiceHold":
		v.Hold()
	case "voiceEdit":
		task, ok := strOf(m, "task")
		if !ok || units(task) > voiceTaskMax {
			return errors.New("A task is text, up to 20,000 characters.")
		}
		v.Edit(task)
	case "voiceChooseAgent", "voiceChangeAgent":
		tool, ok := core.ParseTool(optStrOf(m, "tool"))
		if !ok {
			return errors.New("Choose an agent Hover knows.")
		}
		if typ == "voiceChooseAgent" {
			v.ChooseAgent(tool)
		} else {
			v.ChangeAgent(tool)
		}
	case "voiceToggleCloud":
		v.ToggleCloud()
	case "voiceChangeFolder":
		// A voice project's id, or null for the default workspace.
		val, has := m.Get("project")
		if !has || val.IsNull() {
			v.ChangeFolder(nil)
			break
		}
		id, ok := val.AsStr()
		if !ok || id == "" || units(id) > 64 {
			return errors.New("project is a project id, or null for the default workspace.")
		}
		v.ChangeFolder(&id)
	case "voiceChangeRepo":
		repo, err := repoOf(m)
		if err != nil {
			return err
		}
		v.ChangeRepo(repo)
	case "voiceRetry":
		v.Retry()
	case "voiceUnattach":
		i, ok := intOf(m, "index")
		if !ok || i < 0 || i > 1000 {
			return errors.New("index is the number of a screenshot.")
		}
		v.Unattach(int(i))
	}
	return nil
}

// repoOf is `voiceChangeRepo`'s {kind: folder|empty|named, name}.
func repoOf(m core.JSON) (voice.Repo, error) {
	kind, _ := strOf(m, "kind")
	switch kind {
	case "folder":
		return voice.Repo{Kind: voice.RepoFolder}, nil
	case "empty":
		return voice.Repo{Kind: voice.RepoEmpty}, nil
	case "named":
		name, _ := strOf(m, "name")
		if !repoName.MatchString(name) {
			return voice.Repo{}, errors.New("A repository is named owner/name.")
		}
		return voice.Repo{Kind: voice.RepoNamed, Name: name}, nil
	}
	return voice.Repo{}, errors.New("kind is folder, empty or named.")
}

// voiceAudio is `voiceAudio` {pcm} (base64: 16 kHz, mono, 16-bit little-endian samples) or
// {error} (why the microphone failed, as voice.Source.Failed says it). It goes to the
// recording `voicePress` began; with none, it is dropped.
func (b *Backend) voiceAudio(m core.JSON) error {
	k := &b.voiceKit
	k.mu.Lock()
	mic := k.mic
	k.mu.Unlock()
	if v, has := m.Get("error"); has && !v.IsNull() {
		why, ok := v.AsStr()
		if !ok || why == "" || units(why) > 300 {
			return errors.New("error is a short text.")
		}
		if mic != nil {
			mic.Fail(why)
		}
		return nil
	}
	text, ok := strOf(m, "pcm")
	if !ok || len(text) > base64.StdEncoding.EncodedLen(audioBlockMax) {
		return errors.New("pcm is a block of up to 128 KiB, base64.")
	}
	raw, ok := core.FromBase64(text)
	if !ok || len(raw)%2 != 0 || len(raw) > audioBlockMax {
		return errors.New("pcm is 16-bit samples, base64.")
	}
	if mic == nil {
		return nil
	}
	samples := make([]int16, len(raw)/2)
	for i := range samples {
		samples[i] = int16(uint16(raw[2*i]) | uint16(raw[2*i+1])<<8)
	}
	mic.Feed(samples)
	return nil
}

// MARK: Phonon and Groq

// phononEvent is `phonon`: {ok, ready, state{kind, done, total, message}, facts{...}}.
func (b *Backend) phononEvent(err error) core.JSON {
	p := b.kit().phonon
	s, f := p.State(), p.Facts()
	kinds := [...]string{"notInstalled", "unsupported", "downloading", "verifying", "installing", "ready", "cancelled", "failed"}
	props := []core.Prop{
		core.P("type", jst("phonon")), core.P("ok", jbool(err == nil)),
		core.P("ready", jbool(p.Speech() != nil)),
		core.P("state", core.JObj(core.P("kind", jst(kinds[s.Kind])), core.P("done", jint(int64(s.Done))), core.P("total", jint(int64(s.Total))), core.P("message", jst(s.Message)))),
		core.P("facts", core.JObj(core.P("model", jst(f.Model)), core.P("version", jst(f.Version)), core.P("downloadBytes", jint(int64(f.DownloadBytes))),
			core.P("diskBytes", jint(int64(f.DiskBytes))), core.P("peakDiskBytes", jint(int64(f.PeakDiskBytes))), core.P("folder", jst(f.Folder)))),
	}
	if err != nil {
		props = append(props, core.P("error", jst(err.Error())))
	}
	return core.JObj(props...)
}

func (b *Backend) sendPhonon(err error) {
	if !b.link.closed() {
		b.link.out.Send(b.phononEvent(err))
	}
}

// phonon is `phonon` {op}: get, download, repair, cancel or remove, each answered as
// `phonon` (and again as a setup goes on): Phonon is local speech, set up from pinned
// downloads.
func (b *Backend) phonon(m core.JSON) error {
	op := "get"
	if v, has := m.Get("op"); has {
		o, ok := v.AsStr()
		if !ok {
			return errors.New("op is get, download, repair, cancel or remove.")
		}
		op = o
	}
	p := b.kit().phonon
	switch op {
	case "get":
	case "download":
		p.Download()
	case "repair":
		p.Repair()
	case "cancel":
		p.Cancel()
	case "remove":
		b.sendPhonon(p.Remove())
		return nil
	default:
		return errors.New("op is get, download, repair, cancel or remove.")
	}
	b.sendPhonon(nil)
	return nil
}

// groqCheck is `groqCheck`: an authenticated list of Groq's models with the key kept for
// voice. Answered as `groqCheck` {ok, said}; the key is never in it.
func (b *Backend) groqCheck() {
	key, _ := b.secretStore().Get(core.GroqSecret)
	out, closed := b.link.out, b.link.closed
	go func() {
		said, ok := "The key works.", true
		if key == "" {
			said, ok = "Add your Groq key first.", false
		} else if err := voice.CheckGroq(voice.GroqBase, key); err != nil {
			said, ok = err.Error(), false
		}
		if !closed() {
			out.Send(core.JObj(core.P("type", jst("groqCheck")), core.P("ok", jbool(ok)), core.P("said", jst(said))))
		}
	}()
}

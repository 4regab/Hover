package agents

// tests/attach.rs. Attaching to a Kiro Web session that went on working in the cloud: the
// session is loaded again, its replay is read for the turn that was cut off (the part
// after the user's last message), and what comes next is followed to the turn's end.
// Against a stand-in agent over pipes that speaks as Kiro's cloud does.

import (
	"bufio"
	"fmt"
	"os"
	"reflect"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

func cloudUpdate(out *pipeOut, u string) {
	out.say(fmt.Sprintf(`{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"c1","update":%s}}`, u))
}

// cloudReplay is the conversation as a load replays it: a finished first turn, then a
// second that was cut off, which done says did (or did not) finish while the client was
// away.
func cloudReplay(out *pipeOut, done bool) {
	cloudUpdate(out, `{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"first"}}`)
	cloudUpdate(out, `{"sessionUpdate":"tool_call","toolCallId":"old1","kind":"read","title":"Read File","status":"completed","locations":[{"path":"old.rs"}]}`)
	cloudUpdate(out, `{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"First answer."}}`)
	cloudUpdate(out, `{"sessionUpdate":"session_info_update","_meta":{"kiro":{"kind":"turn_completion"}}}`)
	cloudUpdate(out, `{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"second"}}`)
	cloudUpdate(out, `{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":" one"}}`)
	cloudUpdate(out, `{"sessionUpdate":"tool_call","toolCallId":"cut1","kind":"read","title":"Read File","status":"completed","locations":[{"path":"cut.rs"}]}`)
	if done {
		cloudUpdate(out, `{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Second answer, finished while away."}}`)
		cloudUpdate(out, `{"sessionUpdate":"session_info_update","_meta":{"kiro":{"kind":"turn_completion"}}}`)
	}
}

type cloudGot struct {
	mu  sync.Mutex
	all []core.JSON
}

func (g *cloudGot) first(method string) (core.JSON, bool) {
	g.mu.Lock()
	defer g.mu.Unlock()
	i := slices.IndexFunc(g.all, func(m core.JSON) bool { return strAt(m, "method") == method })
	if i < 0 {
		return core.JNull, false
	}
	return g.all[i], true
}

func cloudHost(done, live bool) (*AcpHost, *cloudGot) { return cloudHostWith(done, live, false) }

// cloudHostWith: same, Kiro gives the same sessions whichever source it is asked for.
func cloudHostWith(done, live, same bool) (*AcpHost, *cloudGot) {
	got := &cloudGot{}
	host := AcpHostWithConnect(core.Kiro, core.DefaultAgentOptions, func() (*Link, error) {
		link, _, err := pipeLink(func(from *bufio.Reader, out *pipeOut) {
			eachLine(from, func(line string) {
				m := jsonOf(line)
				got.mu.Lock()
				got.all = append(got.all, m)
				got.mu.Unlock()
				var id *int64
				if v, ok := m.Get("id"); ok {
					if n, err := v.I64(); err == nil {
						id = &n
					}
				}
				method := strAt(m, "method")
				var r *string
				switch method {
				case "initialize":
					r = sp(`{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"list":{}},"_meta":{"kiro":{"executionTargets":["local","cloud-sandbox"]}}}}`)
				case "session/list":
					// For Kiro Web's: two pages, a Kiro Web session and this computer's,
					// then another. For this computer's: only its own. (same: the same for
					// both, with nothing marked.)
					p, ok := m.Get("params")
					if !ok {
						p = core.JNull
					}
					remote := strings.Contains(p.Compact(), `"remote"`)
					_, hasCursor := p.Get("cursor")
					switch {
					case same:
						r = sp(`{"sessions":[{"sessionId":"a1","title":"One"},{"sessionId":"a2","title":"Two"}]}`)
					case !remote:
						r = sp(`{"sessions":[{"sessionId":"l1","cwd":"/home/me","title":"Local one"}]}`)
					case !hasCursor:
						r = sp(`{"sessions":[{"sessionId":"c1","cwd":"/sandbox","title":"Fix the footer","updatedAt":"2026-10-05T10:00:00Z"},{"sessionId":"l1","cwd":"/home/me","title":"Local one"}],"nextCursor":"p2"}`)
					default:
						r = sp(`{"sessions":[{"sessionId":"c2","cwd":"/sandbox","title":"  Add tests  "}]}`)
					}
				case "session/load":
					cloudReplay(out, done)
					r = sp(`{"configOptions":[]}`)
				}
				if id != nil && r != nil {
					out.say(fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"result":%s}`, *id, *r))
				}
				// After the load has answered, the cloud goes on with the turn.
				if live && method == "session/load" {
					go func() {
						time.Sleep(300 * time.Millisecond)
						cloudUpdate(out, `{"sessionUpdate":"tool_call","toolCallId":"live1","kind":"edit","title":"Edit File","status":"completed","locations":[{"path":"new.rs"}]}`)
						cloudUpdate(out, `{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Finished the work."}}`)
						cloudUpdate(out, `{"sessionUpdate":"session_info_update","_meta":{"kiro":{"kind":"turn_completion"}}}`)
					}()
				}
			})
		})
		return link, err
	})
	return host, got
}

func attachTo(t *testing.T, host *AcpHost) (KiroResult, []KiroEvent) {
	dir := newDir(t, "attach")
	rec := &recorders{}
	r := host.Runner()(RunArgs{Folder: dir, Prompt: AttachPrompt, Progress: func(KiroPhase) {}, Ct: NewCancel(), Resume: sp("c1"), Events: rec.e, Cloud: []string{}})
	return r, rec.evs()
}

func stepIDs(ev []KiroEvent) []string {
	var out []string
	for _, e := range ev {
		if e.Step != nil {
			out = append(out, e.Step.ID)
		}
	}
	return out
}

func TestATurnThatFinishedWhileAwayGivesItsAnswerAndSteps(t *testing.T) {
	host, got := cloudHost(true, false)
	defer host.Shutdown("test")
	r, ev := attachTo(t, host)
	if r.State != core.Completed || r.Text != "Second answer, finished while away." {
		t.Error(r)
	}
	if ids := stepIDs(ev); !slices.Contains(ids, "cut1") || slices.Contains(ids, "old1") {
		t.Error("only the cut-off turn's steps:", ids)
	}
	load, _ := got.first("session/load")
	if !strings.Contains(load.Compact(), `"sessionSource":"remote"`) {
		t.Error(load.Compact())
	}
	if _, ok := got.first("session/prompt"); ok {
		t.Error("nothing is prompted")
	}
}

func TestATurnStillRunningIsFollowedToItsEnd(t *testing.T) {
	host, _ := cloudHost(false, true)
	defer host.Shutdown("test")
	r, ev := attachTo(t, host)
	if r.State != core.Completed || r.Text != "Finished the work." {
		t.Error(r)
	}
	if ids := stepIDs(ev); !slices.Contains(ids, "cut1") || !slices.Contains(ids, "live1") || slices.Contains(ids, "old1") {
		t.Error(ids)
	}
}

func TestKiroWebSessionsAreListedPageByPageWithoutTheLocalOnes(t *testing.T) {
	host, got := cloudHost(true, false)
	defer host.Shutdown("test")
	list, err := host.CloudSessions()
	if err != nil {
		t.Fatal(err)
	}
	type row struct {
		id, title string
		updated   bool
	}
	var ids []row
	for _, c := range list.Sessions {
		ids = append(ids, row{c.ID, c.Title, c.Updated != nil})
	}
	if !reflect.DeepEqual(ids, []row{{"c1", "Fix the footer", true}, {"c2", "Add tests", false}}) {
		t.Error("this computer's session is left out:", ids)
	}
	if list.Note != "" {
		t.Error(list.Note)
	}
	first, _ := got.first("session/list")
	if !strings.Contains(first.Compact(), `"sessionSource":"remote"`) {
		t.Error(first.Compact())
	}
}

func TestAKiroWebConversationIsReadBackTurnByTurn(t *testing.T) {
	host, _ := cloudHost(true, false)
	defer host.Shutdown("test")
	turns, err := host.CloudTranscript("c1", os.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	type row struct {
		prompt, text string
		steps        []string
		completed    bool
	}
	var got []row
	for _, tt := range turns {
		var steps []string
		for _, s := range tt.Steps {
			steps = append(steps, s.ID)
		}
		got = append(got, row{tt.Prompt, tt.Text, steps, tt.Completed})
	}
	if !reflect.DeepEqual(got, []row{{"first", "First answer.", []string{"old1"}, true}, {"second one", "Second answer, finished while away.", []string{"cut1"}, true}}) {
		t.Error(got)
	}
}

// When Kiro gives the same sessions whichever source it is asked for, Hover can't tell
// which are Kiro Web's: it shows none, and says so in words.
func TestWhenKiroCannotBeToldApartItSaysSo(t *testing.T) {
	host, _ := cloudHostWith(true, false, true)
	defer host.Shutdown("test")
	list, err := host.CloudSessions()
	if err != nil {
		t.Fatal(err)
	}
	if len(list.Sessions) != 0 || !strings.Contains(list.Note, "listed 2 for Kiro Web and 2 for this computer") || !strings.Contains(list.Note, "can’t tell") {
		t.Error(list)
	}
}

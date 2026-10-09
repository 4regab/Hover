package backend

import (
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"time"

	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/quota"
)

// Backend.RefreshQuotas: the quotas the user switched on, read one after another and sent
// as one `quotas` message (every five minutes, and when asked). Claude Code's sign-in is
// the Mac app's to read from the Keychain (the backend has no business there): it is asked
// for with `readClaudeCredentials` and comes back as `claudeCredentials`.

// credentialsWait is how long the host has to answer `readClaudeCredentials`.
const credentialsWait = 15 * time.Second

// credentials is the read of Claude Code's sign-in in flight, if any.
type credentials struct {
	mu sync.Mutex
	ch chan *string
}

// answer: `claudeCredentials` arrived; json is the Keychain item's text, nil when there is none.
func (c *credentials) answer(json *string) {
	c.mu.Lock()
	ch := c.ch
	c.ch = nil
	c.mu.Unlock()
	if ch != nil {
		ch <- json
	}
}

func (c *credentials) ask(out *Out) *string {
	ch := make(chan *string, 1)
	c.mu.Lock()
	c.ch = ch
	c.mu.Unlock()
	out.Send(core.JObj(core.P("type", jst("readClaudeCredentials"))))
	var got *string
	select {
	case got = <-ch:
	case <-time.After(credentialsWait):
	}
	c.mu.Lock()
	c.ch = nil
	c.mu.Unlock()
	return got
}

// claude is Quota.Claude's choice of sign-in: on a Mac the host's, else the file; elsewhere
// the file. quota does the asking of api.anthropic.com.
func claude(mac bool, host *string, file string, now time.Time) quota.Reading {
	switch {
	case mac && host != nil:
		return quota.ClaudeWith(*host, quota.ClaudeURL, now)
	case mac && !isFile(file):
		return quota.Fail(quota.ClaudeKeychainMissing)
	}
	return quota.ClaudeAt(file, quota.ClaudeURL, now)
}

// value is {ok, used, detail} for one reading.
func value(r quota.Reading) core.JSON {
	return core.JObj(core.P("ok", jbool(r.OK())), core.P("used", core.JOptDouble(r.Used)), core.P("detail", jst(r.Detail)))
}

// readAll is the `quotas` message: every quota switched on, read now. Blocks (kiro-cli
// takes seconds).
func readAll(settings *core.Settings, c *credentials, out *Out) core.JSON {
	var values []core.Prop
	for _, id := range core.NotchItems {
		if !settings.HasNotchItem(id) {
			continue
		}
		now := time.Now().UTC()
		var r quota.Reading
		switch id {
		case core.NotchCodex:
			r = quota.Codex(now)
		case core.NotchKiro:
			r = quota.Kiro()
		case core.NotchClaude:
			mac := runtime.GOOS == "darwin"
			var host *string
			if mac {
				host = c.ask(out)
			}
			r = claude(mac, host, filepath.Join(quota.ClaudeHome(), ".credentials.json"), now)
		default:
			r = quota.Cursor(now)
		}
		values = append(values, core.P(id, value(r)))
	}
	return core.JObj(core.P("type", jst("quotas")), core.P("values", core.JObj(values...)))
}

func isFile(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.Mode().IsRegular()
}

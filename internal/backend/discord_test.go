package backend

// `initialize` with `hostDiscord: true`: the Windows host shows the Discord status itself, so
// the backend starts no client of its own and the status is shown once. The Mac host never
// sends the flag and keeps the backend's client (the first half of the test shows it runs).

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

// logHas says whether hover.log in the data folder has the words.
func logHas(dir, words string) bool {
	b, err := os.ReadFile(filepath.Join(dir, "hover.log"))
	return err == nil && strings.Contains(string(b), words)
}

// discordHost starts a backend whose settings have the Discord switch on, sends `initialize`
// with the extra fields, and returns it started.
func discordHost(t *testing.T, extra msg) (*host, string) {
	t.Helper()
	dir := t.TempDir()
	noticed(t, dir, func(s *core.Settings) { s.SetDiscordPresence(true) })
	h := startHost(t, dir)
	init := msg{"type": "initialize", "key": testKey}
	for k, v := range extra {
		init[k] = v
	}
	h.send(init)
	h.next("initialized")
	return h, dir
}

func TestTheBackendRunsTheDiscordClientUnlessTheHostDoesIt(t *testing.T) {
	// Without the flag (the Mac host): the client looks for Discord at once and, finding none
	// on this computer, says so once in the log.
	h, dir := discordHost(t, nil)
	deadline := time.Now().Add(20 * time.Second)
	for !logHas(dir, "discord status not sent") && time.Now().Before(deadline) {
		time.Sleep(50 * time.Millisecond)
	}
	if !logHas(dir, "discord status not sent") {
		t.Fatalf("the backend's own Discord client did not run\n%s", h.stderr.String())
	}
	h.stop()

	// With the flag: no client, so nothing looks for Discord, however long it waits and
	// whether a task works or not.
	h, dir = discordHost(t, msg{"hostDiscord": true})
	h.send(msg{"type": "ready"})
	h.next("state")
	h.send(msg{"type": "saveSettings", "discordPresence": true})
	h.next("preferences")
	time.Sleep(2 * time.Second)
	if logHas(dir, "discord") {
		t.Fatalf("the backend ran a Discord client although the host does it\n%s", h.stderr.String())
	}
	if code := h.stop(); code != 0 {
		t.Fatalf("exit %d\n%s", code, h.stderr.String())
	}
}

func TestAHostDiscordFlagOfTheWrongKindChangesNothing(t *testing.T) {
	// Only `true` takes the client away: a string or a number is not the flag.
	for _, v := range []any{"true", 1, nil, []any{true}} {
		h, dir := discordHost(t, msg{"hostDiscord": v})
		deadline := time.Now().Add(20 * time.Second)
		for !logHas(dir, "discord status not sent") && time.Now().Before(deadline) {
			time.Sleep(50 * time.Millisecond)
		}
		if !logHas(dir, "discord status not sent") {
			t.Fatalf("hostDiscord %v took the backend's client away\n%s", v, h.stderr.String())
		}
		h.stop()
	}
}

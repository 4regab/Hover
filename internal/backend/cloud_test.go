package backend

// Rows 30 to 32 of docs/development/windows-csharp.md B2: Kiro Web's repositories and the
// sessions of Kiro Web and of this computer. There is no Kiro here to answer, so what is
// checked is that the request reaches Kiro's runtime and that a bad id or a list that was
// never asked for is refused; the listing itself is what Kiro's ACP host gives.

import (
	"strings"
	"testing"
)

// answered is the answer to a request that goes to Kiro, which this machine does not have
// (or, where it has one, signed out under the empty home folder the test gives it): ok or
// not, but a whole answer that ends.
func answered(t *testing.T, h *host, command msg, typ string) msg {
	t.Helper()
	h.send(command)
	return h.until("the end of "+typ, func(m msg) bool {
		return m["type"] == typ && (m["done"] == nil || m["done"] == true)
	})
}

func TestKiroWebAndCliListingsAnswerAndEnd(t *testing.T) {
	h := taskHost(t, "echo")
	for _, typ := range []string{"cloudRepos", "webSessions", "cliSessions"} {
		m := answered(t, h, msg{"type": typ}, typ)
		if ok, isBool := m["ok"].(bool); !isBool || !ok && str(m, "error") == "" {
			t.Fatalf("%s: %v", typ, m)
		}
		if m["ok"] == true {
			if _, has := m["repos"]; typ == "cloudRepos" && !has {
				t.Fatalf("%s has no repos: %v", typ, m)
			}
			if _, has := m["sessions"]; typ != "cloudRepos" && !has {
				t.Fatalf("%s has no sessions: %v", typ, m)
			}
		}
	}
}

func TestAdoptTakesOnlyWhatWasListed(t *testing.T) {
	h := taskHost(t, "echo")
	for _, typ := range []string{"adoptWeb", "adoptCli"} {
		for name, c := range map[string]msg{
			"an id that was never listed": {"type": typ, "id": "never-listed"},
			"no id":                       {"type": typ},
			"an id that is a number":      {"type": typ, "id": 5},
			"an id that is empty":         {"type": typ, "id": ""},
			"an id that is huge":          {"type": typ, "id": strings.Repeat("x", 201)},
			"an id that is a path":        {"type": typ, "id": "../../etc/passwd"},
		} {
			if why := h.refused(c, typ); why == "" {
				t.Fatal(typ, name)
			}
		}
	}
	// And nothing came to a desk.
	if n := len(arr(h.state(), "sessions")); n != 0 {
		t.Fatalf("%d sessions", n)
	}
}

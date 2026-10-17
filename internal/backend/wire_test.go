package backend

import (
	"bytes"
	"strings"
	"testing"
	"time"
)

func TestOverlongHostLineDoesNotEndTheBackend(t *testing.T) {
	old := lineLimit
	lineLimit = 64
	t.Cleanup(func() { lineLimit = old })

	var in bytes.Buffer
	in.WriteString(strings.Repeat("x", 200))
	in.WriteByte('\n')
	in.WriteString("{\"type\":\"ping\"}\n")

	var out bytes.Buffer
	done := make(chan struct{})
	go func() {
		Run(&in, NewOut(&out))
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("backend did not return")
	}
	text := out.String()
	if !strings.Contains(text, "too long") {
		t.Fatalf("the long line was not skipped: %s", text)
	}
	if !strings.Contains(text, "Initialize the backend first.") {
		t.Fatalf("the command after the long line was never read: %s", text)
	}
}

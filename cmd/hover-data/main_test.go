package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/4regab/Hover/internal/core"
)

// A key set with hover-data opens the way Hover opens it, a wrong or missing one is told, and
// the key is in nothing printed.
func TestSecretSetAndCheckNeverPrintTheKey(t *testing.T) {
	data := t.TempDir()
	const key = "gsk_0123456789abcdef"
	run := func(op, name, stdin string) (int, string) {
		var out, errOut bytes.Buffer
		code := secret(op, data, name, strings.NewReader(stdin), &out, &errOut)
		text := out.String() + errOut.String()
		if strings.Contains(text, "0123456789abcdef") {
			t.Fatalf("the key was printed: %s", text)
		}
		return code, text
	}
	if code, _ := run("check", core.GroqSecret, ""); code != 1 {
		t.Fatal("check with no key file should fail")
	}
	if _, err := os.Stat(filepath.Join(data, "note.key")); err == nil {
		t.Fatal("check made a key")
	}
	if code, text := run("set", core.GroqSecret, key+"\r\nsecond line ignored\n"); code != 0 {
		t.Fatal(code, text)
	}
	if code, text := run("check", core.GroqSecret, ""); code != 0 || !strings.Contains(text, "kept") {
		t.Fatal(code, text)
	}
	if code, _ := run("check", core.GroqSecret, key+"\n"); code != 0 {
		t.Fatal("the same key")
	}
	if code, text := run("check", core.GroqSecret, "gsk_other\n"); code != 1 || !strings.Contains(text, "not the key given") {
		t.Fatal(code, text)
	}
	if code, _ := run("check", "cleanup.openai", ""); code != 1 {
		t.Fatal("another name is not kept")
	}
	for _, c := range []struct{ op, name, stdin string }{
		{"set", "voice.other", key}, {"burn", core.GroqSecret, key}, {"set", core.GroqSecret, ""}, {"set", core.GroqSecret, "\n"},
	} {
		if code, _ := run(c.op, c.name, c.stdin); code == 0 {
			t.Fatalf("%+v was accepted", c)
		}
	}
	// secrets.dat holds no plain text, and Hover's own reader opens it with note.key.
	raw, err := os.ReadFile(filepath.Join(data, "secrets.dat"))
	if err != nil || bytes.Contains(raw, []byte("0123456789abcdef")) {
		t.Fatalf("secrets.dat: %v", err)
	}
	stored, _ := os.ReadFile(filepath.Join(data, "note.key"))
	k, kerr := core.SystemKeyGuard{}.Unwrap(stored)
	if kerr != nil {
		t.Fatal(kerr)
	}
	if v, ok := core.NewSecrets(filepath.Join(data, "secrets.dat"), core.CryptoWithKey([32]byte(k))).Get(core.GroqSecret); !ok || v != key {
		t.Fatal("secrets.dat does not open with the data folder's key")
	}
}

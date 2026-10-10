package core

import (
	"bytes"
	"encoding/hex"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// The tests of crypto.rs, one for one.

// A host's key is taken once, 32 bytes, and then it is Hover's key. This test owns the
// process's key (the other tests here never call GlobalCrypto).
func TestTheHostsKeyIsTakenOnce(t *testing.T) {
	if UseHostKey(bytes.Repeat([]byte{1}, 31)) == nil || UseHostKey(nil) == nil {
		t.Fatal("a key that isn't 32 bytes was taken")
	}
	if err := UseHostKey(bytes.Repeat([]byte{9}, 32)); err != nil {
		t.Fatal(err)
	}
	g := GlobalCrypto()
	if g == nil {
		t.Fatal("the host's key")
	}
	if got := CryptoWithKey([32]byte(bytes.Repeat([]byte{9}, 32))).Open(g.Seal("x")); got != "x" {
		t.Fatal(got)
	}
	if UseHostKey(bytes.Repeat([]byte{9}, 32)) == nil {
		t.Fatal("not twice")
	}
}

// NIST SP 800-38D / the GCM spec's test case 14 (256-bit zero key, zero IV, one zero
// block): the framing puts its IV, ciphertext and tag end to end.
func TestFramesAKnownGcmVector(t *testing.T) {
	c := CryptoWithKey([32]byte{})
	sealed := c.SealWith(strings.Repeat("\x00", 16), [12]byte{})
	if got := hex.EncodeToString(sealed); got != "000000000000000000000000cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919" {
		t.Fatal(got)
	}
	if c.Open(sealed) != strings.Repeat("\x00", 16) {
		t.Fatal("open")
	}
}

// CryptoTests, ported.
func TestRoundTripsUnicodeWithAFreshNonceAndRefusesTampering(t *testing.T) {
	c := CryptoWithKey([32]byte(bytes.Repeat([]byte{7}, 32)))
	text := "Привет 👋\nsecret note"
	a, b := c.Seal(text), c.Seal(text)
	if c.Open(a) != text || c.Open(b) != text || bytes.Equal(a, b) || len(a) != 12+len(text)+16 {
		t.Fatal("round trip")
	}
	tampered := c.Seal("do not alter")
	tampered[len(tampered)-1] ^= 1
	if c.Open(tampered) != "" || c.Open(make([]byte, 8)) != "" || c.Open(nil) != "" {
		t.Fatal("tampering opened")
	}
	if CryptoWithKey([32]byte(bytes.Repeat([]byte{8}, 32))).Open(a) != "" || c.Open(c.Seal("")) != "" {
		t.Fatal("another key opened")
	}
}

// plainGuard: XOR stands in for the platform's wrapping; "LOCKED" reads as a keyring
// that isn't there now, "GONE" as one whose item is gone for good.
type plainGuard struct{}

func xor(b []byte) []byte {
	out := make([]byte, len(b))
	for i, x := range b {
		out[i] = x ^ 0x5a
	}
	return out
}

func (plainGuard) Wrap(key []byte) ([]byte, error) { return xor(key), nil }
func (plainGuard) Unwrap(s []byte) ([]byte, *KeyError) {
	switch string(s) {
	case "LOCKED":
		return nil, KeyNotNow("the keyring isn't running")
	case "GONE":
		return nil, KeyNever("the keyring has no such item")
	}
	return xor(s), nil
}
func (plainGuard) Inherited() ([]byte, *KeyError) { return nil, nil }

func keyDir(t *testing.T) string { return t.TempDir() }

func aside(t *testing.T, d string) [][]byte {
	var out [][]byte
	entries, _ := os.ReadDir(d)
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), "note.key.unreadable-") {
			b, _ := os.ReadFile(filepath.Join(d, e.Name()))
			out = append(out, b)
		}
	}
	return out
}

func TestTheKeyIsKeptAndAnUnreadableOneSetAsideNeverLost(t *testing.T) {
	d := keyDir(t)
	f := filepath.Join(d, "note.key")
	sealed := LoadOrCreateCrypto(f, plainGuard{}).Seal("x")
	if LoadOrCreateCrypto(f, plainGuard{}).Open(sealed) != "x" {
		t.Fatal("the key wasn't kept")
	}
	if runtime.GOOS != "windows" {
		if st, _ := os.Stat(f); st.Mode().Perm() != 0o600 {
			t.Fatal(st.Mode())
		}
	}
	for _, bad := range [][]byte{[]byte("short"), []byte("GONE")} {
		os.WriteFile(f, bad, 0o600)
		c := LoadOrCreateCrypto(f, plainGuard{})
		if c.Open(sealed) != "" {
			t.Fatal("the old key opened")
		}
		if b, _ := os.ReadFile(f); len(b) != 32 {
			t.Fatal("a new key")
		}
		found := false
		for _, a := range aside(t, d) {
			found = found || bytes.Equal(a, bad)
		}
		if !found {
			t.Fatal("the old one kept beside it")
		}
		entries, _ := os.ReadDir(d)
		for _, e := range entries {
			if e.Name() != "note.key" {
				os.Remove(filepath.Join(d, e.Name()))
			}
		}
	}
}

// earlier is the earlier build's key, or a store that can't be asked.
type earlier struct {
	key []byte
	err *KeyError
}

func (e earlier) Wrap(k []byte) ([]byte, error)       { return plainGuard{}.Wrap(k) }
func (e earlier) Unwrap(s []byte) ([]byte, *KeyError) { return plainGuard{}.Unwrap(s) }
func (e earlier) Inherited() ([]byte, *KeyError)      { return e.key, e.err }

func TestAHistoryWithoutItsNoteKeyKeepsTheKeyAnEarlierBuildLeft(t *testing.T) {
	four := bytes.Repeat([]byte{4}, 32)
	old := CryptoWithKey([32]byte(four)).Seal("history")
	withHistory := func() string {
		d := keyDir(t)
		os.MkdirAll(filepath.Join(d, "agents"), 0o755)
		os.WriteFile(filepath.Join(d, "agents", "index.dat"), old, 0o644)
		return d
	}
	// No history yet: the earlier key is not asked for, a new one is made.
	if LoadOrCreateCrypto(filepath.Join(keyDir(t), "note.key"), earlier{key: four}).Open(old) != "" {
		t.Fatal("asked for the earlier key with no history")
	}
	// A history: its key is carried on with, and written back the usual way.
	d := withHistory()
	if LoadOrCreateCrypto(filepath.Join(d, "note.key"), earlier{key: four}).Open(old) != "history" {
		t.Fatal("the earlier key")
	}
	if LoadOrCreateCrypto(filepath.Join(d, "note.key"), plainGuard{}).Open(old) != "history" {
		t.Fatal("written back")
	}
	// No earlier key, or one that isn't 32 bytes: a new key, as before.
	for _, none := range []earlier{{}, {key: bytes.Repeat([]byte{1}, 5)}} {
		if LoadOrCreateCrypto(filepath.Join(withHistory(), "note.key"), none) == nil {
			t.Fatal("no key")
		}
	}
	// The store can't be asked now: no key this run, and no note.key written.
	d = withHistory()
	if LoadOrCreateCrypto(filepath.Join(d, "note.key"), earlier{err: KeyNotNow("locked")}) != nil {
		t.Fatal("a key while the store can't be asked")
	}
	if _, err := os.Stat(filepath.Join(d, "note.key")); err == nil {
		t.Fatal("note.key written")
	}
}

func TestAKeyThatCantBeReadNowIsLeftAloneAndNothingIsSealed(t *testing.T) {
	d := keyDir(t)
	f := filepath.Join(d, "note.key")
	os.WriteFile(f, []byte("LOCKED"), 0o600)
	if LoadOrCreateCrypto(f, plainGuard{}) != nil {
		t.Fatal("a key")
	}
	if b, _ := os.ReadFile(f); string(b) != "LOCKED" {
		t.Fatal("untouched, for the next start")
	}
	if len(aside(t, d)) != 0 {
		t.Fatal("set aside")
	}
}

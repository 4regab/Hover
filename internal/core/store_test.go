package core

import (
	"bytes"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

// The tests of store.rs, secrets.rs and images.rs, one for one.

func TestADocumentIsSealedAndComesBackWhole(t *testing.T) {
	d := t.TempDir()
	s := SealedIn(d, "jobs", CryptoWithKey([32]byte(bytes.Repeat([]byte{5}, 32))))
	if _, ok := s.Read(); ok {
		t.Fatal("a document before any write")
	}
	if err := s.Write(JObj(P("Secret", JStr("hunter2")))); err != nil {
		t.Fatal(err)
	}
	raw, _ := os.ReadFile(s.File())
	if bytes.Contains(raw, []byte("hunter2")) {
		t.Fatal("sealed, not plain text")
	}
	v, _ := s.Read()
	if x, _ := v.Get("Secret"); func() string { s, _ := x.AsStr(); return s }() != "hunter2" {
		t.Fatal(v.Compact())
	}
	// Another key opens nothing and the file is set aside, not written over.
	other := SealedIn(d, "jobs", CryptoWithKey([32]byte(bytes.Repeat([]byte{6}, 32))))
	if _, ok := other.Read(); ok {
		t.Fatal("another key read it")
	}
	if _, err := os.Stat(filepath.Join(d, "jobs.dat")); err == nil {
		t.Fatal("jobs.dat still there")
	}
	found := false
	entries, _ := os.ReadDir(d)
	for _, e := range entries {
		found = found || strings.HasSuffix(e.Name(), ".bad")
	}
	if !found {
		t.Fatal("no .bad")
	}
}

func TestKeysAreSealedOnDiskAndWithoutAKeyLiveOnlyInMemory(t *testing.T) {
	d := t.TempDir()
	f := filepath.Join(d, "secrets.dat")
	c := key3()
	s := NewSecrets(f, c)
	if st, err := s.Set("voice.groq", ptr("  gsk_SECRETVALUE ")); err != nil || st != Saved {
		t.Fatal(st, err)
	}
	raw, _ := os.ReadFile(f)
	if bytes.Contains(raw, []byte("SECRETVALUE")) {
		t.Fatal("sealed, not plain")
	}
	if v, _ := NewSecrets(f, c).Get("voice.groq"); v != "gsk_SECRETVALUE" {
		t.Fatal(v)
	}
	if _, ok := NewSecrets(f, CryptoWithKey([32]byte(bytes.Repeat([]byte{4}, 32)))).Get("voice.groq"); ok {
		t.Fatal("another key opens nothing")
	}
	s.Set("voice.groq", ptr(" "))
	if NewSecrets(f, c).Has("voice.groq") {
		t.Fatal("a blank key forgets it")
	}
	mem := NewSecrets(filepath.Join(d, "none.dat"), nil)
	if st, _ := mem.Set("voice.groq", ptr("k")); st != ThisRunOnly {
		t.Fatal(st)
	}
	if v, _ := mem.Get("voice.groq"); v != "k" {
		t.Fatal(v)
	}
	if _, err := os.Stat(filepath.Join(d, "none.dat")); err == nil {
		t.Fatal("nothing written without Hover's key")
	}
}

func TestSavesAsSaveImagesDoes(t *testing.T) {
	d := t.TempDir()
	s := JStr
	items := []JSON{s("data:image/png;base64,iVBO"), s("data:image/bmp;base64,AAAA"), s("data:text/plain;base64,AAAA"),
		s("data:image/png,AAAA"), s("data:image/jpeg;x=1;base64,/9j/"), s("data:image/gif;base64,R0l"), JInt(1), s("data:image/png;base64,AAAA")}
	saved := SaveImages(items, d)
	var names []string
	for _, p := range saved {
		names = append(names, filepath.Base(p))
	}
	if len(names) != 2 || !strings.HasSuffix(names[0], ".png") || !strings.HasSuffix(names[1], ".jpg") {
		t.Fatal(names)
	}
	if len(names[0]) != 24+4 || names[0][8:9] != "-" {
		t.Fatal(names[0])
	}
	if b, _ := os.ReadFile(saved[0]); !bytes.Equal(b, []byte{0x89, 0x50, 0x4E}) {
		t.Fatal(b)
	}
	var five []JSON
	for i := 0; i < 5; i++ {
		five = append(five, s("data:image/webp;base64,UklG"))
	}
	if n := len(SaveImages(five, d)); n != 4 {
		t.Fatal(n)
	}
}

func TestTheSizeCheckCountsTheCommaAsCSharpDoes(t *testing.T) {
	d := t.TempDir()
	// (len - comma) * 3 / 4 must not pass 8 MiB, with the comma counted.
	if n := len(SaveImages([]JSON{JStr("data:image/png;base64," + strings.Repeat("A", 11_184_808))}, d)); n != 1 {
		t.Fatal(n)
	}
	if n := len(SaveImages([]JSON{JStr("data:image/png;base64," + strings.Repeat("A", 11_184_812))}, d)); n != 0 {
		t.Fatal(n)
	}
}

func TestOldFilesAreSwept(t *testing.T) {
	d := t.TempDir()
	os.WriteFile(filepath.Join(d, "new.png"), []byte("x"), 0o644)
	os.Mkdir(filepath.Join(d, "sub"), 0o755)
	SweepImages(d, time.Now().Add(13*24*time.Hour))
	if _, err := os.Stat(filepath.Join(d, "new.png")); err != nil {
		t.Fatal("swept too soon")
	}
	SweepImages(d, time.Now().Add(15*24*time.Hour))
	if _, err := os.Stat(filepath.Join(d, "new.png")); err == nil {
		t.Fatal("not swept")
	}
	if !isDir(filepath.Join(d, "sub")) {
		t.Fatal("a folder was swept")
	}
}

func TestBase64AsConvertReadsIt(t *testing.T) {
	if b, ok := FromBase64("SGVs bG8=\r\n"); !ok || string(b) != "Hello" {
		t.Fatal(b)
	}
	if b, ok := FromBase64(""); !ok || !reflect.DeepEqual(b, []byte{}) {
		t.Fatal(b)
	}
	for _, bad := range []string{"SGVsbG8", "SG=sbG8=", "SGVs=G8=", "S===", "SGVsbG8\f="} {
		if _, ok := FromBase64(bad); ok {
			t.Errorf("%q read", bad)
		}
	}
}

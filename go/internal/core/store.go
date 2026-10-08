package core

// store.rs, secrets.rs and images.rs.

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf16"
)

// MARK: store.rs

// Sealed is one sealed JSON document on disk: what the orchestration records, schedules,
// watches, custom agents and the like are kept in. Sealed with Hover's key as the history
// is, written to a temporary file and then renamed over the old one, so a crash mid-write
// never leaves half a document. A file that can't be read is set aside, not written over.
type Sealed struct {
	file   string
	crypto *Crypto
}

func NewSealed(file string, c *Crypto) *Sealed { return &Sealed{file, c} }

// SealedIn is the document under name in dir (<dir>/<name>.dat).
func SealedIn(dir, name string, c *Crypto) *Sealed {
	return NewSealed(filepath.Join(dir, name+".dat"), c)
}

func (s *Sealed) File() string { return s.file }

// Read is the document, or none when there is none yet or it can't be read (then the
// file is kept beside it as .bad, and the log says why).
func (s *Sealed) Read() (JSON, bool) {
	b, err := os.ReadFile(s.file)
	if err != nil {
		return JNull, false
	}
	v, err := ParseJSON(s.crypto.Open(b))
	if err == nil && !v.IsNull() {
		return v, true
	}
	why := "empty"
	if err != nil {
		why = err.Error()
	}
	Logf("store: %s unreadable - %s", s.file, why)
	os.Rename(s.file, s.file+"."+GUIDN()+".bad")
	return JNull, false
}

func (s *Sealed) Write(v JSON) error {
	if err := os.MkdirAll(filepath.Dir(s.file), 0o755); err != nil {
		return err
	}
	tmp := s.file + ".tmp"
	if err := os.WriteFile(tmp, s.crypto.Seal(v.Compact()), 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, s.file)
}

func (s *Sealed) Remove() { os.Remove(s.file) }

// MARK: secrets.rs

// Stored is where a key that was set now lives.
type Stored int

const (
	Saved Stored = iota
	ThisRunOnly
)

// Secrets are the API keys the user gives Hover (Groq, the cleanup service): sealed with
// Hover's own key in secrets.dat, never in settings.json, the history or the log. Without
// that key this run, a key is kept in memory only until Hover quits, and the caller says
// so: never written in the clear.
type Secrets struct {
	file   string
	crypto *Crypto
	mu     sync.Mutex
	m      []KV[string] // every key by name, read once
	loaded bool
}

func NewSecrets(file string, c *Crypto) *Secrets { return &Secrets{file: file, crypto: c} }

// SystemSecrets is the real one: secrets.dat beside settings.json, sealed with Hover's key.
func SystemSecrets() *Secrets {
	return NewSecrets(filepath.Join(Support(), "secrets.dat"), GlobalCrypto())
}

// Persistent: keys set now are kept across restarts.
func (s *Secrets) Persistent() bool { return s.crypto != nil }

func (s *Secrets) load() {
	if s.loaded {
		return
	}
	s.loaded, s.m = true, []KV[string]{}
	if s.crypto == nil {
		return
	}
	b, err := os.ReadFile(s.file)
	if err != nil {
		return
	}
	// Unsealed by this key or not at all: a file that won't open reads as none.
	if v, err := ParseJSON(s.crypto.Open(b)); err == nil {
		if m, ok, err := OptMap(v, itemText); err == nil && ok {
			s.m = m
		}
	}
}

func (s *Secrets) Get(name string) (string, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.load()
	for _, kv := range s.m {
		if kv.Key == name && kv.Val != "" {
			return kv.Val, true
		}
	}
	return "", false
}

func (s *Secrets) Has(name string) bool { _, ok := s.Get(name); return ok }

// Set sets (or with nil or blank, forgets) a key. An error when it couldn't be written;
// the message never holds the key.
func (s *Secrets) Set(name string, value *string) (Stored, error) {
	s.mu.Lock()
	s.load()
	kept := s.m[:0]
	for _, kv := range s.m {
		if kv.Key != name {
			kept = append(kept, kv)
		}
	}
	s.m = kept
	if value != nil {
		if v := strings.TrimSpace(*value); v != "" {
			s.m = append(s.m, KV[string]{name, v})
		}
	}
	props := make([]Prop, len(s.m))
	for i, kv := range s.m {
		props[i] = P(kv.Key, JStr(kv.Val))
	}
	text := JObj(props...).Compact()
	s.mu.Unlock()
	if s.crypto == nil {
		return ThisRunOnly, nil
	}
	tmp := strings.TrimSuffix(s.file, filepath.Ext(s.file)) + ".dat.tmp"
	err := writePrivate(tmp, s.crypto.Seal(text))
	if err == nil {
		err = os.Rename(tmp, s.file)
	}
	if err != nil {
		return Saved, fmt.Errorf("Hover couldn’t save the key: %s", errKind(err))
	}
	return Saved, nil
}

// errKind is the error without its path, as io::ErrorKind prints: a key's name may be in it.
func errKind(err error) string {
	var pe *os.PathError
	if errors.As(err, &pe) {
		return pe.Err.Error()
	}
	var le *os.LinkError
	if errors.As(err, &le) {
		return le.Err.Error()
	}
	return err.Error()
}

// MARK: images.rs

// KiroPage.ImagesFolder and SaveImages: pasted pictures kept as files in kiro-images,
// for the agent to read and the office to show; files older than two weeks go the first
// time the folder is asked for in a run.

const (
	MaxImages     = 4
	MaxImageBytes = 8 * 1024 * 1024
	imagesKeep    = 14 * 24 * time.Hour
)

var swept atomic.Bool

// ImagesFolder is the folder under the data folder, made if needed, swept once per run.
func ImagesFolder(support string) string {
	dir := filepath.Join(support, "kiro-images")
	os.MkdirAll(dir, 0o755)
	if !swept.Swap(true) {
		SweepImages(dir, time.Now())
	}
	return dir
}

// SweepImages deletes the files (not folders) last written more than 14 days before now.
func SweepImages(dir string, now time.Time) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return
	}
	cut := now.Add(-imagesKeep)
	for _, e := range entries {
		info, err := e.Info()
		if err == nil && info.Mode().IsRegular() && info.ModTime().Before(cut) {
			os.Remove(filepath.Join(dir, e.Name()))
		}
	}
}

// SaveImages saves a message's images, as data: URLs, into the folder. Only PNG, JPEG,
// GIF and WebP, up to four of 8 MiB; anything else is skipped, and an item that isn't a
// string ends the list, as the C# loop breaks there.
func SaveImages(items []JSON, dir string) []string {
	var saved []string
	for _, item := range items {
		url, ok := item.AsStr()
		if !ok || len(saved) >= MaxImages {
			break
		}
		comma := strings.IndexByte(url, ',')
		if comma < 0 || !strings.HasPrefix(url, "data:image/") || !strings.HasSuffix(url[:comma], ";base64") {
			continue
		}
		// url[11..url.IndexOf(';')]: up to the first ';', wherever it is.
		semi := strings.IndexByte(url, ';')
		if semi < 11 {
			continue
		}
		var ext string
		switch url[11:semi] {
		case "png":
			ext = ".png"
		case "jpeg":
			ext = ".jpg"
		case "gif":
			ext = ".gif"
		case "webp":
			ext = ".webp"
		default:
			continue
		}
		// (url.Length - comma) * 3 / 4, in UTF-16 units as C# counts them.
		if len(utf16.Encode([]rune(url[comma:])))*3/4 > MaxImageBytes {
			continue
		}
		b, ok := FromBase64(url[comma+1:])
		if !ok {
			continue
		}
		name := []rune(LocalCompact() + "-" + GUIDN())
		file := filepath.Join(dir, string(name[:24])+ext)
		if err := os.WriteFile(file, b, 0o644); err != nil {
			Logf("kiro office: couldn't save a pasted image - %v", err)
			continue
		}
		saved = append(saved, file)
	}
	return saved
}

// FromBase64 is Convert.FromBase64String: spaces, tabs, CR and LF anywhere are skipped;
// the rest must be whole quads with at most two '=' at the end. encoding/base64 takes
// other whitespace rules and padding in the middle differently, so it isn't used.
func FromBase64(s string) ([]byte, bool) {
	const a = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
	var in []byte
	for i := 0; i < len(s); i++ {
		if c := s[i]; c != ' ' && c != '\t' && c != '\r' && c != '\n' {
			in = append(in, c)
		}
	}
	if len(in)%4 != 0 {
		return nil, false
	}
	out := make([]byte, 0, len(in)/4*3)
	quads := len(in) / 4
	for q := 0; q < quads; q++ {
		var n uint32
		pad := 0
		for i, ch := range in[q*4 : q*4+4] {
			var v uint32
			switch {
			case ch == '=' && i >= 2 && q+1 == quads:
				pad++
			case pad > 0:
				return nil, false
			default:
				k := strings.IndexByte(a, ch)
				if k < 0 {
					return nil, false
				}
				v = uint32(k)
			}
			n = n<<6 | v
		}
		out = append(out, byte(n>>16))
		if pad < 2 {
			out = append(out, byte(n>>8))
		}
		if pad < 1 {
			out = append(out, byte(n))
		}
	}
	return out, true
}

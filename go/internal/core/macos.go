package core

// platform/macos.rs, the parts that need no macOS: the Keychain guard over whatever answers
// for the Keychain, the key marker, and the LaunchAgent's text and files. Compiled on every
// OS, as Rust's is, so the logic is the same on all of them. Only the calls into
// Security.framework are darwin's (keychain_darwin.go); platform_darwin.go wires them in.

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"unicode"
	"unicode/utf8"
)

// Keychain is what the Keychain is asked, so the guard's logic runs anywhere.
type Keychain interface {
	Set(service, account string, secret []byte) error
	// Get is nil, nil when there is no such item; an error when the Keychain can't be asked
	// now (locked, a prompt dismissed or denied, no login session).
	Get(service, account string) ([]byte, error)
}

// KeychainService is the generic password's service; its account is note.key:<id>.
const KeychainService = "Hover"

// keychainMarker is what note.key holds when the key itself is in the Keychain: this
// marker and the item's id. A key made for another item never replaces this one's.
const keychainMarker = "hover-key:keychain:"

// Where the Swift host of the first macOS build kept the history key (32 bytes).
const (
	swiftService = "dev.hover.history"
	swiftAccount = "history-v1"
)

func keychainAccount(id string) string { return "note.key:" + id }

func keychainMarkerFile(id string) []byte { return []byte(keychainMarker + id + "\n") }

// keychainMarkerID is the item id a note.key names, false when it is not a Keychain marker.
// An id is what Wrap makes (hex); anything else is not trusted as an account name.
func keychainMarkerID(stored []byte) (string, bool) {
	if !utf8.Valid(stored) {
		return "", false
	}
	rest, ok := strings.CutPrefix(string(stored), keychainMarker)
	if !ok {
		return "", false
	}
	id := strings.TrimSpace(rest)
	if id == "" || len(id) > 64 {
		return "", false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c == '-') {
			return "", false
		}
	}
	return id, true
}

// KeychainGuard keeps the key in the Keychain, encrypted with the login and unlocked with
// it, as DPAPI keeps it on Windows. Without one (no login keychain, a refused write)
// note.key holds the key itself, readable by this user only (0600). Either way, other
// programs of the same user can read it, as with DPAPI.
type KeychainGuard struct{ K Keychain }

func (g KeychainGuard) Wrap(key []byte) ([]byte, error) {
	var b [8]byte
	if _, err := rand.Read(b[:]); err != nil {
		return nil, err
	}
	id := hex.EncodeToString(b[:])
	if err := g.K.Set(KeychainService, keychainAccount(id), key); err != nil {
		Logf("no Keychain (%v); note.key keeps the key, for this user only", err)
		return append([]byte(nil), key...), nil
	}
	return keychainMarkerFile(id), nil
}

func (g KeychainGuard) Unwrap(stored []byte) ([]byte, *KeyError) {
	if id, ok := keychainMarkerID(stored); ok {
		// A locked Keychain or a dismissed prompt may be fine next time; an item that isn't
		// there is gone for good.
		k, err := g.K.Get(KeychainService, keychainAccount(id))
		switch {
		case err != nil:
			return nil, KeyNotNow(err.Error())
		case k == nil:
			return nil, KeyNever("the Keychain has no Hover key " + id)
		}
		return k, nil
	}
	if len(stored) == 32 {
		return append([]byte(nil), stored...), nil
	}
	return nil, KeyNever("note.key is not a key this build can read (a Windows DPAPI key only opens on Windows)")
}

func (g KeychainGuard) Inherited() ([]byte, *KeyError) {
	k, err := g.K.Get(swiftService, swiftAccount)
	if err != nil {
		return nil, KeyNotNow(err.Error())
	}
	if len(k) == 32 {
		return k, nil
	}
	return nil, nil
}

// MARK: Launch at login

// LaunchAgentID is the LaunchAgent's label and file name: the bundle identifier the first
// macOS build used.
const LaunchAgentID = "dev.hover.desktop"

// xmlEscape is text in a plist <string>.
var xmlEscape = strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;", `"`, "&quot;", "'", "&apos;").Replace

// launchAgentPlist is the LaunchAgent that starts Hover at login: the program, no
// arguments. Aqua only, so an ssh session's login doesn't start a window-less copy.
func launchAgentPlist(id, exe string) string {
	return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n" +
		"<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n" +
		"<plist version=\"1.0\">\n<dict>\n" +
		"\t<key>Label</key>\n\t<string>" + xmlEscape(id) + "</string>\n" +
		"\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>" + xmlEscape(exe) + "</string>\n\t</array>\n" +
		"\t<key>RunAtLoad</key>\n\t<true/>\n" +
		"\t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n" +
		"\t<key>ProcessType</key>\n\t<string>Interactive</string>\n" +
		"</dict>\n</plist>\n"
}

// keyIsTrue is <key>name</key> followed by <true/>.
func keyIsTrue(plist, key string) bool {
	k := "<key>" + key + "</key>"
	i := strings.Index(plist, k)
	return i >= 0 && strings.HasPrefix(strings.TrimLeftFunc(plist[i+len(k):], unicode.IsSpace), "<true/>")
}

// plistStartsAtLogin: whether a LaunchAgent plist starts a program at login: it has one,
// RunAtLoad is true, and it isn't Disabled.
func plistStartsAtLogin(plist string) bool {
	return strings.Contains(plist, "<key>ProgramArguments</key>") && keyIsTrue(plist, "RunAtLoad") && !keyIsTrue(plist, "Disabled")
}

func agentFile(dir, id string) string { return filepath.Join(dir, id+".plist") }

func agentEnabled(dir, id string) bool {
	b, err := os.ReadFile(agentFile(dir, id))
	// read_to_string: text that isn't UTF-8 is no plist.
	return err == nil && utf8.Valid(b) && plistStartsAtLogin(string(b))
}

// setAgent writes or removes the agent (exe nil). No launchctl: launchd reads
// ~/Library/LaunchAgents at the next login, and starting it now would only launch a second
// copy.
func setAgent(dir, id string, exe *string) error {
	f := agentFile(dir, id)
	if exe == nil {
		if err := os.Remove(f); err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
		return nil
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	return os.WriteFile(f, []byte(launchAgentPlist(id, *exe)), 0o644)
}

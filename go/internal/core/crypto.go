package core

// crypto.rs: AES-256-GCM over what Hover seals, framed nonce ‖ ciphertext ‖ tag (12 + n +
// 16 bytes, no associated data). The key is 32 random bytes kept in note.key, wrapped by
// the platform's KeyGuard: DPAPI (current user) on Windows, a 0600 file elsewhere (the
// Secret Service and the Keychain come with phases 6 and 7).

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

const (
	NonceSize = 12
	TagSize   = 16
)

// KeyError is why a stored key couldn't be read, and whether it can be later.
type KeyError struct {
	Reason string
	// Transient: it may read next time (the keyring isn't running or stayed locked). The
	// file is left alone and nothing is sealed this run.
	Transient bool
}

func (e *KeyError) Error() string { return e.Reason }

func KeyNever(reason string) *KeyError  { return &KeyError{reason, false} }
func KeyNotNow(reason string) *KeyError { return &KeyError{reason, true} }

// KeyGuard is how note.key keeps the key from anyone else: what the file holds for a key,
// and the key back from what the file holds.
type KeyGuard interface {
	Wrap(key []byte) ([]byte, error)
	Unwrap(stored []byte) ([]byte, *KeyError)
	// Inherited is the key an earlier build left in the platform's own store, for a
	// history that has no note.key beside it. nil, nil when there is none; a transient
	// error when the store can't be asked now.
	Inherited() ([]byte, *KeyError)
}

type Crypto struct{ aead cipher.AEAD }

func CryptoWithKey(key [32]byte) *Crypto {
	block, err := aes.NewCipher(key[:])
	if err != nil {
		panic(err) // a 32-byte key always makes AES-256
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		panic(err)
	}
	return &Crypto{aead}
}

// LoadOrCreateCrypto is LoadOrCreateKey, except that a key is never destroyed: the stored
// key when it unwraps to 32 bytes. One that can never be read (DPAPI refuses, the keyring
// item is gone, the file is foreign) is moved aside as note.key.unreadable-<yyyyMMddHHmmss>
// for recovery by hand, and a new key made. One that can't be read now is left as it is,
// and there is no key this run: nil, so nothing is sealed that the next run couldn't open.
// nil too when a new key can't be stored.
func LoadOrCreateCrypto(file string, guard KeyGuard) *Crypto {
	if _, err := os.Stat(file); err == nil {
		var plain []byte
		var kerr *KeyError
		if stored, err := ReadFile(file); err != nil {
			kerr = KeyNotNow(err.Error())
		} else {
			plain, kerr = guard.Unwrap(stored)
		}
		switch {
		case kerr == nil && len(plain) == 32:
			return CryptoWithKey([32]byte(plain))
		case kerr == nil:
			if !setAside(file, "it doesn't hold a 32-byte key") {
				return nil
			}
		case kerr.Transient:
			Logf("key unwrap failed — %s; no history this run, trying again next start", kerr.Reason)
			return nil
		default:
			if !setAside(file, kerr.Reason) {
				return nil
			}
		}
	}
	// A history with no note.key beside it was sealed by a key an earlier build kept
	// elsewhere: a new one would leave that history unreadable (and never destroy it).
	var key [32]byte
	_, statErr := os.Stat(filepath.Join(filepath.Dir(file), "agents", "index.dat"))
	var inherited []byte
	var kerr *KeyError
	if statErr == nil {
		inherited, kerr = guard.Inherited()
	}
	switch {
	case kerr != nil:
		Logf("key lookup failed — %s; no history this run, trying again next start", kerr.Reason)
		return nil
	case len(inherited) == 32:
		copy(key[:], inherited)
		Logf("note.key is missing; carrying on with the key the history was sealed with")
	default:
		if _, err := rand.Read(key[:]); err != nil {
			panic("the system has no randomness")
		}
	}
	wrapped, err := guard.Wrap(key[:])
	if err == nil {
		err = writePrivate(file, wrapped)
	}
	if err != nil {
		Logf("key write failed — %v; no history this run", err)
		return nil
	}
	return CryptoWithKey(key)
}

// Seal is Crypto.Seal: the text's UTF-8 under a fresh nonce.
func (c *Crypto) Seal(text string) []byte {
	var nonce [NonceSize]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		panic("the system has no randomness")
	}
	return c.SealWith(text, nonce)
}

// SealWith seals under a given nonce. GCM's Seal appends ciphertext ‖ tag to the nonce:
// the framing Hover writes.
func (c *Crypto) SealWith(text string, nonce [NonceSize]byte) []byte {
	out := make([]byte, NonceSize, NonceSize+len(text)+TagSize)
	copy(out, nonce[:])
	return c.aead.Seal(out, nonce[:], []byte(text), nil)
}

// Open is Crypto.Open: the text, or "" when the data is missing, short or not this key's.
func (c *Crypto) Open(data []byte) string {
	if len(data) < NonceSize+TagSize {
		return ""
	}
	plain, err := c.aead.Open(nil, data[:NonceSize], data[NonceSize:], nil)
	if err != nil {
		Logf("unseal failed — The computed authentication tag did not match the input authentication tag.")
		return ""
	}
	// Encoding.UTF8.GetString: bad sequences become U+FFFD.
	return Lossy(plain)
}

// setAside moves an unreadable key out of the way (the planner's naming), or gives up
// (false) when it can't: a key is never overwritten.
func setAside(file, why string) bool {
	to := file + ".unreadable-" + strings.ReplaceAll(LocalCompact(), "-", "")
	if err := Rename(file, to); err != nil {
		Logf("key unwrap failed — %s; couldn't set it aside (%v), no history this run", why, err)
		return false
	}
	Logf("key unwrap failed — %s; kept as %s, a new key made", why, to)
	return true
}

var global struct {
	sync.Mutex
	set    bool
	crypto *Crypto
}

// GlobalCrypto is Hover's key, loaded or made on first use (the C# static field); nil when
// this run has none (see LoadOrCreateCrypto), and then the history is off.
func GlobalCrypto() *Crypto {
	global.Lock()
	defer global.Unlock()
	if !global.set {
		global.crypto, global.set = LoadOrCreateCrypto(KeyFile(), SystemKeyGuard{}), true
	}
	return global.crypto
}

// UseHostKey is Crypto.InitializeKey: the key a host supplies (the Mac app's, from its
// Keychain), used in place of note.key for the whole run. Once, 32 bytes, and before
// anything asked for the key: a repeated or late call, or a key of another length, is
// refused and changes nothing.
func UseHostKey(key []byte) error {
	global.Lock()
	defer global.Unlock()
	if len(key) != 32 || global.set {
		return errors.New("Invalid or repeated history key initialization.")
	}
	global.crypto, global.set = CryptoWithKey([32]byte(key)), true
	return nil
}

// GUIDN is Guid.NewGuid().ToString("N"): a version-4 GUID as 32 lower-case hex digits.
func GUIDN() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic("the system has no randomness")
	}
	b[6] = b[6]&0x0F | 0x40
	b[8] = b[8]&0x3F | 0x80
	return fmt.Sprintf("%x", b)
}

// WritePrivate writes a file only its owner can read.
func WritePrivate(file string, b []byte) error { return writePrivate(file, b) }

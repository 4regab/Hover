//go:build darwin

package core

import (
	"errors"
	"fmt"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
)

// Security.framework's generic passwords in the login keychain, called through purego (no
// C compiler; a secret on a `security` command line shows in ps). The same calls the Rust
// build makes through security-framework 3.7: SecItemAdd, and SecItemUpdate when the item
// is already there; SecItemCopyMatching to read. So the Rust and the Go build see each
// other's items.

const (
	cfStringEncodingUTF8 = 0x08000100
	errSecItemNotFound   = -25300
	errSecDuplicateItem  = -25299
)

var kc struct {
	once sync.Once
	err  error

	stringCreate  func(alloc uintptr, b *byte, n int, enc uint32, external bool) uintptr
	stringGetCStr func(s uintptr, buf *byte, size int, enc uint32) bool
	dataCreate    func(alloc uintptr, b *byte, n int) uintptr
	dataLength    func(d uintptr) int
	dataBytes     func(d uintptr) unsafe.Pointer
	dictCreate    func(alloc uintptr, keys, values *uintptr, n int, keyCB, valueCB uintptr) uintptr
	release       func(o uintptr)

	copyMatching func(query uintptr, result *uintptr) int32
	add          func(attrs uintptr, result uintptr) int32
	update       func(query, attrs uintptr) int32
	errMessage   func(status int32, reserved uintptr) uintptr

	// The framework's constants: kCFTypeDictionary…CallBacks are structs (their address is
	// what is passed); the rest are variables holding a reference.
	keyCB, valueCB                                       uintptr
	yes, class, generic, service, account, data, retData uintptr
	matchLimit, limitOne                                 uintptr
}

// cfGlobal is the reference a framework variable holds: Dlsym gives the variable's address.
func cfGlobal(h uintptr, name string) (uintptr, error) {
	p, err := purego.Dlsym(h, name)
	if err != nil {
		return 0, err
	}
	return *(*uintptr)(*(*unsafe.Pointer)(unsafe.Pointer(&p))), nil
}

func loadKeychain() error {
	kc.once.Do(func() {
		cf, err := purego.Dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			kc.err = fmt.Errorf("CoreFoundation: %w", err)
			return
		}
		sec, err := purego.Dlopen("/System/Library/Frameworks/Security.framework/Security", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			kc.err = fmt.Errorf("Security: %w", err)
			return
		}
		purego.RegisterLibFunc(&kc.stringCreate, cf, "CFStringCreateWithBytes")
		purego.RegisterLibFunc(&kc.stringGetCStr, cf, "CFStringGetCString")
		purego.RegisterLibFunc(&kc.dataCreate, cf, "CFDataCreate")
		purego.RegisterLibFunc(&kc.dataLength, cf, "CFDataGetLength")
		purego.RegisterLibFunc(&kc.dataBytes, cf, "CFDataGetBytePtr")
		purego.RegisterLibFunc(&kc.dictCreate, cf, "CFDictionaryCreate")
		purego.RegisterLibFunc(&kc.release, cf, "CFRelease")
		purego.RegisterLibFunc(&kc.copyMatching, sec, "SecItemCopyMatching")
		purego.RegisterLibFunc(&kc.add, sec, "SecItemAdd")
		purego.RegisterLibFunc(&kc.update, sec, "SecItemUpdate")
		purego.RegisterLibFunc(&kc.errMessage, sec, "SecCopyErrorMessageString")
		if kc.keyCB, err = purego.Dlsym(cf, "kCFTypeDictionaryKeyCallBacks"); err != nil {
			kc.err = err
			return
		}
		if kc.valueCB, err = purego.Dlsym(cf, "kCFTypeDictionaryValueCallBacks"); err != nil {
			kc.err = err
			return
		}
		for _, g := range []struct {
			into *uintptr
			h    uintptr
			name string
		}{
			{&kc.yes, cf, "kCFBooleanTrue"},
			{&kc.class, sec, "kSecClass"}, {&kc.generic, sec, "kSecClassGenericPassword"},
			{&kc.service, sec, "kSecAttrService"}, {&kc.account, sec, "kSecAttrAccount"},
			{&kc.data, sec, "kSecValueData"}, {&kc.retData, sec, "kSecReturnData"},
			{&kc.matchLimit, sec, "kSecMatchLimit"}, {&kc.limitOne, sec, "kSecMatchLimitOne"},
		} {
			if *g.into, err = cfGlobal(g.h, g.name); err != nil {
				kc.err = err
				return
			}
		}
	})
	return kc.err
}

// kcDescribe is Keychain status -25308 (User interaction is not allowed.), as the Rust build
// words it.
func kcDescribe(status int32) string {
	s := fmt.Sprintf("Keychain status %d", status)
	if m := kc.errMessage(status, 0); m != 0 {
		buf := make([]byte, 512)
		if kc.stringGetCStr(m, &buf[0], len(buf), cfStringEncodingUTF8) {
			n := 0
			for n < len(buf) && buf[n] != 0 {
				n++
			}
			if n > 0 {
				s += " (" + string(buf[:n]) + ")"
			}
		}
		kc.release(m)
	}
	return s
}

// The extra zero byte is something to point at when the text or data is empty.
func cfString(s string) uintptr {
	b := append([]byte(s), 0)
	return kc.stringCreate(0, &b[0], len(b)-1, cfStringEncodingUTF8, false)
}

func cfData(d []byte) uintptr {
	b := append(append([]byte(nil), d...), 0)
	return kc.dataCreate(0, &b[0], len(d))
}

// cfDict is a dictionary of key, value, key, value… The dictionary keeps what it is given,
// so the caller still lets go of its own references.
func cfDict(kv ...uintptr) uintptr {
	n := len(kv) / 2
	keys, values := make([]uintptr, n), make([]uintptr, n)
	for i := 0; i < n; i++ {
		keys[i], values[i] = kv[2*i], kv[2*i+1]
	}
	return kc.dictCreate(0, &keys[0], &values[0], n, kc.keyCB, kc.valueCB)
}

// Login is the login keychain.
type Login struct{}

func (Login) Set(service, account string, secret []byte) error {
	if err := loadKeychain(); err != nil {
		return err
	}
	svc, acct, data := cfString(service), cfString(account), cfData(secret)
	defer kc.release(svc)
	defer kc.release(acct)
	defer kc.release(data)
	add := cfDict(kc.class, kc.generic, kc.service, svc, kc.account, acct, kc.data, data)
	defer kc.release(add)
	status := kc.add(add, 0)
	if status == errSecDuplicateItem {
		query := cfDict(kc.class, kc.generic, kc.service, svc, kc.account, acct)
		defer kc.release(query)
		update := cfDict(kc.data, data)
		defer kc.release(update)
		status = kc.update(query, update)
	}
	if status != 0 {
		return errors.New(kcDescribe(status))
	}
	return nil
}

// kcFind is the first generic password of a service (and an account, when given): nil, nil
// when there is none.
func kcFind(service, account string) ([]byte, error) {
	if err := loadKeychain(); err != nil {
		return nil, err
	}
	svc := cfString(service)
	defer kc.release(svc)
	pairs := []uintptr{kc.class, kc.generic, kc.service, svc, kc.retData, kc.yes}
	if account != "" {
		acct := cfString(account)
		defer kc.release(acct)
		pairs = append(pairs, kc.account, acct)
	} else {
		pairs = append(pairs, kc.matchLimit, kc.limitOne)
	}
	query := cfDict(pairs...)
	defer kc.release(query)
	var out uintptr
	switch status := kc.copyMatching(query, &out); status {
	case 0:
	case errSecItemNotFound:
		return nil, nil
	default:
		return nil, errors.New(kcDescribe(status))
	}
	if out == 0 {
		return nil, errors.New("Keychain gave no data")
	}
	defer kc.release(out)
	n := kc.dataLength(out)
	b := make([]byte, n)
	if n > 0 {
		copy(b, unsafe.Slice((*byte)(kc.dataBytes(out)), n))
	}
	return b, nil
}

func (Login) Get(service, account string) ([]byte, error) { return kcFind(service, account) }

// KeychainFind is a generic password by service alone, whatever its account: another
// program's item (Claude Code's sign-in), which the user is asked to allow. Read-only.
// nil, nil when there is no such item.
func KeychainFind(service string) ([]byte, error) { return kcFind(service, "") }

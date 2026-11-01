package core

import (
	"bytes"
	"testing"
)

// A host with no key this run pins "no key": nothing loads or makes note.key afterwards, and
// a key sent later is refused. This test owns the process's key, as the host-key test does.
func TestAHostWithNoKeyKeepsTheHistoryOff(t *testing.T) {
	reset := func() {
		global.Lock()
		global.set, global.crypto = false, nil
		global.Unlock()
	}
	reset()
	t.Cleanup(reset)
	if err := UseNoHostKey(); err != nil {
		t.Fatal(err)
	}
	if GlobalCrypto() != nil {
		t.Fatal("a key appeared")
	}
	if UseHostKey(bytes.Repeat([]byte{7}, 32)) == nil || UseNoHostKey() == nil {
		t.Fatal("a second key decision was taken")
	}
}

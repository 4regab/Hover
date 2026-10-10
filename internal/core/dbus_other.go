//go:build !windows && !linux

package core

import "errors"

// The Mac keeps the key in the Keychain (phase 7); until then it has no Secret Service and
// no settings portal.
func storeSecret(bus, id string, key []byte) error {
	return errors.New("no Secret Service on this system")
}

func findSecret(bus, id string) ([]byte, error) {
	return nil, errors.New("the key is in the Secret Service, which this system doesn't have")
}

func portalLook(bus string) (*uint32, *bool, bool) { return nil, nil, false }

func watchPortal(bus string, changed func()) bool { return false }

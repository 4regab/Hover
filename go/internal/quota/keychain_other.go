//go:build !darwin

package quota

// claudeKeychain: only a Mac keeps Claude Code's sign-in in the Keychain; elsewhere it is
// the file.
func claudeKeychain() (*string, error) { return nil, nil }

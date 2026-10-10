//go:build darwin

package quota

import "github.com/4regab/Hover/internal/core"

// claudeKeychain is Claude Code's sign-in from the login Keychain (the user is asked to
// allow it once). Nil where there is no such item. Read-only.
func claudeKeychain() (*string, error) {
	b, err := core.KeychainFind(ClaudeKeychainService)
	if err != nil || b == nil {
		return nil, err
	}
	text := core.TextOf(b)
	return &text, nil
}

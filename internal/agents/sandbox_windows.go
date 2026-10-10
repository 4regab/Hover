package agents

import "os"

// PrivateDir: Windows has no sandbox, and a folder made in the user's profile is theirs.
func PrivateDir(dir string) error { return os.MkdirAll(dir, 0o777) }

func writePrivate(file, text string) error { return os.WriteFile(file, []byte(text), 0o666) }

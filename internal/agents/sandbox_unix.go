//go:build unix

package agents

import (
	"fmt"
	"os"
	"syscall"
)

// PrivateDir: the folder exists, is the user's own and no link, and only they can enter it.
func PrivateDir(dir string) error {
	if err := os.MkdirAll(dir, 0o777); err != nil {
		return err
	}
	fi, err := os.Lstat(dir)
	if err != nil {
		return err
	}
	st, ok := fi.Sys().(*syscall.Stat_t)
	if !fi.IsDir() || !ok || int(st.Uid) != os.Geteuid() {
		return fmt.Errorf("%s isn’t a folder of the user’s own", dir)
	}
	return os.Chmod(dir, 0o700)
}

func writePrivate(file, text string) error {
	if err := os.WriteFile(file, []byte(text), 0o666); err != nil {
		return err
	}
	return os.Chmod(file, 0o600)
}

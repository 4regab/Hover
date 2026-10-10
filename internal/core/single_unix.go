//go:build !windows

package core

import (
	"bufio"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
)

// runtimeDir is where the lock and the socket live, for this user alone: $XDG_RUNTIME_DIR
// (0700 by the spec) where there is one; on macOS, which has none, $TMPDIR (a per-user
// 0700 folder) when the socket's path fits in it; else a 0700 folder of this user's in
// /tmp.
func runtimeDir(sock string) (string, error) {
	if d := os.Getenv("XDG_RUNTIME_DIR"); filepath.IsAbs(d) && isDir(d) {
		return d, nil
	}
	uid := os.Getuid()
	mine := func(d string) bool {
		st, err := os.Stat(d)
		if err != nil || !st.IsDir() || st.Mode().Perm()&0o077 != 0 {
			return false
		}
		s, ok := st.Sys().(*syscall.Stat_t)
		return ok && int(s.Uid) == uid
	}
	if runtime.GOOS == "darwin" {
		if d := os.Getenv("TMPDIR"); filepath.IsAbs(d) && mine(d) && socketFits(d, sock) {
			return d, nil
		}
	}
	base := os.TempDir()
	if runtime.GOOS == "darwin" {
		base = "/tmp"
	}
	d := filepath.Join(base, fmt.Sprintf("hover-%d", uid))
	if err := os.MkdirAll(d, 0o700); err != nil {
		return "", err
	}
	if err := os.Chmod(d, 0o700); err != nil {
		return "", err
	}
	// A folder someone else made first is not ours to put a lock in.
	if !mine(d) {
		return "", fmt.Errorf("%s belongs to someone else", d)
	}
	return d, nil
}

func claim(name string, onShow func(*string)) (*Instance, error) {
	low := strings.ToLower(name)
	dir, err := runtimeDir(low + ".sock")
	if err != nil {
		return nil, err
	}
	lock, err := os.OpenFile(filepath.Join(dir, low+".lock"), os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return nil, err
	}
	sock := filepath.Join(dir, low+".sock")
	// flock, as Rust's File::try_lock takes it, so the two builds see each other's lock.
	if err := syscall.Flock(int(lock.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		lock.Close()
		if !errors.Is(err, syscall.EWOULDBLOCK) {
			return nil, err
		}
		// The token lets the running copy's window take focus on Wayland (and startup
		// notification on X11): this launch's right, passed on.
		token := os.Getenv("XDG_ACTIVATION_TOKEN")
		if token == "" {
			token = os.Getenv("DESKTOP_STARTUP_ID")
		}
		if c, err := net.Dial("unix", sock); err == nil {
			fmt.Fprintf(c, "show %s\n", token)
			c.Close()
		}
		return nil, nil
	}
	// Held: any socket left there is a crashed copy's.
	os.Remove(sock)
	l, err := net.Listen("unix", sock)
	if err != nil {
		lock.Close()
		return nil, err
	}
	go func() {
		for {
			c, err := l.Accept()
			if err != nil {
				return
			}
			line, _ := bufio.NewReader(c).ReadString('\n')
			c.Close()
			if rest, ok := strings.CutPrefix(strings.TrimRight(line, "\r\n"), "show"); ok {
				var token *string
				if t := strings.TrimSpace(rest); t != "" {
					token = &t
				}
				onShow(token)
			}
		}
	}()
	return &Instance{release: func() {
		l.Close()
		os.Remove(sock)
		lock.Close()
	}}, nil
}

//go:build unix

package core

import (
	"os/signal"
	"syscall"
)

func init() {
	// Go turns EPIPE on stdout or stderr into a fatal SIGPIPE (os/signal: a write
	// on fd 1 or 2 kills the process unless SIGPIPE is ignored). Hover's stdout is
	// the host's pipe and stderr is the log. A task logs and writes state the whole
	// time it runs; the reader of either pipe going away must not take the process
	// down. The Mac UI then quits because the backend process ended.
	signal.Ignore(syscall.SIGPIPE)
}

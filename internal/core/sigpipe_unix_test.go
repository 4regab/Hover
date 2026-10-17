//go:build unix

package core

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"syscall"
	"testing"
)

func TestClosedStderrDoesNotKillTheProcess(t *testing.T) {
	if os.Getenv("HOVER_EPIPE_CHILD") == "1" {
		// fd 2 is a pipe whose read end is already closed.
		fmt.Fprintln(os.Stderr, "still here")
		Logf("still logging")
		os.Exit(0)
	}
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	r.Close()
	cmd := exec.Command(os.Args[0], "-test.run=^TestClosedStderrDoesNotKillTheProcess$")
	cmd.Env = append(os.Environ(), "HOVER_EPIPE_CHILD=1")
	cmd.Stderr = w
	cmd.Stdout = io.Discard
	err = cmd.Run()
	w.Close()
	if err == nil {
		return
	}
	ee, ok := err.(*exec.ExitError)
	if !ok {
		t.Fatal(err)
	}
	ws, ok := ee.Sys().(syscall.WaitStatus)
	if ok && ws.Signaled() && ws.Signal() == syscall.SIGPIPE {
		t.Fatal("a write to a closed stderr killed the process with SIGPIPE")
	}
	t.Fatal(err)
}

//go:build unix

package agents

import (
	"os"
	"os/exec"
	"runtime"
	"strconv"
	"sync"
	"syscall"

	"github.com/4regab/Hover/go/internal/core"
)

func hideWindow(*exec.Cmd) {}

// batCommand: only Windows has batch files (isShim is false elsewhere).
func batCommand(script string, args []string) *exec.Cmd { return exec.Command(script, args...) }

func prepare(cmd *exec.Cmd) {
	if cmd.SysProcAttr == nil {
		cmd.SysProcAttr = &syscall.SysProcAttr{}
	}
	// A group of its own: a kill of the group reaches whatever the tool starts, and
	// Ctrl+C in Hover's terminal doesn't.
	cmd.SysProcAttr.Setsid = true
	// Linux only: macOS has no PDEATHSIG, and there the group's watchdog (attach) is
	// what ends the tool when Hover goes. Go kills the child itself should Hover be
	// gone by the time the prctl ran.
	deathSignal(cmd.SysProcAttr)
}

type spawnJob struct {
	cmd  *exec.Cmd
	back chan error
}

var (
	spawnerOnce sync.Once
	spawner     chan spawnJob
)

// spawn starts every child from one thread that lives as long as Hover: PR_SET_PDEATHSIG
// fires when the thread that forked ends, not the process, so a tool started from a
// thread that went would die with it.
func spawn(cmd *exec.Cmd) error {
	spawnerOnce.Do(func() {
		spawner = make(chan spawnJob)
		go func() {
			runtime.LockOSThread() // never unlocked: the thread stays this goroutine's
			for j := range spawner {
				j.back <- j.cmd.Start()
			}
		}()
	})
	back := make(chan error, 1)
	spawner <- spawnJob{cmd, back}
	return <-back
}

type groupImp struct {
	pgid     int
	mu       sync.Mutex
	watchdog *exec.Cmd
	feed     *os.File
}

func attach(pid int) (*groupImp, error) {
	g := &groupImp{pgid: pid}
	// Reads until Hover's end closes (Hover exited, however), then kills the group. In
	// a session of its own too, so a terminal's signals miss it.
	w := exec.Command("/bin/sh", "-c", `read _; kill -KILL -- -"$0" 2>/dev/null`, strconv.Itoa(pid))
	w.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	r, feed, err := os.Pipe()
	if err == nil {
		w.Stdin = r
		err = w.Start()
		r.Close()
		if err != nil {
			feed.Close()
		}
	}
	if err != nil {
		core.Logf("child group: no watchdog - %v", err)
	} else {
		g.watchdog, g.feed = w, feed
	}
	return g, nil
}

// dropWatchdog kills the watchdog before its stdin closes, so it never kills the group.
func (g *groupImp) dropWatchdog() {
	g.mu.Lock()
	w, feed := g.watchdog, g.feed
	g.watchdog, g.feed = nil, nil
	g.mu.Unlock()
	if w != nil {
		w.Process.Kill()
		feed.Close()
		go w.Wait()
	}
}

func (g *groupImp) kill() {
	// The watchdog first, so its kill can never land on a reused group id.
	g.dropWatchdog()
	syscall.Kill(-g.pgid, syscall.SIGKILL)
}

func (g *groupImp) release() { g.dropWatchdog() }

func (g *groupImp) close() {}

// detach puts a program Hover lets go of (an editor) in a process group of its own, so
// Hover's signals and the agents' stops never reach it.
func detach(cmd *exec.Cmd) {
	if cmd.SysProcAttr == nil {
		cmd.SysProcAttr = &syscall.SysProcAttr{}
	}
	cmd.SysProcAttr.Setpgid = true
}

//go:build linux

package audio

import (
	"errors"
	"os"
	"os/exec"
	"sync"

	"golang.org/x/sys/unix"
)

// PipeWire's own player, pw-cat, fed raw 16-bit stereo at 44.1 kHz on its standard input:
// no PulseAudio (legacy, not a target) and no C compiler. The pipe is kept small (8 KB, about
// 45 ms), so a fade the music asks for is heard within that, not a pipe's worth later.
//
// ponytail: a program in the way; libpipewire through a binding is the upgrade, if a machine
// has PipeWire without pw-cat (its package is pipewire-bin, pipewire-utils or pipewire-tools).

const bufFrames = Rate / 20

type stream struct {
	cmd  *exec.Cmd
	w    *os.File
	fill func([]float32)
	stop chan struct{}
	done sync.WaitGroup
	once sync.Once
}

func open(fill func([]float32)) (Output, error) {
	path, err := exec.LookPath("pw-cat")
	if err != nil {
		return nil, errors.New("PipeWire's pw-cat isn't installed")
	}
	r, w, err := os.Pipe()
	if err != nil {
		return nil, err
	}
	_, _ = unix.FcntlInt(w.Fd(), unix.F_SETPIPE_SZ, 8192)
	cmd := exec.Command(path, "--playback", "--format", "s16", "--rate", "44100", "--channels", "2",
		"--latency", "100ms", "--media-role", "Music", "-")
	cmd.Stdin = r
	if err := cmd.Start(); err != nil {
		r.Close()
		w.Close()
		return nil, err
	}
	r.Close()
	s := &stream{cmd: cmd, w: w, fill: fill, stop: make(chan struct{})}
	s.done.Add(1)
	go s.run()
	return s, nil
}

// run writes a buffer whenever the pipe has room: the write blocks until pw-cat has taken
// the last, which is what paces it.
func (s *stream) run() {
	defer s.done.Done()
	f := make([]float32, bufFrames*Channels)
	b := make([]byte, len(f)*2)
	for {
		select {
		case <-s.stop:
			return
		default:
		}
		s.fill(f)
		for i, v := range f {
			n := int16(max(-1, min(1, v)) * 32767)
			b[2*i], b[2*i+1] = byte(n), byte(n>>8)
		}
		if _, err := s.w.Write(b); err != nil {
			return
		}
	}
}

func (s *stream) Close() {
	s.once.Do(func() {
		close(s.stop)
		// Ends the write the loop may be blocked in, and the player.
		s.w.Close()
		_ = s.cmd.Process.Kill()
		s.done.Wait()
		_ = s.cmd.Wait()
	})
}

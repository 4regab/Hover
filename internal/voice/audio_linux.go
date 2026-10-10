//go:build linux

package voice

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os/exec"
	"strconv"
	"sync"
	"sync/atomic"
)

// PipeWire's own recorder, pw-record, asked for 16 kHz mono 16-bit (PipeWire converts the
// rate) and read from its standard output in 50 ms pieces. No PulseAudio (legacy, not a
// target) and no C compiler.
//
// ponytail: a program in the way; libpipewire through a binding is the upgrade.

const chunkSamples = Rate / 20

// device is an input PipeWire lists.
type device struct{ name, description string }

// inputs are the microphones (Audio/Source nodes; the speakers' monitors are not), by
// `pw-dump`.
func inputs() ([]device, error) {
	path, err := exec.LookPath("pw-dump")
	if err != nil {
		return nil, errors.New("PipeWire's pw-dump isn't installed")
	}
	out, err := exec.Command(path).Output()
	if err != nil {
		return nil, err
	}
	var objs []struct {
		Type string `json:"type"`
		Info struct {
			Props map[string]any `json:"props"`
		} `json:"info"`
	}
	if err := json.Unmarshal(out, &objs); err != nil {
		return nil, err
	}
	var res []device
	for _, o := range objs {
		if o.Type != "PipeWire:Interface:Node" {
			continue
		}
		p := o.Info.Props
		// A microphone, or a virtual source (a loopback, a noise filter).
		if class, _ := p["media.class"].(string); class != "Audio/Source" && class != "Audio/Source/Virtual" {
			continue
		}
		name, _ := p["node.name"].(string)
		desc, _ := p["node.description"].(string)
		if desc == "" {
			desc = name
		}
		if name != "" {
			res = append(res, device{name, desc})
		}
	}
	return res, nil
}

// Microphones are the input devices' names, for Settings (the system's default isn't listed).
func Microphones() []string {
	devs, _ := inputs()
	var out []string
	for _, d := range devs {
		dup := false
		for _, o := range out {
			dup = dup || o == d.description
		}
		if !dup {
			out = append(out, d.description)
		}
	}
	return out
}

type mic struct {
	cmd    *exec.Cmd
	mu     sync.Mutex
	buf    []int16
	max    int
	level  atomic.Uint32
	failed atomic.Pointer[string]
	done   sync.WaitGroup
	closed atomic.Bool
}

// OpenMic is the real microphone: the device by name ("" the default).
func OpenMic(device string, max int) (Source, error) {
	path, err := exec.LookPath("pw-record")
	if err != nil {
		return nil, errors.New("PipeWire’s pw-record isn’t installed. Install PipeWire’s tools (pipewire-utils or pipewire-bin).")
	}
	args := []string{"--format", "s16", "--rate", strconv.Itoa(Rate), "--channels", "1", "--latency", "50ms"}
	if device != "" {
		devs, _ := inputs()
		target := ""
		for _, d := range devs {
			if d.description == device || d.name == device {
				target = d.name
				break
			}
		}
		// The one picked, or an error: recording another microphone without saying so isn't on.
		if target == "" {
			return nil, fmt.Errorf("The microphone “%s” isn’t connected. Pick another in Settings → Voice.", device)
		}
		args = append(args, "--target", target)
	}
	cmd := exec.Command(path, append(args, "-")...)
	out, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, errors.New("The microphone failed to start: " + err.Error())
	}
	m := &mic{cmd: cmd, max: max}
	m.done.Add(1)
	go m.run(out)
	return m, nil
}

func (m *mic) run(r io.Reader) {
	defer m.done.Done()
	b := make([]byte, chunkSamples*2)
	chunk := make([]int16, chunkSamples)
	for {
		n, err := io.ReadFull(r, b)
		if n >= 2 {
			k := n / 2
			for i := 0; i < k; i++ {
				chunk[i] = int16(uint16(b[2*i]) | uint16(b[2*i+1])<<8)
			}
			m.level.Store(floatBits(levelOf16(chunk[:k])))
			m.mu.Lock()
			if room := m.max - len(m.buf); room > 0 {
				m.buf = append(m.buf, chunk[:min(k, room)]...)
			}
			m.mu.Unlock()
		}
		if err != nil {
			if !m.closed.Load() {
				msg := "The microphone isn’t available any more."
				m.failed.Store(&msg)
			}
			return
		}
	}
}

func (m *mic) Level() float32 { return floatFromBits(m.level.Load()) }
func (m *mic) Samples() int   { m.mu.Lock(); defer m.mu.Unlock(); return len(m.buf) }
func (m *mic) Failed() string {
	if p := m.failed.Load(); p != nil {
		return *p
	}
	return ""
}
func (m *mic) Since(from int) []int16 {
	m.mu.Lock()
	defer m.mu.Unlock()
	if from > len(m.buf) {
		return nil
	}
	return append([]int16(nil), m.buf[from:]...)
}

// Finish lets the device go before anything else happens, and hands the samples over.
func (m *mic) Finish() []int16 {
	m.closed.Store(true)
	_ = m.cmd.Process.Kill()
	m.done.Wait()
	_ = m.cmd.Wait()
	m.mu.Lock()
	defer m.mu.Unlock()
	out := m.buf
	m.buf = nil
	return out
}

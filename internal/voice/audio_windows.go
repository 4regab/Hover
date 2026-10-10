//go:build windows

package voice

import (
	"errors"
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"unsafe"

	"golang.org/x/sys/windows"
)

// winmm's waveIn: 16 kHz mono 16-bit asked of the device (Windows converts the rate), in
// four buffers of 50 ms, taken as the device fills each (a wait on an event). No COM and no
// C compiler.
//
// ponytail: winmm names a device in 31 characters, so a long name is matched by its start;
// WASAPI's full names are the upgrade.
var (
	winmm             = windows.NewLazySystemDLL("winmm.dll")
	pWaveInGetNumDevs = winmm.NewProc("waveInGetNumDevs")
	pWaveInGetDevCaps = winmm.NewProc("waveInGetDevCapsW")
	pWaveInOpen       = winmm.NewProc("waveInOpen")
	pWaveInClose      = winmm.NewProc("waveInClose")
	pWaveInReset      = winmm.NewProc("waveInReset")
	pWaveInStart      = winmm.NewProc("waveInStart")
	pWaveInPrepare    = winmm.NewProc("waveInPrepareHeader")
	pWaveInUnprepare  = winmm.NewProc("waveInUnprepareHeader")
	pWaveInAddBuffer  = winmm.NewProc("waveInAddBuffer")
)

const (
	waveMapper    = ^uintptr(0)
	callbackEvent = 0x00050000
	whdrDone      = 0x1
	inBuffers     = 4
	inBufSamples  = Rate / 20
)

type waveFormat struct {
	Tag, Channels        uint16
	SamplesPerSec, Bytes uint32
	BlockAlign, Bits     uint16
	Size                 uint16
}

type waveHdr struct {
	Data           *byte
	Length, Got    uint32
	User           uintptr
	Flags, Loops   uint32
	Next, Reserved uintptr
}

type waveInCaps struct {
	Mid, Pid uint16
	Version  uint32
	Name     [32]uint16
	Formats  uint32
	Channels uint16
	_        uint16
}

// Microphones are the input devices' names, for Settings (the system's default isn't listed).
func Microphones() []string {
	n, _, _ := pWaveInGetNumDevs.Call()
	var out []string
	for i := uintptr(0); i < n; i++ {
		var c waveInCaps
		if r, _, _ := pWaveInGetDevCaps.Call(i, uintptr(unsafe.Pointer(&c)), unsafe.Sizeof(c)); r != 0 {
			continue
		}
		name := windows.UTF16ToString(c.Name[:])
		dup := false
		for _, o := range out {
			dup = dup || o == name
		}
		if name != "" && !dup {
			out = append(out, name)
		}
	}
	return out
}

type mic struct {
	h      uintptr
	event  windows.Handle
	hdrs   [inBuffers]waveHdr
	data   [inBuffers][]int16
	mu     sync.Mutex
	buf    []int16
	max    int
	level  atomic.Uint32
	failed atomic.Pointer[string]
	stop   chan struct{}
	done   sync.WaitGroup
}

// OpenMic is the real microphone: the device by name ("" the default).
func OpenMic(device string, max int) (Source, error) {
	id := waveMapper
	if device != "" {
		n, _, _ := pWaveInGetNumDevs.Call()
		found := false
		for i := uintptr(0); i < n && !found; i++ {
			var c waveInCaps
			if r, _, _ := pWaveInGetDevCaps.Call(i, uintptr(unsafe.Pointer(&c)), unsafe.Sizeof(c)); r != 0 {
				continue
			}
			if name := windows.UTF16ToString(c.Name[:]); name == device || (len(name) >= 31 && strings.HasPrefix(device, name)) {
				id, found = i, true
			}
		}
		// The one picked, or an error: recording another microphone without saying so isn't on.
		if !found {
			return nil, fmt.Errorf("The microphone “%s” isn’t connected. Pick another in Settings → Voice.", device)
		}
	} else if n, _, _ := pWaveInGetNumDevs.Call(); n == 0 {
		return nil, errors.New("No microphone is connected.")
	}
	ev, err := windows.CreateEvent(nil, 0, 0, nil)
	if err != nil {
		return nil, err
	}
	m := &mic{event: ev, max: max, stop: make(chan struct{})}
	f := waveFormat{Tag: 1, Channels: 1, SamplesPerSec: Rate, Bits: 16}
	f.BlockAlign = f.Channels * f.Bits / 8
	f.Bytes = f.SamplesPerSec * uint32(f.BlockAlign)
	if r, _, _ := pWaveInOpen.Call(uintptr(unsafe.Pointer(&m.h)), id, uintptr(unsafe.Pointer(&f)), uintptr(ev), 0, callbackEvent); r != 0 {
		windows.CloseHandle(ev)
		return nil, micError(r)
	}
	for i := range m.hdrs {
		m.data[i] = make([]int16, inBufSamples)
		m.hdrs[i] = waveHdr{Data: (*byte)(unsafe.Pointer(&m.data[i][0])), Length: uint32(len(m.data[i]) * 2)}
		pWaveInPrepare.Call(m.h, uintptr(unsafe.Pointer(&m.hdrs[i])), unsafe.Sizeof(m.hdrs[i]))
		pWaveInAddBuffer.Call(m.h, uintptr(unsafe.Pointer(&m.hdrs[i])), unsafe.Sizeof(m.hdrs[i]))
	}
	if r, _, _ := pWaveInStart.Call(m.h); r != 0 {
		m.release()
		return nil, micError(r)
	}
	m.done.Add(1)
	go m.run()
	return m, nil
}

func micError(code uintptr) error {
	switch code {
	case 4: // MMSYSERR_ALLOCATED
		return errors.New("Another app is using the microphone.")
	case 2, 6, 7: // BADDEVICEID, NODRIVER, NOMEM
		return errors.New("The microphone isn’t available any more.")
	}
	return fmt.Errorf("The microphone failed: error %d", code)
}

func (m *mic) run() {
	defer m.done.Done()
	for {
		select {
		case <-m.stop:
			return
		default:
		}
		windows.WaitForSingleObject(m.event, 200)
		for i := range m.hdrs {
			h := &m.hdrs[i]
			if h.Flags&whdrDone == 0 {
				continue
			}
			n := int(h.Got) / 2
			chunk := m.data[i][:n]
			m.level.Store(floatBits(levelOf16(chunk)))
			m.mu.Lock()
			if room := m.max - len(m.buf); room > 0 {
				m.buf = append(m.buf, chunk[:min(n, room)]...)
			}
			m.mu.Unlock()
			h.Flags &^= whdrDone
			select {
			case <-m.stop:
				return
			default:
				pWaveInAddBuffer.Call(m.h, uintptr(unsafe.Pointer(h)), unsafe.Sizeof(*h))
			}
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

func (m *mic) release() {
	pWaveInReset.Call(m.h)
	for i := range m.hdrs {
		pWaveInUnprepare.Call(m.h, uintptr(unsafe.Pointer(&m.hdrs[i])), unsafe.Sizeof(m.hdrs[i]))
	}
	pWaveInClose.Call(m.h)
	windows.CloseHandle(m.event)
}

// Finish lets the device go before anything else happens, and hands the samples over.
func (m *mic) Finish() []int16 {
	close(m.stop)
	windows.SetEvent(m.event)
	m.done.Wait()
	m.release()
	m.mu.Lock()
	defer m.mu.Unlock()
	out := m.buf
	m.buf = nil
	return out
}

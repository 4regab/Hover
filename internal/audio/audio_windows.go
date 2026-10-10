//go:build windows

package audio

import (
	"errors"
	"fmt"
	"sync"
	"unsafe"

	"golang.org/x/sys/windows"
)

// winmm's waveOut: 16-bit stereo at 44.1 kHz in four buffers of 100 ms, refilled as the
// device finishes each (a wait on an event). No COM and no C compiler.
var (
	winmm             = windows.NewLazySystemDLL("winmm.dll")
	pWaveOutOpen      = winmm.NewProc("waveOutOpen")
	pWaveOutClose     = winmm.NewProc("waveOutClose")
	pWaveOutReset     = winmm.NewProc("waveOutReset")
	pWaveOutPrepare   = winmm.NewProc("waveOutPrepareHeader")
	pWaveOutUnprepare = winmm.NewProc("waveOutUnprepareHeader")
	pWaveOutWrite     = winmm.NewProc("waveOutWrite")
)

const (
	waveMapper    = ^uintptr(0)
	callbackEvent = 0x00050000
	whdrDone      = 0x1
	buffers       = 4
	bufFrames     = Rate / 10
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

type stream struct {
	h     uintptr
	event windows.Handle
	hdrs  [buffers]waveHdr
	data  [buffers][]int16
	fill  func([]float32)
	stop  chan struct{}
	done  sync.WaitGroup
}

func open(fill func([]float32)) (Output, error) {
	ev, err := windows.CreateEvent(nil, 0, 0, nil)
	if err != nil {
		return nil, err
	}
	f := waveFormat{Tag: 1, Channels: Channels, SamplesPerSec: Rate, Bits: 16}
	f.BlockAlign = f.Channels * f.Bits / 8
	f.Bytes = f.SamplesPerSec * uint32(f.BlockAlign)
	s := &stream{event: ev, fill: fill, stop: make(chan struct{})}
	if r, _, _ := pWaveOutOpen.Call(uintptr(unsafe.Pointer(&s.h)), waveMapper, uintptr(unsafe.Pointer(&f)), uintptr(ev), 0, callbackEvent); r != 0 {
		windows.CloseHandle(ev)
		return nil, fmt.Errorf("waveOutOpen: error %d", r)
	}
	for i := range s.hdrs {
		s.data[i] = make([]int16, bufFrames*Channels)
		s.hdrs[i] = waveHdr{Data: (*byte)(unsafe.Pointer(&s.data[i][0])), Length: uint32(len(s.data[i]) * 2)}
		if r, _, _ := pWaveOutPrepare.Call(s.h, uintptr(unsafe.Pointer(&s.hdrs[i])), unsafe.Sizeof(s.hdrs[i])); r != 0 {
			s.release()
			return nil, errors.New("waveOutPrepareHeader failed")
		}
		s.queue(i)
	}
	s.done.Add(1)
	go s.run()
	return s, nil
}

// queue fills buffer i and hands it to the device.
func (s *stream) queue(i int) {
	f := make([]float32, len(s.data[i]))
	s.fill(f)
	for k, v := range f {
		s.data[i][k] = int16(max(-1, min(1, v)) * 32767)
	}
	pWaveOutWrite.Call(s.h, uintptr(unsafe.Pointer(&s.hdrs[i])), unsafe.Sizeof(s.hdrs[i]))
}

func (s *stream) run() {
	defer s.done.Done()
	for {
		select {
		case <-s.stop:
			return
		default:
		}
		windows.WaitForSingleObject(s.event, 200)
		for i := range s.hdrs {
			if s.hdrs[i].Flags&whdrDone != 0 {
				s.hdrs[i].Flags &^= whdrDone
				select {
				case <-s.stop:
					return
				default:
					s.queue(i)
				}
			}
		}
	}
}

func (s *stream) release() {
	pWaveOutReset.Call(s.h)
	for i := range s.hdrs {
		pWaveOutUnprepare.Call(s.h, uintptr(unsafe.Pointer(&s.hdrs[i])), unsafe.Sizeof(s.hdrs[i]))
	}
	pWaveOutClose.Call(s.h)
	windows.CloseHandle(s.event)
}

func (s *stream) Close() {
	close(s.stop)
	windows.SetEvent(s.event)
	s.done.Wait()
	s.release()
}

package voice

import (
	"bufio"
	"encoding/binary"
	"os"
	"path/filepath"

	"github.com/4regab/Hover/internal/core"
)

// The one recording on disk: a 16 kHz mono 16-bit PCM WAV in Hover's own temp folder, made
// just before transcription and deleted when closed, on every path. Nothing is kept: a file
// left by a crash is swept the next time voice starts.

// wavDir is Hover's own folder for it, so a sweep only ever touches Hover's files.
func wavDir() string { return filepath.Join(os.TempDir(), "hover-voice") }

// Sweep removes recordings an earlier run left behind (a crash between write and close).
func Sweep() {
	list, _ := os.ReadDir(wavDir())
	for _, e := range list {
		if filepath.Ext(e.Name()) == ".wav" {
			os.Remove(filepath.Join(wavDir(), e.Name()))
		}
	}
}

// TempWav is the file; Close deletes it.
type TempWav struct{ path string }

func (t *TempWav) Path() string { return t.path }
func (t *TempWav) Close()       { os.Remove(t.path) }

// wavSize is the WAV's size for this many samples: the 44-byte header and two bytes each.
func wavSize(samples int) int64 { return 44 + 2*int64(samples) }

// WriteWav writes the samples (16 kHz mono) as a new file. An error leaves nothing behind.
func WriteWav(samples []int16, rate uint32) (*TempWav, error) {
	if err := os.MkdirAll(wavDir(), 0o755); err != nil {
		return nil, err
	}
	f := &TempWav{filepath.Join(wavDir(), core.GUIDN()+".wav")}
	fh, err := os.Create(f.path)
	if err != nil {
		return nil, err
	}
	fail := func(err error) (*TempWav, error) { fh.Close(); f.Close(); return nil, err }
	w := bufio.NewWriter(fh)
	data := uint32(2 * len(samples))
	hdr := []any{[]byte("RIFF"), 36 + data, []byte("WAVEfmt "), uint32(16), uint16(1), uint16(1), rate, rate * 2, uint16(2), uint16(16), []byte("data"), data}
	for _, v := range hdr {
		if err := binary.Write(w, binary.LittleEndian, v); err != nil {
			return fail(err)
		}
	}
	if err := binary.Write(w, binary.LittleEndian, samples); err != nil {
		return fail(err)
	}
	if err := w.Flush(); err != nil {
		return fail(err)
	}
	if err := fh.Sync(); err != nil {
		return fail(err)
	}
	return f, fh.Close()
}

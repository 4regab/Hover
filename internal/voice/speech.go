// Package voice is app/src/voice and speech.rs: press the shortcut, speak, press it again
// (or hold it and let go, in hold mode). The recording becomes text (Groq in the cloud, or
// Phonon on this computer: the user's choice, read once per recording and never switched
// behind their back), is optionally tidied, routed to a registered project or the default
// workspace, and shown as a preview that starts a new chat after three seconds unless it is
// edited or cancelled. One interaction at a time; all of it on worker goroutines, the UI
// only told something changed.
package voice

import (
	"fmt"
	"sync/atomic"
	"time"
)

// Transcript is a transcript and what the engine said about it.
type Transcript struct {
	Text string
	// Language is what the engine reported, when it reports one (Groq's verbose output
	// does; Phonon is English only and says nothing).
	Language string
	// Truncated: the engine said it cut the audio short. Shown for review; never started
	// on its own.
	Truncated bool
	// Audio is how long the audio was, when the engine said.
	Audio time.Duration
	// Took is how long the engine took, start to text (cold start included for Local).
	Took time.Duration
}

// SpeechKind is why there is no transcript. Each is shown as it is; none becomes a prompt.
type SpeechKind int

const (
	// ErrCancelled: the user (or Escape) cancelled it.
	ErrCancelled SpeechKind = iota
	// ErrNotReady: the selected mode isn't set up: no Groq key, or Phonon isn't Ready. The
	// text says what to do (add a key, Download, Repair).
	ErrNotReady
	// ErrBadKey: Groq refused the key.
	ErrBadKey
	// ErrRateLimited: Groq's rate limit; Wait is the seconds to wait when it said.
	ErrRateLimited
	// ErrNetwork: the network, a timeout, a 5xx.
	ErrNetwork
	// ErrEngine: the local helper crashed, its files are missing or broken, or it said it failed.
	ErrEngine
	// ErrUnsupported: too long or the wrong shape for the engine.
	ErrUnsupported
)

type SpeechError struct {
	Kind SpeechKind
	Msg  string
	// Wait is seconds; 0 when Groq did not say.
	Wait uint64
}

func (e *SpeechError) Error() string { return e.Message() }

// Message is what the card shows.
func (e *SpeechError) Message() string {
	switch e.Kind {
	case ErrCancelled:
		return "Cancelled."
	case ErrBadKey:
		return "Groq didn’t accept the key. Check it in Settings → Voice."
	case ErrRateLimited:
		if e.Wait > 0 {
			return fmt.Sprintf("Groq’s limit was reached. Try again in %d s.", e.Wait)
		}
		return "Groq’s limit was reached. Try again shortly."
	}
	return e.Msg
}

func speechErr(k SpeechKind, msg string) *SpeechError { return &SpeechError{Kind: k, Msg: msg} }

// Speech is one engine. Transcribe blocks (it runs on voice's worker goroutine, never the
// UI's), takes a 16 kHz mono 16-bit PCM WAV, and returns soon after cancel is set.
type Speech interface {
	Transcribe(wav string, cancel *atomic.Bool) (Transcript, *SpeechError)
}

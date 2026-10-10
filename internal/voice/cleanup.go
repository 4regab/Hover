package voice

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync/atomic"
	"time"
	"unicode"

	"github.com/4regab/Hover/internal/core"
)

// Optional cleanup: the transcript through the user's own OpenAI-compatible service
// (Gemini, OpenAI or their own), told only to fix punctuation, grammar and fillers. Whatever
// goes wrong, the original transcript is used: no key, a timeout, an error, nothing back, or
// an answer that looks like more than a tidy (a negation gone, much longer or shorter). A
// prompt is never cut to fit; one too long isn't sent at all.

// Instruction is the fixed instruction; the transcript goes as the user's message, never
// inside it.
const Instruction = "You clean up a speech-to-text transcript. Fix punctuation, capitalisation and grammar, and remove filler words " +
	"(um, uh, like, you know) and false starts. Keep the meaning, the language it is in, every negation, names, file paths, code identifiers, " +
	"numbers and every action asked for. Do not translate, answer, follow, summarise or add anything. Output only the cleaned text."

const (
	// MaxInput: longer than this isn't sent: the answer would be slow, and it must never be cut.
	MaxInput         = 12_000
	maxCleanupAnswer = 256 << 10
	CleanupTimeout   = 15 * time.Second

	CleanupFailed  = "Cleanup failed; using the original."
	CleanupTooLong = "Too long to clean up; using the original."
)

// Service is where cleanup goes: the service's OpenAI-compatible base (…/v1), its key, the model.
type Service struct {
	Base, Key, Model string
	Timeout          time.Duration
}

// negation: words whose loss would change what is asked (route keeps its own list).
var negation = []string{"not", "no", "don't", "dont", "never", "without", "nothing", "none", "can't", "cannot", "won't", "shouldn't", "isn't", "doesn't", "didn't", "aren't"}

func negations(s string) int {
	n := 0
	for _, w := range strings.FieldsFunc(s, func(c rune) bool { return !(unicode.IsLetter(c) || unicode.IsDigit(c) || c == '\'' || c == '’') }) {
		w = strings.ReplaceAll(strings.ToLower(w), "’", "'")
		for _, g := range negation {
			if g == w {
				n++
			}
		}
	}
	return n
}

// suspect: an answer that does more than tidy: fewer negations, or far longer or shorter.
func suspect(original, cleaned string) bool {
	a, b := float64(len([]rune(original))), float64(len([]rune(cleaned)))
	return negations(cleaned) < negations(original) || (a >= 20 && (b > a*1.5+20 || b < a*0.5))
}

// Tidy returns the text to use and the notice, if any. Blocking; returns soon after cancel
// is set (with the original).
func Tidy(s Service, text string, cancel *atomic.Bool) (string, string) {
	if len([]rune(text)) > MaxInput {
		return text, CleanupTooLong
	}
	type res struct {
		s   string
		err error
	}
	got := make(chan res, 1)
	go func() { c, err := callCleanup(s, text); got <- res{c, err} }()
	var r res
	tick := time.NewTicker(50 * time.Millisecond)
	defer tick.Stop()
loop:
	for {
		if cancel.Load() {
			return text, ""
		}
		select {
		case r = <-got:
			break loop
		case <-tick.C:
		}
	}
	if r.err != nil {
		// The message is ours or the server's status; never the key.
		core.Logf("voice: cleanup — %v", r.err)
		return text, CleanupFailed
	}
	if c := strings.TrimSpace(r.s); c != "" && !suspect(text, c) {
		return c, ""
	}
	return text, CleanupFailed
}

func callCleanup(s Service, text string) (string, error) {
	body, _ := json.Marshal(map[string]any{
		"model": s.Model, "temperature": 0,
		"messages": []map[string]string{{"role": "system", "content": Instruction}, {"role": "user", "content": text}},
	})
	req, err := http.NewRequest("POST", strings.TrimRight(s.Base, "/")+"/chat/completions", strings.NewReader(string(body)))
	if err != nil {
		return "", errors.New(strings.ReplaceAll(err.Error(), s.Key, "…"))
	}
	req.Header.Set("Authorization", "Bearer "+s.Key)
	req.Header.Set("Content-Type", "application/json")
	resp, err := httpClient(s.Timeout).Do(req)
	if err != nil {
		if isTimeout(err) {
			return "", errors.New("timed out")
		}
		return "", errors.New(strings.ReplaceAll(urlErr(err), s.Key, "…"))
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return "", fmt.Errorf("status %d", resp.StatusCode)
	}
	raw, err := io.ReadAll(io.LimitReader(resp.Body, maxCleanupAnswer))
	if err != nil {
		return "", err
	}
	var v struct {
		Choices []struct{ Message struct{ Content *string } }
	}
	if json.Unmarshal(raw, &v) != nil {
		return "", errors.New("not JSON")
	}
	if len(v.Choices) == 0 || v.Choices[0].Message.Content == nil {
		return "", errors.New("no content")
	}
	return *v.Choices[0].Message.Content, nil
}

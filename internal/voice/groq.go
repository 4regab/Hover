package voice

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/internal/core"
)

// Cloud speech: the recording to Groq's OpenAI-compatible transcription endpoint, with the
// user's own key. No language is sent, so Groq detects it (and mixed speech stays as said);
// verbose_json says which language it heard. The key is sent only as the Authorization
// header and is never logged or put in an error.

// GroqBase is Groq's address.
const GroqBase = "https://api.groq.com/openai/v1"

// maxUpload is Groq's attachment limit (25 MB on the free tier). Ten minutes of 16 kHz mono
// 16-bit is 19.2 MB.
const maxUpload = 25_000_000

// Groq's answers are a few KB; more than this is not an answer.
const maxAnswer = 1 << 20

// Groq is the cloud engine.
type Groq struct {
	Base  string
	key   string
	model string
	// Limit is the whole request's time limit; 0 is a minute plus the upload at 100 KB/s.
	Limit time.Duration
}

func NewGroq(key, model string) *Groq {
	return &Groq{Base: groqBase(), key: key, model: model}
}

// groqBase is Groq's address, or for measuring against a fake Groq on this computer,
// HOVER_GROQ_BASE. Only a plain-HTTP loopback address is taken (http://127.0.0.1:PORT/…),
// so the variable can't send a recording or a key anywhere else; anything other is ignored.
func groqBase() string {
	b := os.Getenv("HOVER_GROQ_BASE")
	if rest, ok := strings.CutPrefix(b, "http://127.0.0.1:"); ok {
		port, _, _ := strings.Cut(rest, "/")
		if n, err := strconv.ParseUint(port, 10, 16); err == nil && n <= 65535 {
			return strings.TrimRight(b, "/")
		}
	}
	return GroqBase
}

// scrub: the key never shows in a message, whatever the server echoes.
func scrub(s, key string) string {
	if key == "" {
		return s
	}
	return strings.ReplaceAll(s, key, "…")
}

// serverMessage is the server's own error text, when it gave one.
func serverMessage(body string) string {
	var v struct {
		Error struct{ Message string }
	}
	if json.Unmarshal([]byte(body), &v) != nil {
		return ""
	}
	r := []rune(v.Error.Message)
	return string(r[:min(len(r), 300)])
}

// httpClient is an HTTP client for Groq and cleanup: a time limit on the whole call, and
// statuses read rather than thrown. Keep-alives are off, as net.rs's has none.
func httpClient(limit time.Duration) *http.Client {
	tr := http.DefaultTransport.(*http.Transport).Clone()
	tr.DisableKeepAlives = true
	return &http.Client{Timeout: limit, Transport: tr}
}

func (g *Groq) Transcribe(wav string, cancel *atomic.Bool) (Transcript, *SpeechError) {
	st, err := os.Stat(wav)
	if err != nil {
		return Transcript{}, speechErr(ErrEngine, "The recording couldn’t be read: "+err.Error())
	}
	size := st.Size()
	if size > maxUpload {
		return Transcript{}, speechErr(ErrUnsupported, fmt.Sprintf("The recording is %.1f MB; Groq takes up to 25 MB.", float64(size)/1e6))
	}
	limit := g.Limit
	if limit == 0 {
		limit = 60*time.Second + time.Duration(size/100_000)*time.Second
	}
	started := time.Now()
	ctx, stop := context.WithCancel(context.Background())
	type res struct {
		t   Transcript
		err *SpeechError
	}
	got := make(chan res, 1)
	// The request on a goroutine of its own, so a cancel returns now rather than when the
	// server answers; its late answer goes nowhere.
	go func() { t, e := g.post(ctx, wav, size, limit); got <- res{t, e} }()
	tick := time.NewTicker(50 * time.Millisecond)
	defer tick.Stop()
	for {
		if cancel.Load() {
			stop()
			return Transcript{}, speechErr(ErrCancelled, "")
		}
		select {
		case r := <-got:
			stop()
			r.t.Took = time.Since(started)
			return r.t, r.err
		case <-tick.C:
		}
	}
}

func (g *Groq) post(ctx context.Context, wav string, size int64, limit time.Duration) (Transcript, *SpeechError) {
	file, err := openShared(wav)
	if err != nil {
		return Transcript{}, speechErr(ErrEngine, "The recording couldn’t be read: "+err.Error())
	}
	defer file.Close()
	b := "hover" + core.GUIDN()
	field := func(n, v string) string {
		return "--" + b + "\r\nContent-Disposition: form-data; name=\"" + n + "\"\r\n\r\n" + v + "\r\n"
	}
	head := field("model", g.model) + field("response_format", "verbose_json") + field("temperature", "0") +
		"--" + b + "\r\nContent-Disposition: form-data; name=\"file\"; filename=\"voice.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
	tail := "\r\n--" + b + "--\r\n"
	body := io.MultiReader(strings.NewReader(head), file, strings.NewReader(tail))
	req, err := http.NewRequestWithContext(ctx, "POST", g.Base+"/audio/transcriptions", body)
	if err != nil {
		return Transcript{}, speechErr(ErrNetwork, scrub("Groq couldn’t be reached: "+err.Error(), g.key))
	}
	req.ContentLength = int64(len(head)) + size + int64(len(tail))
	req.Header.Set("Authorization", "Bearer "+g.key)
	req.Header.Set("Content-Type", "multipart/form-data; boundary="+b)
	resp, err := httpClient(limit).Do(req)
	if err != nil {
		switch {
		case ctx.Err() != nil:
			return Transcript{}, speechErr(ErrCancelled, "")
		case isTimeout(err):
			return Transcript{}, speechErr(ErrNetwork, "Groq didn’t answer in time.")
		}
		return Transcript{}, speechErr(ErrNetwork, scrub("Groq couldn’t be reached: "+urlErr(err), g.key))
	}
	defer resp.Body.Close()
	var retry uint64
	if v, err := strconv.ParseUint(strings.TrimSpace(resp.Header.Get("Retry-After")), 10, 64); err == nil {
		retry = v
	}
	raw, _ := io.ReadAll(io.LimitReader(resp.Body, maxAnswer))
	text := string(raw)
	status := resp.StatusCode
	switch status {
	case 200:
	case 401:
		return Transcript{}, &SpeechError{Kind: ErrBadKey}
	case 429:
		return Transcript{}, &SpeechError{Kind: ErrRateLimited, Wait: retry}
	case 413:
		return Transcript{}, speechErr(ErrUnsupported, "The recording is too large for Groq.")
	case 400, 415, 422:
		m := serverMessage(text)
		if m == "" {
			m = fmt.Sprintf("Groq couldn’t use the recording (%d).", status)
		}
		return Transcript{}, speechErr(ErrUnsupported, scrub(m, g.key))
	default:
		m := ""
		if sm := serverMessage(text); sm != "" {
			m = ": " + sm
		}
		return Transcript{}, speechErr(ErrNetwork, scrub(fmt.Sprintf("Groq had a problem (%d)%s. Try again.", status, m), g.key))
	}
	var v struct {
		Text     *string
		Language *string
		Duration *float64
	}
	if json.Unmarshal(raw, &v) != nil {
		return Transcript{}, speechErr(ErrNetwork, "Groq’s answer couldn’t be read.")
	}
	if v.Text == nil {
		return Transcript{}, speechErr(ErrNetwork, "Groq’s answer had no text.")
	}
	t := Transcript{Text: strings.TrimSpace(*v.Text)}
	if v.Language != nil {
		t.Language = *v.Language
	}
	if v.Duration != nil && *v.Duration >= 0 {
		t.Audio = time.Duration(*v.Duration * float64(time.Second))
	}
	return t, nil
}

func isTimeout(err error) bool {
	type timeout interface{ Timeout() bool }
	for e := error(err); e != nil; {
		if t, ok := e.(timeout); ok && t.Timeout() {
			return true
		}
		u, ok := e.(interface{ Unwrap() error })
		if !ok {
			break
		}
		e = u.Unwrap()
	}
	return false
}

// urlErr is an error without the request's URL (Go puts it in; the key is not in it, but the
// words are ours).
func urlErr(err error) string {
	if ue, ok := err.(*url.Error); ok {
		return ue.Err.Error()
	}
	return err.Error()
}

// CheckGroq is Settings' check of a key: an authenticated list of models. Blocking.
func CheckGroq(base, key string) error {
	key = strings.TrimSpace(key)
	if key == "" {
		return fmt.Errorf("Enter a Groq key first.")
	}
	req, _ := http.NewRequest("GET", base+"/models", nil)
	req.Header.Set("Authorization", "Bearer "+key)
	resp, err := httpClient(15 * time.Second).Do(req)
	if err != nil {
		if isTimeout(err) {
			return fmt.Errorf("Groq didn’t answer in time.")
		}
		return fmt.Errorf("%s", scrub("Groq couldn’t be reached: "+urlErr(err), key))
	}
	defer resp.Body.Close()
	io.Copy(io.Discard, io.LimitReader(resp.Body, maxAnswer))
	switch resp.StatusCode {
	case 200:
		return nil
	case 401:
		return fmt.Errorf("Groq didn’t accept the key.")
	case 429:
		return fmt.Errorf("Groq’s limit was reached. Try again shortly.")
	}
	return fmt.Errorf("Groq answered %d. Try again.", resp.StatusCode)
}

package agents

// Plain HTTP/1.1 to OpenCode's own server on this PC (what OpenCodeHost's HttpClient
// does): one connection per request, Basic auth, a deadline, and a stop that ends the
// request. Only loopback is ever spoken to, so there is no TLS. Rust wrote its own client
// to need no crate; Go's net/http is in the standard library, so it is used.
// The event stream is the same request with its body read line by line.

import (
	"bufio"
	"context"
	"encoding/base64"
	"errors"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strconv"
	"strings"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/internal/core"
)

type HttpErrKind int

const (
	// HttpIo: connecting, reading or writing failed (HttpRequestException).
	HttpIo HttpErrKind = iota
	// HttpTimeout: the deadline passed.
	HttpTimeout
	// HttpCancelled: the stop ended the request.
	HttpCancelled
)

type HttpErr struct {
	Kind HttpErrKind
	Msg  string // HttpIo's
}

func (e *HttpErr) Error() string {
	switch e.Kind {
	case HttpTimeout:
		return "timed out"
	case HttpCancelled:
		return "cancelled"
	}
	return e.Msg
}

// HttpClient is where the server listens and how to sign in to it.
type HttpClient struct {
	host string
	port uint16
	auth string
}

// One connection per request, never through a proxy, and no compression asked for (as
// the Rust client: a compressed event stream would sit in the decoder).
var httpTransport = &http.Transport{Proxy: nil, DisableKeepAlives: true, DisableCompression: true}
var httpRaw = &http.Client{Transport: httpTransport}

// HostPort is the host of an http:// URL, and its port (80 when none is named).
func HostPort(u string) (string, uint16, bool) {
	rest, ok := strings.CutPrefix(u, "http://")
	if !ok {
		return "", 0, false
	}
	auth := rest
	if i := strings.IndexAny(rest, "/?#"); i >= 0 {
		auth = rest[:i]
	}
	var host string
	port := uint16(80)
	if h, ok := strings.CutPrefix(auth, "["); ok {
		hh, p, ok := strings.Cut(h, "]")
		if !ok {
			return "", 0, false
		}
		host = hh
		if p, ok := strings.CutPrefix(p, ":"); ok {
			if port, ok = rustU16(p); !ok {
				return "", 0, false
			}
		}
	} else if i := strings.LastIndexByte(auth, ':'); i >= 0 {
		host = auth[:i]
		if port, ok = rustU16(auth[i+1:]); !ok {
			return "", 0, false
		}
	} else {
		host = auth
	}
	if host == "" {
		return "", 0, false
	}
	return host, port, true
}

// rustU16 is str::parse::<u16>: decimal digits, a + allowed before them.
func rustU16(p string) (uint16, bool) {
	n, err := strconv.ParseUint(strings.TrimPrefix(p, "+"), 10, 16)
	return uint16(n), err == nil
}

// IsLoopback is Uri.IsLoopback for a host name: localhost, 127.x.x.x or ::1 (as Rust's
// IpAddr, not an IPv4 address written as IPv6, and no zone).
func IsLoopback(host string) bool {
	if len(host) == len("localhost") && asciiPrefixFold(host, "localhost") {
		return true
	}
	a, err := netip.ParseAddr(host)
	if err != nil || a.Zone() != "" {
		return false
	}
	return a.Is4() && a.As4()[0] == 127 || a.Is6() && a == netip.IPv6Loopback()
}

// NewHttpClient is the server at an http:// URL, signed in as user with password.
func NewHttpClient(u, user, password string) (*HttpClient, bool) {
	host, port, ok := HostPort(u)
	if !ok {
		return nil, false
	}
	return &HttpClient{host, port, "Basic " + base64.StdEncoding.EncodeToString([]byte(user+":"+password))}, true
}

// HttpResponse is an open response: its status and its body, and the stop's hold on the
// request. Close it when done (Text does).
type HttpResponse struct {
	Status  int
	body    *bufio.Reader
	raw     io.ReadCloser
	stopped *atomic.Bool
	cancel  context.CancelFunc
	reg     Registration
}

func httpWhy(stopped *atomic.Bool, err error) *HttpErr {
	if stopped.Load() {
		return &HttpErr{Kind: HttpCancelled}
	}
	var ne net.Error
	if errors.Is(err, context.DeadlineExceeded) || errors.As(err, &ne) && ne.Timeout() {
		return &HttpErr{Kind: HttpTimeout}
	}
	var ue *url.Error
	if errors.As(err, &ue) {
		err = ue.Err
	}
	return &HttpErr{Kind: HttpIo, Msg: err.Error()}
}

// Open sends a request and reads the answer's head; the body is read from the response.
// Past timeout (0: no limit) it gives up; stop ends it.
func (c *HttpClient) Open(method, target string, body *string, timeout time.Duration, stop *Cancel) (*HttpResponse, *HttpErr) {
	if stop != nil && stop.IsCancelled() {
		return nil, &HttpErr{Kind: HttpCancelled}
	}
	var ctx context.Context
	var cancel context.CancelFunc
	if timeout > 0 {
		ctx, cancel = context.WithTimeout(context.Background(), timeout)
	} else {
		ctx, cancel = context.WithCancel(context.Background())
	}
	stopped := &atomic.Bool{}
	var reg Registration
	if stop != nil {
		reg = stop.OnCancel(func() { stopped.Store(true); cancel() })
	}
	fail := func(err error) (*HttpResponse, *HttpErr) {
		e := httpWhy(stopped, err)
		reg.Remove()
		cancel()
		return nil, e
	}
	var rd io.Reader
	if body != nil {
		rd = strings.NewReader(*body)
	}
	req, err := http.NewRequestWithContext(ctx, method, "http://"+net.JoinHostPort(c.host, strconv.Itoa(int(c.port)))+target, rd)
	if err != nil {
		return fail(err)
	}
	req.Header.Set("Authorization", c.auth)
	req.Header.Set("Accept", "*/*")
	if body != nil {
		req.Header.Set("Content-Type", "application/json; charset=utf-8")
	}
	req.Close = true
	resp, err := httpRaw.Do(req)
	if err != nil {
		return fail(err)
	}
	return &HttpResponse{Status: resp.StatusCode, body: bufio.NewReader(resp.Body), raw: resp.Body, stopped: stopped, cancel: cancel, reg: reg}, nil
}

// Close lets the request go: its connection closed and the stop's hold on it removed.
func (r *HttpResponse) Close() {
	r.raw.Close()
	r.cancel()
	r.reg.Remove()
}

// Text is the whole body as text (bad UTF-8 replaced), the response closed.
func (r *HttpResponse) Text() (int, string, *HttpErr) {
	defer r.Close()
	b, err := io.ReadAll(r.body)
	if err != nil {
		return 0, "", httpWhy(r.stopped, err)
	}
	if r.stopped.Load() {
		return 0, "", &HttpErr{Kind: HttpCancelled}
	}
	return r.Status, core.Lossy(b), nil
}

// Line is the next line of the body without its end; false at its end.
func (r *HttpResponse) Line() (string, bool, *HttpErr) {
	b, err := r.body.ReadBytes('\n')
	if len(b) == 0 {
		if err == io.EOF && !r.stopped.Load() {
			return "", false, nil
		}
		if err == nil || err == io.EOF {
			return "", false, &HttpErr{Kind: HttpCancelled}
		}
		return "", false, httpWhy(r.stopped, err)
	}
	if err != nil && err != io.EOF {
		return "", false, httpWhy(r.stopped, err)
	}
	for len(b) > 0 && (b[len(b)-1] == '\n' || b[len(b)-1] == '\r') {
		b = b[:len(b)-1]
	}
	return core.Lossy(b), true, nil
}

// Call is a whole request: the status and the body's text.
func (c *HttpClient) Call(method, target string, body *string, timeout time.Duration, stop *Cancel) (int, string, *HttpErr) {
	r, err := c.Open(method, target, body, timeout, stop)
	if err != nil {
		return 0, "", err
	}
	return r.Text()
}

//go:build linux

// Package linux is the Linux half of what platform/win is on Windows: the desktop portals,
// the tray, notifications, the file pickers and the clipboard. Wayland only (no X11, no
// XWayland); everything here talks D-Bus (godbus, pure Go) or runs the desktop's own tools,
// so there is no C compiler in it.
package linux

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"net/url"
	"strings"
	"time"

	"github.com/godbus/dbus/v5"
)

const (
	portalBus  = "org.freedesktop.portal.Desktop"
	portalPath = dbus.ObjectPath("/org/freedesktop/portal/desktop")
)

// Connect is a session bus connection, or one at that address (the checks' own bus). It
// never starts a bus: a machine with none has no portals, tray or notifications.
func Connect(bus string) (*dbus.Conn, error) {
	var c *dbus.Conn
	var err error
	if bus == "" {
		c, err = dbus.SessionBusPrivateNoAutoStartup()
	} else {
		c, err = dbus.Dial(bus)
	}
	if err != nil {
		return nil, err
	}
	if err := c.Auth(nil); err != nil {
		c.Close()
		return nil, err
	}
	if err := c.Hello(); err != nil {
		c.Close()
		return nil, err
	}
	return c, nil
}

func token() string {
	var b [6]byte
	_, _ = rand.Read(b[:])
	return "hover" + hex.EncodeToString(b[:])
}

// requestPath is where a portal call made with that handle token answers: the sender's
// unique name with its colon dropped and its dots made underscores.
func requestPath(conn *dbus.Conn, tok string) dbus.ObjectPath {
	sender := strings.ReplaceAll(strings.TrimPrefix(conn.Names()[0], ":"), ".", "_")
	return dbus.ObjectPath("/org/freedesktop/portal/desktop/request/" + sender + "/" + tok)
}

// Response is what a portal request ends with: 0 done, 1 cancelled by the user, 2 anything
// else, and its results.
type Response struct {
	Code    uint32
	Results map[string]dbus.Variant
}

// request makes a portal call that answers with a Response signal. call is given the handle
// token to put in its options. The signal is asked for before the call, or a quick answer
// is missed.
func request(conn *dbus.Conn, wait time.Duration, call func(tok string) *dbus.Call) (Response, error) {
	tok := token()
	want := requestPath(conn, tok)
	ch := make(chan *dbus.Signal, 8)
	conn.Signal(ch)
	defer conn.RemoveSignal(ch)
	opts := []dbus.MatchOption{dbus.WithMatchInterface("org.freedesktop.portal.Request"), dbus.WithMatchMember("Response")}
	if err := conn.AddMatchSignal(opts...); err != nil {
		return Response{}, err
	}
	defer conn.RemoveMatchSignal(opts...)
	var handle dbus.ObjectPath
	if err := call(tok).Store(&handle); err != nil {
		return Response{}, err
	}
	// Older portals answer on another path than the one worked out: the one returned counts.
	if handle == "" {
		handle = want
	}
	timeout := time.After(wait)
	for {
		select {
		case sig := <-ch:
			if sig == nil {
				return Response{}, errors.New("the bus went away")
			}
			if sig.Name != "org.freedesktop.portal.Request.Response" || sig.Path != handle || len(sig.Body) < 2 {
				continue
			}
			code, _ := sig.Body[0].(uint32)
			res, _ := sig.Body[1].(map[string]dbus.Variant)
			return Response{code, res}, nil
		case <-timeout:
			return Response{}, errors.New("the portal got no answer")
		}
	}
}

// fileURI is the path a file:// URI names.
func fileURI(u string) (string, bool) {
	p, err := url.Parse(u)
	if err != nil || p.Scheme != "file" {
		return "", false
	}
	return p.Path, true
}

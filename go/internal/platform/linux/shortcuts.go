//go:build linux

package linux

import (
	"errors"
	"fmt"
	"sync"
	"sync/atomic"
	"time"

	"github.com/godbus/dbus/v5"
)

// Global shortcuts. Wayland lets no program grab a key for itself; the compositor does, and
// the GlobalShortcuts portal is how a program asks it to (KDE, GNOME 48 and later, Hyprland).
// The user may be asked to confirm, and may pick another key than the preferred one.
// Compositors without the portal (sway, for one) are bound in their own config to
// `hoverai --toggle`.

const shortcutsIface = "org.freedesktop.portal.GlobalShortcuts"

// Shortcut is one thing to bind: an id, what it does in words, and a preferred trigger in the
// shortcuts spec's form ("CTRL+ALT+n").
type Shortcut struct {
	ID, Description, Trigger string
	// Down is called when it goes down (the first of a held key, once); Up when it is let go.
	Down, Up func()
	down     atomic.Bool
}

// IsDown says the shortcut is held now.
func (s *Shortcut) IsDown() bool { return s.down.Load() }

type bindSpec struct {
	ID    string
	Props map[string]dbus.Variant
}

// Shortcuts binds shortcuts in the portal, each in a session of its own, so that one the
// desktop refuses (or is still asking the user about) doesn't touch the others.
type Shortcuts struct {
	bus string
	mu  sync.Mutex
	all map[string]*session
}

type session struct {
	conn *dbus.Conn
	path dbus.ObjectPath
	s    *Shortcut
}

func NewShortcuts(bus string) *Shortcuts { return &Shortcuts{bus: bus, all: map[string]*session{}} }

// Set binds s under id, in place of what was bound under it (a nil s lets go of it). The
// error says why it could not.
func (g *Shortcuts) Set(id string, s *Shortcut) error {
	g.mu.Lock()
	old := g.all[id]
	delete(g.all, id)
	g.mu.Unlock()
	old.close()
	if s == nil {
		return nil
	}
	conn, err := Connect(g.bus)
	if err != nil {
		return errors.New("There is no desktop portal to bind a global shortcut.")
	}
	obj := conn.Object(portalBus, portalPath)
	// The signals first, or a press right after the binding is missed.
	ch := make(chan *dbus.Signal, 16)
	conn.Signal(ch)
	if err := conn.AddMatchSignal(dbus.WithMatchObjectPath(portalPath), dbus.WithMatchInterface(shortcutsIface)); err != nil {
		conn.Close()
		return err
	}
	resp, err := request(conn, 30*time.Second, func(tok string) *dbus.Call {
		return obj.Call(shortcutsIface+".CreateSession", 0, map[string]dbus.Variant{
			"handle_token": dbus.MakeVariant(tok), "session_handle_token": dbus.MakeVariant(token())})
	})
	if err != nil || resp.Code != 0 {
		conn.Close()
		return errors.New("There is no desktop portal to bind a global shortcut.")
	}
	sess, _ := resp.Results["session_handle"].Value().(string)
	spec := []bindSpec{{s.ID, map[string]dbus.Variant{
		"description": dbus.MakeVariant(s.Description), "preferred_trigger": dbus.MakeVariant(s.Trigger)}}}
	// The user may be asked in the compositor's dialog: that takes as long as it takes.
	resp, err = request(conn, 10*time.Minute, func(tok string) *dbus.Call {
		return obj.Call(shortcutsIface+".BindShortcuts", 0, dbus.ObjectPath(sess), spec, "", map[string]dbus.Variant{"handle_token": dbus.MakeVariant(tok)})
	})
	if err != nil {
		conn.Close()
		return fmt.Errorf("The shortcut couldn’t be registered (%v).", err)
	}
	if resp.Code != 0 {
		conn.Close()
		return errors.New("The shortcut wasn’t accepted by the desktop.")
	}
	e := &session{conn: conn, path: dbus.ObjectPath(sess), s: s}
	g.mu.Lock()
	g.all[id] = e
	g.mu.Unlock()
	go e.listen(ch)
	return nil
}

func (e *session) close() {
	if e == nil {
		return
	}
	_ = e.conn.Object(portalBus, e.path).Call("org.freedesktop.portal.Session.Close", 0).Err
	e.conn.Close()
}

// Close lets go of everything.
func (g *Shortcuts) Close() {
	g.mu.Lock()
	all := g.all
	g.all = map[string]*session{}
	g.mu.Unlock()
	for _, e := range all {
		e.close()
	}
}

func (e *session) listen(ch chan *dbus.Signal) {
	a := e.s
	for sig := range ch {
		if len(sig.Body) < 2 || (sig.Name != shortcutsIface+".Activated" && sig.Name != shortcutsIface+".Deactivated") {
			continue
		}
		if p, _ := sig.Body[0].(dbus.ObjectPath); p != e.path {
			continue
		}
		if id, _ := sig.Body[1].(string); id != a.ID {
			continue
		}
		if sig.Name == shortcutsIface+".Activated" {
			if !a.down.Swap(true) && a.Down != nil {
				a.Down()
			}
		} else if a.down.Swap(false) && a.Up != nil {
			a.Up()
		}
	}
}

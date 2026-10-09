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

// Shortcuts is a session with the portal and what is bound in it.
type Shortcuts struct {
	bus  string
	mu   sync.Mutex
	conn *dbus.Conn
	sess dbus.ObjectPath
	all  map[string]*Shortcut
}

func NewShortcuts(bus string) *Shortcuts { return &Shortcuts{bus: bus, all: map[string]*Shortcut{}} }

// Set binds s (or, with a nil s, lets go of id). A new session is made with everything that
// is bound, as the portal cannot take a shortcut back. The error says why it could not.
func (g *Shortcuts) Set(id string, s *Shortcut) error {
	g.mu.Lock()
	defer g.mu.Unlock()
	if s == nil {
		delete(g.all, id)
	} else {
		g.all[id] = s
	}
	g.closeLocked()
	if len(g.all) == 0 {
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
		return errors.New("The desktop has no global shortcuts portal. Bind a key to `hoverai --toggle` in its settings.")
	}
	sess, _ := resp.Results["session_handle"].Value().(string)
	var specs []bindSpec
	for _, a := range g.all {
		specs = append(specs, bindSpec{a.ID, map[string]dbus.Variant{
			"description": dbus.MakeVariant(a.Description), "preferred_trigger": dbus.MakeVariant(a.Trigger)}})
	}
	// The user may be asked in the compositor's dialog: that takes as long as it takes.
	resp, err = request(conn, 10*time.Minute, func(tok string) *dbus.Call {
		return obj.Call(shortcutsIface+".BindShortcuts", 0, dbus.ObjectPath(sess), specs, "", map[string]dbus.Variant{"handle_token": dbus.MakeVariant(tok)})
	})
	if err != nil {
		conn.Close()
		return fmt.Errorf("The shortcut couldn’t be registered (%v).", err)
	}
	if resp.Code != 0 {
		conn.Close()
		return errors.New("The shortcut wasn’t accepted by the desktop.")
	}
	g.conn, g.sess = conn, dbus.ObjectPath(sess)
	go g.listen(conn, ch, dbus.ObjectPath(sess))
	return nil
}

func (g *Shortcuts) closeLocked() {
	if g.conn != nil {
		_ = g.conn.Object(portalBus, g.sess).Call("org.freedesktop.portal.Session.Close", 0).Err
		g.conn.Close()
		g.conn = nil
	}
}

// Close lets go of everything.
func (g *Shortcuts) Close() {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.all = map[string]*Shortcut{}
	g.closeLocked()
}

func (g *Shortcuts) listen(conn *dbus.Conn, ch chan *dbus.Signal, sess dbus.ObjectPath) {
	for sig := range ch {
		if len(sig.Body) < 2 || (sig.Name != shortcutsIface+".Activated" && sig.Name != shortcutsIface+".Deactivated") {
			continue
		}
		if s, _ := sig.Body[0].(dbus.ObjectPath); s != sess {
			continue
		}
		id, _ := sig.Body[1].(string)
		g.mu.Lock()
		a := g.all[id]
		g.mu.Unlock()
		if a == nil {
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

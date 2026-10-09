//go:build linux

package core

import (
	"errors"
	"time"

	"github.com/godbus/dbus/v5"
)

// platform/linux.rs's D-Bus half: the Secret Service that keeps note.key's key, and the
// settings portal that says dark or light. godbus is pure Go: no C compiler.

// connect is the session bus, or a bus at that address (the checks' own bus).
func connect(bus string) (*dbus.Conn, error) {
	var c *dbus.Conn
	var err error
	if bus == "" {
		// Never starts a bus: a machine with none falls back to the file, as the Rust app does.
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

// MARK: The Secret Service

const (
	secretBus        = "org.freedesktop.secrets"
	secretPath       = dbus.ObjectPath("/org/freedesktop/secrets")
	secretPromptWait = 120 * time.Second
)

// secret is the Service's (oayays): the session, its parameters, the value, its type.
type secret struct {
	Session     dbus.ObjectPath
	Parameters  []byte
	Value       []byte
	ContentType string
}

type secretService struct {
	conn    *dbus.Conn
	session dbus.ObjectPath
}

func openSecretService(bus string) (*secretService, error) {
	c, err := connect(bus)
	if err != nil {
		return nil, err
	}
	var out dbus.Variant
	var session dbus.ObjectPath
	if err := c.Object(secretBus, secretPath).Call("org.freedesktop.Secret.Service.OpenSession", 0, "plain", dbus.MakeVariant("")).Store(&out, &session); err != nil {
		c.Close()
		return nil, err
	}
	return &secretService{conn: c, session: session}, nil
}

func (s *secretService) close() { s.conn.Close() }

func (s *secretService) service() dbus.BusObject { return s.conn.Object(secretBus, secretPath) }

func secretAttributes(id string) map[string]string {
	return map[string]string{"xdg:schema": "dev.hover.Key", "application": "Hover", "hover-key": id}
}

// unlock unlocks what is locked, through the keyring's own prompt when it needs one.
func (s *secretService) unlock(paths []dbus.ObjectPath) error {
	if len(paths) == 0 {
		return nil
	}
	var done []dbus.ObjectPath
	var prompt dbus.ObjectPath
	if err := s.service().Call("org.freedesktop.Secret.Service.Unlock", 0, paths).Store(&done, &prompt); err != nil {
		return err
	}
	_, err := s.prompt(prompt)
	return err
}

func (s *secretService) prompt(prompt dbus.ObjectPath) (dbus.Variant, error) {
	if prompt == "/" {
		return dbus.Variant{}, nil
	}
	// The signal is asked for before the prompt is shown, or a quick answer is missed.
	ch := make(chan *dbus.Signal, 4)
	s.conn.Signal(ch)
	defer s.conn.RemoveSignal(ch)
	if err := s.conn.AddMatchSignal(dbus.WithMatchObjectPath(prompt), dbus.WithMatchInterface("org.freedesktop.Secret.Prompt"), dbus.WithMatchMember("Completed")); err != nil {
		return dbus.Variant{}, err
	}
	if err := s.conn.Object(secretBus, prompt).Call("org.freedesktop.Secret.Prompt.Prompt", 0, "").Err; err != nil {
		return dbus.Variant{}, err
	}
	timeout := time.After(secretPromptWait)
	for {
		select {
		case sig := <-ch:
			if sig == nil {
				return dbus.Variant{}, errors.New("the keyring went away")
			}
			if sig.Path != prompt || sig.Name != "org.freedesktop.Secret.Prompt.Completed" || len(sig.Body) != 2 {
				continue
			}
			if dismissed, _ := sig.Body[0].(bool); dismissed {
				return dbus.Variant{}, errors.New("the keyring's prompt was dismissed")
			}
			v, _ := sig.Body[1].(dbus.Variant)
			return v, nil
		case <-timeout:
			return dbus.Variant{}, errors.New("the keyring's prompt got no answer")
		}
	}
}

// find is the key, or nil when the keyring has no such item.
func (s *secretService) find(id string) ([]byte, error) {
	var unlocked, locked []dbus.ObjectPath
	if err := s.service().Call("org.freedesktop.Secret.Service.SearchItems", 0, secretAttributes(id)).Store(&unlocked, &locked); err != nil {
		return nil, err
	}
	items := append(append([]dbus.ObjectPath(nil), unlocked...), locked...)
	if len(items) == 0 {
		return nil, nil
	}
	first := items[0]
	if err := s.unlock(locked); err != nil {
		return nil, err
	}
	var got map[dbus.ObjectPath]secret
	if err := s.service().Call("org.freedesktop.Secret.Service.GetSecrets", 0, []dbus.ObjectPath{first}, s.session).Store(&got); err != nil {
		return nil, err
	}
	sec, ok := got[first]
	if !ok {
		return nil, errors.New("the keyring gave no secret")
	}
	return sec.Value, nil
}

func (s *secretService) store(id string, key []byte) error {
	var coll dbus.ObjectPath
	if err := s.service().Call("org.freedesktop.Secret.Service.ReadAlias", 0, "default").Store(&coll); err != nil {
		return err
	}
	if coll == "/" {
		return errors.New("the keyring has no default collection")
	}
	c := s.conn.Object(secretBus, coll)
	locked, err := c.GetProperty("org.freedesktop.Secret.Collection.Locked")
	if err != nil {
		return err
	}
	if l, _ := locked.Value().(bool); l {
		if err := s.unlock([]dbus.ObjectPath{coll}); err != nil {
			return err
		}
	}
	props := map[string]dbus.Variant{
		"org.freedesktop.Secret.Item.Label":      dbus.MakeVariant("Hover (note.key)"),
		"org.freedesktop.Secret.Item.Attributes": dbus.MakeVariant(secretAttributes(id)),
	}
	var item, prompt dbus.ObjectPath
	if err := c.Call("org.freedesktop.Secret.Collection.CreateItem", 0, props, secret{s.session, []byte{}, key, "application/octet-stream"}, true).Store(&item, &prompt); err != nil {
		return err
	}
	_, err = s.prompt(prompt)
	return err
}

// storeSecret keeps the key in the Secret Service under id.
func storeSecret(bus, id string, key []byte) error {
	s, err := openSecretService(bus)
	if err != nil {
		return err
	}
	defer s.close()
	return s.store(id, key)
}

// findSecret is the key under id: nil when there is no such item; an error when the keyring
// can't be asked (that may be fixed later).
func findSecret(bus, id string) ([]byte, error) {
	s, err := openSecretService(bus)
	if err != nil {
		return nil, err
	}
	defer s.close()
	return s.find(id)
}

// MARK: The settings portal

const (
	portalBus  = "org.freedesktop.portal.Desktop"
	portalPath = dbus.ObjectPath("/org/freedesktop/portal/desktop")
	portalSet  = "org.freedesktop.portal.Settings"
)

// portalRead is a setting through ReadOne (portal version 2), else Read, whose value comes
// wrapped in one more variant.
func portalRead(o dbus.BusObject, ns, key string) (dbus.Variant, bool) {
	var v dbus.Variant
	if err := o.Call(portalSet+".ReadOne", 0, ns, key).Store(&v); err == nil {
		return v, true
	}
	if err := o.Call(portalSet+".Read", 0, ns, key).Store(&v); err != nil {
		return dbus.Variant{}, false
	}
	if inner, ok := v.Value().(dbus.Variant); ok {
		return inner, true
	}
	return v, true
}

// portalLook is (color-scheme, enable-animations) from the portal; ok is false when there is
// no portal.
func portalLook(bus string) (scheme *uint32, anim *bool, ok bool) {
	c, err := connect(bus)
	if err != nil {
		return nil, nil, false
	}
	defer c.Close()
	o := c.Object(portalBus, portalPath)
	if v, got := portalRead(o, "org.freedesktop.appearance", "color-scheme"); got {
		if n, is := v.Value().(uint32); is {
			scheme = &n
		}
	}
	if v, got := portalRead(o, "org.gnome.desktop.interface", "enable-animations"); got {
		if b, is := v.Value().(bool); is {
			anim = &b
		}
	}
	return scheme, anim, scheme != nil || anim != nil
}

// watchPortal calls changed for each of the portal's SettingChanged signals about the look,
// until the bus goes. It returns false at once when there is no portal.
func watchPortal(bus string, changed func()) bool {
	if _, _, ok := portalLook(bus); !ok {
		return false
	}
	c, err := connect(bus)
	if err != nil {
		return false
	}
	if err := c.AddMatchSignal(dbus.WithMatchObjectPath(portalPath), dbus.WithMatchInterface(portalSet), dbus.WithMatchMember("SettingChanged")); err != nil {
		c.Close()
		return false
	}
	ch := make(chan *dbus.Signal, 8)
	c.Signal(ch)
	go func() {
		defer c.Close()
		for sig := range ch {
			if len(sig.Body) < 2 {
				continue
			}
			ns, _ := sig.Body[0].(string)
			if ns == "org.freedesktop.appearance" || ns == "org.gnome.desktop.interface" {
				changed()
			}
		}
	}()
	return true
}

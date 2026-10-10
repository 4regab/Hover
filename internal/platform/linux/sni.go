//go:build linux

package linux

import (
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"image"
	"image/png"
	"os"
	"sync"

	"github.com/godbus/dbus/v5"
	"github.com/godbus/dbus/v5/introspect"
	"github.com/godbus/dbus/v5/prop"
	xdraw "golang.org/x/image/draw"
)

// sni.rs: Services/TrayIcon.cs on Linux: a StatusNotifierItem (the tray protocol KDE, the
// GNOME AppIndicator extension, Xfce, Cinnamon and wlroots bars host) with its menu over
// com.canonical.dbusmenu, and the balloon as an org.freedesktop.Notifications notification.
// A left click opens the app window; the menu is Actions.BuildMainMenu.

// MenuItem is a tray menu entry: a separator, or a label with an optional tick.
type MenuItem struct {
	Label string
	Check *bool
	Sep   bool
}

// Event is a click on the tray: the icon (Activate) or a menu item (Item is its index).
type Event struct {
	Activate bool
	Item     int
}

// Pixmap is an icon frame: width, height and ARGB32 bytes in network order, as the protocol asks.
type Pixmap struct {
	W, H int32
	Data []byte
}

type tooltip struct {
	IconName    string
	IconData    []Pixmap
	Title       string
	Description string
}

// item is org.kde.StatusNotifierItem's methods.
type item struct{ on func(Event) }

func (i item) Activate(x, y int32) *dbus.Error          { i.on(Event{Activate: true}); return nil }
func (i item) SecondaryActivate(x, y int32) *dbus.Error { i.on(Event{Activate: true}); return nil }

// The host shows the menu itself from /MenuBar.
func (i item) ContextMenu(x, y int32) *dbus.Error                 { return nil }
func (i item) Scroll(delta int32, orientation string) *dbus.Error { return nil }

// layout is a menu node, (ia{sv}av): its id, its properties, its children as variants.
type layout struct {
	ID       int32
	Props    map[string]dbus.Variant
	Children []dbus.Variant
}

type groupProps struct {
	ID    int32
	Props map[string]dbus.Variant
}

type groupEvent struct {
	ID    int32
	Event string
	Data  dbus.Variant
	Time  uint32
}

// dbusMenu is com.canonical.dbusmenu: item n of the menu has id n + 1; the root is 0.
type dbusMenu struct {
	t  *Tray
	on func(Event)
}

func props(m MenuItem) map[string]dbus.Variant {
	p := map[string]dbus.Variant{}
	if m.Sep {
		p["type"] = dbus.MakeVariant("separator")
	} else {
		p["label"] = dbus.MakeVariant(m.Label)
		p["enabled"] = dbus.MakeVariant(true)
		if m.Check != nil {
			p["toggle-type"] = dbus.MakeVariant("checkmark")
			state := int32(0)
			if *m.Check {
				state = 1
			}
			p["toggle-state"] = dbus.MakeVariant(state)
		}
	}
	p["visible"] = dbus.MakeVariant(true)
	return p
}

func (d dbusMenu) snapshot() ([]MenuItem, uint32) {
	d.t.mu.Lock()
	defer d.t.mu.Unlock()
	return append([]MenuItem(nil), d.t.menu...), d.t.rev
}

func (d dbusMenu) GetLayout(parent, depth int32, names []string) (uint32, layout, *dbus.Error) {
	menu, rev := d.snapshot()
	node := func(i int) layout {
		return layout{int32(i) + 1, props(menu[i]), []dbus.Variant{}}
	}
	if parent > 0 && int(parent) <= len(menu) {
		return rev, node(int(parent) - 1), nil
	}
	root := layout{0, map[string]dbus.Variant{"children-display": dbus.MakeVariant("submenu")}, nil}
	for i := range menu {
		root.Children = append(root.Children, dbus.MakeVariant(node(i)))
	}
	if root.Children == nil {
		root.Children = []dbus.Variant{}
	}
	return rev, root, nil
}

func (d dbusMenu) GetGroupProperties(ids []int32, names []string) ([]groupProps, *dbus.Error) {
	menu, _ := d.snapshot()
	out := []groupProps{}
	for _, id := range ids {
		if id >= 1 && int(id) <= len(menu) {
			out = append(out, groupProps{id, props(menu[id-1])})
		}
	}
	return out, nil
}

func (d dbusMenu) GetProperty(id int32, name string) (dbus.Variant, *dbus.Error) {
	menu, _ := d.snapshot()
	if id >= 1 && int(id) <= len(menu) {
		if v, ok := props(menu[id-1])[name]; ok {
			return v, nil
		}
	}
	return dbus.MakeVariant(""), nil
}

func (d dbusMenu) Event(id int32, event string, data dbus.Variant, time uint32) *dbus.Error {
	if event == "clicked" && id >= 1 {
		d.on(Event{Item: int(id) - 1})
	}
	return nil
}

func (d dbusMenu) EventGroup(events []groupEvent) ([]int32, *dbus.Error) {
	for _, e := range events {
		if e.Event == "clicked" && e.ID >= 1 {
			d.on(Event{Item: int(e.ID) - 1})
		}
	}
	return []int32{}, nil
}

func (d dbusMenu) AboutToShow(id int32) (bool, *dbus.Error) { return false, nil }
func (d dbusMenu) AboutToShowGroup(ids []int32) ([]int32, []int32, *dbus.Error) {
	return []int32{}, []int32{}, nil
}

// Tray is the icon and its menu.
type Tray struct {
	conn *dbus.Conn
	mu   sync.Mutex
	menu []MenuItem
	rev  uint32
}

const (
	itemIface = "org.kde.StatusNotifierItem"
	menuIface = "com.canonical.dbusmenu"
)

// StartTray shows the icon (frames from IconPixmaps), the menu, and what a click does. The
// error says there is no session bus or no tray host to register with. bus is a D-Bus
// address instead of the session bus (the checks' own); "" is the session bus.
func StartTray(bus string, icon []Pixmap, menu []MenuItem, on func(Event)) (*Tray, error) {
	conn, err := Connect(bus)
	if err != nil {
		return nil, err
	}
	t := &Tray{conn: conn, menu: menu, rev: 1}
	if icon == nil {
		icon = []Pixmap{}
	}
	itemProps := prop.New(conn, "/StatusNotifierItem", map[string]map[string]*prop.Prop{itemIface: {
		"Category":   {Value: "ApplicationStatus", Emit: prop.EmitFalse},
		"Id":         {Value: "hover", Emit: prop.EmitFalse},
		"Title":      {Value: "Hover", Emit: prop.EmitFalse},
		"Status":     {Value: "Active", Emit: prop.EmitFalse},
		"WindowId":   {Value: int32(0), Emit: prop.EmitFalse},
		"IconName":   {Value: "", Emit: prop.EmitFalse},
		"IconPixmap": {Value: icon, Emit: prop.EmitFalse},
		"ToolTip":    {Value: tooltip{"", []Pixmap{}, "Hover", ""}, Emit: prop.EmitFalse},
		"ItemIsMenu": {Value: false, Emit: prop.EmitFalse},
		"Menu":       {Value: dbus.ObjectPath("/MenuBar"), Emit: prop.EmitFalse},
	}})
	if itemProps == nil {
		conn.Close()
		return nil, errors.New("the tray's properties could not be exported")
	}
	menuProps := prop.New(conn, "/MenuBar", map[string]map[string]*prop.Prop{menuIface: {
		"Version":       {Value: uint32(3), Emit: prop.EmitFalse},
		"TextDirection": {Value: "ltr", Emit: prop.EmitFalse},
		"Status":        {Value: "normal", Emit: prop.EmitFalse},
		"IconThemePath": {Value: []string{}, Emit: prop.EmitFalse},
	}})
	if menuProps == nil {
		conn.Close()
		return nil, errors.New("the tray menu's properties could not be exported")
	}
	it, dm := item{on}, dbusMenu{t, on}
	for _, e := range []struct {
		path  dbus.ObjectPath
		iface string
		obj   any
		props *prop.Properties
	}{{"/StatusNotifierItem", itemIface, it, itemProps}, {"/MenuBar", menuIface, dm, menuProps}} {
		if err := conn.Export(e.obj, e.path, e.iface); err != nil {
			conn.Close()
			return nil, err
		}
		node := &introspect.Node{Name: string(e.path), Interfaces: []introspect.Interface{
			introspect.IntrospectData, prop.IntrospectData,
			{Name: e.iface, Methods: introspect.Methods(e.obj), Properties: e.props.Introspection(e.iface),
				Signals: []introspect.Signal{{Name: "LayoutUpdated", Args: []introspect.Arg{{Name: "revision", Type: "u"}, {Name: "parent", Type: "i"}}}}},
		}}
		if err := conn.Export(introspect.NewIntrospectable(node), e.path, "org.freedesktop.DBus.Introspectable"); err != nil {
			conn.Close()
			return nil, err
		}
	}
	name := fmt.Sprintf("org.kde.StatusNotifierItem-%d-1", os.Getpid())
	if r, err := conn.RequestName(name, 0); err != nil || r != dbus.RequestNameReplyPrimaryOwner {
		conn.Close()
		return nil, fmt.Errorf("the tray name %s is taken", name)
	}
	w := conn.Object("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher")
	if err := w.Call("org.kde.StatusNotifierWatcher.RegisterStatusNotifierItem", 0, name).Err; err != nil {
		conn.Close()
		return nil, fmt.Errorf("no tray host (%v)", err)
	}
	return t, nil
}

// SetMenu is the menu changed (the shortcut's label, Launch at Login's tick).
func (t *Tray) SetMenu(menu []MenuItem) {
	t.mu.Lock()
	t.menu = menu
	t.rev++
	rev := t.rev
	t.mu.Unlock()
	_ = t.conn.Emit("/MenuBar", menuIface+".LayoutUpdated", rev, int32(0))
}

// Close takes the icon away.
func (t *Tray) Close() { t.conn.Close() }

// Notify is TrayIcon.Notify: a notification of six seconds, as the balloon was.
func Notify(bus, title, text string) (uint32, error) {
	conn, err := Connect(bus)
	if err != nil {
		return 0, err
	}
	defer conn.Close()
	var id uint32
	hints := map[string]dbus.Variant{"desktop-entry": dbus.MakeVariant("hover")}
	err = conn.Object("org.freedesktop.Notifications", "/org/freedesktop/Notifications").
		Call("org.freedesktop.Notifications.Notify", 0, "Hover", uint32(0), "hover", title, text, []string{}, hints, int32(6000)).Store(&id)
	return id, err
}

// IconPixmaps is hover.ico's frames as the protocol's ARGB32: 16, 22, 24, 32 and 48 px.
func IconPixmaps() []Pixmap {
	frames := icoFrames(iconBytes)
	if len(frames) == 0 {
		return nil
	}
	var out []Pixmap
	for _, size := range []int{16, 22, 24, 32, 48} {
		// The frame nearest in size, scaled to it.
		best := frames[0]
		for _, f := range frames {
			if abs(f.Bounds().Dx()-size) < abs(best.Bounds().Dx()-size) {
				best = f
			}
		}
		r := image.NewNRGBA(image.Rect(0, 0, size, size))
		xdraw.CatmullRom.Scale(r, r.Bounds(), best, best.Bounds(), xdraw.Src, nil)
		data := make([]byte, 0, size*size*4)
		for i := 0; i < len(r.Pix); i += 4 {
			data = append(data, r.Pix[i+3], r.Pix[i], r.Pix[i+1], r.Pix[i+2])
		}
		out = append(out, Pixmap{int32(size), int32(size), data})
	}
	return out
}

func abs(n int) int {
	if n < 0 {
		return -n
	}
	return n
}

// icoFrames decodes an .ico: each frame is a PNG, or a bitmap without its file header (the
// colours as BGRA and a mask of the see-through pixels under them).
func icoFrames(ico []byte) []image.Image {
	if len(ico) < 6 {
		return nil
	}
	n := int(binary.LittleEndian.Uint16(ico[4:]))
	var out []image.Image
	for i := 0; i < n && 6+16*(i+1) <= len(ico); i++ {
		e := ico[6+16*i:]
		size, off := int(binary.LittleEndian.Uint32(e[8:])), int(binary.LittleEndian.Uint32(e[12:]))
		if off < 0 || size < 0 || off+size > len(ico) {
			continue
		}
		data := ico[off : off+size]
		if bytes.HasPrefix(data, []byte("\x89PNG")) {
			if im, err := png.Decode(bytes.NewReader(data)); err == nil {
				out = append(out, im)
			}
			continue
		}
		if im := dib(data); im != nil {
			out = append(out, im)
		}
	}
	return out
}

// dib is a 32-bit bitmap from an icon: BITMAPINFOHEADER, then the pixels bottom row first.
func dib(d []byte) image.Image {
	if len(d) < 40 || binary.LittleEndian.Uint32(d) != 40 {
		return nil
	}
	w, h2 := int(int32(binary.LittleEndian.Uint32(d[4:]))), int(int32(binary.LittleEndian.Uint32(d[8:])))
	bits := int(binary.LittleEndian.Uint16(d[14:]))
	h := h2 / 2 // the height counts the mask
	if bits != 32 || w <= 0 || h <= 0 || len(d) < 40+w*h*4 {
		return nil
	}
	img := image.NewNRGBA(image.Rect(0, 0, w, h))
	px := d[40:]
	for y := 0; y < h; y++ {
		row := px[(h-1-y)*w*4:]
		for x := 0; x < w; x++ {
			b, g, r, a := row[4*x], row[4*x+1], row[4*x+2], row[4*x+3]
			img.Pix[(y*w+x)*4], img.Pix[(y*w+x)*4+1], img.Pix[(y*w+x)*4+2], img.Pix[(y*w+x)*4+3] = r, g, b, a
		}
	}
	return img
}

//go:build linux

package wayland

import (
	"errors"
	"fmt"
	"os"
	"strings"
)

// The interfaces used, with the opcodes of the requests and events Hover uses (from the
// protocol XML: wayland.xml, xdg-shell.xml, wlr-layer-shell-unstable-v1.xml,
// cursor-shape-v1.xml, fractional-scale-v1.xml, viewporter.xml,
// text-input-unstable-v3.xml).
const (
	// wl_display
	displaySync        = 0
	displayGetRegistry = 1
	// wl_registry
	registryBind = 0
	// wl_compositor
	compositorCreateSurface = 0
	compositorCreateRegion  = 1
	// wl_surface
	surfaceDestroy           = 0
	surfaceAttach            = 1
	surfaceFrame             = 3
	surfaceSetOpaqueRegion   = 4
	surfaceSetInputRegion    = 5
	surfaceCommit            = 6
	surfaceSetBufferScale    = 8
	surfaceDamageBuffer      = 9
	surfaceEventEnter        = 0
	surfaceEventLeave        = 1
	surfaceEventPrefScale    = 2
	regionDestroy            = 0
	regionAdd                = 1
	shmCreatePool            = 0
	poolCreateBuffer         = 0
	poolDestroy              = 1
	bufferDestroy            = 0
	bufferEventRelease       = 0
	seatGetPointer           = 0
	seatGetKeyboard          = 1
	seatEventCapabilities    = 0
	pointerEventEnter        = 0
	pointerEventLeave        = 1
	pointerEventMotion       = 2
	pointerEventButton       = 3
	pointerEventAxis         = 4
	pointerEventAxisDiscrete = 8
	pointerEventAxis120      = 9
	keyboardEventKeymap      = 0
	keyboardEventEnter       = 1
	keyboardEventLeave       = 2
	keyboardEventKey         = 3
	keyboardEventModifiers   = 4
	keyboardEventRepeat      = 5
	outputEventGeometry      = 0
	outputEventMode          = 1
	outputEventDone          = 2
	outputEventScale         = 3
	outputEventName          = 4
	// xdg_wm_base
	wmBaseGetXdgSurface = 2
	wmBasePong          = 3
	wmBaseEventPing     = 0
	// xdg_surface
	xdgSurfaceDestroy        = 0
	xdgSurfaceGetToplevel    = 1
	xdgSurfaceSetGeometry    = 3
	xdgSurfaceAckConfigure   = 4
	xdgSurfaceEventConfigure = 0
	// xdg_toplevel
	toplevelDestroy        = 0
	toplevelSetTitle       = 2
	toplevelSetAppID       = 3
	toplevelMove           = 5
	toplevelResize         = 6
	toplevelSetMaxSize     = 7
	toplevelSetMinSize     = 8
	toplevelSetMaximized   = 9
	toplevelUnsetMaximized = 10
	toplevelSetMinimized   = 13
	toplevelEventConfigure = 0
	toplevelEventClose     = 1
	// zwlr_layer_shell_v1 and its surface
	layerShellGetSurface   = 0
	layerSetSize           = 0
	layerSetAnchor         = 1
	layerSetExclusive      = 2
	layerSetMargin         = 3
	layerSetKeyboard       = 4
	layerAckConfigure      = 6
	layerDestroy           = 7
	layerEventConfigure    = 0
	layerEventClosed       = 1
	cursorShapeGetPointer  = 1
	cursorShapeSetShape    = 1
	callbackEventDone      = 0
	registryEventGlobal    = 0
	registryEventRemove    = 1
	shmFormatARGB8888      = 0
	shmFormatXRGB8888      = 1
	layerTop               = 2
	layerOverlay           = 3
	anchorTop              = 1
	keyboardNone           = 0
	keyboardExclusive      = 1
	keyboardOnDemand       = 2
	btnLeft, btnRight      = 0x110, 0x111
	btnMiddle              = 0x112
	stateMaximized         = 1
	stateActivated         = 4
	layerKeyboardOnDemandV = 4
	// wp_fractional_scale_manager_v1 and wp_fractional_scale_v1
	fracManagerGetScale = 1
	fracDestroy         = 0
	fracEventPreferred  = 0
	// wp_viewporter and wp_viewport
	viewporterGetViewport  = 1
	viewportDestroy        = 0
	viewportSetDestination = 2
	// zwp_text_input_manager_v3 and zwp_text_input_v3
	textManagerGetInput     = 1
	textInputDestroy        = 0
	textInputEnable         = 1
	textInputDisable        = 2
	textInputSetSurrounding = 3
	textInputSetChangeCause = 4
	textInputSetContentType = 5
	textInputSetCursorRect  = 6
	textInputCommit         = 7
	textInputEventEnter     = 0
	textInputEventLeave     = 1
	textInputEventPreedit   = 2
	textInputEventCommit    = 3
	textInputEventDelete    = 4
	textInputEventDone      = 5
)

type global struct {
	name, version uint32
}

// Output is a display.
type Output struct {
	Obj        *Object
	Name       string
	X, Y       int32
	W, H       int32 // the current mode, in pixels
	Scale      int32
	GeometryOK bool
}

// Display is the connection with the globals bound.
type Display struct {
	C       *Conn
	reg     *Object
	globals map[string]global

	compositor  *Object
	compVersion uint32
	shm         *Object
	wmBase      *Object
	layerShell  *Object
	layerVer    uint32
	cursorShape *Object
	// fracScale and viewporter come together: a surface is scaled by a viewport to the size
	// the fractional scale asks for. Either missing, neither is used.
	fracScale  *Object
	viewporter *Object
	seatObj    *Object
	seat       *Seat
	outputs    []*Output
	hasARGB    bool

	surfaces map[ID]*Win

	loop *Loop
	// Error is the connection's end: a protocol error or the compositor going.
	Error error
	// Logf, when set, gets the layer's own notes.
	Logf func(format string, a ...any)
}

// Open connects and binds what Hover needs. layer-shell is optional (a GNOME desktop has
// none: the notch is then an ordinary window); the others are required.
func Open() (*Display, error) {
	c, err := Dial()
	if err != nil {
		return nil, err
	}
	return OpenOn(c)
}

// OpenOn is Open on a connection already made.
func OpenOn(c *Conn) (*Display, error) {
	d := &Display{C: c, globals: map[string]global{}, surfaces: map[ID]*Win{}}
	d.reg = c.New("wl_registry")
	d.reg.On = func(op uint16, r *Reader) {
		if op == registryEventGlobal {
			name, iface, ver := r.U32(), r.Str(), r.U32()
			d.globals[iface] = global{name, ver}
			if iface == "wl_output" {
				d.addOutput(name, ver)
			}
		}
	}
	c.Req(1, displayGetRegistry).Obj(d.reg).Send()
	if err := d.Roundtrip(); err != nil {
		return nil, err
	}
	need := func(iface string, max uint32) (*Object, uint32, error) {
		g, ok := d.globals[iface]
		if !ok {
			return nil, 0, fmt.Errorf("the compositor has no %s", iface)
		}
		v := min(g.version, max)
		return d.bind(g, iface, v), v, nil
	}
	var err error
	if d.compositor, d.compVersion, err = need("wl_compositor", 6); err != nil {
		return nil, err
	}
	if d.shm, _, err = need("wl_shm", 1); err != nil {
		return nil, err
	}
	d.shm.On = func(op uint16, r *Reader) {
		if op == 0 && r.U32() == shmFormatARGB8888 {
			d.hasARGB = true
		}
	}
	if d.wmBase, _, err = need("xdg_wm_base", 2); err != nil {
		return nil, err
	}
	d.wmBase.On = func(op uint16, r *Reader) {
		if op == wmBaseEventPing {
			serial := r.U32()
			c.Req(d.wmBase.ID, wmBasePong).U32(serial).Send()
		}
	}
	if g, ok := d.globals["zwlr_layer_shell_v1"]; ok {
		d.layerVer = min(g.version, 4)
		d.layerShell = d.bind(g, "zwlr_layer_shell_v1", d.layerVer)
	}
	if g, ok := d.globals["wp_cursor_shape_manager_v1"]; ok {
		d.cursorShape = d.bind(g, "wp_cursor_shape_manager_v1", 1)
	}
	if g, ok := d.globals["wp_fractional_scale_manager_v1"]; ok {
		if v, ok := d.globals["wp_viewporter"]; ok {
			d.fracScale = d.bind(g, "wp_fractional_scale_manager_v1", 1)
			d.viewporter = d.bind(v, "wp_viewporter", 1)
		}
	}
	if g, ok := d.globals["wl_seat"]; ok {
		d.seatObj = d.bind(g, "wl_seat", min(g.version, 5))
		d.seat = newSeat(d, d.seatObj)
		// Input methods (compose, CJK, on-screen keyboards) talk through the seat's text input.
		if m, ok := d.globals["zwp_text_input_manager_v3"]; ok {
			d.seat.ti = newTextInput(d, d.bind(m, "zwp_text_input_manager_v3", 1), d.seatObj)
		}
	}
	// The outputs' modes and scales, and the seat's capabilities, come once bound.
	if err := d.Roundtrip(); err != nil {
		return nil, err
	}
	if err := d.Roundtrip(); err != nil {
		return nil, err
	}
	if len(d.outputs) == 0 {
		return nil, errors.New("the compositor has no display")
	}
	return d, nil
}

func (d *Display) bind(g global, iface string, version uint32) *Object {
	o := d.C.New(iface)
	d.C.Req(d.reg.ID, registryBind).U32(g.name).Str(iface).U32(version).U32(o.ID).Send()
	return o
}

func (d *Display) addOutput(name, ver uint32) {
	o := d.bind(global{name, ver}, "wl_output", min(ver, 4))
	out := &Output{Obj: o, Scale: 1}
	d.outputs = append(d.outputs, out)
	o.On = func(op uint16, r *Reader) {
		switch op {
		case outputEventGeometry:
			out.X, out.Y = r.I32(), r.I32()
			out.GeometryOK = true
		case outputEventMode:
			flags, w, h := r.U32(), r.I32(), r.I32()
			if flags&1 != 0 { // current
				out.W, out.H = w, h
			}
		case outputEventScale:
			out.Scale = max(r.I32(), 1)
		case outputEventName:
			out.Name = r.Str()
		}
	}
}

// Primary is the display the notch goes on: $HOVER_OUTPUT by name, else the first one the
// compositor lists (Wayland has no primary display; compositors list theirs first).
func (d *Display) Primary() *Output {
	if want := os.Getenv("HOVER_OUTPUT"); want != "" {
		for _, o := range d.outputs {
			if strings.EqualFold(o.Name, want) {
				return o
			}
		}
	}
	return d.outputs[0]
}

// Outputs are the displays.
func (d *Display) Outputs() []*Output { return d.outputs }

// HasLayerShell says the compositor can place the notch itself.
func (d *Display) HasLayerShell() bool { return d.layerShell != nil }

// Roundtrip waits for the compositor to answer everything sent so far.
func (d *Display) Roundtrip() error {
	cb := d.C.New("wl_callback")
	done := false
	cb.On = func(op uint16, r *Reader) { done = true }
	d.C.Req(1, displaySync).Obj(cb).Send()
	if err := d.C.Flush(); err != nil {
		return err
	}
	for !done {
		if err := d.C.Dispatch(true); err != nil {
			return err
		}
	}
	d.C.Forget(cb)
	return nil
}

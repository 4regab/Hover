//go:build linux

package wayland

import (
	"errors"
	"fmt"
	"image"
	"io"
	"strings"
	"sync/atomic"
	"time"
	"unicode/utf8"
	"unsafe"

	"gioui.org/f32"
	"gioui.org/gpu/headless"
	"gioui.org/io/input"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/io/transfer"
	"gioui.org/layout"
	"gioui.org/op"
	"gioui.org/unit"
	"golang.org/x/sys/unix"

	"github.com/4regab/Hover/internal/platform/linux"
)

// Kind is what a window is for.
type Kind uint8

const (
	// KindNotch is the notch: a layer-shell surface at the top of the display, see-through,
	// not taking the keyboard unless asked.
	KindNotch Kind = iota
	// KindFrame is the app window: no frame of its own (Hover draws the title bar).
	KindFrame
	// KindDialog is a small window of a fixed size.
	KindDialog
)

// Options are what a window is made with. Size is the client area in logical pixels.
type Options struct {
	Kind       Kind
	Title      string
	W, H       float32
	MinW, MinH float32
}

// Handlers are what the window tells the app, on the UI thread.
type Handlers struct {
	OnClose func() bool
	OnFocus func(bool)
	OnState func()
	OnPress func()
}

// Win is a window Gio draws into: Gio renders each frame offscreen (headless EGL), and the
// pixels go to the compositor in a shared-memory buffer.
//
// ponytail: a read-back and a copy per frame; the notch is small and the frames are few.
// Importing the render as a dmabuf is the upgrade.
type Win struct {
	d    *Display
	Kind Kind
	opts Options

	surf    *Object
	xdgSurf *Object
	top     *Object
	layer   *Object
	region  *Object

	// Draw builds one frame's operations; see shell.Window.
	draw func(gtx layout.Context, scale float32) bool
	H    Handlers

	configured bool
	visible    bool
	gone       bool
	lw, lh     int     // the size in logical pixels
	scale      float64 // pixels to a logical pixel; whole unless the compositor asks for a fraction
	prefScale  int
	fscale     uint32 // the fractional scale, in 120ths (0: none asked for yet)
	entered    map[ID]int32

	// fs and vp are the fractional-scale and viewport add-ons of the surface (nil without
	// them). With a fractional scale the buffer is lw x lh times it, the buffer scale stays 1
	// and the viewport says the surface is lw x lh; vpSet is whether it was told so.
	fs, vp *Object
	vpSet  bool

	router  input.Router
	ops     op.Ops
	t0      time.Time
	pending atomic.Bool
	cursor  pointer.Cursor
	ime     input.EditorState
	imeOn   bool // a text box has the keyboard, so an input method may write into it
	frames  atomic.Uint64
	focused bool

	hl      *headless.Window
	hlW     int
	hlH     int
	img     *image.RGBA
	pool    *Object
	poolFD  int
	mem     []byte
	bufs    [2]*shmBuf
	dirty   bool
	waiting bool
	waitAt  time.Time

	maximized, activated, minimized bool
	opaque                          bool
	// OnKeyboard is told first when the window gains or loses the keyboard (the notch's own
	// rules; OnFocus in the handlers is the app's).
	OnKeyboard func(bool)
}

type shmBuf struct {
	obj  *Object
	off  int
	busy bool
}

// NewWindow makes a window; it shows once the compositor has configured it.
func (d *Display) NewWindow(o Options) (*Win, error) {
	w := &Win{d: d, Kind: o.Kind, opts: o, t0: time.Now(), scale: 1, entered: map[ID]int32{}, poolFD: -1,
		lw: int(o.W), lh: int(o.H), dirty: true, visible: true, opaque: o.Kind != KindNotch}
	w.surf = d.C.New("wl_surface")
	d.surfaces[w.surf.ID] = w
	d.C.Req(d.compositor.ID, compositorCreateSurface).Obj(w.surf).Send()
	w.surf.On = w.surfaceEvent
	if d.fracScale != nil && d.viewporter != nil {
		w.vp = d.C.New("wp_viewport")
		d.C.Req(d.viewporter.ID, viewporterGetViewport).Obj(w.vp).Obj(w.surf).Send()
		w.fs = d.C.New("wp_fractional_scale_v1")
		d.C.Req(d.fracScale.ID, fracManagerGetScale).Obj(w.fs).Obj(w.surf).Send()
		w.fs.On = func(op uint16, r *Reader) {
			if op == fracEventPreferred {
				w.fscale = r.U32()
				w.rescale()
			}
		}
	}
	if o.Kind == KindNotch {
		if d.layerShell == nil {
			return nil, errors.New("this desktop can't place the notch: its compositor has no layer-shell (GNOME's Mutter has none; KDE, Sway, Hyprland and others do)")
		}
		w.makeLayer()
	} else {
		w.makeToplevel()
	}
	d.C.Req(w.surf.ID, surfaceCommit).Send()
	return w, nil
}

func (w *Win) surfaceEvent(op uint16, r *Reader) {
	switch op {
	case surfaceEventEnter:
		if o := r.Obj(); o != nil {
			for _, out := range w.d.outputs {
				if out.Obj.ID == o.ID {
					w.entered[o.ID] = out.Scale
				}
			}
			w.rescale()
		}
	case surfaceEventLeave:
		if o := r.Obj(); o != nil {
			delete(w.entered, o.ID)
			w.rescale()
		}
	case surfaceEventPrefScale:
		w.prefScale = int(r.I32())
		w.rescale()
	}
}

func (w *Win) rescale() {
	s := 1.0
	switch {
	case w.fracOn():
		s = float64(w.fscale) / 120
	case w.prefScale > 0:
		s = float64(w.prefScale)
	default:
		for _, v := range w.entered {
			s = max(s, float64(v))
		}
		if len(w.entered) == 0 && w.Kind == KindNotch {
			s = float64(w.d.Primary().Scale)
		}
	}
	if s != w.scale {
		w.scale = max(s, 1)
		w.dirty = true
		if w.H.OnState != nil {
			w.H.OnState()
		}
	}
}

// fracOn says the surface is scaled by a viewport to a fractional scale the compositor named.
func (w *Win) fracOn() bool { return w.vp != nil && w.fscale > 0 }

// px is a length in logical pixels as pixels of the buffer: rounded halfway up, as the
// fractional-scale protocol asks, or times the whole scale.
func (w *Win) px(l int) int {
	if w.fracOn() {
		return (l*int(w.fscale) + 60) / 120
	}
	return l * int(w.scale)
}

// MARK: shell.Window

func (w *Win) SetDraw(f func(gtx layout.Context, scale float32) bool) { w.draw = f }
func (w *Win) SetHandlers(h Handlers)                                 { w.H = h }
func (w *Win) Gone() bool                                             { return w.gone }
func (w *Win) Frames() uint64                                         { return w.frames.Load() }
func (w *Win) Size() (int, int)                                       { return w.px(w.lw), w.px(w.lh) }
func (w *Win) Scale() float64                                         { return w.scale }
func (w *Win) Visible() bool                                          { return w.visible && w.configured && !w.minimized }
func (w *Win) Minimized() bool                                        { return w.minimized }
func (w *Win) Maximized() bool                                        { return w.maximized }
func (w *Win) Focused() bool                                          { return w.focused }
func (w *Win) Caption(dark bool, panel uint32)                        {}
func (w *Win) Execute(c input.Command)                                { w.router.Source().Execute(c) }

// Invalidate asks for a frame. Any goroutine; asks made before it is drawn count once.
func (w *Win) Invalidate() {
	if w.pending.Swap(true) {
		return
	}
	w.d.loop.UIDo(func() { w.pending.Store(false); w.dirty = true })
}

func (w *Win) Show() {
	w.visible = true
	w.dirty = true
}

func (w *Win) Hide() {
	w.visible = false
	w.d.C.Req(w.surf.ID, surfaceAttach).Obj(nil).I32(0).I32(0).Send()
	w.d.C.Req(w.surf.ID, surfaceCommit).Send()
}

func (w *Win) Minimize() {
	if w.top != nil {
		w.minimized = true
		w.d.C.Req(w.top.ID, toplevelSetMinimized).Send()
	}
}

func (w *Win) ToggleMaximize() {
	if w.top == nil {
		return
	}
	if w.maximized {
		w.d.C.Req(w.top.ID, toplevelUnsetMaximized).Send()
	} else {
		w.d.C.Req(w.top.ID, toplevelSetMaximized).Send()
	}
}

// DragMove starts moving the window with the pointer, which has a button down in it.
func (w *Win) DragMove() {
	if w.top != nil && w.d.seat != nil {
		w.d.C.Req(w.top.ID, toplevelMove).Obj(w.d.seat.obj).U32(w.d.seat.pressSerial).Send()
	}
}

// ResizeFrom starts resizing from an edge (a Windows hit-test code: 10 left, 11 right, 12
// top, 13 top left, 14 top right, 15 bottom, 16 bottom left, 17 bottom right).
func (w *Win) ResizeFrom(edge int) {
	edges := map[int]uint32{10: 4, 11: 8, 12: 1, 13: 5, 14: 9, 15: 2, 16: 6, 17: 10}[edge]
	if w.top != nil && w.d.seat != nil && edges != 0 {
		w.d.C.Req(w.top.ID, toplevelResize).Obj(w.d.seat.obj).U32(w.d.seat.pressSerial).U32(edges).Send()
	}
}

// Close takes the window away.
func (w *Win) Close() {
	if w.gone {
		return
	}
	w.gone = true
	c := w.d.C
	switch {
	case w.top != nil:
		c.Req(w.top.ID, toplevelDestroy).Send()
		c.Req(w.xdgSurf.ID, xdgSurfaceDestroy).Send()
	case w.layer != nil:
		c.Req(w.layer.ID, layerDestroy).Send()
	}
	if w.fs != nil {
		c.Req(w.fs.ID, fracDestroy).Send()
		c.Forget(w.fs)
		c.Req(w.vp.ID, viewportDestroy).Send()
		c.Forget(w.vp)
		w.fs, w.vp = nil, nil
	}
	c.Req(w.surf.ID, surfaceDestroy).Send()
	delete(w.d.surfaces, w.surf.ID)
	w.releaseBuffers()
	if w.hl != nil {
		w.hl.Release()
		w.hl = nil
	}
	if w.d.seat != nil {
		if w.d.seat.ptrFocus == w {
			w.d.seat.ptrFocus = nil
		}
		if w.d.seat.kbFocus == w {
			w.d.seat.kbFocus = nil
		}
		if ti := w.d.seat.ti; ti != nil && ti.surf == w {
			ti.surf = nil
			ti.reset()
		}
	}
	if w.H.OnState != nil {
		w.H.OnState()
	}
}

// MARK: Roles

func (w *Win) makeToplevel() {
	d, c := w.d, w.d.C
	w.xdgSurf = c.New("xdg_surface")
	c.Req(d.wmBase.ID, wmBaseGetXdgSurface).Obj(w.xdgSurf).Obj(w.surf).Send()
	w.top = c.New("xdg_toplevel")
	c.Req(w.xdgSurf.ID, xdgSurfaceGetToplevel).Obj(w.top).Send()
	c.Req(w.top.ID, toplevelSetTitle).Str(w.opts.Title).Send()
	c.Req(w.top.ID, toplevelSetAppID).Str("hover").Send()
	if w.opts.MinW > 0 || w.opts.MinH > 0 {
		c.Req(w.top.ID, toplevelSetMinSize).I32(int32(w.opts.MinW)).I32(int32(w.opts.MinH)).Send()
	}
	if w.Kind == KindDialog {
		c.Req(w.top.ID, toplevelSetMinSize).I32(int32(w.opts.W)).I32(int32(w.opts.H)).Send()
		c.Req(w.top.ID, toplevelSetMaxSize).I32(int32(w.opts.W)).I32(int32(w.opts.H)).Send()
	}
	var nw, nh int
	var states []byte
	w.top.On = func(op uint16, r *Reader) {
		switch op {
		case toplevelEventConfigure:
			nw, nh = int(r.I32()), int(r.I32())
			states = append(states[:0], r.Array()...)
		case toplevelEventClose:
			if w.H.OnClose == nil || w.H.OnClose() {
				w.Close()
			}
		}
	}
	w.xdgSurf.On = func(op uint16, r *Reader) {
		if op != xdgSurfaceEventConfigure {
			return
		}
		c.Req(w.xdgSurf.ID, xdgSurfaceAckConfigure).U32(r.U32()).Send()
		maxed, act := false, false
		for i := 0; i+4 <= len(states); i += 4 {
			switch *(*uint32)(unsafe.Pointer(&states[i])) {
			case stateMaximized:
				maxed = true
			case stateActivated:
				act = true
			}
		}
		changed := !w.configured || maxed != w.maximized
		w.maximized = maxed
		if act {
			w.minimized = false
		}
		if nw > 0 && nh > 0 && (nw != w.lw || nh != w.lh) {
			w.lw, w.lh = nw, nh
			changed = true
		}
		w.configured = true
		w.dirty = true
		if changed && w.H.OnState != nil {
			w.H.OnState()
		}
	}
}

func (w *Win) makeLayer() {
	d, c := w.d, w.d.C
	out := d.Primary()
	w.layer = c.New("zwlr_layer_surface_v1")
	c.Req(d.layerShell.ID, layerShellGetSurface).Obj(w.layer).Obj(w.surf).Obj(out.Obj).U32(layerTop).Str("hover").Send()
	c.Req(w.layer.ID, layerSetAnchor).U32(anchorTop).Send()
	c.Req(w.layer.ID, layerSetExclusive).I32(-1).Send()
	c.Req(w.layer.ID, layerSetKeyboard).U32(keyboardNone).Send()
	c.Req(w.layer.ID, layerSetSize).U32(uint32(max(w.lw, 1))).U32(uint32(max(w.lh, 1))).Send()
	w.layer.On = func(op uint16, r *Reader) {
		switch op {
		case layerEventConfigure:
			serial, nw, nh := r.U32(), int(r.U32()), int(r.U32())
			c.Req(w.layer.ID, layerAckConfigure).U32(serial).Send()
			if nw > 0 && nh > 0 {
				w.lw, w.lh = nw, nh
			}
			w.configured = true
			w.dirty = true
		case layerEventClosed:
			w.Close()
		}
	}
}

// SetLayerSize, SetKeyboard and SetInput are the notch's (NotchPlat in the shell uses them).

// PlaceLayer sizes the layer surface in logical pixels.
func (w *Win) PlaceLayer(lw, lh int) {
	if w.layer == nil || (lw == w.lw && lh == w.lh && w.configured) {
		return
	}
	w.lw, w.lh = lw, lh
	w.d.C.Req(w.layer.ID, layerSetSize).U32(uint32(lw)).U32(uint32(lh)).Send()
	w.d.C.Req(w.surf.ID, surfaceCommit).Send()
	w.dirty = true
}

// SetKeyboard is the layer's keyboard interactivity: 0 none, 1 exclusive, 2 on demand.
func (w *Win) SetKeyboard(mode uint32) {
	if w.layer == nil {
		return
	}
	if mode == keyboardOnDemand && w.d.layerVer < layerKeyboardOnDemandV {
		mode = keyboardExclusive
	}
	w.d.C.Req(w.layer.ID, layerSetKeyboard).U32(mode).Send()
	w.d.C.Req(w.surf.ID, surfaceCommit).Send()
}

// Rect is a box in logical pixels in the window.
type Rect struct{ X, Y, W, H int32 }

// SetInput limits where the surface takes the pointer to these boxes (nothing else of it
// is hit; the pointer goes on to the windows below).
func (w *Win) SetInput(rects []Rect) {
	c := w.d.C
	reg := c.New("wl_region")
	c.Req(w.d.compositor.ID, compositorCreateRegion).Obj(reg).Send()
	for _, r := range rects {
		c.Req(reg.ID, regionAdd).I32(r.X).I32(r.Y).I32(r.W).I32(r.H).Send()
	}
	c.Req(w.surf.ID, surfaceSetInputRegion).Obj(reg).Send()
	c.Req(reg.ID, regionDestroy).Send()
	c.Forget(reg)
	c.Req(w.surf.ID, surfaceCommit).Send()
}

// MARK: Drawing

// paintPending draws the windows that asked for a frame and can take one.
func (d *Display) paintPending() {
	for _, w := range d.surfaces {
		if !w.dirty || !w.configured || !w.visible || w.gone || w.minimized {
			continue
		}
		if w.waiting && time.Since(w.waitAt) < 100*time.Millisecond {
			continue
		}
		w.dirty = false
		w.paint()
	}
}

func (w *Win) now() time.Duration { return time.Since(w.t0) }

func (w *Win) paint() {
	if w.draw == nil {
		return
	}
	pw, ph := w.px(w.lw), w.px(w.lh)
	if pw <= 0 || ph <= 0 {
		return
	}
	if err := w.ensure(pw, ph); err != nil {
		w.d.logf("window graphics: %v", err)
		return
	}
	buf := w.freeBuffer()
	if buf == nil {
		w.dirty = true // drawn when the compositor lets a buffer go
		w.waiting, w.waitAt = true, time.Now()
		return
	}
	w.ops.Reset()
	gtx := layout.Context{
		Ops: &w.ops, Now: time.Now(), Metric: unit.Metric{PxPerDp: 1, PxPerSp: 1},
		Constraints: layout.Exact(image.Pt(pw, ph)), Source: w.router.Source(),
	}
	animating := w.draw(gtx, float32(w.scale))
	w.router.Frame(&w.ops)
	if _, txt, ok := w.router.WriteClipboard(); ok {
		_ = linux.SetClipboard(string(txt))
	}
	if w.router.ClipboardRequested() {
		w.router.Queue(transfer.DataEvent{Type: "application/text", Open: func() io.ReadCloser {
			return io.NopCloser(strings.NewReader(linux.ClipboardText()))
		}})
		animating = true
	}
	w.ime = w.router.EditorState()
	w.imeFrame()
	if err := w.hl.Frame(&w.ops); err != nil {
		w.d.logf("window frame: %v", err)
		return
	}
	if err := w.hl.Screenshot(w.img); err != nil {
		w.d.logf("window read-back: %v", err)
		return
	}
	// Gio's RGBA, premultiplied, into the compositor's ARGB8888 (B, G, R, A in memory).
	dst := w.mem[buf.off : buf.off+pw*ph*4]
	src := w.img.Pix
	for i := 0; i+3 < len(src) && i+3 < len(dst); i += 4 {
		dst[i], dst[i+1], dst[i+2], dst[i+3] = src[i+2], src[i+1], src[i], src[i+3]
	}
	buf.busy = true
	c, id := w.d.C, w.surf.ID
	if w.fracOn() {
		// The buffer is the fraction; the viewport says how big the surface is.
		c.Req(id, surfaceSetBufferScale).I32(1).Send()
		c.Req(w.vp.ID, viewportSetDestination).I32(int32(w.lw)).I32(int32(w.lh)).Send()
		w.vpSet = true
	} else {
		c.Req(id, surfaceSetBufferScale).I32(int32(w.scale)).Send()
		if w.vpSet {
			c.Req(w.vp.ID, viewportSetDestination).I32(-1).I32(-1).Send()
			w.vpSet = false
		}
	}
	c.Req(id, surfaceAttach).Obj(buf.obj).I32(0).I32(0).Send()
	c.Req(id, surfaceDamageBuffer).I32(0).I32(0).I32(int32(pw)).I32(int32(ph)).Send()
	cb := c.New("wl_callback")
	cb.On = func(op uint16, r *Reader) {
		w.waiting = false
		c.Forget(cb)
	}
	c.Req(id, surfaceFrame).Obj(cb).Send()
	w.waiting, w.waitAt = true, time.Now()
	if w.opaque {
		w.setOpaque(pw, ph)
	}
	c.Req(id, surfaceCommit).Send()
	w.frames.Add(1)
	if cur := w.router.Cursor(); cur != w.cursor {
		w.cursor = cur
		if s := w.d.seat; s != nil && s.ptrFocus == w {
			s.applyCursor()
		}
	}
	if animating {
		w.dirty = true
	}
}

// setOpaque tells the compositor the window hides what is behind it (so it can skip
// blending).
func (w *Win) setOpaque(pw, ph int) {
	if w.region != nil {
		w.d.C.Forget(w.region)
	}
	c := w.d.C
	w.region = c.New("wl_region")
	c.Req(w.d.compositor.ID, compositorCreateRegion).Obj(w.region).Send()
	c.Req(w.region.ID, regionAdd).I32(0).I32(0).I32(int32(w.lw)).I32(int32(w.lh)).Send()
	c.Req(w.surf.ID, surfaceSetOpaqueRegion).Obj(w.region).Send()
	c.Req(w.region.ID, regionDestroy).Send()
	c.Forget(w.region)
	w.region = nil
}

// ensure has the renderer and the shared memory at this size.
func (w *Win) ensure(pw, ph int) error {
	if w.hl == nil || w.hlW != pw || w.hlH != ph {
		if w.hl != nil {
			w.hl.Release()
			w.hl = nil
		}
		hl, err := headless.NewWindow(pw, ph)
		if err != nil {
			return err
		}
		w.hl, w.hlW, w.hlH = hl, pw, ph
		w.img = image.NewRGBA(image.Rect(0, 0, pw, ph))
		w.releaseBuffers()
	}
	if w.mem == nil {
		return w.makeBuffers(pw, ph)
	}
	return nil
}

func (w *Win) makeBuffers(pw, ph int) error {
	size := pw * ph * 4
	fd, err := unix.MemfdCreate("hover-shm", unix.MFD_CLOEXEC)
	if err != nil {
		return fmt.Errorf("shared memory: %w", err)
	}
	if err := unix.Ftruncate(fd, int64(size*2)); err != nil {
		unix.Close(fd)
		return err
	}
	mem, err := unix.Mmap(fd, 0, size*2, unix.PROT_READ|unix.PROT_WRITE, unix.MAP_SHARED)
	if err != nil {
		unix.Close(fd)
		return err
	}
	c := w.d.C
	send, err := unix.Dup(fd)
	if err != nil {
		unix.Close(fd)
		return err
	}
	w.pool = c.New("wl_shm_pool")
	c.Req(w.d.shm.ID, shmCreatePool).Obj(w.pool).FD(send).I32(int32(size * 2)).Send()
	format := uint32(shmFormatARGB8888)
	if w.opaque {
		format = shmFormatXRGB8888
	}
	for i := range w.bufs {
		b := &shmBuf{obj: c.New("wl_buffer"), off: i * size}
		b.obj.On = func(op uint16, r *Reader) {
			if op == bufferEventRelease {
				b.busy = false
				if w.dirty {
					w.waiting = false
				}
			}
		}
		c.Req(w.pool.ID, poolCreateBuffer).Obj(b.obj).I32(int32(b.off)).I32(int32(pw)).I32(int32(ph)).I32(int32(pw * 4)).U32(format).Send()
		w.bufs[i] = b
	}
	c.Req(w.pool.ID, poolDestroy).Send()
	c.Forget(w.pool)
	w.pool = nil
	w.mem, w.poolFD = mem, fd
	return nil
}

func (w *Win) releaseBuffers() {
	c := w.d.C
	for i, b := range w.bufs {
		if b != nil {
			c.Req(b.obj.ID, bufferDestroy).Send()
			c.Forget(b.obj)
			w.bufs[i] = nil
		}
	}
	if w.mem != nil {
		_ = unix.Munmap(w.mem)
		w.mem = nil
	}
	if w.poolFD >= 0 {
		unix.Close(w.poolFD)
		w.poolFD = -1
	}
}

func (w *Win) freeBuffer() *shmBuf {
	for _, b := range w.bufs {
		if b != nil && !b.busy {
			return b
		}
	}
	return nil
}

// MARK: Input, from the seat

// pt is a point of the surface, in logical pixels, as a point of the buffer. With a fraction
// the buffer is rounded to whole pixels, so the ratio is the buffer's to the surface's.
func (w *Win) pt(x, y float64) f32.Point {
	kx, ky := w.ratio()
	return f32.Pt(float32(x*kx), float32(y*ky))
}

// ratio is buffer pixels to surface pixels, across and down.
func (w *Win) ratio() (kx, ky float64) {
	if w.fracOn() && w.lw > 0 && w.lh > 0 {
		return float64(w.px(w.lw)) / float64(w.lw), float64(w.px(w.lh)) / float64(w.lh)
	}
	return w.scale, w.scale
}

func (w *Win) pointerMove(kind pointer.Kind, x, y float64, b pointer.Buttons, m key.Modifiers) {
	w.router.Queue(pointer.Event{Kind: kind, Source: pointer.Mouse, Buttons: b, Modifiers: m, Position: w.pt(x, y), Time: w.now()})
	w.dirty = true
}

func (w *Win) pointerLeave() {
	// Off every area: hover ends.
	w.router.Queue(pointer.Event{Kind: pointer.Move, Source: pointer.Mouse, Position: f32.Pt(-1e5, -1e5), Time: w.now()})
	w.dirty = true
}

func (w *Win) pointerButton(down bool, b pointer.Buttons, x, y float64, held pointer.Buttons, m key.Modifiers) {
	kind := pointer.Release
	if down {
		kind = pointer.Press
		if w.H.OnPress != nil {
			w.H.OnPress()
		}
	}
	w.router.Queue(pointer.Event{Kind: kind, Source: pointer.Mouse, Buttons: held, Modifiers: m, Position: w.pt(x, y), Time: w.now()})
	w.dirty = true
}

func (w *Win) pointerScroll(x, y float64, sx, sy float32, held pointer.Buttons, m key.Modifiers) {
	w.router.Queue(pointer.Event{Kind: pointer.Scroll, Source: pointer.Mouse, Position: w.pt(x, y), Buttons: held,
		Scroll: f32.Pt(sx, sy), Modifiers: m, Time: w.now()})
	w.dirty = true
}

// focus is the keyboard coming or going.
func (w *Win) focus(on bool) {
	w.focused = on
	if w.OnKeyboard != nil {
		w.OnKeyboard(on)
	}
	if !on {
		w.router.Queue(pointer.Event{Kind: pointer.Cancel})
	}
	w.router.Queue(key.FocusEvent{Focus: on})
	w.dirty = true
	if w.H.OnFocus != nil {
		w.H.OnFocus(on)
	}
}

// keyEvent queues a key, and the text it types. Tab moves the keyboard between the things
// that take it when nothing used the key, as Gio's own window does.
func (w *Win) keyEvent(name key.Name, mods key.Modifiers, st key.State, text string) {
	e := key.Event{Name: name, Modifiers: mods, State: st}
	dir := key.FocusDirection(-1)
	if st == key.Press {
		switch {
		case name == key.NameTab && mods == 0:
			dir = key.FocusForward
		case name == key.NameTab && mods == key.ModShift:
			dir = key.FocusBackward
		}
	}
	if dir != -1 {
		w.router.Queue(input.SystemEvent{Event: e})
		if _, handled := w.router.WakeupTime(); !handled {
			w.router.MoveFocus(dir)
		}
	} else {
		w.router.Queue(e)
	}
	if printable(text) && st == key.Press {
		w.editorInsert(text)
	}
	w.dirty = true
}

// editorInsert is Gio's EditorInsert: the text replaces the selection, then the caret goes
// after it. The selection is kept here between frames, so two characters typed before the
// next frame go in order.
func (w *Win) editorInsert(s string) {
	if ti := w.d.seat.ti; ti != nil {
		ti.viaIM = false
	}
	sel := w.ime.Selection.Range
	start, end := min(sel.Start, sel.End), max(sel.Start, sel.End)
	w.router.Queue(key.EditEvent{Range: key.Range{Start: start, End: end}, Text: s})
	caret := start + utf8.RuneCountInString(s)
	w.router.Queue(key.SelectionEvent{Start: caret, End: caret})
	w.ime.Selection.Range = key.Range{Start: caret, End: caret}
}

// PointerPos is where the pointer is in this window, in logical pixels; false when it is
// somewhere else.
func (w *Win) PointerPos() (x, y float64, in bool) {
	s := w.d.seat
	if s == nil || s.ptrFocus != w {
		return 0, 0, false
	}
	return s.px, s.py, true
}

// ButtonDown says a pointer button is down in this window.
func (w *Win) ButtonDown() bool {
	s := w.d.seat
	return s != nil && s.ptrFocus == w && s.buttons != 0
}

// LogicalSize is the size in logical pixels.
func (w *Win) LogicalSize() (int, int) { return w.lw, w.lh }

// Keyboard modes for SetKeyboard.
const (
	KeyboardNone      = keyboardNone
	KeyboardExclusive = keyboardExclusive
	KeyboardOnDemand  = keyboardOnDemand
)

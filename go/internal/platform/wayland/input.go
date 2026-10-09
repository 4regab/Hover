//go:build linux

package wayland

import (
	"time"
	"unicode"

	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"golang.org/x/sys/unix"
)

// Seat is the keyboard and the pointer.
type Seat struct {
	d       *Display
	obj     *Object
	ptr, kb *Object
	cursor  *Object // wp_cursor_shape_device_v1

	ptrFocus, kbFocus *Win
	px, py            float64 // the pointer in its window, in logical pixels
	buttons           pointer.Buttons
	enterSerial       uint32
	pressSerial       uint32
	cursorShape       uint32

	lay    *keyLayout
	mods   key.Modifiers
	rate   int // repeats a second (0: none)
	delay  time.Duration
	rep    *Timer
	repKey uint32

	discrete bool // an axis_value120 came in this frame: the plain axis event repeats it
}

func newSeat(d *Display, obj *Object) *Seat {
	s := &Seat{d: d, obj: obj, rate: 25, delay: 600 * time.Millisecond}
	obj.On = func(op uint16, r *Reader) {
		if op != seatEventCapabilities {
			return
		}
		caps := r.U32()
		if caps&1 != 0 && s.ptr == nil {
			s.ptr = d.C.New("wl_pointer")
			d.C.Req(obj.ID, seatGetPointer).Obj(s.ptr).Send()
			s.ptr.On = s.pointerEvent
			if d.cursorShape != nil {
				s.cursor = d.C.New("wp_cursor_shape_device_v1")
				d.C.Req(d.cursorShape.ID, cursorShapeGetPointer).Obj(s.cursor).Obj(s.ptr).Send()
			}
		}
		if caps&2 != 0 && s.kb == nil {
			s.kb = d.C.New("wl_keyboard")
			d.C.Req(obj.ID, seatGetKeyboard).Obj(s.kb).Send()
			s.kb.On = s.keyboardEvent
		}
	}
	return s
}

// MARK: Pointer

func (s *Seat) pointerEvent(op uint16, r *Reader) {
	switch op {
	case pointerEventEnter:
		serial, surf, x, y := r.U32(), r.Obj(), r.Fixed(), r.Fixed()
		s.enterSerial = serial
		s.px, s.py = x, y
		if surf != nil {
			s.ptrFocus = s.d.surfaces[surf.ID]
		}
		if w := s.ptrFocus; w != nil {
			w.pointerMove(pointer.Move, x, y, s.buttons, s.mods)
			s.applyCursor()
		}
	case pointerEventLeave:
		r.U32()
		if w := s.ptrFocus; w != nil {
			w.pointerLeave()
		}
		s.ptrFocus = nil
		s.buttons = 0
	case pointerEventMotion:
		r.U32()
		s.px, s.py = r.Fixed(), r.Fixed()
		if w := s.ptrFocus; w != nil {
			kind := pointer.Move
			if s.buttons != 0 {
				kind = pointer.Drag
			}
			w.pointerMove(kind, s.px, s.py, s.buttons, s.mods)
		}
	case pointerEventButton:
		serial, _, btn, state := r.U32(), r.U32(), r.U32(), r.U32()
		var b pointer.Buttons
		switch btn {
		case btnLeft:
			b = pointer.ButtonPrimary
		case btnRight:
			b = pointer.ButtonSecondary
		case btnMiddle:
			b = pointer.ButtonTertiary
		default:
			return
		}
		if state == 1 {
			s.buttons |= b
			s.pressSerial = serial
		} else {
			s.buttons &^= b
		}
		if w := s.ptrFocus; w != nil {
			w.pointerButton(state == 1, b, s.px, s.py, s.buttons, s.mods)
		}
	case pointerEventAxis120:
		// A wheel notch is 120 and scrolls 60 logical pixels, as Slint's winit backend does.
		axis, v := r.U32(), r.I32()
		s.discrete = true
		s.scroll(axis, float64(v)/120*60)
	case pointerEventAxisDiscrete:
		// The old way of saying a notch; the plain axis event after it is the same scroll.
		s.discrete = true
	case pointerEventAxis:
		r.U32()
		axis, v := r.U32(), r.Fixed()
		if s.discrete {
			s.discrete = false
			return
		}
		s.scroll(axis, v)
	}
}

// scroll sends a wheel step: axis 0 is vertical, 1 horizontal; v is in logical pixels, down
// positive.
func (s *Seat) scroll(axis uint32, v float64) {
	w := s.ptrFocus
	if w == nil {
		return
	}
	d := float32(v) * float32(w.scale)
	var x, y float32
	switch {
	case axis == 1:
		x = d
	case s.mods&key.ModShift != 0:
		x = d
	default:
		y = d
	}
	w.pointerScroll(s.px, s.py, x, y, s.buttons, s.mods)
}

// applyCursor sets the pointer's picture for the window it is over.
func (s *Seat) applyCursor() {
	if s.ptr == nil || s.ptrFocus == nil {
		return
	}
	c := s.ptrFocus.cursor
	if c == pointer.CursorNone {
		s.d.C.Req(s.ptr.ID, 0).U32(s.enterSerial).Obj(nil).I32(0).I32(0).Send()
		return
	}
	if s.cursor != nil {
		s.d.C.Req(s.cursor.ID, cursorShapeSetShape).U32(s.enterSerial).U32(shapeOf(c)).Send()
	}
}

// shapeOf is the cursor-shape-v1 shape for a Gio cursor.
func shapeOf(c pointer.Cursor) uint32 {
	switch c {
	case pointer.CursorText:
		return 9
	case pointer.CursorPointer:
		return 4
	case pointer.CursorCrosshair:
		return 8
	case pointer.CursorWait:
		return 6
	case pointer.CursorProgress:
		return 5
	case pointer.CursorNotAllowed:
		return 15
	case pointer.CursorGrab:
		return 16
	case pointer.CursorGrabbing:
		return 17
	case pointer.CursorAllScroll:
		return 32
	case pointer.CursorColResize, pointer.CursorEastWestResize, pointer.CursorEastResize, pointer.CursorWestResize:
		return 26
	case pointer.CursorRowResize, pointer.CursorNorthSouthResize, pointer.CursorNorthResize, pointer.CursorSouthResize:
		return 27
	case pointer.CursorNorthEastResize, pointer.CursorSouthWestResize, pointer.CursorNorthEastSouthWestResize:
		return 28
	case pointer.CursorNorthWestResize, pointer.CursorSouthEastResize, pointer.CursorNorthWestSouthEastResize:
		return 29
	}
	return 1
}

// MARK: Keyboard

func (s *Seat) keyboardEvent(op uint16, r *Reader) {
	switch op {
	case keyboardEventKeymap:
		format, fd, size := r.U32(), r.FD(), r.U32()
		if fd < 0 {
			return
		}
		defer unix.Close(fd)
		if format != 1 {
			return
		}
		data, err := unix.Mmap(fd, 0, int(size), unix.PROT_READ, unix.MAP_PRIVATE)
		if err != nil {
			return
		}
		text := string(data)
		_ = unix.Munmap(data)
		if n := len(text); n > 0 && text[n-1] == 0 {
			text = text[:n-1]
		}
		lay, err := newKeyLayout(text)
		if err != nil {
			s.d.logf("keyboard: %v", err)
			return
		}
		s.lay.close()
		s.lay = lay
	case keyboardEventEnter:
		r.U32()
		surf := r.Obj()
		r.Array()
		if surf != nil {
			s.kbFocus = s.d.surfaces[surf.ID]
		}
		if s.kbFocus != nil {
			s.kbFocus.focus(true)
		}
	case keyboardEventLeave:
		r.U32()
		r.Obj()
		s.stopRepeat()
		if w := s.kbFocus; w != nil {
			s.kbFocus = nil
			w.focus(false)
		}
	case keyboardEventKey:
		r.U32()
		r.U32()
		code, state := r.U32(), r.U32()
		s.key(code, state != 0)
	case keyboardEventModifiers:
		r.U32()
		dep, lat, lock, group := r.U32(), r.U32(), r.U32(), r.U32()
		if s.lay != nil {
			s.lay.update(dep, lat, lock, group)
			s.mods = s.lay.mods()
		}
	case keyboardEventRepeat:
		rate, delay := r.I32(), r.I32()
		s.rate, s.delay = int(rate), time.Duration(delay)*time.Millisecond
	}
}

func (s *Seat) key(code uint32, down bool) {
	w := s.kbFocus
	if w == nil || s.lay == nil {
		return
	}
	name, ok := s.lay.name(code)
	if !ok {
		return
	}
	st := key.Release
	if down {
		st = key.Press
	}
	text := ""
	if down && s.mods&(key.ModCtrl|key.ModAlt|key.ModSuper) == 0 {
		text = s.lay.text(code)
	}
	w.keyEvent(name, s.mods, st, text)
	if !down {
		if s.repKey == code {
			s.stopRepeat()
		}
		return
	}
	s.stopRepeat()
	if s.rate > 0 && s.lay.repeats(code) {
		s.repKey = code
		s.rep = s.d.loop.After(s.delay, func() {
			s.rep = s.d.loop.Every(time.Second/time.Duration(s.rate), func() {
				if w := s.kbFocus; w != nil && s.lay != nil {
					t := ""
					if s.mods&(key.ModCtrl|key.ModAlt|key.ModSuper) == 0 {
						t = s.lay.text(code)
					}
					w.keyEvent(name, s.mods, key.Press, t)
				}
			})
		})
	}
}

func (s *Seat) stopRepeat() {
	s.rep.Stop()
	s.rep = nil
	s.repKey = 0
}

// printable is text worth inserting: not a control character.
func printable(s string) bool {
	if s == "" {
		return false
	}
	for _, r := range s {
		if !unicode.IsPrint(r) {
			return false
		}
	}
	return true
}

func (d *Display) logf(format string, a ...any) {
	if d.Logf != nil {
		d.Logf(format, a...)
	}
}

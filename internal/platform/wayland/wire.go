//go:build linux

// Package wayland is a Wayland client in plain Go: the wire protocol, the few interfaces
// Hover needs (compositor, shm, seat, output, xdg-shell, wlr-layer-shell, cursor-shape), and
// windows that Gio draws into by headless rendering and a shared-memory buffer. Wayland
// only: no X11, no XWayland. The only C in it is Gio's own EGL (the Linux app is a cgo
// build for that); the protocol is spoken directly on the socket.
package wayland

import (
	"encoding/binary"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"sync"

	"golang.org/x/sys/unix"
)

// ID is a protocol object's number.
type ID = uint32

// Conn is the connection to the compositor. Everything on it runs on one goroutine, the
// loop's; requests made from another must go through Loop.Do.
type Conn struct {
	c      *net.UnixConn
	out    []byte
	outFDs []int
	in     []byte
	inFDs  []int
	next   ID
	free   []ID
	objs   map[ID]*Object
	mu     sync.Mutex
}

// Object is a protocol object: its interface name and what its events call.
type Object struct {
	ID    ID
	Iface string
	// On handles an event. The reader is only valid during the call.
	On func(opcode uint16, r *Reader)
}

// Dial connects to $WAYLAND_DISPLAY (or wayland-0) under $XDG_RUNTIME_DIR.
func Dial() (*Conn, error) {
	name := os.Getenv("WAYLAND_DISPLAY")
	if name == "" {
		name = "wayland-0"
	}
	path := name
	if !filepath.IsAbs(path) {
		dir := os.Getenv("XDG_RUNTIME_DIR")
		if dir == "" {
			return nil, errors.New("no Wayland display: $XDG_RUNTIME_DIR is not set")
		}
		path = filepath.Join(dir, name)
	}
	return DialPath(path)
}

// DialPath connects to a socket.
func DialPath(path string) (*Conn, error) {
	c, err := net.DialUnix("unix", nil, &net.UnixAddr{Name: path, Net: "unix"})
	if err != nil {
		return nil, fmt.Errorf("no Wayland display (%v)", err)
	}
	cn := &Conn{c: c, next: 2, objs: map[ID]*Object{}}
	cn.objs[1] = &Object{ID: 1, Iface: "wl_display"}
	return cn, nil
}

// Close ends the connection.
func (c *Conn) Close() { c.c.Close() }

// FD is the socket, for the loop's poll.
func (c *Conn) FD() (int, error) {
	raw, err := c.c.SyscallConn()
	if err != nil {
		return -1, err
	}
	fd := -1
	_ = raw.Control(func(f uintptr) { fd = int(f) })
	return fd, nil
}

// New makes an object with a new id (the client's ids count up from 2; a freed one is
// used again once the compositor has said it is gone).
func (c *Conn) New(iface string) *Object {
	var id ID
	if n := len(c.free); n > 0 {
		id, c.free = c.free[n-1], c.free[:n-1]
	} else {
		id = c.next
		c.next++
	}
	o := &Object{ID: id, Iface: iface}
	c.objs[id] = o
	return o
}

// Forget drops an object (after its destructor request); its id comes back with delete_id.
func (c *Conn) Forget(o *Object) { delete(c.objs, o.ID) }

// MARK: Writing

// Writer builds a request.
type Writer struct {
	c    *Conn
	obj  ID
	op   uint16
	body []byte
}

// Req starts a request: opcode of obj.
func (c *Conn) Req(obj ID, op uint16) *Writer { return &Writer{c: c, obj: obj, op: op} }

func (w *Writer) U32(v uint32) *Writer {
	w.body = binary.LittleEndian.AppendUint32(w.body, v)
	return w
}
func (w *Writer) I32(v int32) *Writer { return w.U32(uint32(v)) }

// Fixed is a 24.8 fixed-point number.
func (w *Writer) Fixed(v float64) *Writer { return w.I32(int32(v * 256)) }

// Obj is an object argument (0 for null).
func (w *Writer) Obj(o *Object) *Writer {
	if o == nil {
		return w.U32(0)
	}
	return w.U32(o.ID)
}

// Str is a string: its length with the NUL, the bytes, padding to 4.
func (w *Writer) Str(s string) *Writer {
	w.U32(uint32(len(s) + 1))
	w.body = append(w.body, s...)
	w.body = append(w.body, 0)
	for len(w.body)%4 != 0 {
		w.body = append(w.body, 0)
	}
	return w
}

// FD sends a file descriptor with the message (it is not in the body).
func (w *Writer) FD(fd int) *Writer { w.c.outFDs = append(w.c.outFDs, fd); return w }

// Send queues the request; Flush writes it.
func (w *Writer) Send() {
	size := 8 + len(w.body)
	w.c.out = binary.LittleEndian.AppendUint32(w.c.out, w.obj)
	w.c.out = binary.LittleEndian.AppendUint32(w.c.out, uint32(size)<<16|uint32(w.op))
	w.c.out = append(w.c.out, w.body...)
}

// Flush writes what is queued. The descriptors go with the first byte of the batch, which
// is the same message (or one of them) for the compositor.
func (c *Conn) Flush() error {
	for len(c.out) > 0 {
		var oob []byte
		if len(c.outFDs) > 0 {
			oob = unix.UnixRights(c.outFDs...)
		}
		n, oobn, err := c.c.WriteMsgUnix(c.out, oob, nil)
		if err != nil {
			return err
		}
		_ = oobn
		c.out = c.out[n:]
		if len(c.outFDs) > 0 {
			for _, fd := range c.outFDs {
				unix.Close(fd)
			}
			c.outFDs = nil
		}
	}
	c.out = c.out[:0]
	return nil
}

// MARK: Reading

// Reader reads an event's arguments.
type Reader struct {
	c    *Conn
	b    []byte
	fds  *[]int
	fail bool
}

func (r *Reader) U32() uint32 {
	if len(r.b) < 4 {
		r.fail = true
		return 0
	}
	v := binary.LittleEndian.Uint32(r.b)
	r.b = r.b[4:]
	return v
}
func (r *Reader) I32() int32     { return int32(r.U32()) }
func (r *Reader) Fixed() float64 { return float64(r.I32()) / 256 }

// Str is a string argument.
func (r *Reader) Str() string {
	n := int(r.U32())
	if n == 0 {
		return ""
	}
	padded := (n + 3) &^ 3
	if len(r.b) < padded {
		r.fail = true
		return ""
	}
	s := string(r.b[:n-1])
	r.b = r.b[padded:]
	return s
}

// Array is an array argument.
func (r *Reader) Array() []byte {
	n := int(r.U32())
	padded := (n + 3) &^ 3
	if len(r.b) < padded {
		r.fail = true
		return nil
	}
	a := r.b[:n]
	r.b = r.b[padded:]
	return a
}

// Obj is an object argument, as the object (nil if it is gone or null).
func (r *Reader) Obj() *Object { return r.c.objs[r.U32()] }

// FD takes the next descriptor that came with the message.
func (r *Reader) FD() int {
	if r.fds == nil || len(*r.fds) == 0 {
		r.fail = true
		return -1
	}
	fd := (*r.fds)[0]
	*r.fds = (*r.fds)[1:]
	return fd
}

// Dispatch reads what the socket has and calls each event's handler. block waits for at
// least one read.
func (c *Conn) Dispatch(block bool) error {
	buf := make([]byte, 64<<10)
	oob := make([]byte, unix.CmsgSpace(4*28))
	if !block {
		raw, err := c.c.SyscallConn()
		if err != nil {
			return err
		}
		var rerr error
		var n, oobn int
		err = raw.Read(func(fd uintptr) bool {
			n, oobn, _, _, rerr = unix.Recvmsg(int(fd), buf, oob, unix.MSG_DONTWAIT|unix.MSG_CMSG_CLOEXEC)
			return true
		})
		if err != nil {
			return err
		}
		if rerr == unix.EAGAIN {
			return nil
		}
		if rerr != nil {
			return rerr
		}
		if n == 0 {
			return errors.New("the compositor closed the connection")
		}
		return c.take(buf[:n], oob[:oobn])
	}
	n, oobn, _, _, err := c.c.ReadMsgUnix(buf, oob)
	if err != nil {
		return err
	}
	if n == 0 {
		return errors.New("the compositor closed the connection")
	}
	return c.take(buf[:n], oob[:oobn])
}

func (c *Conn) take(data, oob []byte) error {
	if len(oob) > 0 {
		msgs, err := unix.ParseSocketControlMessage(oob)
		if err == nil {
			for _, m := range msgs {
				if fds, err := unix.ParseUnixRights(&m); err == nil {
					c.inFDs = append(c.inFDs, fds...)
				}
			}
		}
	}
	c.in = append(c.in, data...)
	for len(c.in) >= 8 {
		id := binary.LittleEndian.Uint32(c.in)
		w := binary.LittleEndian.Uint32(c.in[4:])
		size, op := int(w>>16), uint16(w)
		if size < 8 || size > len(c.in) {
			if size < 8 {
				return errors.New("a broken message from the compositor")
			}
			break // the rest of it is still to come
		}
		body := append([]byte(nil), c.in[8:size]...)
		c.in = c.in[size:]
		if err := c.event(id, op, body); err != nil {
			return err
		}
	}
	return nil
}

func (c *Conn) event(id ID, op uint16, body []byte) error {
	if id == 1 {
		r := &Reader{c: c, b: body}
		switch op {
		case 0: // error(object, code, message)
			obj, code, msg := r.U32(), r.U32(), r.Str()
			name := "?"
			if o := c.objs[obj]; o != nil {
				name = o.Iface
			}
			return fmt.Errorf("Wayland protocol error on %s %d: code %d: %s", name, obj, code, msg)
		case 1: // delete_id
			did := r.U32()
			if _, still := c.objs[did]; !still && did > 1 {
				c.free = append(c.free, did)
			}
		}
		return nil
	}
	o := c.objs[id]
	if o == nil || o.On == nil {
		// An event for an object already destroyed, or one nobody listens to: its
		// descriptors still have to be closed.
		for _, fd := range c.inFDs {
			_ = fd
		}
		return nil
	}
	r := &Reader{c: c, b: body, fds: &c.inFDs}
	o.On(op, r)
	return nil
}

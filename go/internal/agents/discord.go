package agents

// discord.rs: Hover on the user's Discord status, "Playing Hover", the agents at work and
// for how long Hover has been open. It talks to the Discord app on this computer over the
// local connection Discord opens for this (a Unix socket, a named pipe on Windows). No
// internet, no sign-in and no token. Off until the user switches it on in Settings →
// Integrations.
//
// The status clears by itself when Hover quits (the connection closes) or the switch goes off.

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strconv"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

// discordAppID is the "Hover" app in Discord's Developer Portal. Its name there is what
// shows after "Playing".
const discordAppID = "1556886510786314252"

// discordIcon is the picture beside the status (Discord fetches it through its own proxy).
const discordIcon = "https://raw.githubusercontent.com/4regab/Hover/main/assets/hover.png"

// Discord allows 5 status changes per 20 s. One per 5 s stays under it.
const discordGap = 5 * time.Second

// Without news, the status is sent again this often: it finds Discord after it was
// started late and puts the status back after Discord was restarted.
const discordTick = 30 * time.Second

var discordWake = make(chan struct{}, 1)

// DiscordWake: something changed (a switch, a task), look again now.
func DiscordWake() {
	select {
	case discordWake <- struct{}{}:
	default:
	}
}

// discordSleep waits for DiscordWake or d; true when woken.
func discordSleep(d time.Duration) bool {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-discordWake:
		return true
	case <-t.C:
		return false
	}
}

// DiscordStart starts the status for this run. The goroutine idles while the switch is off.
func DiscordStart(settings *core.Settings, sessions *KiroSessions) {
	sessions.OnChanged(DiscordWake)
	want := func() []string {
		if !settings.DiscordPresence() {
			return nil
		}
		return discordWorking(sessions)
	}
	go discordRun(discordAppID, want, discordConnect)
}

// discordWorking are the tools with a task at work, each once, in the order they started.
// Never nil: nil is the switch off.
func discordWorking(sessions *KiroSessions) []string {
	names := []string{}
	for _, s := range sessions.AllLight() {
		if s.Busy() && !slices.Contains(names, s.Tool.Name()) {
			names = append(names, s.Tool.Name())
		}
	}
	return names
}

// discordActivity is the status: how many agents are at work and which, or Idle.
func discordActivity(tools []string, started int64) core.JSON {
	details := "Idle"
	switch n := len(tools); {
	case n == 1:
		details = "1 agent working"
	case n > 1:
		details = fmt.Sprintf("%d agents working", n)
	}
	props := []core.Prop{core.P("details", core.JStr(details))}
	if len(tools) > 0 {
		joined := ""
		for i, t := range tools {
			if i > 0 {
				joined += ", "
			}
			joined += t
		}
		props = append(props, core.P("state", core.JStr(runesFrom(joined, 0, 128))))
	}
	props = append(props, core.P("timestamps", core.JObj(core.P("start", core.JInt(started)))),
		core.P("assets", core.JObj(core.P("large_image", core.JStr(discordIcon)), core.P("large_text", core.JStr("Hover")))))
	return core.JObj(props...)
}

// discordRun is the loop: want says nil while the status is off, else the tools at work.
func discordRun(appID string, want func() []string, connect func() (io.ReadWriteCloser, error)) {
	started := time.Now().Unix()
	var link *discordLink
	// What Discord shows now (nil: nothing), and when it was last told.
	var shown *string
	var told time.Time
	complained, force := false, false
	for {
		tools := want()
		wait := discordTick
		if tools == nil {
			if link != nil {
				link.set(nil)
				link.pipe.Close()
				link = nil
			}
			shown, complained = nil, false
		} else if b := discordActivity(tools, started); force || shown == nil || *shown != b.Compact() {
			text := b.Compact()
			if gap := discordGap - time.Since(told); !told.IsZero() && gap > 0 {
				wait = gap
			} else {
				told = time.Now()
				l := link
				link = nil
				var err error
				if l == nil {
					var p io.ReadWriteCloser
					if p, err = connect(); err == nil {
						if l, err = discordOpen(p, appID); err != nil {
							p.Close()
						}
					}
				}
				if err == nil {
					if err = l.set(&b); err != nil {
						l.pipe.Close()
					}
				}
				if err == nil {
					link, shown, complained = l, &text, false
				} else if !complained {
					// Not running is the usual reason, and is tried again on the next tick.
					core.Logf("discord status not sent: %v", err)
					complained = true
				}
			}
		}
		force = !discordSleep(wait)
	}
}

// discordLink is a connection to Discord that has shaken hands.
type discordLink struct {
	pipe  io.ReadWriteCloser
	nonce uint64
}

// discordFrame is one message: opcode and length (both u32, little endian), then JSON.
func discordFrame(op uint32, body string) []byte {
	v := make([]byte, 8, 8+len(body))
	binary.LittleEndian.PutUint32(v, op)
	binary.LittleEndian.PutUint32(v[4:], uint32(len(body)))
	return append(v, body...)
}

func discordRead(p io.Reader) (uint32, core.JSON, error) {
	var head [8]byte
	if _, err := io.ReadFull(p, head[:]); err != nil {
		return 0, core.JNull, err
	}
	op, n := binary.LittleEndian.Uint32(head[:]), binary.LittleEndian.Uint32(head[4:])
	if n > 64*1024 {
		return 0, core.JNull, errors.New("Discord sent an oversized message")
	}
	body := make([]byte, n)
	if _, err := io.ReadFull(p, body); err != nil {
		return 0, core.JNull, err
	}
	v, err := core.ParseJSON(core.Lossy(body))
	return op, v, err
}

func discordOpen(pipe io.ReadWriteCloser, appID string) (*discordLink, error) {
	hello := core.JObj(core.P("v", core.JInt(1)), core.P("client_id", core.JStr(appID))).Compact()
	if _, err := pipe.Write(discordFrame(0, hello)); err != nil {
		return nil, err
	}
	_, reply, err := discordRead(pipe)
	if err != nil {
		return nil, err
	}
	if evt, _ := str(reply, "evt"); evt != "READY" {
		return nil, fmt.Errorf("Discord refused: %s", reply.Compact())
	}
	return &discordLink{pipe: pipe}, nil
}

// set shows the status, or clears it (nil). Waits for Discord's answer so an error is seen.
func (l *discordLink) set(activity *core.JSON) error {
	l.nonce++
	nonce := strconv.FormatUint(l.nonce, 10)
	args := []core.Prop{core.P("pid", core.JInt(int64(os.Getpid())))}
	if activity != nil {
		args = append(args, core.P("activity", *activity))
	}
	msg := core.JObj(core.P("cmd", core.JStr("SET_ACTIVITY")), core.P("args", core.JObj(args...)), core.P("nonce", core.JStr(nonce)))
	if _, err := l.pipe.Write(discordFrame(1, msg.Compact())); err != nil {
		return err
	}
	// ponytail: on Windows a pipe read has no timeout, so a Discord that never answers
	// stalls this goroutine (only the status). Upgrade: overlapped I/O.
	for range 8 {
		op, reply, err := discordRead(l.pipe)
		if err != nil {
			return err
		}
		switch n, _ := str(reply, "nonce"); {
		case op == 3:
			// Ping: answered with the same body.
			if _, err := l.pipe.Write(discordFrame(4, reply.Compact())); err != nil {
				return err
			}
		case op == 2:
			return errors.New("Discord closed the connection")
		case n == nonce:
			if evt, _ := str(reply, "evt"); evt == "ERROR" {
				data := ""
				if d, ok := reply.Get("data"); ok {
					data = d.Compact()
				}
				return fmt.Errorf("Discord said: %s", data)
			}
			return nil
		}
	}
	return errors.New("Discord did not answer")
}

// timedConn gives each read and write 5 s, as Rust's socket timeouts did.
type timedConn struct{ net.Conn }

func (c timedConn) Read(b []byte) (int, error) {
	c.SetReadDeadline(time.Now().Add(5 * time.Second))
	return c.Conn.Read(b)
}

func (c timedConn) Write(b []byte) (int, error) {
	c.SetWriteDeadline(time.Now().Add(5 * time.Second))
	return c.Conn.Write(b)
}

var errNoDiscord = errors.New("Discord isn't running")

// discordConnect is Discord's connection: the first of discord-ipc-0 to -9 that answers, in
// the places the desktop app, Flatpak and Snap put it.
func discordConnect() (io.ReadWriteCloser, error) {
	if runtime.GOOS == "windows" {
		for i := range 10 {
			if f, err := os.OpenFile(fmt.Sprintf(`\\.\pipe\discord-ipc-%d`, i), os.O_RDWR, 0); err == nil {
				return f, nil
			}
		}
		return nil, errNoDiscord
	}
	inside := []string{"", "app/com.discordapp.Discord/", "app/dev.vencord.Vesktop/", ".flatpak/com.discordapp.Discord/xdg-run/",
		".flatpak/dev.vencord.Vesktop/xdg-run/", "snap.discord/", "snap.discord-canary/"}
	var bases []string
	for _, v := range []string{"XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"} {
		if b, ok := os.LookupEnv(v); ok {
			bases = append(bases, b)
		}
	}
	bases = slices.Compact(append(bases, "/tmp"))
	for _, base := range bases {
		for _, in := range inside {
			for i := range 10 {
				path := filepath.Join(base, in, fmt.Sprintf("discord-ipc-%d", i))
				if _, err := os.Stat(path); err != nil {
					continue
				}
				if c, err := net.Dial("unix", path); err == nil {
					return timedConn{c}, nil
				}
			}
		}
	}
	return nil, errNoDiscord
}

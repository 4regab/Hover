package agents

import (
	"bufio"
	"encoding/base64"
	"fmt"
	"net"
	"strings"
	"testing"
	"time"
)

func TestHelpersMatchDotnet(t *testing.T) {
	if base64.StdEncoding.EncodeToString([]byte("opencode:pw")) != "b3BlbmNvZGU6cHc=" || base64.StdEncoding.EncodeToString([]byte("ab")) != "YWI=" {
		t.Error("base64")
	}
	if got := EscapeData("/tmp/hover oc ü"); got != "%2Ftmp%2Fhover%20oc%20%C3%BC" {
		t.Error(got)
	}
	for u, want := range map[string]string{"http://127.0.0.1:4096": "127.0.0.1 4096", "http://localhost:9/x": "localhost 9", "http://[::1]:5/": "::1 5"} {
		h, p, ok := HostPort(u)
		if !ok || fmt.Sprintf("%s %d", h, p) != want {
			t.Errorf("%s: %s %d", u, h, p)
		}
	}
	if !IsLoopback("127.0.0.1") || !IsLoopback("::1") || !IsLoopback("LOCALHOST") || IsLoopback("0.0.0.0") || IsLoopback("10.0.0.2") {
		t.Error("loopback")
	}
}

func TestChunkedAndSizedBodiesAndAStop(t *testing.T) {
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	port := l.Addr().(*net.TCPAddr).Port
	go func() {
		for i := 0; ; i++ {
			s, err := l.Accept()
			if err != nil {
				return
			}
			r := bufio.NewReader(s)
			for {
				h, _ := r.ReadString('\n')
				if strings.TrimSpace(h) == "" {
					break
				}
			}
			switch i {
			case 0:
				s.Write([]byte("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"))
			case 1:
				s.Write([]byte("HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\n\r\n{}"))
			default:
				s.Write([]byte("HTTP/1.1 200 OK\r\n\r\ndata: a\n\n"))
				time.Sleep(5 * time.Second)
			}
			s.Close()
		}
	}()
	c, _ := NewHttpClient(fmt.Sprintf("http://127.0.0.1:%d", port), "opencode", "pw")
	if st, body, err := c.Call("GET", "/a", nil, 5*time.Second, nil); err != nil || st != 200 || body != "hello world" {
		t.Errorf("%d %q %v", st, body, err)
	}
	if st, body, err := c.Call("GET", "/b", nil, 5*time.Second, nil); err != nil || st != 404 || body != "{}" {
		t.Errorf("%d %q %v", st, body, err)
	}
	stop := NewCancel()
	r, herr := c.Open("GET", "/event", nil, 0, stop)
	if herr != nil {
		t.Fatal(herr)
	}
	if line, ok, err := r.Line(); !ok || err != nil || line != "data: a" {
		t.Errorf("%q", line)
	}
	if line, ok, err := r.Line(); !ok || err != nil || line != "" {
		t.Errorf("%q", line)
	}
	go func() { time.Sleep(100 * time.Millisecond); stop.Cancel() }()
	start := time.Now()
	if _, _, err := r.Line(); err == nil || err.Kind != HttpCancelled {
		t.Errorf("%v", err)
	}
	if time.Since(start) > 2*time.Second {
		t.Error("slow stop")
	}
}

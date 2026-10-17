package core

import (
	"bufio"
	"io"
	"os"
	"runtime"
	"strings"
	"testing"
)

func TestReadLineLimitKeepsShortLines(t *testing.T) {
	r := bufio.NewReader(strings.NewReader("a\r\nb\n\nc"))
	got := []string{}
	for {
		line, skipped, err := ReadLineLimit(r, 16)
		if skipped {
			t.Fatal("skipped a short line")
		}
		if len(line) > 0 || err == nil {
			got = append(got, string(line))
		}
		if err != nil {
			if err != io.EOF {
				t.Fatal(err)
			}
			break
		}
	}
	if strings.Join(got, ",") != "a,b,,c" {
		t.Fatalf("%q", got)
	}
}

func TestReadLineLimitDropsARunawayLineWithoutKeepingIt(t *testing.T) {
	const limit = 1024
	const huge = 32 << 20
	pr, pw, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer pr.Close()
	go func() {
		defer pw.Close()
		buf := make([]byte, 64<<10)
		for i := range buf {
			buf[i] = 'x'
		}
		left := huge
		for left > 0 {
			n := len(buf)
			if n > left {
				n = left
			}
			if _, err := pw.Write(buf[:n]); err != nil {
				return
			}
			left -= n
		}
		_, _ = pw.Write([]byte("\n{\"ok\":true}\n"))
	}()
	r := bufio.NewReader(pr)
	runtime.GC()
	var before runtime.MemStats
	runtime.ReadMemStats(&before)
	line, skipped, err := ReadLineLimit(r, limit)
	var after runtime.MemStats
	runtime.ReadMemStats(&after)
	if !skipped || line != nil || err != nil {
		t.Fatalf("skipped=%v line=%d err=%v", skipped, len(line), err)
	}
	if grew := after.TotalAlloc - before.TotalAlloc; grew > 4<<20 {
		t.Fatalf("the runaway line was kept: alloc grew %d", grew)
	}
	next, skipped, err := ReadLineLimit(r, limit)
	if skipped || err != nil || string(next) != `{"ok":true}` {
		t.Fatalf("next %q skipped=%v err=%v", next, skipped, err)
	}
}

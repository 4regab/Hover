package agents

import (
	"bufio"
	"fmt"
	"os"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
)

func TestAKilledGroupTakesWhatItStarted(t *testing.T) {
	c := Hidden("/bin/sh", "-c", "sleep 300 & echo $!; wait")
	null, _ := os.Open(os.DevNull)
	defer null.Close()
	c.Stdin = null
	g, err := Spawn(c)
	if err != nil {
		t.Fatal(err)
	}
	_, out, _ := g.TakePipes()
	line, _ := bufio.NewReader(out).ReadString('\n')
	grandchild, err := strconv.Atoi(strings.TrimSpace(line))
	if err != nil {
		t.Fatal(err)
	}
	if syscall.Kill(grandchild, 0) != nil {
		t.Fatal("the grandchild doesn't run")
	}
	g.Kill()
	start := time.Now()
	for syscall.Kill(grandchild, 0) == nil && time.Since(start) < 5*time.Second {
		time.Sleep(20 * time.Millisecond)
	}
	// Killed, and reaped by init (it was re-parented), or a zombie at worst.
	state, _ := os.ReadFile(fmt.Sprintf("/proc/%d/stat", grandchild))
	if s := string(state); s != "" && !strings.Contains(s, ") Z") {
		t.Error(s)
	}
}

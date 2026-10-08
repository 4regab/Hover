package core

import (
	"fmt"
	"os"
	"strings"
	"testing"
	"time"
)

// The tests of single.rs, one for one.

func TestASocketPathMustFitSunPath(t *testing.T) {
	if !socketFits("/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T", "hover.sock") {
		t.Fatal("the macOS TMPDIR fits")
	}
	if socketFits("/"+strings.Repeat("d", 90), "hover.sock") {
		t.Fatal("a 91-byte folder doesn't")
	}
	if !socketFits("/tmp/hover-501", "hovertest123456.sock") {
		t.Fatal("/tmp fits")
	}
}

func TestASecondClaimAsksTheFirstToShow(t *testing.T) {
	name := fmt.Sprintf("HoverTest%d", os.Getpid())
	shown := make(chan struct{}, 1)
	first, err := ClaimNamed(name, func(*string) { shown <- struct{}{} })
	if err != nil || first == nil {
		t.Fatal("the first claim", err)
	}
	if second, err := ClaimNamed(name, func(*string) {}); err != nil || second != nil {
		t.Fatal("a second claim got the instance", err)
	}
	select {
	case <-shown:
	case <-time.After(5 * time.Second):
		t.Fatal("the first copy was not asked to show")
	}
	first.Release()
	// Once the first has gone, the name is free again.
	again, err := ClaimNamed(name, func(*string) {})
	if err != nil || again == nil {
		t.Fatal("the name is free again", err)
	}
	again.Release()
}

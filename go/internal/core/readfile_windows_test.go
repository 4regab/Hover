package core

import (
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"testing"
)

// A history save writes a temporary file and renames it over the real one. On Windows that
// fails while anything has the real one open, unless it was opened to allow it.

// The cause: Go's own open does not allow it. If a later Go does, this fails, and
// ReadFile is no longer needed.
func TestGoOpensAFileOnWindowsWithoutLettingOthersRenameOverIt(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "index.dat")
	os.WriteFile(target, []byte("old"), 0o666)
	os.WriteFile(target+".tmp", []byte("new"), 0o666)
	f, err := os.Open(target)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := os.Rename(target+".tmp", target); err == nil {
		t.Error("a rename over a file opened with os.Open worked")
	} else {
		t.Logf("as expected: %v", err)
	}
}

// The fix: a file read with ReadFile can be renamed over at any moment.
func TestAFileBeingReadWithReadFileCanBeRenamedOver(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "index.dat")
	os.WriteFile(target, []byte("0"), 0o666)
	stop := make(chan struct{})
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for {
			select {
			case <-stop:
				return
			default:
				ReadFile(target)
			}
		}
	}()
	failed := 0
	for i := range 500 {
		tmp := target + ".tmp"
		if err := os.WriteFile(tmp, []byte(strconv.Itoa(i)), 0o666); err != nil {
			t.Fatal(err)
		}
		if err := os.Rename(tmp, target); err != nil {
			failed++
			t.Log(err)
		}
	}
	close(stop)
	wg.Wait()
	if failed != 0 {
		t.Errorf("%d of 500 renames failed while the file was read with ReadFile", failed)
	}
	if b, err := ReadFile(target); err != nil || string(b) != "499" {
		t.Errorf("%q %v", b, err)
	}
	if _, err := ReadFile(filepath.Join(dir, "none")); !os.IsNotExist(err) {
		t.Errorf("a missing file: %v", err)
	}
}

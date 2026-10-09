package core

import (
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"testing"
)

// A history save writes a temporary file and renames it over the real one. On Windows that
// fails while another goroutine has the real one open, unless it was opened to allow it.
// This measures it, both ways, and the CI log has the numbers.
func TestAFileBeingReadCanBeRenamedOver(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "index.dat")
	run := func(read func(string) ([]byte, error)) (failed, total int) {
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
					read(target)
				}
			}
		}()
		for i := range 300 {
			tmp := target + ".tmp"
			if err := os.WriteFile(tmp, []byte(strconv.Itoa(i)), 0o666); err != nil {
				t.Fatal(err)
			}
			if err := os.Rename(tmp, target); err != nil {
				failed++
			}
			total++
		}
		close(stop)
		wg.Wait()
		return
	}
	plain, n := run(os.ReadFile)
	t.Logf("os.ReadFile: %d of %d renames failed while it was read", plain, n)
	shared, n := run(ReadFile)
	t.Logf("core.ReadFile: %d of %d renames failed while it was read", shared, n)
	if shared != 0 {
		t.Errorf("%d renames failed while the file was read with ReadFile", shared)
	}
	if b, err := ReadFile(target); err != nil || string(b) != "299" {
		t.Errorf("%q %v", b, err)
	}
	if _, err := ReadFile(filepath.Join(dir, "none")); !os.IsNotExist(err) {
		t.Errorf("a missing file: %v", err)
	}
}

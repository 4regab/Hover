package core

import (
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"testing"
)

// A history save writes a temporary file and renames it over the real one. On Windows two
// things stop that while the real one is being read, and Rust's std gets round both:
//  1. Go opens the file without letting others rename it (Open allows it).
//  2. Even then MoveFileExW answers "access denied" when it replaces a file that is open
//     (Rename tries again with POSIX semantics, as Rust's rename does).

// The first cause: Go's own open does not allow a rename over. If a later Go does, this
// fails, and Open is no longer needed.
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

// renamesWhileRead renames a new file over target 500 times while another goroutine reads
// it all the time, and says how many renames failed.
func renamesWhileRead(t *testing.T, read func(string) ([]byte, error), rename func(string, string) error) (failed int, last error) {
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
				read(target)
			}
		}
	}()
	for i := range 500 {
		tmp := target + ".tmp"
		if err := os.WriteFile(tmp, []byte(strconv.Itoa(i)), 0o666); err != nil {
			t.Fatal(err)
		}
		if err := rename(tmp, target); err != nil {
			failed, last = failed+1, err
		}
	}
	close(stop)
	wg.Wait()
	// When every rename went through, the last one's text is what is there.
	if b, err := ReadFile(target); failed == 0 && (err != nil || string(b) != "499") {
		t.Errorf("the last save: %q %v", b, err)
	}
	return
}

// The second cause: with sharing allowed, a plain os.Rename still fails now and then. This
// logs how often (CI shows it); it does not assert, since it depends on timing.
func TestAPlainRenameOverAFileBeingReadWithSharingStillFailsNowAndThen(t *testing.T) {
	failed, last := renamesWhileRead(t, ReadFile, os.Rename)
	t.Logf("os.Rename with ReadFile: %d of 500 renames failed (%v)", failed, last)
}

// The fix: a file read with ReadFile can be renamed over with Rename at any moment.
func TestAFileBeingReadWithReadFileCanBeRenamedOver(t *testing.T) {
	if failed, last := renamesWhileRead(t, ReadFile, Rename); failed != 0 {
		t.Errorf("%d of 500 renames failed while the file was read with ReadFile (%v)", failed, last)
	}
}

func TestReadFileOfAMissingFileIsNotExist(t *testing.T) {
	if _, err := ReadFile(filepath.Join(t.TempDir(), "none")); !os.IsNotExist(err) {
		t.Errorf("a missing file: %v", err)
	}
	if err := Rename(filepath.Join(t.TempDir(), "none"), filepath.Join(t.TempDir(), "to")); err == nil {
		t.Error("a rename of a missing file worked")
	}
}

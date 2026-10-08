package agents

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

// AcpHostTests.Only_a_full_path_to_an_existing_folder_is_usable.
func TestOnlyAFullPathToAnExistingFolderIsUsable(t *testing.T) {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-usable-%d", os.Getpid()))
	os.MkdirAll(d, 0o777)
	f := filepath.Join(d, "f.txt")
	os.WriteFile(f, nil, 0o666)
	for _, bad := range []string{"", "  ", "relative/dir", filepath.Join(d, "gone"), f} {
		if UsableFolder(bad) {
			t.Errorf("%q is usable", bad)
		}
	}
	if !UsableFolder(d) {
		t.Errorf("%q isn't usable", d)
	}
}

func TestKiroAgentsByTheirNames(t *testing.T) {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-kagents-%d", os.Getpid()))
	a := filepath.Join(d, ".kiro/agents")
	os.MkdirAll(a, 0o777)
	os.WriteFile(filepath.Join(a, "one.json"), []byte(`{"name":"reviewer"}`), 0o666)
	os.WriteFile(filepath.Join(a, "two.json"), []byte("not json"), 0o666)
	os.WriteFile(filepath.Join(a, "three.json"), []byte(`{"name":"bad name!"}`), 0o666)
	got := KiroAgents(d)
	if !slices.Contains(got, "reviewer") || !slices.Contains(got, "two") || slices.ContainsFunc(got, func(n string) bool { return strings.Contains(n, " ") }) {
		t.Errorf("%q", got)
	}
}

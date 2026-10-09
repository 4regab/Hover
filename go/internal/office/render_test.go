package office

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// office.wgsl is a copy (go:embed can't reach outside the module): it must not drift from
// the Rust office's.
func TestTheShaderIsTheRustOfficesOwn(t *testing.T) {
	_, f, _, _ := runtime.Caller(0)
	b, err := os.ReadFile(filepath.Join(filepath.Dir(f), "..", "..", "..", "crates", "hover-office", "src", "office.wgsl"))
	if err != nil {
		t.Fatal(err)
	}
	if lf := func(s string) string { return strings.ReplaceAll(s, "\r\n", "\n") }; lf(string(b)) != lf(officeWGSL) {
		t.Error("go/internal/office/office.wgsl differs from crates/hover-office/src/office.wgsl: copy it again")
	}
}

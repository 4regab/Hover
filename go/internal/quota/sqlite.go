package quota

// sqlite.rs: one scalar out of an SQLite file, opened read-only, as Microsoft.Data.Sqlite's
// ExecuteScalar gives it. Windows calls the winsqlite3.dll that ships with Windows 10 and
// 11 (sqlite_windows.go), so nothing is compiled or bundled. The other systems have no
// reader yet (sqlite_other.go).

import "unicode/utf16"

// ScalarKind is what ExecuteScalar hands back, as far as Quota.CursorToken looks at it.
type ScalarKind int

const (
	ScalarOther ScalarKind = iota
	ScalarText
	ScalarBlob
)

type Scalar struct {
	Kind ScalarKind
	Text string
	Blob []byte
}

// utf16Lossy is String::from_utf16_lossy: a lone surrogate becomes U+FFFD.
func utf16Lossy(units []uint16) string { return string(utf16.Decode(units)) }

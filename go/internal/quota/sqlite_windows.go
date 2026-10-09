package quota

import (
	"fmt"
	"unsafe"

	"golang.org/x/sys/windows"
)

// winsqlite3.dll is called through the calls `scalar` and `exec` make, loaded when first
// used, so Hover starts on a Windows without it.
var (
	winsqlite = windows.NewLazySystemDLL("winsqlite3.dll")
	pOpen     = winsqlite.NewProc("sqlite3_open_v2")
	pBusy     = winsqlite.NewProc("sqlite3_busy_timeout")
	pPrepare  = winsqlite.NewProc("sqlite3_prepare_v2")
	pStep     = winsqlite.NewProc("sqlite3_step")
	pColType  = winsqlite.NewProc("sqlite3_column_type")
	pColText  = winsqlite.NewProc("sqlite3_column_text")
	pColBlob  = winsqlite.NewProc("sqlite3_column_blob")
	pColBytes = winsqlite.NewProc("sqlite3_column_bytes")
	pFinalize = winsqlite.NewProc("sqlite3_finalize")
	pClose    = winsqlite.NewProc("sqlite3_close")
	pErrmsg   = winsqlite.NewProc("sqlite3_errmsg")
	pExec     = winsqlite.NewProc("sqlite3_exec")
)

const (
	sqliteOpenReadonly  = 0x1
	sqliteOpenReadwrite = 0x2
	sqliteOpenCreate    = 0x4
	sqliteRow           = 100
	sqliteDone          = 101
	sqliteText          = 3
	sqliteBlob          = 4
)

// at is the memory a pointer that SQLite handed back points at.
func at(p uintptr) unsafe.Pointer { return *(*unsafe.Pointer)(unsafe.Pointer(&p)) }

func cstring(p uintptr) string {
	if p == 0 {
		return ""
	}
	n := 0
	for *(*byte)(unsafe.Add(at(p), n)) != 0 {
		n++
	}
	return string(unsafe.Slice((*byte)(at(p)), n))
}

func message(db uintptr, code uintptr) error {
	m, _, _ := pErrmsg.Call(db)
	// Microsoft.Data.Sqlite's SqliteException message.
	return fmt.Errorf("SQLite Error %d: '%s'.", int32(code), cstring(m))
}

// scalar is the first column of the first row, or Other when there is no row or it isn't
// text or a blob. Read-only and closed at once, so Hover never holds the database open; a
// lock is waited on for two seconds (DefaultTimeout = 2), no longer.
func scalar(path, sql string) (Scalar, error) {
	name, err := windows.BytePtrFromString(path)
	if err != nil {
		return Scalar{}, err
	}
	query, err := windows.BytePtrFromString(sql)
	if err != nil {
		return Scalar{}, err
	}
	var db uintptr
	rc, _, _ := pOpen.Call(uintptr(unsafe.Pointer(name)), uintptr(unsafe.Pointer(&db)), sqliteOpenReadonly, 0)
	defer pClose.Call(db)
	if int32(rc) != 0 {
		return Scalar{}, message(db, rc)
	}
	pBusy.Call(db, 2000)
	var stmt uintptr
	rc, _, _ = pPrepare.Call(db, uintptr(unsafe.Pointer(query)), ^uintptr(0), uintptr(unsafe.Pointer(&stmt)), 0)
	if int32(rc) != 0 {
		return Scalar{}, message(db, rc)
	}
	defer pFinalize.Call(stmt)
	step, _, _ := pStep.Call(stmt)
	switch int32(step) {
	case sqliteRow:
		kind, _, _ := pColType.Call(stmt, 0)
		switch int32(kind) {
		case sqliteText:
			p, _, _ := pColText.Call(stmt, 0)
			n, _, _ := pColBytes.Call(stmt, 0)
			if p == 0 {
				return Scalar{Kind: ScalarText}, nil
			}
			return Scalar{Kind: ScalarText, Text: string(unsafe.Slice((*byte)(at(p)), int(int32(n))))}, nil
		case sqliteBlob:
			p, _, _ := pColBlob.Call(stmt, 0)
			n, _, _ := pColBytes.Call(stmt, 0)
			if p == 0 {
				return Scalar{Kind: ScalarBlob}, nil
			}
			return Scalar{Kind: ScalarBlob, Blob: append([]byte(nil), unsafe.Slice((*byte)(at(p)), int(int32(n)))...)}, nil
		}
		return Scalar{}, nil
	case sqliteDone:
		return Scalar{}, nil
	}
	return Scalar{}, message(db, step)
}

// exec runs SQL against a file, made if missing: the tests' stand-in for Cursor writing its
// database.
func exec(path, sql string) error {
	name, err := windows.BytePtrFromString(path)
	if err != nil {
		return err
	}
	query, err := windows.BytePtrFromString(sql)
	if err != nil {
		return err
	}
	var db uintptr
	rc, _, _ := pOpen.Call(uintptr(unsafe.Pointer(name)), uintptr(unsafe.Pointer(&db)), sqliteOpenReadwrite|sqliteOpenCreate, 0)
	defer pClose.Call(db)
	if int32(rc) != 0 {
		return message(db, rc)
	}
	rc, _, _ = pExec.Call(db, uintptr(unsafe.Pointer(query)), 0, 0, 0)
	if int32(rc) != 0 {
		return message(db, rc)
	}
	return nil
}

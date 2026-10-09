//go:build !windows

package quota

import "errors"

// scalar has no SQLite to read with here yet. ponytail: Cursor's sign-in is only read on
// Windows for now. Linux (phase 6) needs a library that needs no C compiler and no system
// libsqlite3 (an AppImage can't count on one): a pure-Go SQLite, or dlopen. The Mac
// (phase 7) has /usr/lib/libsqlite3.dylib to call.
func scalar(path, sql string) (Scalar, error) {
	return Scalar{}, errors.New("reading Cursor’s sign-in isn’t supported on this system yet")
}

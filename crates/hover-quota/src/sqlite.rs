//! One scalar out of an SQLite file, opened read-only, as Microsoft.Data.Sqlite's
//! ExecuteScalar gives it. Linux uses the SQLite that libsqlite3-sys builds in; Windows
//! calls the winsqlite3.dll that ships with Windows 10 and 11, and macOS the libsqlite3
//! that ships with it (nothing is compiled for either).

use std::ffi::{c_int, CStr, CString};
#[cfg(not(target_os = "linux"))]
use std::ffi::{c_char, c_void};

#[cfg(target_os = "linux")]
use libsqlite3_sys::sqlite3;

#[cfg(not(target_os = "linux"))]
#[allow(non_camel_case_types)]
pub enum sqlite3 {}
#[cfg(not(target_os = "linux"))]
#[allow(non_camel_case_types)]
pub enum sqlite3_stmt {}

#[cfg(target_os = "linux")]
use libsqlite3_sys::{sqlite3_busy_timeout, sqlite3_close, sqlite3_column_blob, sqlite3_column_bytes, sqlite3_column_text,
    sqlite3_column_type, sqlite3_errmsg, sqlite3_exec, sqlite3_finalize, sqlite3_open_v2, sqlite3_prepare_v2, sqlite3_step};

/// The calls `scalar` and `exec` make, declared once for the two OSes that ship a SQLite
/// of their own; each is given its library and ABI below.
#[cfg(not(target_os = "linux"))]
macro_rules! system_sqlite {
    ($(#[$lib:meta])* $abi:literal) => {
        $(#[$lib])*
        extern $abi {
            fn sqlite3_open_v2(filename: *const c_char, db: *mut *mut sqlite3, flags: c_int, vfs: *const c_char) -> c_int;
            fn sqlite3_busy_timeout(db: *mut sqlite3, ms: c_int) -> c_int;
            fn sqlite3_prepare_v2(db: *mut sqlite3, sql: *const c_char, n: c_int, stmt: *mut *mut sqlite3_stmt, tail: *mut *const c_char) -> c_int;
            fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
            fn sqlite3_column_type(stmt: *mut sqlite3_stmt, col: c_int) -> c_int;
            fn sqlite3_column_text(stmt: *mut sqlite3_stmt, col: c_int) -> *const u8;
            fn sqlite3_column_blob(stmt: *mut sqlite3_stmt, col: c_int) -> *const c_void;
            fn sqlite3_column_bytes(stmt: *mut sqlite3_stmt, col: c_int) -> c_int;
            fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
            fn sqlite3_close(db: *mut sqlite3) -> c_int;
            fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
            fn sqlite3_exec(db: *mut sqlite3, sql: *const c_char, cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>, arg: *mut c_void, err: *mut *mut c_char) -> c_int;
        }
    };
}

// raw-dylib: no import library is needed to build, so the MSVC check cross-compiles.
#[cfg(windows)]
system_sqlite!(#[link(name = "winsqlite3", kind = "raw-dylib")] "system");

// /usr/lib/libsqlite3.dylib, which every Mac has (the SDK links it through its .tbd).
#[cfg(target_os = "macos")]
system_sqlite!(#[link(name = "sqlite3")] "C");

const SQLITE_OPEN_READONLY: c_int = 0x1;
const SQLITE_OPEN_READWRITE: c_int = 0x2;
const SQLITE_OPEN_CREATE: c_int = 0x4;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_TEXT: c_int = 3;
const SQLITE_BLOB: c_int = 4;

/// What ExecuteScalar hands back, as far as Quota.CursorToken looks at it.
#[derive(Debug, PartialEq)]
pub enum Scalar { Text(String), Blob(Vec<u8>), Other }

struct Db(*mut sqlite3);

impl Drop for Db {
    fn drop(&mut self) { unsafe { sqlite3_close(self.0); } }
}

fn message(db: *mut sqlite3, code: c_int) -> String {
    let m = unsafe { sqlite3_errmsg(db) };
    let text = if m.is_null() { String::new() } else { unsafe { CStr::from_ptr(m) }.to_string_lossy().into_owned() };
    // Microsoft.Data.Sqlite's SqliteException message.
    format!("SQLite Error {code}: '{text}'.")
}

/// The first column of the first row, or Other when there is no row or it isn't
/// text or a blob. Read-only and closed at once, so Hover never holds the database
/// open; a lock is waited on for two seconds (DefaultTimeout = 2), no longer.
pub fn scalar(path: &std::path::Path, sql: &str) -> Result<Scalar, String> {
    let name = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let sql = CString::new(sql).map_err(|e| e.to_string())?;
    let mut raw = std::ptr::null_mut();
    let rc = unsafe { sqlite3_open_v2(name.as_ptr(), &mut raw, SQLITE_OPEN_READONLY, std::ptr::null()) };
    let db = Db(raw);
    if rc != 0 { return Err(message(db.0, rc)); }
    unsafe { sqlite3_busy_timeout(db.0, 2000); }
    let mut stmt = std::ptr::null_mut();
    let rc = unsafe { sqlite3_prepare_v2(db.0, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) };
    if rc != 0 { return Err(message(db.0, rc)); }
    let out = unsafe {
        match sqlite3_step(stmt) {
            SQLITE_ROW => match sqlite3_column_type(stmt, 0) {
                SQLITE_TEXT => {
                    let p = sqlite3_column_text(stmt, 0);
                    let n = sqlite3_column_bytes(stmt, 0) as usize;
                    Ok(Scalar::Text(if p.is_null() { String::new() } else { String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned() }))
                }
                SQLITE_BLOB => {
                    let p = sqlite3_column_blob(stmt, 0) as *const u8;
                    let n = sqlite3_column_bytes(stmt, 0) as usize;
                    Ok(Scalar::Blob(if p.is_null() { vec![] } else { std::slice::from_raw_parts(p, n).to_vec() }))
                }
                _ => Ok(Scalar::Other),
            },
            SQLITE_DONE => Ok(Scalar::Other),
            rc => Err(message(db.0, rc)),
        }
    };
    unsafe { sqlite3_finalize(stmt); }
    out
}

/// Runs SQL against a file, made if missing: the tests' stand-in for Cursor writing
/// its database.
#[doc(hidden)]
pub fn exec(path: &std::path::Path, sql: &str) -> Result<(), String> {
    let name = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let sql = CString::new(sql).map_err(|e| e.to_string())?;
    let mut raw = std::ptr::null_mut();
    let rc = unsafe { sqlite3_open_v2(name.as_ptr(), &mut raw, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, std::ptr::null()) };
    let db = Db(raw);
    if rc != 0 { return Err(message(db.0, rc)); }
    let rc = unsafe { sqlite3_exec(db.0, sql.as_ptr(), None, std::ptr::null_mut(), std::ptr::null_mut()) };
    if rc != 0 { return Err(message(db.0, rc)); }
    Ok(())
}

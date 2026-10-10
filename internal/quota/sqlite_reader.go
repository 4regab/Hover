//go:build !windows

package quota

import (
	"encoding/binary"
	"errors"
	"fmt"
	"os"
	"regexp"
	"strings"
)

// A read-only reader of SQLite files in plain Go, for where there is no system library to
// call: Linux (an AppImage can't count on libsqlite3, and no C compiler is wanted) and the
// Mac until its app calls /usr/lib/libsqlite3.dylib. It answers the one question Hover asks,
// the shape `SELECT <col> FROM <table> WHERE <col> = '<text>'`, by walking the table's
// b-tree: the first row whose column equals the text, and the other column it names.
//
// ponytail: tables only (no index, no view, no WITHOUT ROWID), UTF-8 databases, and the
// one query shape. The file is read whole (Cursor's is a few MB). A write-ahead log is
// laid over it, as SQLite does, so what Cursor saved a moment ago is seen.

var selectOne = regexp.MustCompile(`(?is)^\s*SELECT\s+(\w+)\s+FROM\s+(\w+)\s+WHERE\s+(\w+)\s*=\s*'((?:[^']|'')*)'\s*;?\s*$`)

func scalar(path, sql string) (Scalar, error) {
	m := selectOne.FindStringSubmatch(sql)
	if m == nil {
		return Scalar{}, errors.New("SQLite Error 1: 'this reader answers only SELECT col FROM table WHERE col = text'.")
	}
	db, err := openDB(path)
	if err != nil {
		return Scalar{}, fmt.Errorf("SQLite Error 14: 'unable to open database file (%v)'.", err)
	}
	return db.lookup(m[1], m[2], m[3], strings.ReplaceAll(m[4], "''", "'"))
}

type sqliteDB struct {
	data     []byte
	pageSize int
	usable   int
	pages    int
	wal      map[uint32][]byte // page number to its latest committed content
}

func openDB(path string) (*sqliteDB, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if len(data) < 100 || string(data[:16]) != "SQLite format 3\x00" {
		return nil, errors.New("not an SQLite file")
	}
	d := &sqliteDB{data: data}
	d.pageSize = int(binary.BigEndian.Uint16(data[16:]))
	if d.pageSize == 1 {
		d.pageSize = 65536
	}
	d.usable = d.pageSize - int(data[20])
	if d.pageSize < 512 || d.pageSize&(d.pageSize-1) != 0 {
		return nil, errors.New("bad page size")
	}
	if enc := binary.BigEndian.Uint32(data[56:]); enc != 0 && enc != 1 {
		return nil, errors.New("only UTF-8 databases are read")
	}
	d.pages = len(data) / d.pageSize
	if w, err := os.ReadFile(path + "-wal"); err == nil {
		d.overlay(w)
	}
	return d, nil
}

// overlay reads the write-ahead log: the frames up to the last commit whose salts and
// checksums are right.
func (d *sqliteDB) overlay(w []byte) {
	if len(w) < 32 {
		return
	}
	magic := binary.BigEndian.Uint32(w)
	if magic != 0x377f0682 && magic != 0x377f0683 {
		return
	}
	var order binary.ByteOrder = binary.LittleEndian
	if magic == 0x377f0683 {
		order = binary.BigEndian
	}
	sum := func(b []byte, s0, s1 uint32) (uint32, uint32) {
		for i := 0; i+8 <= len(b); i += 8 {
			s0 += order.Uint32(b[i:]) + s1
			s1 += order.Uint32(b[i+4:]) + s0
		}
		return s0, s1
	}
	if int(binary.BigEndian.Uint32(w[8:])) != d.pageSize {
		return
	}
	s0, s1 := sum(w[:24], 0, 0)
	if s0 != binary.BigEndian.Uint32(w[24:]) || s1 != binary.BigEndian.Uint32(w[28:]) {
		return
	}
	salt := w[16:24]
	pending := map[uint32][]byte{}
	d.wal = map[uint32][]byte{}
	for off := 32; off+24+d.pageSize <= len(w); off += 24 + d.pageSize {
		h := w[off : off+24]
		if string(h[8:16]) != string(salt) {
			break
		}
		s0, s1 = sum(h[:8], s0, s1)
		s0, s1 = sum(w[off+24:off+24+d.pageSize], s0, s1)
		if s0 != binary.BigEndian.Uint32(h[16:]) || s1 != binary.BigEndian.Uint32(h[20:]) {
			break
		}
		pending[binary.BigEndian.Uint32(h)] = w[off+24 : off+24+d.pageSize]
		// A commit frame carries the size of the database after it: what came before is final.
		if size := binary.BigEndian.Uint32(h[4:]); size != 0 {
			for n, p := range pending {
				d.wal[n] = p
			}
			pending = map[uint32][]byte{}
			d.pages = int(size)
		}
	}
}

func (d *sqliteDB) page(n uint32) ([]byte, error) {
	if n == 0 || int(n) > d.pages {
		return nil, fmt.Errorf("page %d is outside the file", n)
	}
	if p, ok := d.wal[n]; ok {
		return p, nil
	}
	start := (int(n) - 1) * d.pageSize
	if start+d.pageSize > len(d.data) {
		return nil, fmt.Errorf("page %d is cut short", n)
	}
	return d.data[start : start+d.pageSize], nil
}

func varint(b []byte) (uint64, int) {
	var v uint64
	for i := 0; i < 8 && i < len(b); i++ {
		v = v<<7 | uint64(b[i]&0x7f)
		if b[i]&0x80 == 0 {
			return v, i + 1
		}
	}
	if len(b) < 9 {
		return 0, 0
	}
	return v<<8 | uint64(b[8]), 9
}

// value is one column of a record.
type value struct {
	kind ScalarKind
	text string
	blob []byte
	num  int64
	null bool
}

// record decodes a row's payload.
func record(p []byte) ([]value, error) {
	hlen, n := varint(p)
	if n == 0 || int(hlen) > len(p) {
		return nil, errors.New("a damaged record")
	}
	var types []uint64
	for off := n; off < int(hlen); {
		t, k := varint(p[off:])
		if k == 0 {
			return nil, errors.New("a damaged record header")
		}
		types = append(types, t)
		off += k
	}
	body := p[hlen:]
	var out []value
	for _, t := range types {
		var size int
		switch {
		case t <= 4:
			size = int(t) // 0 null, 1..4 integers of that many bytes
		case t == 5:
			size = 6
		case t == 6 || t == 7:
			size = 8
		case t == 8 || t == 9:
			size = 0
		case t >= 12:
			size = int(t-12) / 2
		}
		if size > len(body) {
			return nil, errors.New("a damaged record body")
		}
		v := value{kind: ScalarOther}
		switch {
		case t == 0:
			v.null = true
		case t >= 12 && t%2 == 0:
			v = value{kind: ScalarBlob, blob: append([]byte(nil), body[:size]...)}
		case t >= 13:
			v = value{kind: ScalarText, text: string(body[:size])}
		case t >= 1 && t <= 7:
			// A big-endian two's complement integer of size bytes.
			var n int64
			for i := 0; i < size; i++ {
				n = n<<8 | int64(body[i])
			}
			if size < 8 && body[0]&0x80 != 0 {
				n -= 1 << (8 * uint(size))
			}
			v.num = n
		case t == 9:
			v.num = 1
		}
		body = body[size:]
		out = append(out, v)
	}
	return out, nil
}

// payload is a cell's whole payload: the part on the page, then its overflow pages.
func (d *sqliteDB) payload(pg []byte, off int, total int, leaf bool) ([]byte, error) {
	// The most a b-tree page keeps on itself (table leaf: U-35; the minimum is (U-12)*32/255-23).
	maxLocal := d.usable - 35
	minLocal := (d.usable-12)*32/255 - 23
	if total <= maxLocal {
		if off+total > len(pg) {
			return nil, errors.New("a cell runs off its page")
		}
		return pg[off : off+total], nil
	}
	local := minLocal + (total-minLocal)%(d.usable-4)
	if local > maxLocal {
		local = minLocal
	}
	if off+local+4 > len(pg) {
		return nil, errors.New("a cell runs off its page")
	}
	out := append([]byte(nil), pg[off:off+local]...)
	next := binary.BigEndian.Uint32(pg[off+local:])
	for len(out) < total && next != 0 {
		op, err := d.page(next)
		if err != nil {
			return nil, err
		}
		next = binary.BigEndian.Uint32(op)
		take := min(total-len(out), d.usable-4)
		out = append(out, op[4:4+take]...)
	}
	if len(out) != total {
		return nil, errors.New("an overflow chain ends early")
	}
	return out, nil
}

// walk calls f with each row of the table b-tree at root, until f says stop.
func (d *sqliteDB) walk(root uint32, f func(row []value) (stop bool, err error)) error {
	var visit func(n uint32, depth int) (bool, error)
	visit = func(n uint32, depth int) (bool, error) {
		if depth > 40 {
			return false, errors.New("a b-tree too deep")
		}
		pg, err := d.page(n)
		if err != nil {
			return false, err
		}
		base := 0
		if n == 1 {
			base = 100 // the file header is on the first page
		}
		kind := pg[base]
		count := int(binary.BigEndian.Uint16(pg[base+3:]))
		switch kind {
		case 0x05: // interior table page
			ptrs := base + 12
			for i := 0; i < count; i++ {
				cell := int(binary.BigEndian.Uint16(pg[ptrs+2*i:]))
				if stop, err := visit(binary.BigEndian.Uint32(pg[cell:]), depth+1); stop || err != nil {
					return stop, err
				}
			}
			return visit(binary.BigEndian.Uint32(pg[base+8:]), depth+1)
		case 0x0d: // leaf table page
			ptrs := base + 8
			for i := 0; i < count; i++ {
				cell := int(binary.BigEndian.Uint16(pg[ptrs+2*i:]))
				if cell >= len(pg) {
					return false, errors.New("a cell outside its page")
				}
				size, k := varint(pg[cell:])
				if k == 0 {
					return false, errors.New("a damaged cell")
				}
				_, k2 := varint(pg[cell+k:]) // the row id
				p, err := d.payload(pg, cell+k+k2, int(size), true)
				if err != nil {
					return false, err
				}
				row, err := record(p)
				if err != nil {
					return false, err
				}
				if stop, err := f(row); stop || err != nil {
					return stop, err
				}
			}
			return false, nil
		}
		return false, fmt.Errorf("page %d is not a table page (%#x)", n, kind)
	}
	_, err := visit(root, 0)
	return err
}

var createCols = regexp.MustCompile(`(?is)\((.*)\)`)

// columns names a table's columns, in order, from its CREATE TABLE text.
func columns(sql string) []string {
	m := createCols.FindStringSubmatch(sql)
	if m == nil {
		return nil
	}
	// Split at the commas that are not inside brackets.
	var parts []string
	depth, start := 0, 0
	for i, c := range m[1] {
		switch c {
		case '(':
			depth++
		case ')':
			depth--
		case ',':
			if depth == 0 {
				parts = append(parts, m[1][start:i])
				start = i + 1
			}
		}
	}
	parts = append(parts, m[1][start:])
	var out []string
	for _, p := range parts {
		f := strings.Fields(p)
		if len(f) == 0 {
			continue
		}
		switch strings.ToUpper(f[0]) {
		case "PRIMARY", "UNIQUE", "CHECK", "FOREIGN", "CONSTRAINT":
			continue
		}
		out = append(out, strings.Trim(f[0], "\"`[]'"))
	}
	return out
}

func (d *sqliteDB) lookup(col, table, keyCol, key string) (Scalar, error) {
	var root uint32
	var cols []string
	err := d.walk(1, func(row []value) (bool, error) {
		// sqlite_master: type, name, tbl_name, rootpage, sql.
		if len(row) < 5 || row[0].text != "table" || !strings.EqualFold(row[1].text, table) {
			return false, nil
		}
		root = rootPage(row[3])
		cols = columns(row[4].text)
		return true, nil
	})
	if err != nil {
		return Scalar{}, fmt.Errorf("SQLite Error 11: '%v'.", err)
	}
	if root == 0 {
		return Scalar{}, fmt.Errorf("SQLite Error 1: 'no such table: %s'.", table)
	}
	ki, vi := -1, -1
	for i, c := range cols {
		if strings.EqualFold(c, keyCol) {
			ki = i
		}
		if strings.EqualFold(c, col) {
			vi = i
		}
	}
	if ki < 0 || vi < 0 {
		return Scalar{}, fmt.Errorf("SQLite Error 1: 'no such column: %s'.", map[bool]string{true: col, false: keyCol}[vi < 0])
	}
	res := Scalar{Kind: ScalarOther}
	err = d.walk(root, func(row []value) (bool, error) {
		if ki >= len(row) || vi >= len(row) || row[ki].kind != ScalarText || row[ki].text != key {
			return false, nil
		}
		v := row[vi]
		res = Scalar{Kind: v.kind, Text: v.text, Blob: v.blob}
		return true, nil
	})
	if err != nil {
		return Scalar{}, fmt.Errorf("SQLite Error 11: '%v'.", err)
	}
	return res, nil
}

// rootPage is sqlite_master's rootpage column, an integer.
func rootPage(v value) uint32 { return uint32(v.num) }

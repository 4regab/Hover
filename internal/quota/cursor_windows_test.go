package quota

import (
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"
	"unicode/utf16"
)

// Cursor's sign-in is read out of a real SQLite file through winsqlite3.dll, which is on
// every Windows 10 and 11; the file is made the same way.
func TestCursorTakesItsTokenFromTheDatabase(t *testing.T) {
	dir := t.TempDir()
	db := filepath.Join(dir, "state.vscdb")
	eq(t, CursorAt(db, "http://127.0.0.1:9/", time.Now()).Detail, "Cursor isn’t installed, or hasn’t been signed in to.", "no file")
	if err := exec(db, "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);"); err != nil {
		t.Fatal(err)
	}
	eq(t, CursorAt(db, "http://127.0.0.1:9/", time.Now()).Detail, "Sign in to Cursor first.", "no token")

	exp := time.Now().Add(time.Hour).Unix()
	token := fmt.Sprintf("%s.%s.sig", b64("{}"), b64(fmt.Sprintf(`{"sub":"github|user_9","exp":%d}`, exp)))
	// Cursor stores it as quoted text; a blob in UTF-16 reads the same.
	if err := exec(db, fmt.Sprintf(`INSERT INTO ItemTable VALUES ('cursorAuth/accessToken', ' "%s" ');`, token)); err != nil {
		t.Fatal(err)
	}
	got, err := CursorToken(db)
	if err != nil || got == nil || *got != token {
		t.Fatalf("%v %v", got, err)
	}
	var hex strings.Builder
	for _, u := range utf16.Encode([]rune(token)) {
		fmt.Fprintf(&hex, "%02X%02X", u&0xFF, u>>8)
	}
	if err := exec(db, fmt.Sprintf("INSERT INTO ItemTable VALUES ('cursorAuth/accessToken', X'%s');", hex.String())); err != nil {
		t.Fatal(err)
	}
	got, err = CursorToken(db)
	if err != nil || got == nil || *got != token {
		t.Fatalf("%v %v", got, err)
	}

	url, req := serve(t, 200, `{"membershipType":"pro","individualUsage":{"plan":{"totalPercentUsed":12}}}`)
	r := CursorAt(db, url, time.Now())
	wantUsed(t, r, 12)
	eq(t, r.Detail, "Pro · 12% of plan", "detail")
	if !strings.Contains(req(), "WorkosCursorSessionToken=user_9%3A%3A"+token) {
		t.Error("no cookie sent")
	}
	url, _ = serve(t, 403, "")
	eq(t, CursorAt(db, url, time.Now()).Detail, "cursor.com refused Cursor’s sign-in — open Cursor to renew it.", "refused")

	writeFile(t, db, "not a database, just some bytes long enough to have a header..........................................")
	if d := CursorAt(db, url, time.Now()).Detail; !strings.HasPrefix(d, "Couldn’t read Cursor’s sign-in: ") {
		t.Error(d)
	}
}

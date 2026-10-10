package quota

// tests/quota.rs: tests/Hover.Tests/LayoutAndQuotaTests.cs (QuotaTests), ported case for
// case, then the readers' failures, which the C# tests don't cover: those expected values
// are Quota.cs's own strings, read from the source.

import (
	"fmt"
	"math"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// now is new DateTime(2026, 9, 26, 12, 0, 0, DateTimeKind.Local).
func now() time.Time { return time.Date(2026, 9, 26, 12, 0, 0, 0, time.Local).UTC() }

// iso is DateTimeOffset.ToString("o") of a local time: 2026-09-26T11:55:00.0000000+02:00.
func iso(t time.Time) string { return t.In(time.Local).Format("2006-01-02T15:04:05.0000000-07:00") }

func sp(s string) *string { return &s }

func wantUsed(t *testing.T, r Reading, want float64) {
	t.Helper()
	if r.Used == nil || math.Abs(*r.Used-want) > 0.001 {
		t.Errorf("used %v, want %v (%s)", r.Used, want, r.Detail)
	}
}

func eq[T comparable](t *testing.T, got, want T, what string) {
	t.Helper()
	if got != want {
		t.Errorf("%s: got %v, want %v", what, got, want)
	}
}

func TestKiroReadsTheBarTheCreditsThePlanAndTheReset(t *testing.T) {
	output := "\x1b[1m┃  | KIRO FREE ┃\n┃ Monthly credits: ┃\n┃ ████████ 42% (resets on 10/01) ┃\n┃ (21.00 of 50 covered in plan) ┃\n"
	q := ParseKiro(output)
	wantUsed(t, q, 42)
	eq(t, q.Detail, "KIRO FREE · 21 of 50 credits · resets 10/01", "detail")
	wantUsed(t, ParseKiro("(10.5 of 50 covered in plan)"), 21)
	if ParseKiro("Not logged in. Run kiro-cli login.").OK() || ParseKiro("anything else").OK() {
		t.Error("a failure was read as a reading")
	}
}

func codexLine(primaryReset int64) string {
	return fmt.Sprintf(`{"timestamp":"%s","type":"event_msg","payload":{"type":"token_count","rate_limits":{`+
		`"primary":{"used_percent":37.5,"window_minutes":300,"resets_at":%d},`+
		`"secondary":{"used_percent":12,"window_minutes":10080,"resets_in_seconds":3600}}}}`, iso(now().Add(-5*time.Minute)), primaryReset)
}

func TestCodexShowsTheWindowNearestItsLimit(t *testing.T) {
	q, ok := ParseCodexLine(codexLine(now().Add(2*time.Hour).Unix()), now())
	if !ok {
		t.Fatal("no reading")
	}
	wantUsed(t, q, 37.5)
	// The rest, from the interpolation: " · as of {at:d MMM HH:mm}", at in local time.
	eq(t, q.Detail, "5h 38% · week 12% · as of 26 Sep 11:55", "detail")
}

func TestCodexCountsAWindowThatHasResetAsEmpty(t *testing.T) {
	q, ok := ParseCodexLine(codexLine(now().Add(-time.Hour).Unix()), now())
	if !ok {
		t.Fatal("no reading")
	}
	wantUsed(t, q, 12)
}

func TestCodexSkipsLinesWithoutLimits(t *testing.T) {
	for _, line := range []string{
		`{"payload":{"rate_limits":null}}`,
		`{"payload":{"rate_limits":{"primary":{"used_percent":"x"}}}}`,
		"not json",
		// FromUnixTimeSeconds out of range threw ArgumentOutOfRangeException, which the C# catches.
		`{"payload":{"rate_limits":{"primary":{"used_percent":5,"resets_at":1e20}}}}`,
	} {
		if q, ok := ParseCodexLine(line, now()); ok {
			t.Errorf("%s: read %+v", line, q)
		}
	}
}

func TestCodexLabelsItsWindowsByTheirMinutes(t *testing.T) {
	// mins switch { >= 10000 => "week", >= 60 => $"{Math.Round(mins / 60)}h", > 0 => $"{mins}m", _ => fallback }
	q, ok := ParseCodexLine(`{"payload":{"rate_limits":{"primary":{"used_percent":1,"window_minutes":150},"secondary":{"used_percent":2,"window_minutes":45.5}}}}`, now())
	if !ok {
		t.Fatal("no reading")
	}
	// 150 / 60 = 2.5, and Math.Round takes it to the even 2.
	if !strings.HasPrefix(q.Detail, "2h 1% · 45.5m 2%") {
		t.Error(q.Detail)
	}
	q, ok = ParseCodexLine(`{"payload":{"rate_limits":{"secondary":{"used_percent":250}}}}`, now())
	if !ok {
		t.Fatal("no reading")
	}
	wantUsed(t, q, 100)
	if !strings.HasPrefix(q.Detail, "week 100% · as of ") {
		t.Error(q.Detail)
	}
}

func writeFile(t *testing.T, p, text string) {
	t.Helper()
	os.MkdirAll(filepath.Dir(p), 0o777)
	if err := os.WriteFile(p, []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

func TestCodexReadsTheNewestSessionLog(t *testing.T) {
	home := t.TempDir()
	day := filepath.Join(home, "sessions", "2026", "09", "26")
	writeFile(t, filepath.Join(day, "rollout-a.jsonl"), fmt.Sprintf("{\"x\":1}\n%s\n{\"type\":\"other\"}\n", codexLine(now().Add(2*time.Hour).Unix())))
	wantUsed(t, CodexIn(home, now()), 37.5)
	// Not a rollout, and a newer rollout with no limits: the one with limits is read.
	writeFile(t, filepath.Join(day, "notes.jsonl"), "{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":99}}}}\n")
	time.Sleep(20 * time.Millisecond)
	writeFile(t, filepath.Join(day, "rollout-b.jsonl"), "{\"x\":1}\r\n")
	wantUsed(t, CodexIn(home, now()), 37.5)
	eq(t, CodexIn(filepath.Join(home, "none"), now()).Detail, "No Codex sessions on this PC yet.", "no sessions")
	empty := t.TempDir()
	os.MkdirAll(filepath.Join(empty, "sessions"), 0o777)
	eq(t, CodexIn(empty, now()).Detail, "Codex hasn’t recorded any limits yet — use it once.", "no limits")
}

// A long session: its file was modified before a newer, shorter one, but its last event is
// the newest (Windows leaves an open file's modified time behind), so its limits are the
// ones read.
func TestCodexRanksItsLogsByTheirLastEvent(t *testing.T) {
	home := t.TempDir()
	day := filepath.Join(home, "sessions", "2026", "09", "26")
	line := func(used float64, at time.Time) string {
		return fmt.Sprintf(`{"timestamp":"%s","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":%v,"window_minutes":300,"resets_in_seconds":3600}}}}`+"\n", iso(at), used)
	}
	writeFile(t, filepath.Join(day, "rollout-long.jsonl"), line(70, now().Add(-time.Minute)))
	time.Sleep(20 * time.Millisecond)
	writeFile(t, filepath.Join(day, "rollout-short.jsonl"), line(10, now().Add(-3*time.Hour)))
	wantUsed(t, CodexIn(home, now()), 70)
}

// b64 is Convert.ToBase64String(...).TrimEnd('=').Replace('+', '-').Replace('/', '_').
func b64(s string) string {
	const a = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
	b := []byte(s)
	var o strings.Builder
	for i := 0; i < len(b); i += 3 {
		c := b[i:min(i+3, len(b))]
		var n uint32
		for j := range 3 {
			n <<= 8
			if j < len(c) {
				n |= uint32(c[j])
			}
		}
		for j := 0; j <= len(c); j++ {
			o.WriteByte(a[n>>(18-6*j)&63])
		}
	}
	return o.String()
}

func TestCursorCookieIsTheUserIdAndTheToken(t *testing.T) {
	exp := time.Now().Add(time.Hour).Unix()
	token := fmt.Sprintf(`%s.%s.sig`, b64(`{"alg":"HS256"}`), b64(fmt.Sprintf(`{"sub":"auth0|user_01ABC","exp":%d}`, exp)))
	got, ok := CursorCookie(token, time.Now())
	eq(t, ok, true, "a good token")
	eq(t, got, "WorkosCursorSessionToken=user_01ABC%3A%3A"+token, "cookie")
	if _, ok := CursorCookie(fmt.Sprintf("%s.%s.s", b64("{}"), b64(`{"sub":"a|u1","exp":1}`)), time.Now()); ok {
		t.Error("expired")
	}
	if _, ok := CursorCookie(fmt.Sprintf("%s.%s.s", b64("{}"), b64(`{"sub":"a|b c"}`)), time.Now()); ok {
		t.Error("bad id")
	}
	if _, ok := CursorCookie("abc", time.Now()); ok {
		t.Error("not a JWT")
	}
}

func TestCursorReadsPlanUsage(t *testing.T) {
	for _, c := range []struct {
		json string
		used float64
	}{
		{`{"membershipType":"pro","individualUsage":{"plan":{"used":500,"limit":2000,"totalPercentUsed":33.4}}}`, 33.4},
		{`{"individualUsage":{"plan":{"used":500,"limit":2000}}}`, 25},
		{`{"individualUsage":{"plan":{"autoPercentUsed":10,"apiPercentUsed":30}}}`, 20},
		{`{"teamUsage":{"pooled":{"used":1,"limit":4}},"membershipType":5}`, 25},
	} {
		wantUsed(t, ParseCursorSummary(c.json), c.used)
	}
	// The detail, from the interpolation.
	eq(t, ParseCursorSummary(`{"membershipType":"pro","individualUsage":{"plan":{"totalPercentUsed":33.5}}}`).Detail, "Pro · 34% of plan", "detail")
	end := time.Date(2026, 10, 3, 9, 0, 0, 0, time.Local).UTC()
	eq(t, ParseCursorSummary(fmt.Sprintf(`{"individualUsage":{"plan":{"apiPercentUsed":7}},"billingCycleEnd":"%s"}`, iso(end))).Detail, "7% of plan · resets 3 Oct", "detail with a reset")
}

func TestCursorWithoutUsageIsAReadableFailure(t *testing.T) {
	if ParseCursorSummary("{}").OK() || ParseCursorSummary("[").OK() {
		t.Error("a failure was read as a reading")
	}
	eq(t, ParseCursorSummary("{}").Detail, "Cursor didn’t report plan usage.", "empty")
	eq(t, ParseCursorSummary("[").Detail, "Cursor’s answer couldn’t be read.", "torn")
}

func TestClaudeShowsTheWindowClosestToItsLimit(t *testing.T) {
	soon := iso(now().Add(2 * time.Hour))
	later := iso(now().Add(72 * time.Hour))
	r := ParseClaudeUsage(fmt.Sprintf(`{"five_hour":{"utilization":18.0,"resets_at":"%s"},"seven_day":{"utilization":46.0,"resets_at":"%s"},"seven_day_opus":null}`, soon, later), sp("max"), now())
	wantUsed(t, r, 46)
	// The reset of the fuller window, with its date since it isn't today.
	eq(t, r.Detail, "Max · 5h 18% · week 46% · resets 29 Sep 12:00", "detail")
	r = ParseClaudeUsage(fmt.Sprintf(`{"five_hour":{"utilization":50,"resets_at":"%s"}}`, soon), nil, now())
	eq(t, r.Detail, "5h 50% · resets 14:00", "same day")
}

func TestClaudeWindowPastItsResetIsEmptyAgain(t *testing.T) {
	gone := iso(now().Add(-5 * time.Minute))
	r := ParseClaudeUsage(fmt.Sprintf(`{"five_hour":{"utilization":90,"resets_at":"%s"},"seven_day":{"utilization":12,"resets_at":null}}`, gone), nil, now())
	wantUsed(t, r, 12)
	if ParseClaudeUsage("{}", nil, now()).OK() || ParseClaudeUsage("<html>", nil, now()).OK() {
		t.Error("a failure was read as a reading")
	}
}

func TestClaudeSignInIsReadFromTheCredentialsFile(t *testing.T) {
	s := ClaudeSignIn(`{"claudeAiOauth":{"accessToken":"test-access-token","refreshToken":"r","expiresAt":1790000000000,"subscriptionType":"pro"}}`)
	if s.Token == nil || *s.Token != "test-access-token" || s.Plan == nil || *s.Plan != "pro" {
		t.Errorf("%+v", s)
	}
	if s.Expires == nil || !s.Expires.Equal(time.UnixMilli(1_790_000_000_000)) {
		t.Errorf("%v", s.Expires)
	}
	if ClaudeSignIn(`{"other":1}`).Token != nil || ClaudeSignIn("nope").Token != nil {
		t.Error("a sign-in out of nothing")
	}
}

// MARK: The readers, against stand-ins

// serve is one HTTP answer from a local socket, and the request it got.
func serve(t *testing.T, status int, body string) (string, func() string) {
	t.Helper()
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { l.Close() })
	got := make(chan string, 1)
	go func() {
		c, err := l.Accept()
		if err != nil {
			got <- ""
			return
		}
		defer c.Close()
		req := make([]byte, 8192)
		n, _ := c.Read(req)
		fmt.Fprintf(c, "HTTP/1.1 %d X\r\nContent-Type: application/json\r\nContent-Length: %d\r\nConnection: close\r\n\r\n%s", status, len(body), body)
		got <- string(req[:n])
	}()
	return "http://" + l.Addr().String() + "/usage", func() string {
		select {
		case r := <-got:
			return r
		case <-time.After(10 * time.Second):
			t.Fatal("the stand-in server was never asked")
			return ""
		}
	}
}

func credentials(t *testing.T, dir string, expiresMs int64) string {
	f := filepath.Join(dir, ".credentials.json")
	writeFile(t, f, fmt.Sprintf(`{"claudeAiOauth":{"accessToken":"tok","expiresAt":%d,"subscriptionType":"pro"}}`, expiresMs))
	return f
}

// A Mac's sign-in comes from the Keychain as the same JSON text, not a file.
func TestClaudeReadsASignInHandedOverAsText(t *testing.T) {
	later := time.Now().Add(time.Hour).UnixMilli()
	json := fmt.Sprintf(`{"claudeAiOauth":{"accessToken":"tok","expiresAt":%d,"subscriptionType":"max"}}`, later)
	url, req := serve(t, 200, `{"five_hour":{"utilization":40,"resets_at":null}}`)
	r := ClaudeWith(json, url, time.Now())
	wantUsed(t, r, 40)
	if !strings.Contains(strings.ToLower(req()), "authorization: bearer tok") {
		t.Error("no bearer token sent")
	}
	eq(t, ClaudeWith("{}", "http://127.0.0.1:9/", time.Now()).Detail, "Sign in to Claude Code with a Claude plan (Pro or Max) first.", "no sign-in")
	eq(t, ClaudeWith("not json", "http://127.0.0.1:9/", time.Now()).Detail, "Sign in to Claude Code with a Claude plan (Pro or Max) first.", "not json")
	eq(t, ClaudeKeychainService, "Claude Code-credentials", "service")
}

// Cursor's database is under Electron's settings folder: %APPDATA% on Windows,
// ~/Library/Application Support on a Mac, ~/.config on Linux.
func TestCursorKeepsItsStateUnderTheElectronSettingsFolder(t *testing.T) {
	eq(t, CursorDBUnder(filepath.FromSlash("/Users/u/Library/Application Support")),
		filepath.Join(filepath.FromSlash("/Users/u/Library/Application Support"), "Cursor", "User", "globalStorage", "state.vscdb"), "path")
}

func TestClaudeAsksWithItsOwnSignInAndExplainsRefusals(t *testing.T) {
	dir := t.TempDir()
	later := time.Now().Add(time.Hour).UnixMilli()
	eq(t, ClaudeAt(filepath.Join(dir, "none.json"), "http://127.0.0.1:9/", time.Now()).Detail, "Sign in to Claude Code with a Claude plan (Pro or Max) first.", "no file")
	expired := credentials(t, dir, time.Now().UnixMilli()+30_000)
	eq(t, ClaudeAt(expired, "http://127.0.0.1:9/", time.Now()).Detail, "Claude Code’s sign-in has expired — run claude to renew it.", "expired")

	f := credentials(t, dir, later)
	url, req := serve(t, 200, `{"five_hour":{"utilization":20,"resets_at":null},"seven_day":{"utilization":7}}`)
	r := ClaudeAt(f, url, time.Now())
	wantUsed(t, r, 20)
	eq(t, r.Detail, "Pro · 5h 20% · week 7%", "detail")
	got := strings.ToLower(req())
	if !strings.HasPrefix(got, "get /usage ") {
		t.Errorf("%q", got)
	}
	for _, h := range []string{"authorization: bearer tok", "anthropic-beta: oauth-2025-04-20", "accept: application/json", "user-agent: hover"} {
		if !strings.Contains(got, h) {
			t.Errorf("%s in %q", h, got)
		}
	}
	for _, c := range []struct {
		status int
		why    string
	}{
		{401, "Anthropic refused Claude Code’s sign-in — run claude to renew it."},
		{403, "This sign-in can’t read plan usage — run claude and sign in again."},
		{429, "Anthropic is limiting usage checks; Hover tries again in five minutes."},
		{500, "api.anthropic.com answered 500."},
	} {
		url, _ := serve(t, c.status, "{}")
		eq(t, ClaudeAt(f, url, time.Now()).Detail, c.why, fmt.Sprint(c.status))
	}
	// Nothing listening.
	dead, _ := net.Listen("tcp", "127.0.0.1:0")
	url = "http://" + dead.Addr().String() + "/"
	dead.Close()
	eq(t, ClaudeAt(f, url, time.Now()).Detail, "Couldn’t reach api.anthropic.com.", "unreachable")
}

// The raw numbers behind ParseKiro's percent: kept for the daily credits.
func TestKiroUsageKeepsTheCreditsThePlanAndTheResetAsPrinted(t *testing.T) {
	u, ok := ParseKiroUsage("\x1b[1m┃  | KIRO PRO ┃\n┃ ████████ 42% (resets on 10/01) ┃\n┃ (21.00 of 50 covered in plan) ┃\n")
	if !ok || u.Used != 21 || u.Limit != 50 || *u.Plan != "KIRO PRO" || *u.Reset != "10/01" {
		t.Errorf("%+v %v", u, ok)
	}
	// The other reset format, and credits in 0.01 steps.
	u, ok = ParseKiroUsage("KIRO POWER\n██ 3%\n(20.37 of 1000 covered in plan) resets on 2026-10-01")
	if !ok || u.Used != 20.37 || u.Limit != 1000 || *u.Plan != "KIRO POWER" || *u.Reset != "2026-10-01" {
		t.Errorf("%+v %v", u, ok)
	}
	// Colour codes inside the line don't hide it; a report without plan or reset still counts.
	u, ok = ParseKiroUsage("\x1b[32m(\x1b[0m0.5 of 50 covered in plan)\x1b[0m")
	if !ok || u.Used != 0.5 || u.Limit != 50 || u.Plan != nil || u.Reset != nil {
		t.Errorf("%+v %v", u, ok)
	}
}

func TestKiroUsageNeedsTheCreditLineAndALimit(t *testing.T) {
	for _, text := range []string{"████████ 42% (resets on 10/01)", "(5 of 0 covered in plan)", "Not logged in. Run kiro-cli login.", ""} {
		if u, ok := ParseKiroUsage(text); ok {
			t.Errorf("%q: %+v", text, u)
		}
	}
	// ParseKiro is as it was.
	eq(t, ParseKiro("(21.00 of 50 covered in plan) resets on 10/01").Detail, "21 of 50 credits · resets 10/01", "detail")
}

// Package quota is Core/Quota.cs: how much of each AI tool's plan is used. None of them
// publishes a quota API, so each is read the way the tool itself exposes it, read-only,
// and nothing leaves the PC except each tool's own usage request with its own sign-in:
//
//	Kiro CLI     `kiro-cli chat --no-interactive /usage`, the CLI's printed report.
//	Codex        the rate-limit snapshot Codex writes into its session logs.
//	Cursor       cursor.com/api/usage-summary, with the token Cursor keeps locally.
//	Claude Code  api.anthropic.com/api/oauth/usage, with Claude Code's own sign-in.
//
// The parsers here take text and a clock, so they are tested on their own. The readers
// that touch the disk, the network and kiro-cli are in read.go, and OwlApp's five-minute
// refresh is schedule.go.
package quota

import (
	"errors"
	"fmt"
	"math"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/dlclark/regexp2"

	"github.com/4regab/Hover/go/internal/core"
)

// Reading is one reading of how much of a plan is used. Used is nil when there is nothing
// to show; Detail then says why ("kiro-cli not found", "Sign in to Cursor").
type Reading struct {
	Used   *float64
	Detail string
}

func (r Reading) OK() bool { return r.Used != nil }

func Fail(why string) Reading { return Reading{Detail: why} }

func newReading(used float64, detail string) Reading { return Reading{&used, detail} }

// Notch items: what the resting notch can show, the AI quotas (Core/Layout.cs NotchItem).
const (
	ItemKiro   = "kiro"
	ItemCodex  = "codex"
	ItemCursor = "cursor"
	ItemClaude = "claude"
)

var ItemAll = [4]string{ItemClaude, ItemKiro, ItemCodex, ItemCursor}

func ItemTitle(id string) string {
	switch id {
	case ItemClaude:
		return "Claude Code quota"
	case ItemKiro:
		return "Kiro CLI quota"
	case ItemCodex:
		return "Codex quota"
	case ItemCursor:
		return "Cursor quota"
	}
	return id
}

// ItemShort is the name beside a quota on the notch.
func ItemShort(id string) string {
	switch id {
	case ItemClaude:
		return "Claude"
	case ItemKiro:
		return "Kiro"
	case ItemCodex:
		return "Codex"
	}
	return "Cursor"
}

// MARK: Time, as the C# reads and shows it

// DateTimeOffset.FromUnixTimeSeconds's range: 0001-01-01 to 9999-12-31.
const (
	minUnix = -62_135_596_800
	maxUnix = 253_402_300_799
)

// satInt64 is Rust's `as i64` on a float: it truncates, saturates, and takes NaN to 0.
func satInt64(f float64) int64 {
	switch {
	case f != f:
		return 0
	case f >= 9.223372036854775807e18:
		return math.MaxInt64
	case f <= -9.223372036854775808e18:
		return math.MinInt64
	}
	return int64(f)
}

// fromUnixSecs is DateTimeOffset.FromUnixTimeSeconds((long)v): the cast truncates, and a
// value outside the range throws (false here).
func fromUnixSecs(v float64) (time.Time, bool) {
	s := satInt64(v)
	if s < minUnix || s > maxUnix {
		return time.Time{}, false
	}
	return time.Unix(s, 0).UTC(), true
}

func fromUnixMs(v float64) (time.Time, bool) {
	ms := satInt64(v)
	if ms < minUnix*1000 || ms > maxUnix*1000+999 {
		return time.Time{}, false
	}
	return time.UnixMilli(ms).UTC(), true
}

func floorDiv(a, b int64) int64 {
	q := a / b
	if a%b != 0 && (a < 0) != (b < 0) {
		q--
	}
	return q
}

// ParseTime is DateTimeOffset.TryParse for the ISO 8601 stamps these tools write: an
// offset or Z is kept, none means local time (as .NET assumes). The instant, in UTC.
func ParseTime(s string) (time.Time, bool) {
	st, ok := core.ParseStamp(strings.TrimSpace(s))
	if !ok {
		return time.Time{}, false
	}
	ticks := st.UTCTicks() - 621_355_968_000_000_000
	secs := floorDiv(ticks, 10_000_000)
	return time.Unix(secs, (ticks-secs*10_000_000)*100).UTC(), true
}

var months = [12]string{"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"}

// FormatLocal is a .NET date pattern made of d, MMM, HH and mm, in local time. MMM is
// English (C# uses the current culture's names).
func FormatLocal(t time.Time, pattern string) string {
	l := t.In(time.Local)
	var o strings.Builder
	for rest := pattern; rest != ""; {
		switch {
		case strings.HasPrefix(rest, "MMM"):
			o.WriteString(months[l.Month()-1])
			rest = rest[3:]
		case strings.HasPrefix(rest, "HH"):
			fmt.Fprintf(&o, "%02d", l.Hour())
			rest = rest[2:]
		case strings.HasPrefix(rest, "mm"):
			fmt.Fprintf(&o, "%02d", l.Minute())
			rest = rest[2:]
		case strings.HasPrefix(rest, "d"):
			o.WriteString(strconv.Itoa(l.Day()))
			rest = rest[1:]
		default:
			_, n := utf8.DecodeRuneInString(rest)
			o.WriteString(rest[:n])
			rest = rest[n:]
		}
	}
	return o.String()
}

// sameLocalDay: both times fall on one calendar day here.
func sameLocalDay(a, b time.Time) bool {
	ay, am, ad := a.In(time.Local).Date()
	by, bm, bd := b.In(time.Local).Date()
	return ay == by && am == bm && ad == bd
}

// MARK: JSON bits

// numOf is Quota.N: a number property of an object, else nil.
func numOf(o core.JSON, name string) *float64 {
	v, ok := o.Get(name)
	if !ok || v.Kind() != core.NumKind {
		return nil
	}
	f, err := v.F64()
	if err != nil {
		return nil
	}
	return &f
}

func strOf(o core.JSON, name string) *string {
	v, ok := o.Get(name)
	if !ok {
		return nil
	}
	s, ok := v.AsStr()
	if !ok {
		return nil
	}
	return &s
}

func objOf(o core.JSON, name string) (core.JSON, bool) {
	v, ok := o.Get(name)
	return v, ok && v.Kind() == core.ObjKind
}

// parse is JsonDocument.Parse with its default options: what System.Text.Json refuses,
// this refuses.
func parse(text string) (core.JSON, bool) {
	v, err := core.ParseJSON(text)
	return v, err == nil
}

// capital is char.ToUpperInvariant(s[0]) + s[1..], on UTF-16 units as C# indexes them (a
// surrogate pair stays as it is).
func capital(s string) string {
	r, n := utf8.DecodeRuneInString(s)
	if n == 0 {
		return ""
	}
	if r > 0xFFFF {
		return s
	}
	return string(unicode.ToUpper(r)) + s[n:]
}

// MARK: Kiro CLI

var (
	ansiRe   = regexp2.MustCompile(`\x1B\[[0-9;?]*[A-Za-z]|\x1B\][^\x07]*\x07`, regexp2.None)
	resetRe  = regexp2.MustCompile(`resets on (\d{4}-\d{2}-\d{2}|\d{2}/\d{2})`, regexp2.None)
	planRe   = regexp2.MustCompile(`\b(KIRO(?:[ \t]+[A-Z]+)+)\b`, regexp2.None)
	barRe    = regexp2.MustCompile(`█+\s*(\d+(?:\.\d+)?)\s*%`, regexp2.None)
	creditRe = regexp2.MustCompile(`\((\d+(?:\.\d+)?)\s+of\s+(\d+(?:\.\d+)?)\s+covered`, regexp2.None)
)

// group is the text of a pattern's first group at its first match, nil for no match.
func group(re *regexp2.Regexp, text string, n int) *string {
	m, _ := re.FindStringMatch(text)
	if m == nil {
		return nil
	}
	s := m.GroupByNumber(n).String()
	return &s
}

func stripANSI(s string) string {
	out, err := ansiRe.Replace(s, "", -1, -1)
	if err != nil {
		return s
	}
	return out
}

// numOfText is double.TryParse(s, Float, Invariant): the digits the patterns matched. A
// Unicode digit that \d took but .NET can't parse fails the same way here.
func numOfText(s string) (float64, bool) {
	for i := 0; i < len(s); i++ {
		if s[i] >= 0x80 {
			return 0, false
		}
	}
	f, err := strconv.ParseFloat(s, 64)
	if err != nil && !errors.Is(err, strconv.ErrRange) {
		return 0, false
	}
	return f, true
}

// ParseKiro reads the report, a box: "████ 42% (resets on 10/01)" and "(21.00 of 50
// covered in plan)". The bar's percentage is the share used; the credit line is the
// fallback when a release drops the bar.
func ParseKiro(output string) Reading {
	text := stripANSI(output)
	lower := strings.ToLower(text)
	if strings.Contains(lower, "not logged in") || strings.Contains(lower, "login required") || strings.Contains(lower, "kiro-cli login") {
		return Fail("Run “kiro-cli login” first.")
	}
	if strings.Contains(lower, "could not retrieve usage") {
		return Fail("Kiro couldn’t retrieve usage right now.")
	}

	resetText, planText := "", ""
	if r := group(resetRe, text, 1); r != nil {
		resetText = " · resets " + *r
	}
	if p := group(planRe, text, 1); p != nil {
		planText = strings.TrimSpace(*p) + " · "
	}

	var used *float64
	detail := ""
	if m, _ := creditRe.FindStringMatch(text); m != nil {
		u, ok1 := numOfText(m.GroupByNumber(1).String())
		l, ok2 := numOfText(m.GroupByNumber(2).String())
		if ok1 && ok2 && l > 0 {
			detail = fmt.Sprintf("%s of %s credits", Custom(u, 2), Custom(l, 2))
			p := u / l * 100
			used = &p
		}
	}
	if b := group(barRe, text, 1); b != nil {
		if p, ok := numOfText(*b); ok {
			used = &p
		}
	}
	if used == nil {
		return Fail("Couldn’t read kiro-cli’s usage report.")
	}
	if detail == "" {
		detail = Custom(*used, 0) + "% used"
	}
	return newReading(min(max(*used, 0), 100), planText+detail+resetText)
}

// KiroUsage is the raw numbers of kiro-cli's report, which ParseKiro boils down to a
// percent: what the daily credits are made from. Reset is the text as printed ("10/01" or
// "2026-10-01"), the next reset.
type KiroUsage struct {
	Used, Limit float64
	Plan, Reset *string
}

// ParseKiroUsage is the credit line, the plan and the reset of the same report, or false
// when there is no "X of Y covered" (a release that drops it, a failure) or the plan has
// no credits.
func ParseKiroUsage(output string) (KiroUsage, bool) {
	text := stripANSI(output)
	m, _ := creditRe.FindStringMatch(text)
	if m == nil {
		return KiroUsage{}, false
	}
	used, ok1 := numOfText(m.GroupByNumber(1).String())
	limit, ok2 := numOfText(m.GroupByNumber(2).String())
	if !ok1 || !ok2 || limit <= 0 {
		return KiroUsage{}, false
	}
	u := KiroUsage{Used: used, Limit: limit, Reset: group(resetRe, text, 1)}
	if p := group(planRe, text, 1); p != nil {
		t := strings.TrimSpace(*p)
		u.Plan = &t
	}
	return u, true
}

// MARK: Codex

// ParseCodexLine reads one token_count event from a Codex session log. Its rate_limits
// carry the five-hour window (primary) and the weekly one (secondary). The notch shows
// whichever is closer to the limit; a window whose reset has passed since the snapshot
// counts as empty again.
func ParseCodexLine(line string, now time.Time) (Reading, bool) {
	root, ok := parse(line)
	if !ok {
		return Reading{}, false
	}
	payload, ok := objOf(root, "payload")
	if !ok {
		return Reading{}, false
	}
	limits, ok := objOf(payload, "rate_limits")
	if !ok {
		return Reading{}, false
	}
	at := now
	if ts := strOf(root, "timestamp"); ts != nil {
		if t, ok := ParseTime(*ts); ok {
			at = t
		}
	}

	type window struct {
		used  float64
		label string
	}
	// Each window, or none; false when the C# would have thrown (and the line is skipped).
	read := func(name, fallback string) (w *window, ok bool) {
		o, has := objOf(limits, name)
		if !has {
			return nil, true
		}
		usedP := numOf(o, "used_percent")
		if usedP == nil {
			return nil, true
		}
		used := *usedP
		var reset *time.Time
		if epoch := numOf(o, "resets_at"); epoch != nil {
			t, ok := fromUnixSecs(*epoch)
			if !ok {
				return nil, false
			}
			reset = &t
		} else if secs := numOf(o, "resets_in_seconds"); secs != nil {
			ms := math.Round(*secs * 1000)
			if math.IsInf(ms, 0) || math.IsNaN(ms) || math.Abs(ms) > 3.2e14 {
				return nil, false
			}
			t := at.Add(time.Duration(int64(ms)) * time.Millisecond)
			reset = &t
		}
		if reset != nil && !reset.After(now) {
			used = 0
		}
		mins := 0.0
		if m := numOf(o, "window_minutes"); m != nil {
			mins = *m
		}
		var label string
		switch {
		case mins >= 10000:
			label = "week"
		case mins >= 60:
			label = core.DotnetDouble(math.RoundToEven(mins/60)) + "h"
		case mins > 0:
			label = core.DotnetDouble(mins) + "m"
		default:
			label = fallback
		}
		return &window{min(max(used, 0), 100), label}, true
	}

	var windows []window
	for _, w := range [][2]string{{"primary", "5h"}, {"secondary", "week"}} {
		got, ok := read(w[0], w[1])
		if !ok {
			return Reading{}, false
		}
		if got != nil {
			windows = append(windows, *got)
		}
	}
	if len(windows) == 0 {
		return Reading{}, false
	}
	parts := make([]string, len(windows))
	top := math.Inf(-1)
	for i, w := range windows {
		parts[i] = fmt.Sprintf("%s %s%%", w.label, Custom(w.used, 0))
		top = max(top, w.used)
	}
	return newReading(top, strings.Join(parts, " · ")+" · as of "+FormatLocal(at, "d MMM HH:mm")), true
}

// MARK: Cursor

// base64 is base64url, padded as the C# pads it, then Convert.FromBase64String's rules.
func base64(s string) ([]byte, bool) {
	val := func(c byte) (uint32, bool) {
		switch {
		case c >= 'A' && c <= 'Z':
			return uint32(c - 'A'), true
		case c >= 'a' && c <= 'z':
			return uint32(c-'a') + 26, true
		case c >= '0' && c <= '9':
			return uint32(c-'0') + 52, true
		case c == '+':
			return 62, true
		case c == '/':
			return 63, true
		}
		return 0, false
	}
	var b []byte
	for i := 0; i < len(s); i++ {
		if c := s[i]; c != ' ' && c != '\t' && c != '\r' && c != '\n' {
			b = append(b, c)
		}
	}
	if len(b)%4 != 0 {
		return nil, false
	}
	pad := 0
	for pad < len(b) && b[len(b)-1-pad] == '=' {
		pad++
	}
	if pad > 2 {
		return nil, false
	}
	var out []byte
	var acc uint32
	bits := 0
	for _, c := range b[:len(b)-pad] {
		v, ok := val(c)
		if !ok {
			return nil, false
		}
		acc = acc<<6 | v
		bits += 6
		if bits >= 8 {
			bits -= 8
			out = append(out, byte(acc>>bits))
			acc &= 1<<bits - 1
		}
	}
	// The leftover bits of a padded group must be zero, or .NET refuses it.
	if acc != 0 {
		return nil, false
	}
	return out, true
}

// CursorCookie: Cursor's web session cookie is "user id::token"; the user id is the last
// part of the token's subject. False when the token is malformed or about to expire.
func CursorCookie(token string, utcNow time.Time) (string, bool) {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return "", false
	}
	body := strings.NewReplacer("-", "+", "_", "/").Replace(parts[1])
	body += strings.Repeat("=", (4-len(body)%4)%4)
	bytes, ok := base64(body)
	if !ok || !utf8.Valid(bytes) {
		return "", false
	}
	root, ok := parse(core.TextOf(bytes))
	if !ok {
		return "", false
	}
	if e := numOf(root, "exp"); e != nil {
		t, ok := fromUnixSecs(*e)
		if !ok || !t.After(utcNow.Add(60*time.Second)) {
			return "", false
		}
	}
	sub := strOf(root, "sub")
	if sub == nil {
		return "", false
	}
	id := ""
	for _, p := range strings.Split(*sub, "|") {
		if p != "" {
			id = p
		}
	}
	if id == "" {
		return "", false
	}
	for _, c := range id {
		if !(c < 0x80 && (c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '.' || c == '_' || c == '-')) {
			return "", false
		}
	}
	return "WorkosCursorSessionToken=" + id + "%3A%3A" + token, true
}

// ParseCursorSummary reads usage-summary: the plan's percentages are already in percent.
// Older and team accounts report cents used against a limit instead.
func ParseCursorSummary(text string) Reading {
	root, ok := parse(text)
	if !ok {
		return Fail("Cursor’s answer couldn’t be read.")
	}
	get := func(path ...string) (core.JSON, bool) {
		cur := root
		for _, p := range path {
			if cur.Kind() != core.ObjKind {
				return core.JNull, false
			}
			var ok bool
			if cur, ok = cur.Get(p); !ok {
				return core.JNull, false
			}
		}
		return cur, true
	}
	d := func(path ...string) *float64 {
		v, ok := get(path...)
		if !ok || v.Kind() != core.NumKind {
			return nil
		}
		f, err := v.F64()
		if err != nil {
			return nil
		}
		return &f
	}
	ratio := func(a, b string) *float64 {
		u, l := d(a, b, "used"), d(a, b, "limit")
		if u != nil && l != nil && *l > 0 {
			r := *u / *l * 100
			return &r
		}
		return nil
	}

	auto := d("individualUsage", "plan", "autoPercentUsed")
	api := d("individualUsage", "plan", "apiPercentUsed")
	var used *float64
	for _, c := range []func() *float64{
		func() *float64 { return d("individualUsage", "plan", "totalPercentUsed") },
		func() *float64 {
			switch {
			case auto != nil && api != nil:
				m := (*auto + *api) / 2
				return &m
			case auto != nil:
				return auto
			}
			return api
		},
		func() *float64 { return ratio("individualUsage", "plan") },
		func() *float64 { return ratio("individualUsage", "overall") },
		func() *float64 { return ratio("teamUsage", "pooled") },
	} {
		if used = c(); used != nil {
			break
		}
	}
	if used == nil {
		return Fail("Cursor didn’t report plan usage.")
	}

	detail := ""
	if m := strOf(root, "membershipType"); m != nil && *m != "" {
		detail = capital(*m) + " · "
	}
	detail += Custom(*used, 0) + "% of plan"
	if e := strOf(root, "billingCycleEnd"); e != nil {
		if t, ok := ParseTime(*e); ok {
			detail += " · resets " + FormatLocal(t, "d MMM")
		}
	}
	return newReading(min(max(*used, 0), 100), detail)
}

// CleanToken is trimmed of white space, then of quotes; false when nothing is left.
func CleanToken(s string) (string, bool) {
	s = strings.Trim(strings.TrimSpace(s), `"`)
	return s, s != ""
}

// MARK: Claude Code

// SignIn is Claude Code's sign-in from .credentials.json: {"claudeAiOauth":
// {"accessToken", "expiresAt" (ms), "subscriptionType"}}.
type SignIn struct {
	Token   *string
	Expires *time.Time
	Plan    *string
}

func ClaudeSignIn(text string) SignIn {
	root, ok := parse(text)
	if !ok {
		return SignIn{}
	}
	o, ok := objOf(root, "claudeAiOauth")
	if !ok {
		return SignIn{}
	}
	s := SignIn{Plan: strOf(o, "subscriptionType")}
	if t := strOf(o, "accessToken"); t != nil && *t != "" {
		s.Token = t
	}
	if ms := numOf(o, "expiresAt"); ms != nil {
		t, ok := fromUnixMs(*ms)
		if !ok {
			return SignIn{}
		}
		s.Expires = &t
	}
	return s
}

// ParseClaudeUsage reads the usage answer: five_hour and seven_day, each a utilization in
// percent and an ISO resets_at (null while the window hasn't begun). The notch shows
// whichever is closer to the limit, as it does for Codex.
func ParseClaudeUsage(text string, plan *string, now time.Time) Reading {
	root, ok := parse(text)
	if !ok {
		return Fail("Anthropic’s answer couldn’t be read.")
	}
	type window struct {
		used  float64
		label string
		reset *time.Time
	}
	var windows []window
	for _, w := range [][2]string{{"five_hour", "5h"}, {"seven_day", "week"}} {
		o, ok := objOf(root, w[0])
		if !ok {
			continue
		}
		u := numOf(o, "utilization")
		if u == nil {
			continue
		}
		used := *u
		var reset *time.Time
		if r := strOf(o, "resets_at"); r != nil {
			if t, ok := ParseTime(*r); ok {
				reset = &t
			}
		}
		if reset != nil && !reset.After(now) {
			used = 0
		}
		windows = append(windows, window{min(max(used, 0), 100), w[1], reset})
	}
	if len(windows) == 0 {
		return Fail("Anthropic didn’t report plan usage.")
	}
	// MaxBy: the first of equals.
	top := windows[0]
	for _, w := range windows[1:] {
		if w.used > top.used {
			top = w
		}
	}
	planText := ""
	if plan != nil && *plan != "" {
		planText = capital(*plan) + " · "
	}
	resetText := ""
	if top.reset != nil {
		shown := FormatLocal(*top.reset, "d MMM HH:mm")
		if sameLocalDay(*top.reset, now) {
			shown = FormatLocal(*top.reset, "HH:mm")
		}
		resetText = " · resets " + shown
	}
	parts := make([]string, len(windows))
	for i, w := range windows {
		parts[i] = fmt.Sprintf("%s %s%%", w.label, Custom(w.used, 0))
	}
	return newReading(top.used, planText+strings.Join(parts, " · ")+resetText)
}

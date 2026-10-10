package agents

// Services/Spaces.cs: each project's own desktop, a Cua Space (spaces.cua.ai), a VM or
// container that the agents working in that folder share (each with its own cursor), and
// that the user watches and steps into, instead of the user's own screen. Hover goes
// through Cua's own `cua` CLI (MIT): `cua spaces create|start|stop|delete` for the Space,
// `cua mcp --sandbox <space>` as the session's computer-use MCP server (run by Hover
// outside the agents' sandbox and joined to the agent over the same socket as Hover's
// browser, Bridge), `cua sb view` for the live viewer the Screen panel shows, and
// `cua sb cp` / `sb exec` for an app or files dragged onto the notch. A project's Space is
// made when its first agent's run starts, stopped when no agent of that project is left in
// the office, and deleted with the project's last session.
//
// Cua is kept to the Mac here, as Cua Driver is: where SpacesSupported is false the switch
// is off with SpacesUnsupported beside it. Everything that can be a plain function (names,
// the lists Cua and Lume print, the install script, the progress lines, the viewer's
// address) is one, compiled and tested on every OS.
//
// Every call blocks, so the backend runs them on goroutines of its own.

import (
	"bufio"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net"
	"net/netip"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"

	"github.com/4regab/Hover/internal/core"
	"github.com/dlclark/regexp2"
)

// SpaceServerName is the MCP server a session's tool is given for its project's desktop.
const SpaceServerName = "cua-space"

// SpacesUnsupported is what Settings shows beside the switch where Spaces can't run.
const SpacesUnsupported = "Agent desktops need macOS 26 or later on Apple silicon."

// AppLimit is the largest app that goes into a desktop (its files added up).
const AppLimit uint64 = 4 << 30

// SpacePermissions: computer use inside the Space, and its files; never its shell (the
// agent has its own) or Spaces' admin tools.
const SpacePermissions = "computer:screenshot,computer:click,computer:type,computer:key,computer:scroll,computer:drag,computer:hotkey,computer:window,computer:accessibility,computer:clipboard"

// MARK: Whether and which

// Switches are what the settings say, read whenever it matters: the switch, and the image
// a new Space starts from.
type Switches struct{ On, Linux bool }

var spacesSource struct {
	sync.Mutex
	f func() Switches
}

// SetSpacesSource says where the switches come from: the backend hands in a reader of its
// settings (AgentSpaces and SpaceImage). Off, on the macOS image, until it does.
func SetSpacesSource(f func() Switches) {
	spacesSource.Lock()
	spacesSource.f = f
	spacesSource.Unlock()
}

func switches() Switches {
	spacesSource.Lock()
	f := spacesSource.f
	spacesSource.Unlock()
	if f == nil {
		return Switches{}
	}
	return f()
}

// SupportedOn: Cua Spaces runs its macOS VMs with Apple's virtualization, on macOS 26 or
// later and Apple silicon only. The decision as a function of what the machine says
// (Rust's OS and arch names), so it is tested everywhere.
func SupportedOn(os string, major uint32, arch string) bool {
	return os == "macos" && major >= 26 && (arch == "aarch64" || arch == "arm64")
}

// ProductMajor is the major number of a `sw_vers -productVersion` ("26.0.1"), or 0.
func ProductMajor(text string) uint32 {
	m, _, _ := strings.Cut(strings.TrimSpace(text), ".")
	n, err := strconv.ParseUint(m, 10, 32)
	if err != nil {
		return 0
	}
	return uint32(n)
}

var spacesSupported = sync.OnceValue(func() bool {
	if runtime.GOOS != "darwin" {
		return false
	}
	code, text := SpacesRun("/usr/bin/sw_vers", 10*time.Second, "-productVersion")
	major := uint32(0)
	if code == 0 {
		major = ProductMajor(text)
	}
	arch := runtime.GOARCH
	if arch == "arm64" {
		arch = "aarch64"
	}
	return SupportedOn("macos", major, arch)
})

func SpacesSupported() bool { return spacesSupported() }

// SpacesNote is why the switch is disabled here, or nil.
func SpacesNote() *string {
	if SpacesSupported() {
		return nil
	}
	return sp(SpacesUnsupported)
}

// SpacesWanted: off until switched on in Settings → Computer Use; then it replaces Cua
// Driver on the user's own desktop.
func SpacesWanted() bool { return switches().On && SpacesSupported() }

// SpaceImage is the image a new Space starts from: the macOS VM (two at most on a Mac) or Linux.
func SpaceImage() string {
	if switches().Linux || runtime.GOOS != "darwin" {
		return "linux"
	}
	return "macos:26"
}

func firstFile(paths ...string) string {
	for _, p := range paths {
		if isFile(p) {
			return p
		}
	}
	return ""
}

// CuaCLI is the cua CLI, from PATH or where its installer puts it; "" when neither.
func CuaCLI() string {
	if p := OnPath("cua"); p != "" {
		return p
	}
	return firstFile(filepath.Join(Home(), ".local", "bin", "cua"), "/usr/local/bin/cua", "/opt/homebrew/bin/cua")
}

// LumeExe is Lume, which runs the macOS VMs: the source of truth for whether one is on, and
// how it is sized. Cua's own list has no power state for local VMs.
func LumeExe() string {
	if p := OnPath("lume"); p != "" {
		return p
	}
	return firstFile(filepath.Join(Home(), ".local", "bin", "lume"), filepath.Join(Home(), ".local", "share", "lume", "lume.app", "Contents", "MacOS", "lume"))
}

// MARK: Names

// spacesFullPath is Path.GetFullPath, without the disk: made absolute, . and .. resolved,
// no trailing separator (except a root's).
func spacesFullPath(folder string) string {
	p := folder
	if !filepath.IsAbs(p) && !strings.HasPrefix(p, "/") && !strings.HasPrefix(p, `\`) {
		wd, _ := os.Getwd()
		p = filepath.Join(wd, p)
	}
	return filepath.Clean(p)
}

// spacesFileName is Path.GetFileName: after the last separator.
func spacesFileName(p string) string {
	seps := "/"
	if runtime.GOOS == "windows" {
		seps = `\/:`
	}
	if i := strings.LastIndexAny(p, seps); i >= 0 {
		return p[i+1:]
	}
	return p
}

// SpaceName is the Space a project's agents share: "hover-", the folder's name and a short
// hash of its full path, so two folders called "app" get two desktops. The same folder,
// however it is written, gets the same one; paths compare without case except on Linux.
// The hash is SHA-256's, as the C# and Rust builds made it.
func SpaceName(folder string) string {
	full := spacesFullPath(folder)
	if runtime.GOOS != "linux" {
		full = strings.ToLower(full)
	}
	sum := sha256.Sum256([]byte(full))
	hash := hex.EncodeToString(sum[:])[:6]
	var slug strings.Builder
	for _, c := range strings.ToLower(spacesFileName(full)) {
		if 'a' <= c && c <= 'z' || '0' <= c && c <= '9' {
			slug.WriteRune(c)
		} else if !strings.HasSuffix(slug.String(), "-") {
			slug.WriteByte('-')
		}
	}
	s := strings.Trim(slug.String(), "-")
	if len(s) > 20 {
		s = strings.TrimRight(s[:20], "-")
	}
	if s != "" {
		s += "-"
	}
	return "hover-" + s + hash
}

func SpaceID(folder string) string { return "local:" + SpaceName(folder) }

// SpaceTitle is the project's name as the desktop shows it.
func SpaceTitle(folder string) string {
	seps := "/"
	if runtime.GOOS == "windows" {
		seps = `\/`
	}
	if n := spacesFileName(strings.TrimRight(folder, seps)); n != "" {
		return n
	}
	return folder
}

// SameProject: whether two folders are one project.
func SameProject(a, b string) bool { return SpaceName(a) == SpaceName(b) }

// MARK: What Cua and Lume print

// SpaceSize is what a desktop is given, from this Mac's size: enough to be smooth, leaving
// the user most of their machine. Cua's default (2 cores, 4 GB, 1024×768) is sluggish for
// macOS 26 and blurry in the panel.
type SpaceSize struct {
	CPUs, MemoryGB int32
	Display        string
}

// TargetFor is the size for a Mac of this much memory (whole GB) and this many cores.
func TargetFor(totalGB uint64, cores int) SpaceSize {
	mem := int32(4)
	if totalGB >= 24 {
		mem = 8
	} else if totalGB >= 16 {
		mem = 6
	}
	return SpaceSize{int32(max(2, min(cores/3, 6))), mem, "1024x768"}
}

// totalGB is physical memory in whole GB, or 0 when it can't be read.
var totalGB = sync.OnceValue(func() uint64 {
	var bytes uint64
	switch runtime.GOOS {
	case "darwin":
		if code, text := SpacesRun("/usr/sbin/sysctl", 10*time.Second, "-n", "hw.memsize"); code == 0 {
			bytes, _ = strconv.ParseUint(strings.TrimSpace(text), 10, 64)
		}
	case "linux":
		if b, err := os.ReadFile("/proc/meminfo"); err == nil {
			for _, l := range rustLines(string(b)) {
				if v, ok := strings.CutPrefix(l, "MemTotal:"); ok {
					if v, ok := strings.CutSuffix(strings.TrimSpace(v), "kB"); ok {
						if kb, err := strconv.ParseUint(strings.TrimSpace(v), 10, 64); err == nil {
							bytes = kb << 10
							break
						}
					}
				}
			}
		}
	}
	return bytes >> 30
})

func SpaceTarget() SpaceSize { return TargetFor(totalGB(), runtime.NumCPU()) }

// VmInfo is a local VM as Lume sees it.
type VmInfo struct {
	Exists, Running bool
	CPUs, MemoryGB  int32
	Display         *string
}

// ParseVM reads `lume get --format json` loosely: text before the JSON is skipped, and it
// may be one object or a list of them.
func ParseVM(text string) (VmInfo, bool) {
	i := strings.IndexAny(text, "{[")
	if i < 0 {
		return VmInfo{}, false
	}
	v, err := core.ParseJSON(text[i:])
	if err != nil {
		return VmInfo{}, false
	}
	e := v
	if v.Kind() == core.ArrKind {
		items, _ := v.Items()
		if len(items) == 0 {
			return VmInfo{}, false
		}
		e = items[0]
	}
	if e.Kind() != core.ObjKind {
		return VmInfo{}, false
	}
	status, _ := str(e, "status")
	status = strings.ToLower(status)
	info := VmInfo{Exists: true, Running: status == "running" || status == "booting" || status == "starting", Display: optStr(e, "display")}
	if n, ok := e.Get("cpuCount"); ok && n.Kind() == core.NumKind {
		c, err := strconv.ParseInt(n.Compact(), 10, 32)
		if err == nil {
			info.CPUs = int32(c)
		}
	}
	if n, ok := e.Get("memorySize"); ok && n.Kind() == core.NumKind {
		if m, err := strconv.ParseInt(n.Compact(), 10, 64); err == nil {
			info.MemoryGB = int32(m / (1 << 30))
		}
	}
	return info, true
}

// vm is a local VM as Lume sees it (status running/stopped, its size); false when Lume
// isn't there or doesn't know it.
func vm(name string) (VmInfo, bool) {
	lume := LumeExe()
	if lume == "" {
		return VmInfo{}, false
	}
	code, text := SpacesRun(lume, 15*time.Second, "get", name, "--format", "json")
	if code != 0 {
		return VmInfo{}, strings.Contains(strings.ToLower(text), "not found")
	}
	return ParseVM(text)
}

type SpaceInfo struct {
	ID, Name string
	Running  bool
	OS       *string
}

// ParseSpaceList reads `cua spaces ls --json` loosely: a list, or an object with the list
// in spaces, after whatever notice Cua printed first. A Space without a power state is on.
func ParseSpaceList(text string) []SpaceInfo {
	i := strings.IndexAny(text, "[{")
	if i < 0 {
		return nil
	}
	v, err := core.ParseJSON(text[i:])
	if err != nil {
		return nil
	}
	if v.Kind() != core.ArrKind {
		inner, ok := v.Get("spaces")
		if !ok {
			return nil
		}
		v = inner
	}
	items, err := v.Items()
	if err != nil {
		return nil
	}
	var out []SpaceInfo
	for _, e := range items {
		id, _ := str(e, "id")
		name, ok := str(e, "name")
		if !ok {
			name = id[strings.LastIndexByte(id, ':')+1:]
		}
		power, ok := str(e, "power_state")
		if !ok {
			if power, ok = str(e, "state"); !ok {
				power = "running"
			}
		}
		power = strings.ToLower(power)
		out = append(out, SpaceInfo{id, name, power == "running" || power == "ready" || power == "on", optStr(e, "os")})
	}
	return out
}

// InstallScript is the guest's side of an app sent in: unpack into /Applications (or
// ~/Applications), replacing an older copy, and open it. Every name is single-quoted, so
// none is read as shell.
func InstallScript(guestZip, appName string) string {
	return fmt.Sprintf(`set -e; z=%s; a=%s; d=/Applications; [ -w "$d" ] || { d="$HOME/Applications"; mkdir -p "$d"; }; `, shQuote(guestZip), shQuote(appName)) +
		`rm -rf "$d/$a"; /usr/bin/ditto -x -k "$z" "$d"; rm -f "$z"; /usr/bin/open "$d/$a"`
}

// Frame is one line of progress from cua or an installer: what to show, and how far along,
// when it says.
type Frame struct {
	Line     string
	Fraction *float64
}

var (
	percentRe = regexp2.MustCompile(`(\d{1,3}(?:\.\d+)?)\s?%`, regexp2.None)
	ofRe      = regexp2.MustCompile(`\b(\d+)\s+(\d+)\s*$`, regexp2.None)
	semverRe  = regexp2.MustCompile(`\d+\.\d+\.\d+`, regexp2.None)
)

// FrameOf is a line as the tool printed it: the line itself without colour codes (what an
// error says), and the frame to show. A JSON progress line ({"phase": .., "fraction": ..}),
// a percentage, or "<sent> <total>" give the fraction. False for a blank one.
func FrameOf(raw string) (string, Frame, bool) {
	l := strings.TrimSpace(StripANSI(raw))
	if l == "" {
		return "", Frame{}, false
	}
	line := l
	var fraction *float64
	if strings.HasPrefix(line, "{") {
		if d, err := core.ParseJSON(line); err == nil {
			if n, ok := d.Get("fraction"); ok && n.Kind() == core.NumKind {
				if f, err := strconv.ParseFloat(n.Compact(), 64); err == nil {
					fraction = &f
				}
			}
			if phase, ok := str(d, "phase"); ok {
				switch phase {
				case "pulling":
					line = "Downloading the desktop image…"
				case "creating":
					line = "Making the desktop…"
				case "booting":
					line = "Starting it up…"
				case "waiting_for_services", "connecting":
					line = "Almost ready…"
				case "ready":
					line = "Ready."
				default:
					line = phase
				}
			}
		}
	} else if m, _ := percentRe.FindStringMatch(line); m != nil {
		if p, err := strconv.ParseFloat(m.GroupByNumber(1).String(), 64); err == nil {
			fraction = fp(p / 100)
		}
	} else if m, _ := ofRe.FindStringMatch(line); m != nil {
		sent, e1 := strconv.ParseFloat(m.GroupByNumber(1).String(), 64)
		total, e2 := strconv.ParseFloat(m.GroupByNumber(2).String(), 64)
		if e1 == nil && e2 == nil && total > 0 {
			fraction = fp(sent / total)
		}
	}
	return l, Frame{clipChars(line, 140), fraction}, true
}

// spacesLastLine is the last non-empty line of some output, cut at 200.
func spacesLastLine(text string) *string {
	lines := strings.Split(strings.ReplaceAll(text, "\r", ""), "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		if l := strings.TrimSpace(lines[i]); l != "" {
			return sp(clipChars(l, 200))
		}
	}
	return nil
}

func orLine(p *string, d string) string {
	if p != nil {
		return *p
	}
	return d
}

// ViewerURL is the address of a Space's viewer in what `cua sb view` printed: http(s),
// anything up to /viewer/# and something after it, none of it blank or quoted.
func ViewerURL(text string) (string, bool) {
	for from := 0; ; {
		at := strings.Index(text[from:], "http")
		if at < 0 {
			return "", false
		}
		start := from + at
		from = start + 4
		rest := text[start:]
		scheme := 0
		switch {
		case strings.HasPrefix(rest, "https://"):
			scheme = 8
		case strings.HasPrefix(rest, "http://"):
			scheme = 7
		default:
			continue
		}
		run := rest[scheme:]
		end := strings.IndexFunc(run, func(c rune) bool { return unicode.IsSpace(c) || c == '"' || c == '\'' })
		if end < 0 {
			end = len(run)
		}
		run = run[:end]
		for i := 0; ; {
			j := strings.Index(run[i:], "/viewer/#")
			if j < 0 {
				break
			}
			j += i
			if j >= 1 && len(run) > j+len("/viewer/#") {
				return rest[:scheme+end], true
			}
			i = j + 1
		}
	}
}

// SplitURL is where a viewer's address points: its scheme, host and port, and the rest of
// it after the authority. False for one with a login in it or a port that isn't one.
func SplitURL(u string) (scheme, host string, port uint16, rest string, ok bool) {
	scheme, after, found := strings.Cut(u, "://")
	if !found {
		return
	}
	end := strings.IndexAny(after, "/?#")
	if end < 0 {
		end = len(after)
	}
	authority, rest := after[:end], after[end:]
	if authority == "" || strings.Contains(authority, "@") {
		return
	}
	def := uint16(80)
	if strings.EqualFold(scheme, "https") {
		def = 443
	}
	if v6, isV6 := strings.CutPrefix(authority, "["); isV6 {
		h, p, found := strings.Cut(v6, "]")
		if !found {
			return
		}
		host, port = "["+h+"]", def
		if p != "" {
			pp, has := strings.CutPrefix(p, ":")
			if !has {
				return
			}
			if port, ok = rustU16(pp); !ok {
				return
			}
		}
	} else if i := strings.LastIndexByte(authority, ':'); i >= 0 {
		host = authority[:i]
		if port, ok = rustU16(authority[i+1:]); !ok {
			return
		}
	} else {
		host, port = authority, def
	}
	return scheme, strings.ToLower(host), port, rest, true
}

// LocalHost: whether a viewer at this host is on this Mac or its own network: loopback, or
// a private address of the kind the VM has.
func LocalHost(host string) bool {
	switch host {
	case "127.0.0.1", "localhost", "[::1]":
		return true
	}
	a, err := netip.ParseAddr(host)
	if err != nil || !a.Is4() {
		return false
	}
	b := a.As4()
	return b[0] == 10 || b[0] == 192 && b[1] == 168
}

// MARK: Status and setup

// SpacesStatus is installed, ready (the image is on the Mac and Spaces answer), and what to do.
type SpacesStatus struct {
	Installed, Ready bool
	Version          *string
	Hint             string
	Running          int32
}

// SpacesProgress is what a setup is doing ("installing" or "preparing"), its newest line
// and how far along, and why it stopped if it failed.
type SpacesProgress struct {
	Step     *string
	Line     string
	Fraction *float64
	Error    *string
}

var spacesShared struct {
	sync.Mutex
	progress SpacesProgress
	known    *struct {
		at time.Time
		s  SpacesStatus
	}
	setup *Cancel
}

var spacesListeners struct {
	sync.Mutex
	list []func()
}

// OnSpacesChange is called on the goroutine that made the change, whenever the status, a
// setup's progress or a desktop's state changes: a listener must only post, never block.
func OnSpacesChange(f func()) {
	spacesListeners.Lock()
	spacesListeners.list = append(spacesListeners.list, f)
	spacesListeners.Unlock()
}

func spacesChanged() {
	spacesListeners.Lock()
	all := slices.Clone(spacesListeners.list)
	spacesListeners.Unlock()
	for _, f := range all {
		f()
	}
}

// Progress comes many times a second while an image downloads; the office is told at most
// four times a second, which is all a progress bar needs.
var spacesPending atomic.Bool

func spacesNotify() {
	if spacesPending.Swap(true) {
		return
	}
	go func() {
		time.Sleep(250 * time.Millisecond)
		spacesPending.Store(false)
		spacesChanged()
	}()
}

func SpacesSetup() SpacesProgress {
	spacesShared.Lock()
	defer spacesShared.Unlock()
	return spacesShared.progress
}

func SpacesKnown() (SpacesStatus, bool) {
	spacesShared.Lock()
	defer spacesShared.Unlock()
	if spacesShared.known == nil {
		return SpacesStatus{}, false
	}
	return spacesShared.known.s, true
}

func SpacesBusy() bool {
	spacesShared.Lock()
	defer spacesShared.Unlock()
	return spacesShared.setup != nil
}

func SpacesCancel() {
	spacesShared.Lock()
	c := spacesShared.setup
	spacesShared.Unlock()
	if c != nil {
		c.Cancel()
	}
}

func spacesReport(p SpacesProgress) {
	spacesShared.Lock()
	spacesShared.progress = p
	spacesShared.Unlock()
	spacesChanged()
}

// marker: Hover remembers the image it prepared; Cua's cache keeps the image itself.
func marker() string {
	return filepath.Join(core.Support(), "spaces", "prepared-"+strings.ReplaceAll(SpaceImage(), ":", "-"))
}

func imagePulled() bool { return isFile(marker()) }

// SpacesCheck is installed, ready and what to do, from cua's own commands. Kept for a
// minute. Blocks: call it off the UI goroutine.
func SpacesCheck(fresh bool) SpacesStatus {
	if !fresh {
		spacesShared.Lock()
		k := spacesShared.known
		spacesShared.Unlock()
		if k != nil && time.Since(k.at) < time.Minute {
			return k.s
		}
	}
	s := spacesLook()
	spacesShared.Lock()
	spacesShared.known = &struct {
		at time.Time
		s  SpacesStatus
	}{time.Now(), s}
	spacesShared.Unlock()
	spacesChanged()
	return s
}

func spacesLook() SpacesStatus {
	no := func(hint string) SpacesStatus { return SpacesStatus{Hint: hint} }
	if !SpacesSupported() {
		return no(SpacesUnsupported)
	}
	cua := CuaCLI()
	if cua == "" {
		return no("Set up Cua’s desktop tools to give each agent a desktop of its own.")
	}
	vc, vt := SpacesRun(cua, 20*time.Second, "--version")
	var version *string
	if vc == 0 {
		v := ""
		if m, _ := semverRe.FindStringMatch(vt); m != nil {
			v = m.String()
		}
		version = &v
	}
	lc, lt := SpacesRun(cua, 30*time.Second, "spaces", "ls", "--json")
	var list []SpaceInfo
	if lc == 0 {
		list = ParseSpaceList(lt)
	}
	ready := lc == 0 && imagePulled()
	hint := ""
	switch {
	case lc != 0:
		hint = fmt.Sprintf("Cua Spaces isn’t answering: %s.", orLine(spacesLastLine(lt), "run cua doctor"))
	case !ready:
		hint = "Prepare the desktop image once (a one-time download)."
	}
	// A hover- Space by its id (local:hover-…): Cua's name for a VM can be the guest's hostname.
	mine := int32(0)
	for _, x := range list {
		n, local := strings.CutPrefix(x.ID, "local:")
		if x.Running && (strings.HasPrefix(x.Name, "hover-") || local && strings.HasPrefix(n, "hover-")) {
			mine++
		}
	}
	return SpacesStatus{true, ready, version, hint, mine}
}

// SpacesRunSetup is one click: installs Cua's CLI if missing (Cua's own installer, no
// sign-in), sets up the runtime the image needs (Lume for macOS VMs), and makes and deletes
// one Space so the image is on the Mac before the first task. Blocks until done; it runs
// only when the user asks.
func SpacesRunSetup() {
	ct := NewCancel()
	spacesShared.Lock()
	if spacesShared.setup != nil {
		spacesShared.Unlock()
		return
	}
	spacesShared.setup = ct
	spacesShared.Unlock()
	err := spacesSetupSteps(ct)
	// No longer under way before the end is told: the message it raises reads busy at once.
	spacesShared.Lock()
	spacesShared.setup = nil
	spacesShared.Unlock()
	if se, ok := err.(*StepError); ok && !se.Cancelled {
		spacesReport(SpacesProgress{Error: sp(se.Msg)})
	} else {
		spacesReport(SpacesProgress{})
	}
	SpacesCheck(true)
}

func spacesSetupSteps(ct *Cancel) error {
	if !SpacesSupported() {
		return &StepError{Msg: SpacesUnsupported}
	}
	installing := func(f Frame) {
		spacesReport(SpacesProgress{Step: sp("installing"), Line: f.Line, Fraction: f.Fraction})
	}
	if CuaCLI() == "" {
		// Only Cua's command-line tool: the desktops live in Hover, never in an app of Cua's.
		spacesReport(SpacesProgress{Step: sp("installing"), Line: "Installing Cua’s desktop tools…"})
		if err := spacesStream("/bin/bash", []string{"-c", "set -o pipefail; curl -fsSL https://cua.ai/install.sh | sh -s -- --cli-only --yes --no-onboarding"}, 15*time.Minute, ct, installing); err != nil {
			return err
		}
		if CuaCLI() == "" {
			return &StepError{Msg: "The installer finished, but cua still isn’t found."}
		}
	}
	cua := CuaCLI()
	if strings.HasPrefix(SpaceImage(), "macos") {
		spacesReport(SpacesProgress{Step: sp("installing"), Line: "Setting up the macOS desktop runtime (Lume)…"})
		if err := spacesStream(cua, []string{"runtime", "setup", "lume"}, 15*time.Minute, ct, installing); err != nil {
			return err
		}
	}
	spacesReport(SpacesProgress{Step: sp("preparing"), Line: "Downloading the desktop image (one time; macOS is about 23 GB)…", Fraction: fp(0)})
	probe := "hover-prepare"
	if err := spacesStream(cua, []string{"spaces", "create", SpaceImage(), "--name", probe, "--json"}, time.Hour, ct,
		func(f Frame) { spacesReport(SpacesProgress{Step: sp("preparing"), Line: f.Line, Fraction: f.Fraction}) }); err != nil {
		return err
	}
	SpacesRun(cua, 2*time.Minute, "spaces", "delete", "local:"+probe, "--force")
	m := marker()
	os.MkdirAll(filepath.Dir(m), 0o777)
	os.WriteFile(m, []byte(strconv.FormatInt(time.Now().Unix(), 10)), 0o666)
	return nil
}

// MARK: One Space per project

// SpaceState is what a project's Space is doing, for its desks: "creating", "starting",
// "ready", "stopped" or "failed" with a reason.
type SpaceState struct {
	Phase, Line string
	Fraction    *float64
	Error       *string
}

var spacesMaps = struct {
	sync.Mutex
	states  map[string]SpaceState
	gates   map[string]*sync.Mutex
	folders map[string]string // each Space by name, and the folder it is for (the bridge only knows the name)
	// Each desktop's viewer link, kept while its ticket lasts: opening the panel again (or
	// another agent's of the same project) shows the same live view, not a reload.
	viewers  map[string]viewerLink
	homes    map[string]string
	forwards map[string]uint16
}{states: map[string]SpaceState{}, gates: map[string]*sync.Mutex{}, folders: map[string]string{}, viewers: map[string]viewerLink{}, homes: map[string]string{}, forwards: map[string]uint16{}}

type viewerLink struct {
	url   string
	until time.Time
}

func SpaceStateOf(folder string) (SpaceState, bool) {
	spacesMaps.Lock()
	defer spacesMaps.Unlock()
	s, ok := spacesMaps.states[SpaceName(folder)]
	return s, ok
}

func setSpace(folder string, s SpaceState) {
	name := SpaceName(folder)
	spacesMaps.Lock()
	was, had := spacesMaps.states[name]
	spacesMaps.states[name] = s
	spacesMaps.Unlock()
	// A new phase at once; progress within one, a few times a second.
	if !had || was.Phase != s.Phase {
		spacesChanged()
	} else {
		spacesNotify()
	}
}

func spaceExplain(e string) string {
	l := strings.ToLower(e)
	// A Mac runs two macOS VMs at most: say so plainly.
	switch {
	case strings.Contains(l, "limit"):
		return "This Mac already runs two macOS desktops (Apple’s limit). Remove the agents of another project to free one."
	case strings.Contains(l, "insufficient") || strings.Contains(l, "memory"):
		return "There isn’t enough free memory or disk for the desktop right now."
	}
	return e
}

func newSpaceState(phase, line string, fraction *float64, err *string) SpaceState {
	return SpaceState{phase, line, fraction, err}
}

// EnsureSpace is the project's Space, made or started before an agent's run, with
// progress. Two agents starting together wait for one create. nil when it is ready; else
// why not (the run then goes on without a desktop).
func EnsureSpace(folder string, ct *Cancel) *string {
	cua := CuaCLI()
	if cua == "" || !SpacesWanted() {
		return sp("Agent desktops are off.")
	}
	name := SpaceName(folder)
	spacesMaps.Lock()
	spacesMaps.folders[name] = folder
	gate, ok := spacesMaps.gates[name]
	if !ok {
		gate = &sync.Mutex{}
		spacesMaps.gates[name] = gate
	}
	spacesMaps.Unlock()
	// Waited for as WaitAsync(ct) did: a Stop while another agent of the project makes the
	// Space (up to an hour on the first image) is heard. The state is that agent's, so a
	// stop here leaves it alone; only this call's own create clears it.
	for !gate.TryLock() {
		if ct.IsCancelled() {
			return sp("Stopped.")
		}
		time.Sleep(100 * time.Millisecond)
	}
	defer gate.Unlock()
	if ct.IsCancelled() {
		return sp("Stopped.")
	}
	size := SpaceTarget()
	ready := func() { setSpace(folder, newSpaceState("ready", "The project’s desktop is ready.", fp(1), nil)) }
	// Lume knows whether the VM is on; Cua's list only knows it was made.
	machine, haveVM := vm(name)
	id := "local:" + name
	lc, lt := SpacesRun(cua, 30*time.Second, "spaces", "ls", "--json")
	known := lc == 0 && slices.ContainsFunc(ParseSpaceList(lt), func(x SpaceInfo) bool { return x.ID == id || x.Name == name })
	if haveVM && machine.Exists && machine.Running || !haveVM && known {
		ready()
		return nil
	}
	if haveVM && machine.Exists || known {
		// Off: sized up first if Cua made it small (only while it is off).
		if lume := LumeExe(); haveVM && machine.Exists && (machine.CPUs < size.CPUs || machine.MemoryGB < size.MemoryGB) && lume != "" {
			setSpace(folder, newSpaceState("starting", "Giving the desktop more room…", nil, nil))
			if zc, zt := SpacesRun(lume, 2*time.Minute, "set", name, "--cpu", fmt.Sprint(size.CPUs), "--memory", fmt.Sprintf("%dGB", size.MemoryGB)); zc != 0 {
				core.Logf("spaces: couldn't size %s: %s", name, orLine(spacesLastLine(zt), ""))
			}
		}
		setSpace(folder, newSpaceState("starting", "Starting the project’s desktop…", nil, nil))
		if sc, st := SpacesRun(cua, 4*time.Minute, "spaces", "start", id, "--json"); sc != 0 {
			why := spaceExplain(orLine(spacesLastLine(st), "The desktop didn’t start."))
			setSpace(folder, newSpaceState("failed", "", nil, &why))
			return &why
		}
		ready()
		return nil
	}
	setSpace(folder, newSpaceState("creating", "Making the project’s desktop…", fp(0), nil))
	timeout := time.Hour
	if imagePulled() {
		timeout = 10 * time.Minute
	}
	err := spacesStream(cua, []string{"spaces", "create", SpaceImage(), "--name", name, "--cpus", fmt.Sprint(size.CPUs), "--memory-mb", fmt.Sprint(size.MemoryGB * 1024), "--json"}, timeout, ct,
		func(f Frame) { setSpace(folder, newSpaceState("creating", f.Line, f.Fraction, nil)) })
	if err == nil {
		ready()
		return nil
	}
	if se := err.(*StepError); se.Cancelled {
		spacesMaps.Lock()
		delete(spacesMaps.states, SpaceName(folder))
		spacesMaps.Unlock()
		spacesChanged()
		return sp("Stopped.")
	}
	why := spaceExplain(err.(*StepError).Msg)
	setSpace(folder, newSpaceState("failed", "", nil, &why))
	return &why
}

// StopSpace: off when no agent of the project is left in the office; deleted with the
// project's last session (the backend decides when). Blocks.
func StopSpace(folder string) {
	cua := CuaCLI()
	if cua == "" || !SpacesWanted() {
		return
	}
	spacesMaps.Lock()
	delete(spacesMaps.viewers, SpaceName(folder))
	spacesMaps.Unlock()
	SpacesRun(cua, 2*time.Minute, "spaces", "stop", SpaceID(folder))
	setSpace(folder, newSpaceState("stopped", "The desktop is off. The project’s next task starts it again.", nil, nil))
}

// DeleteSpace: the project's Space and everything in it, gone. Blocks.
func DeleteSpace(folder string) {
	cua := CuaCLI()
	if cua == "" || !SpacesSupported() {
		return
	}
	SpacesRun(cua, 3*time.Minute, "spaces", "delete", SpaceID(folder), "--force")
	spacesMaps.Lock()
	delete(spacesMaps.viewers, SpaceName(folder))
	delete(spacesMaps.states, SpaceName(folder))
	spacesMaps.Unlock()
	spacesChanged()
}

// AroundSpaces is a session's run, with its project's desktop made or started first:
// Starting is reported while that goes on, and a desktop that can't be had lets the run go
// on without one.
func AroundSpaces(inner RunTask) RunTask {
	return func(a RunArgs) KiroResult {
		if SpacesWanted() {
			a.Progress(Starting)
			if why := EnsureSpace(a.Folder, a.Ct); why != nil {
				core.Logf("spaces: %s: %s", a.Folder, *why)
			}
		}
		return inner(a)
	}
}

// MARK: The viewer

func errObj(message string) core.JSON { return core.JObj(core.P("error", core.JStr(message))) }

// Viewer is the live viewer of the project's Space: Cua's own HTML5 viewer, interactive
// (the user can step in), with dropped files going to the Space's Downloads. Blocks; the
// answer is the Screen panel's: {phase, url} when it is ready, {phase, line, fraction,
// error} while it isn't, {error} when there is none.
func Viewer(folder string) core.JSON {
	cua := CuaCLI()
	if cua == "" {
		return errObj("Cua’s desktop tools aren’t installed.")
	}
	name := SpaceName(folder)
	spacesMaps.Lock()
	cached, ok := spacesMaps.viewers[name]
	spacesMaps.Unlock()
	st, has := SpaceStateOf(folder)
	if ok && cached.until.After(time.Now()) && has && st.Phase == "ready" {
		return core.JObj(core.P("phase", core.JStr("ready")), core.P("url", core.JStr(cached.url)))
	}
	// On first: the viewer of a desktop that is off never loads.
	if !(has && (st.Phase == "creating" || st.Phase == "starting")) {
		if why := EnsureSpace(folder, NewCancel()); why != nil {
			return core.JObj(core.P("phase", core.JStr("failed")), core.P("error", core.JStr(*why)))
		}
	}
	if st, has := SpaceStateOf(folder); has && st.Phase != "ready" {
		return core.JObj(core.P("phase", core.JStr(st.Phase)), core.P("line", core.JStr(st.Line)),
			core.P("fraction", core.JOptDouble(st.Fraction)), core.P("error", core.JOptStr(st.Error)))
	}
	code, text := SpacesRun(cua, 45*time.Second, "sb", "view", SpaceID(folder), "--no-open", "--ttl", "12h")
	u, found := ViewerURL(text)
	if !found || code != 0 {
		return errObj(orLine(spacesLastLine(text), "The desktop’s viewer didn’t open."))
	}
	scheme, host, port, rest, ok := SplitURL(u)
	if !ok || !LocalHost(host) {
		return errObj("The viewer isn’t on this Mac.")
	}
	// On the VM's own address the page isn't a secure context, and the viewer falls back to
	// PNG frames (no video, no audio, no clipboard); through localhost it streams.
	switch host {
	case "127.0.0.1", "localhost", "[::1]":
	default:
		if p, ok := localForward(host, port); ok {
			u = fmt.Sprintf("%s://127.0.0.1:%d%s", scheme, p, rest)
		}
	}
	spacesMaps.Lock()
	spacesMaps.viewers[name] = viewerLink{u, time.Now().Add(11 * time.Hour)}
	spacesMaps.Unlock()
	return core.JObj(core.P("phase", core.JStr("ready")), core.P("url", core.JStr(u)))
}

// localForward is one loopback port per desktop's viewer, joined byte for byte to the VM's
// (HTTP and its WebSocket alike); only this Mac can reach it, and the viewer still needs
// its ticket.
func localForward(host string, port uint16) (uint16, bool) {
	key := fmt.Sprintf("%s:%d", host, port)
	spacesMaps.Lock()
	defer spacesMaps.Unlock()
	if p, ok := spacesMaps.forwards[key]; ok {
		return p, true
	}
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		core.Logf("viewer forward: %v", err)
		return 0, false
	}
	local := uint16(l.Addr().(*net.TCPAddr).Port)
	target := net.JoinHostPort(strings.Trim(host, "[]"), fmt.Sprint(port))
	go func() {
		for {
			client, err := l.Accept()
			if err != nil {
				return
			}
			go func() {
				vmc, err := net.DialTimeout("tcp", target, 10*time.Second)
				if err != nil {
					client.Close()
					return
				}
				join(client, vmc)
			}()
		}
	}()
	spacesMaps.forwards[key] = local
	return local, true
}

// join copies bytes both ways until either side is done, then both are closed.
func join(a, b net.Conn) {
	done := make(chan struct{})
	go func() {
		io.Copy(a, b)
		a.Close()
		b.Close()
		close(done)
	}()
	io.Copy(b, a)
	a.Close()
	b.Close()
	<-done
}

// MARK: Apps and files sent in

// walk is every file under a folder, not following links (and, if asked, not the hidden ones).
func walk(root string, skipHidden bool) []string {
	var files []string
	todo := []string{root}
	for len(todo) > 0 {
		dir := todo[len(todo)-1]
		todo = todo[:len(todo)-1]
		entries, err := os.ReadDir(dir)
		if err != nil {
			continue
		}
		for _, e := range entries {
			if skipHidden && strings.HasPrefix(e.Name(), ".") {
				continue
			}
			p := filepath.Join(dir, e.Name())
			switch t := e.Type(); {
			case t&os.ModeSymlink != 0:
			case t.IsDir():
				todo = append(todo, p)
			default:
				files = append(files, p)
			}
		}
	}
	return files
}

// homeOf is the guest's home ($HOME there), asked once per Space.
func homeOf(cua, id string) (string, bool) {
	spacesMaps.Lock()
	h, ok := spacesMaps.homes[id]
	spacesMaps.Unlock()
	if ok {
		return h, true
	}
	code, text := SpacesRun(cua, 30*time.Second, "sb", "exec", id, "echo $HOME")
	if code != 0 {
		return "", false
	}
	lines := strings.Split(strings.ReplaceAll(text, "\r", ""), "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		if l := strings.TrimSpace(lines[i]); strings.HasPrefix(l, "/") {
			spacesMaps.Lock()
			spacesMaps.homes[id] = l
			spacesMaps.Unlock()
			return l, true
		}
	}
	return "", false
}

// SendApp: an app dragged onto the notch, onto a project's desktop. Hover copies the app
// itself into the desktop (a zip of the bundle, through `cua sb cp`) and opens it there.
// Only the app goes, never its data or the user's sign-ins. progress is told each step.
// The answer is {ok, app} or {error}.
func SendApp(folder, appPath string, progress func(string)) core.JSON {
	cua := CuaCLI()
	if cua == "" {
		return errObj("Cua’s desktop tools aren’t installed.")
	}
	app := strings.TrimRight(appPath, `/\`)
	// From the page: only a whole path, so nothing can reach ditto or cua as an option.
	if !filepath.IsAbs(app) || !strings.HasSuffix(strings.ToLower(app), ".app") || !isDir(app) || strings.ContainsRune(app, 0) {
		return errObj("That isn’t an app Hover can send.")
	}
	name := spacesFileName(app)
	var size uint64
	for _, f := range walk(app, false) {
		if st, err := os.Stat(f); err == nil {
			size += uint64(st.Size())
		}
		if size > AppLimit {
			return errObj(name + " is over 4 GB, too big to copy into the desktop.")
		}
	}
	progress("Starting the desktop…")
	if why := EnsureSpace(folder, NewCancel()); why != nil {
		return errObj(*why)
	}
	id := SpaceID(folder)
	home, ok := homeOf(cua, id)
	if !ok {
		return errObj("The desktop didn’t answer.")
	}
	rnd := make([]byte, 4)
	// Two apps sent at once must not share a zip.
	if _, err := rand.Read(rnd); err != nil {
		return errObj("The app didn’t go.")
	}
	zip := filepath.Join(os.TempDir(), "hover-app-"+hex.EncodeToString(rnd)+".zip")
	out := sendAppZip(cua, id, home, app, name, zip, size, progress)
	os.Remove(zip)
	return out
}

func sendAppZip(cua, id, home, app, name, zip string, size uint64, progress func(string)) core.JSON {
	progress("Packing " + name + "…")
	// No extended attributes: a quarantine flag would stop it opening there.
	if zc, zt := SpacesRun("/usr/bin/ditto", 10*time.Minute, "-c", "-k", "--keepParent", "--norsrc", "--noextattr", app, zip); zc != 0 {
		return errObj(orLine(spacesLastLine(zt), name+" couldn’t be packed."))
	}
	progress(fmt.Sprintf("Copying %s (%d MB)…", name, size/(1<<20)))
	guestZip := home + "/Downloads/.hover-" + filepath.Base(zip)
	if cc, ct := SpacesRun(cua, 20*time.Minute, "sb", "cp", zip, id+":"+guestZip); cc != 0 {
		return errObj(orLine(spacesLastLine(ct), name+" couldn’t be copied."))
	}
	progress("Opening " + name + "…")
	if ec, et := SpacesRun(cua, 5*time.Minute, "sb", "exec", id, InstallScript(guestZip, name)); ec != 0 {
		if strings.Contains(et, "-10825") {
			return errObj(name + " needs a newer macOS than the desktop runs.")
		}
		return errObj(orLine(spacesLastLine(et), name+" didn’t open in the desktop."))
	}
	return core.JObj(core.P("ok", core.JBool(true)), core.P("app", core.JStr(fileStem(name))))
}

// SendFiles: files dropped on a project's desktop in the notch go to its Downloads, through
// `cua sb cp` (folders file by file, at most 20 dropped items and 500 files). The answer is
// {ok, sent} or {error} (with how many had gone, when some did).
func SendFiles(folder string, paths []string) core.JSON {
	cua := CuaCLI()
	if cua == "" {
		return errObj("Cua’s desktop tools aren’t installed.")
	}
	if why := EnsureSpace(folder, NewCancel()); why != nil {
		return errObj(*why)
	}
	id := SpaceID(folder)
	home, ok := homeOf(cua, id)
	if !ok {
		return errObj("The desktop didn’t answer.")
	}
	type pair struct{ from, to string }
	var files []pair
	for _, p := range paths[:min(len(paths), 20)] {
		// From the page: only whole paths, so none reaches cua as an option.
		if !filepath.IsAbs(p) {
			continue
		}
		if isFile(p) {
			files = append(files, pair{p, filepath.Base(p)})
		} else if isDir(p) {
			root := filepath.Dir(strings.TrimRight(p, `/\`))
			all := walk(p, true)
			for _, f := range all[:min(len(all), max(500-len(files), 0))] {
				rel, err := filepath.Rel(root, f)
				if err != nil {
					rel = f
				}
				files = append(files, pair{f, strings.ReplaceAll(rel, `\`, "/")})
			}
		}
	}
	sent := 0
	for _, x := range files {
		if code, text := SpacesRun(cua, 5*time.Minute, "sb", "cp", x.from, id+":"+home+"/Downloads/"+x.to); code != 0 {
			return core.JObj(core.P("error", core.JStr(orLine(spacesLastLine(text), "The files didn’t go."))), core.P("sent", core.JInt(int64(sent))))
		}
		sent++
	}
	return core.JObj(core.P("ok", core.JBool(true)), core.P("sent", core.JInt(int64(sent))))
}

// MARK: The session's computer-use server

// SpacesServers is the MCP server a session's tool gets: Hover's relay (as for its
// browser) to a `cua mcp` Hover runs for the project's Space, outside the agents' sandbox.
// Every agent in that folder gets the same Space, each with its own cursor in it. None when
// desktops are off or the tool can't reach Hover's socket (not a Unix).
func SpacesServers(folder string) []McpServer {
	if !SpacesWanted() || folder == "" || CuaCLI() == "" {
		return nil
	}
	name := SpaceName(folder)
	spacesMaps.Lock()
	spacesMaps.folders[name] = folder
	spacesMaps.Unlock()
	return Bridge("space:"+name, SpaceServerName, serveSpace)
}

// serveSpace is `cua mcp` for one agent's connection: the agent's lines go to it, what it
// prints goes back as it comes, and it ends with the connection.
func serveSpace(tag string, fromAgent io.Reader, toAgent io.Writer) {
	name, ok := strings.CutPrefix(tag, "space:")
	if !ok {
		return
	}
	spacesMaps.Lock()
	folder, ok := spacesMaps.folders[name]
	spacesMaps.Unlock()
	if !ok {
		return
	}
	if why := EnsureSpace(folder, NewCancel()); why != nil {
		core.Logf("spaces: %s: %s", folder, *why)
	}
	cua := CuaCLI()
	if cua == "" {
		return
	}
	cmd := Hidden(cua, "mcp", "--sandbox", SpaceID(folder), "--permissions", SpacePermissions)
	cmd.Env = append(cmd.Env, "CUA_TELEMETRY=0")
	group, err := Spawn(cmd)
	if err != nil {
		core.Logf("spaces: couldn’t start cua mcp - %v", err)
		return
	}
	defer group.Close()
	stdin, stdout, stderr := group.TakePipes()
	go func() {
		defer stderr.Close()
		sc := bufio.NewScanner(stderr)
		for sc.Scan() {
			core.Logf("cua mcp: %s", sc.Text())
		}
	}()
	// The agent's side: a line at a time, as MCP's stdio frames it. It ends with the
	// agent's connection.
	go func() {
		defer stdin.Close()
		r := bufio.NewReader(fromAgent)
		for {
			line, ok := readLineRaw(r, lineLimit)
			if !ok {
				return
			}
			if _, err := stdin.Write(line); err != nil {
				return
			}
		}
	}()
	buf := make([]byte, 16384)
	for {
		n, err := stdout.Read(buf)
		if n > 0 {
			if _, werr := toAgent.Write(buf[:n]); werr != nil {
				break
			}
		}
		if err != nil {
			break
		}
	}
	stdout.Close()
}

// readLineRaw is one line with its newline (one added at the end when it had none); false at
// the end, or for a line over limit (no line of MCP is that long; the agent is cut off).
func readLineRaw(r *bufio.Reader, limit int) ([]byte, bool) {
	var line []byte
	for {
		chunk, err := r.ReadSlice('\n')
		line = append(line, chunk...)
		if len(line) > limit {
			return nil, false
		}
		if err == bufio.ErrBufferFull {
			continue
		}
		if len(line) == 0 {
			return nil, false
		}
		if line[len(line)-1] != '\n' {
			line = append(line, '\n')
		}
		return line, true
	}
}

// MARK: Running cua

// SpacesRun is `exe args`, with no stdin, and what it printed (both pipes, without colour
// codes) when it ends; (-1, why) when it couldn't start or took longer than timeout.
func SpacesRun(exe string, timeout time.Duration, args ...string) (int, string) {
	cmd := Hidden(exe, args...)
	cmd.Env = append(cmd.Env, "CUA_TELEMETRY=0")
	g, err := Spawn(cmd)
	if err != nil {
		return -1, err.Error()
	}
	stdin, stdout, stderr := g.TakePipes()
	stdin.Close()
	read := func(p *os.File) chan string {
		ch := make(chan string, 1)
		go func() {
			b, _ := io.ReadAll(p)
			p.Close()
			ch <- core.Lossy(b)
		}()
		return ch
	}
	o, e := read(stdout), read(stderr)
	code, ok := g.WaitTimeout(timeout)
	if !ok {
		g.Close()
		return -1, "Timed out."
	}
	// Ended on its own: what it started (Lume's VM or daemon) is left running.
	g.Release()
	return code, StripANSI(<-o + "\n" + <-e)
}

// spacesStream is `exe args` with a line at a time of what it prints handed on as a frame;
// ends it (and whatever it started) when ct is cancelled or the time is up. Fails with its
// last line.
func spacesStream(exe string, args []string, timeout time.Duration, ct *Cancel, frame func(Frame)) error {
	cmd := Hidden(exe, args...)
	cmd.Env = append(cmd.Env, "CUA_TELEMETRY=0", "CUA_INSTALL_NONINTERACTIVE=1", "NONINTERACTIVE=1")
	g, err := Spawn(cmd)
	if err != nil {
		return &StepError{Msg: err.Error()}
	}
	stdin, stdout, stderr := g.TakePipes()
	stdin.Close()
	rx := make(chan string, 256)
	var readers sync.WaitGroup
	for _, p := range []*os.File{stdout, stderr} {
		readers.Add(1)
		go func() {
			defer readers.Done()
			defer p.Close()
			r := bufio.NewReader(p)
			for {
				b, err := r.ReadBytes('\n')
				if len(b) > 0 {
					// A carriage return redraws a progress line: each piece is a line.
					for _, piece := range strings.FieldsFunc(core.Lossy(b), func(c rune) bool { return c == '\n' || c == '\r' }) {
						rx <- piece
					}
				}
				if err != nil {
					return
				}
			}
		}()
	}
	go func() { readers.Wait(); close(rx) }()
	tail := ""
	take := func(raw string) {
		if last, f, ok := FrameOf(raw); ok {
			tail = last
			frame(f)
		}
	}
	began := time.Now()
	var code int
	for {
		for drained := false; !drained; {
			select {
			case l, ok := <-rx:
				if ok {
					take(l)
				} else {
					drained = true
				}
			default:
				drained = true
			}
		}
		if ct.IsCancelled() {
			g.Close()
			return stepCancelled
		}
		if c, ok := g.WaitTimeout(50 * time.Millisecond); ok {
			code = c
			break
		}
		if time.Since(began) >= timeout {
			g.Close()
			return &StepError{Msg: "It took too long and was stopped."}
		}
	}
	// It ended on its own: what it started (a VM) is Cua's to keep, as the C# left it.
	g.Release()
	drainUntil(rx, 500*time.Millisecond, take)
	if code != 0 {
		if tail == "" {
			return &StepError{Msg: fmt.Sprintf("exit code %d", code)}
		}
		return &StepError{Msg: tail}
	}
	return nil
}

package voice

import (
	"archive/tar"
	"bufio"
	"compress/gzip"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
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

	"golang.org/x/sys/cpu"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

// Local speech (Phonon): its setup from Settings and the on-demand engine.
//
// Phonon-2 runs in the official `fermion` CLI (Python and CPU PyTorch), so Hover keeps a
// Python of its own for it in <support>/phonon/installs/<id>: python-build-standalone's
// CPython, the pinned wheels (pip from local, hash-checked files only), the model as
// fermion's own verifying unpacker writes it, and the model's NOTICE and licences. Every
// byte comes from a pinned URL with a pinned size and SHA-256. Nothing runs at login: each
// recording starts `fermion transcribe --json`, offline, and it exits when done.
//
// Setup builds a new folder in installs/, checks that it turns the bundled sample into the
// expected words, writes ready.json there, and only then points `current` (a small file
// naming the folder) at it, by an atomic rename of that file. Folders are never moved:
// Windows refuses to rename one while a virus scanner still reads a file in it, and the
// check runs where the install will live. So a failed or cancelled repair leaves the
// working install as it was, and a crash at any point leaves a disk that says what it
// holds; folders `current` doesn't name are swept at the next setup.

// InstallKind is the card's state.
type InstallKind int

const (
	NotInstalled InstallKind = iota
	// Unsupported: why this device can't run it, said before any download.
	Unsupported
	// Downloading: Done of Total (Total 0 when unknown).
	Downloading
	Verifying
	Installing
	Ready
	Cancelled
	// Failed keeps a working older install usable (Speech() still gives it).
	Failed
)

// Install is the card's state and what goes with it.
type Install struct {
	Kind        InstallKind
	Done, Total uint64
	Message     string
}

// Facts is what the card shows; real numbers from the pins.
type Facts struct {
	Model   string
	Version string
	// DownloadBytes is the runtime, wheels, model and its attribution files, for this platform.
	DownloadBytes uint64
	// DiskBytes is installed: the runtime, the model and the plane cache fermion writes beside it.
	DiskBytes uint64
	// PeakDiskBytes is during setup: the downloads and the staged install side by side.
	PeakDiskBytes uint64
	// Folder is Hover's own folder for it (<support>/phonon); the install is in it.
	Folder string
}

const (
	phononID  = "phonon-2"
	revision  = "9c7fef3584499a88fe8d394427f45851bbb8b446"
	fermion   = "0.2.5"
	modelDir  = "model_phonon2_c4c_int6"
	margin    = 300 << 20
	maxAudio  = 605 * time.Second
	sampleTxt = "Open the notes folder and add a list of the open tasks."
)

type pin struct {
	triple string
	size   uint64
	sha    string
}

// pythons: python-build-standalone 20260929, CPython 3.12.14, install_only.
var pythons = []pin{
	{"x86_64-pc-windows-msvc", 46_425_719, "28728baf30b65e263f0b25c5a85be8226e7ab0d212fbadd6a8f0f796139fa804"},
	{"x86_64-unknown-linux-gnu", 66_919_180, "06c90b93f419b63371c18f20fed0558a1a901f6518c3c24f755077e048447e7f"},
	{"aarch64-unknown-linux-gnu", 52_300_852, "9c797cf657f6dced51d3e74eeabd7c1ca742d5bf10f66080a290e424ced8edaf"},
}

//go:embed assets/wheels-x86_64-pc-windows-msvc.txt
var wheelsWin string

//go:embed assets/wheels-x86_64-unknown-linux-gnu.txt
var wheelsLinux string

//go:embed assets/wheels-aarch64-unknown-linux-gnu.txt
var wheelsArm string

//go:embed assets/check.wav
var sampleWav []byte

func wheelsOf(triple string) string {
	switch triple {
	case "x86_64-unknown-linux-gnu":
		return wheelsLinux
	case "aarch64-unknown-linux-gnu":
		return wheelsArm
	}
	return wheelsWin
}

// repoFiles are from FermionResearch/Phonon-2 at the revision: the model archive first, then
// the files that must stay with the weights (CC-BY-4.0) and the code (Apache-2.0).
var repoFiles = []struct {
	name string
	size uint64
	sha  string
}{
	{"phonon-2.bps.tar.zst", 163_515_201, "98125795b6dda72f5c6eee9ba33d19815df65dcb18b50a357bf9f73c9935309e"},
	{"NOTICE", 2_741, "00624a5043e7ce74029317b024132ca5286116d6fbf3191d226374bc8273789f"},
	{"LICENSE-WEIGHTS-CC-BY-4.0.txt", 18_657, "9ba9550ad48438d0836ddab3da480b3b69ffa0aac7b7878b5a0039e7ab429411"},
	{"LICENSE-CODE-Apache-2.0.txt", 11_358, "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"},
}

// modelFiles are what fermion's unpacker writes into modelDir (it checks every member
// itself; this is Hover's check of the result, and what a later start looks for).
var modelFiles = []struct {
	name string
	size uint64
	sha  string
}{
	{"config.json", 277_493, "d0daad3b2a182893844f4abdc11e4f5b7083f7d42c8ad7e8203f71559785a31b"},
	{"model.fermion", 177_438_361, "4b6bfa3a12cc3c4e0a54f2ab3ec4ca7a842b09e5c7ecfc8e7ca0ac6cc8c11468"},
	{"packed_manifest.json", 821, "3c1874501a1efb6ef569eab082aa68d91b93ec6e4d2fb00dbef7851edf2ae510"},
}

// diskSizes: installed size, measured after setup; most of it is PyTorch and the 304 MB
// plane cache fermion writes beside the model on its first run. Linux arm64 is an estimate.
var diskSizes = map[string]uint64{"x86_64-pc-windows-msvc": 1_455_000_000, "x86_64-unknown-linux-gnu": 1_826_000_000, "aarch64-unknown-linux-gnu": 1_780_000_000}

const (
	mainPy   = "import sys; sys.argv[0] = 'fermion'; from fermion.cli import main; sys.exit(main())"
	unpackPy = "import sys; from pathlib import Path; from fermion._speech.fetch import _unpack; print(_unpack(Path(sys.argv[1]), Path(sys.argv[2])))"
	vcMsg    = "Phonon needs the Microsoft Visual C++ Redistributable (x64). Install it from https://aka.ms/vs/17/release/vc_redist.x64.exe, then press Download again."
	damaged  = "Phonon’s files are missing or damaged. Press Repair in Settings → Voice."
)

// dl is one file to fetch: where it goes under downloads/, and its pins.
type dl struct {
	name, url string
	size      uint64
	sha       string
}

type wheel struct {
	name, version string
	file          dl
}

// pins is everything one platform's install is made of.
type pins struct {
	triple string
	python dl
	wheels []wheel
	// repo is the model archive, then the attribution files.
	repo  []dl
	model []dl
	disk  uint64
}

func officialPins(triple string) pins {
	py := pythons[0]
	for _, p := range pythons {
		if p.triple == triple {
			py = p
		}
	}
	name := "cpython-3.12.14+20260929-" + py.triple + "-install_only.tar.gz"
	p := pins{triple: triple, python: dl{name: name, url: "https://github.com/astral-sh/python-build-standalone/releases/download/20260929/" + strings.ReplaceAll(name, "+", "%2B"), size: py.size, sha: py.sha}}
	p.wheels = parseLock(wheelsOf(triple))
	for _, r := range repoFiles {
		p.repo = append(p.repo, dl{name: r.name, url: "https://huggingface.co/FermionResearch/Phonon-2/resolve/" + revision + "/" + r.name, size: r.size, sha: r.sha})
	}
	for _, m := range modelFiles {
		p.model = append(p.model, dl{name: m.name, size: m.size, sha: m.sha})
	}
	p.disk = diskSizes[triple]
	if p.disk == 0 {
		p.disk = diskSizes["x86_64-pc-windows-msvc"]
	}
	return p
}

func (p *pins) downloads() []dl {
	out := []dl{p.python}
	for _, w := range p.wheels {
		out = append(out, w.file)
	}
	return append(out, p.repo...)
}

func (p *pins) downloadBytes() uint64 {
	var n uint64
	for _, d := range p.downloads() {
		n += d.size
	}
	return n
}

// identity changes when any pinned byte does: what ready.json must say for an install to count.
func (p *pins) identity() string {
	h := sha256.New()
	h.Write([]byte(p.triple))
	for _, d := range p.downloads() {
		h.Write([]byte(d.sha))
	}
	for _, m := range p.model {
		h.Write([]byte(m.sha))
	}
	return hex.EncodeToString(h.Sum(nil))
}

func parseLock(text string) []wheel {
	var out []wheel
	for _, l := range strings.Split(text, "\n") {
		l = strings.TrimSpace(l)
		if l == "" || strings.HasPrefix(l, "#") {
			continue
		}
		f := strings.Fields(l)
		if len(f) != 5 {
			continue
		}
		size, err := strconv.ParseUint(f[2], 10, 64)
		if err != nil {
			continue
		}
		file := strings.ReplaceAll(f[4][strings.LastIndex(f[4], "/")+1:], "%2B", "+")
		out = append(out, wheel{f[0], f[1], dl{name: "wheels/" + file, url: f[4], size: size, sha: f[3]}})
	}
	return out
}

func versionLabel() string {
	return fmt.Sprintf("%s @ %s · fermion %s", phononID, revision[:7], fermion)
}

func localModel(home string) core.LocalModel {
	return core.LocalModel{ID: phononID, Version: versionLabel(), Folder: home}
}

// tripleOf is this build's platform, when Phonon has wheels for it.
func tripleOf() string {
	switch runtime.GOOS + "/" + runtime.GOARCH {
	case "windows/amd64":
		return "x86_64-pc-windows-msvc"
	case "linux/amd64":
		return "x86_64-unknown-linux-gnu"
	case "linux/arm64":
		return "aarch64-unknown-linux-gnu"
	}
	return ""
}

// LocalNote is why Local speech is off on this OS, for Settings to show beside its choice:
// none on Windows and Linux.
func LocalNote() string { return "" }

// unsupported is why this device can't run Phonon, known before anything is downloaded.
func unsupported() string {
	if tripleOf() == "" {
		return fmt.Sprintf("Local speech runs on Windows x64 and on Linux (x64 or arm64), not %s %s.", runtime.GOOS, runtime.GOARCH)
	}
	if runtime.GOARCH == "amd64" && !cpu.X86.HasSSE41 {
		return "Phonon needs a processor with SSE4.1."
	}
	return osUnsupported()
}

func freeBytes(dir string) (uint64, bool) {
	d := dir
	for {
		if _, err := os.Stat(d); err == nil {
			break
		}
		p := filepath.Dir(d)
		if p == d {
			return 0, false
		}
		d = p
	}
	return freeOn(d)
}

func hashFile(p string, cancel *atomic.Bool) (string, error) {
	f, err := os.Open(p)
	if err != nil {
		return "", &failure{msg: fmt.Sprintf("%s: %v", p, err)}
	}
	defer f.Close()
	h, buf := sha256.New(), make([]byte, 1<<20)
	for {
		if cancel.Load() {
			return "", errCancelled
		}
		n, err := f.Read(buf)
		h.Write(buf[:n])
		if err == io.EOF {
			return hex.EncodeToString(h.Sum(nil)), nil
		}
		if err != nil {
			return "", &failure{msg: fmt.Sprintf("%s: %v", p, err)}
		}
	}
}

// words are the words, for comparing what was heard with what was said.
func words(s string) []string {
	var out []string
	for _, w := range strings.FieldsFunc(s, func(c rune) bool { return !(unicode.IsLetter(c) || unicode.IsNumber(c)) }) {
		out = append(out, strings.ToLower(w))
	}
	return out
}

var errCancelled = errors.New("cancelled")

type failure struct{ msg string }

func (f *failure) Error() string { return f.msg }

func failf(format string, a ...any) error { return &failure{fmt.Sprintf(format, a...)} }

// Phonon is the setup and the engine.
type Phonon struct {
	settings *core.Settings
	root     string
	pins     pins

	mu    sync.Mutex
	state Install
	// job is the running setup's cancel flag.
	job       *atomic.Bool
	listeners []func()
	// setup is held by a setup goroutine from start to clean-up: a cancelled one may still
	// be deleting its staging when the next starts.
	setup sync.Mutex
	// busy is held while a transcription reads current/, so a promotion or Remove never
	// moves files under it.
	busy  sync.Mutex
	inUse atomic.Int64
	// engine is the running transcription's process, for Shutdown().
	engineMu sync.Mutex
	engine   *agents.Group
}

// NewPhonon reads the disk only.
func NewPhonon(settings *core.Settings) *Phonon {
	t := tripleOf()
	if t == "" {
		t = pythons[0].triple
	}
	p := &Phonon{settings: settings, root: filepath.Join(core.Support(), "phonon"), pins: officialPins(t)}
	state := p.readDisk()
	// Settings say what is installed only when the disk agrees.
	v := settings.Voice()
	var want *core.LocalModel
	if h := p.current(); h != "" && state.Kind == Ready {
		m := localModel(h)
		want = &m
	}
	same := (v.Local == nil && want == nil) || (v.Local != nil && want != nil && *v.Local == *want)
	if (state.Kind == Ready || state.Kind == NotInstalled || state.Kind == Unsupported) && !same {
		v.Local = want
		settings.SetVoice(v)
	}
	p.state = state
	return p
}

// current is the install `current` names, if it names one.
func (p *Phonon) current() string {
	b, err := os.ReadFile(filepath.Join(p.root, "current"))
	if err != nil {
		return ""
	}
	name := strings.TrimSpace(string(b))
	if name == "" {
		return ""
	}
	for _, c := range name {
		if !(c < 128 && (unicode.IsLetter(c) || unicode.IsDigit(c) || c == '-')) {
			return ""
		}
	}
	return filepath.Join(p.root, "installs", name)
}

func (p *Phonon) readDisk() Install {
	switch h := p.current(); {
	case h != "" && p.usable(h):
		return Install{Kind: Ready}
	case h != "":
		return Install{Kind: Failed, Message: damaged}
	}
	if why := unsupported(); why != "" {
		return Install{Kind: Unsupported, Message: why}
	}
	return Install{Kind: NotInstalled}
}

// marker: ready.json says this exact set of pins passed the check.
func (p *Phonon) marker(home string) bool {
	b, err := os.ReadFile(filepath.Join(home, "ready.json"))
	if err != nil {
		return false
	}
	var v struct{ Pins string }
	return json.Unmarshal(b, &v) == nil && v.Pins == p.pins.identity()
}

// usable: the marker, the interpreter and the model's files at their sizes: cheap, so it is
// asked before every transcription.
func (p *Phonon) usable(home string) bool {
	model := filepath.Join(home, "model", modelDir)
	if !p.marker(home) {
		return false
	}
	if st, err := os.Stat(pythonExe(home)); err != nil || !st.Mode().IsRegular() {
		return false
	}
	for _, m := range p.pins.model {
		if st, err := os.Stat(filepath.Join(model, m.name)); err != nil || uint64(st.Size()) != m.size {
			return false
		}
	}
	return true
}

func (p *Phonon) State() Install {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.state
}

func (p *Phonon) Facts() Facts {
	d := p.pins.downloadBytes()
	return Facts{Model: "Phonon-2", Version: versionLabel(), DownloadBytes: d, DiskBytes: p.pins.disk, PeakDiskBytes: d + p.pins.disk, Folder: p.root}
}

func (p *Phonon) OnChange(f func()) {
	p.mu.Lock()
	p.listeners = append(p.listeners, f)
	p.mu.Unlock()
}

func (p *Phonon) notify() {
	p.mu.Lock()
	ls := append([]func(){}, p.listeners...)
	p.mu.Unlock()
	for _, l := range ls {
		l()
	}
}

// Download (and Retry): off the UI goroutine. Nothing happens while a setup runs.
func (p *Phonon) Download() { p.start() }

// Repair is the same full setup; the install there stays usable until the new one passed.
func (p *Phonon) Repair() { p.start() }

func (p *Phonon) start() {
	p.mu.Lock()
	if p.job != nil {
		p.mu.Unlock()
		return
	}
	if why := unsupported(); why != "" {
		p.state = Install{Kind: Unsupported, Message: why}
		p.mu.Unlock()
		p.notify()
		return
	}
	job := new(atomic.Bool)
	p.job = job
	p.state = Install{Kind: Downloading, Total: p.pins.downloadBytes()}
	p.mu.Unlock()
	p.notify()
	go func() {
		p.setup.Lock()
		defer p.setup.Unlock()
		home := filepath.Join(p.root, "installs", core.GUIDN())
		err := p.install(job, home)
		if err != nil {
			os.RemoveAll(home)
		}
		var f *failure
		if errors.As(err, &f) {
			core.Logf("phonon: setup failed - %s", f.msg)
		}
		p.mu.Lock()
		// A cancelled job already said Cancelled, and a newer one may own the card now. A
		// cancel that came too late to stop the switch to the new install says Ready.
		if p.job != job {
			if err == nil && p.job == nil {
				p.state = Install{Kind: Ready}
				p.mu.Unlock()
				p.notify()
				return
			}
			p.mu.Unlock()
			return
		}
		p.job = nil
		switch {
		case err == nil:
			p.state = Install{Kind: Ready}
		case errors.Is(err, errCancelled):
			p.state = Install{Kind: Cancelled}
		default:
			p.state = Install{Kind: Failed, Message: err.Error()}
		}
		p.mu.Unlock()
		p.notify()
	}()
}

// setState sets the state, if this job still owns the card.
func (p *Phonon) setState(job *atomic.Bool, s Install) {
	p.mu.Lock()
	if p.job != job {
		p.mu.Unlock()
		return
	}
	p.state = s
	p.mu.Unlock()
	p.notify()
}

func (p *Phonon) Cancel() {
	p.mu.Lock()
	job := p.job
	if job == nil {
		p.mu.Unlock()
		return
	}
	p.job = nil
	job.Store(true)
	p.state = Install{Kind: Cancelled}
	p.mu.Unlock()
	p.notify()
}

func gb(b uint64) string { return fmt.Sprintf("%.1f GB", float64(b)/1e9) }

// install sets up a new install in staging (a fresh folder under installs/).
func (p *Phonon) install(job *atomic.Bool, staging string) error {
	dlDir := filepath.Join(p.root, "downloads")
	if err := os.MkdirAll(p.root, 0o755); err != nil {
		return failf("%v", err)
	}
	p.sweep()

	// Space for every download and the staged install side by side, before a byte comes.
	need := p.pins.downloadBytes() + p.pins.disk + margin
	if free, ok := freeBytes(p.root); ok && free < need {
		return failf("Phonon needs %s free on this drive during setup; %s is free.", gb(need), gb(free))
	}

	// 1. Downloading. A file a cancelled or failed run finished is kept (Verifying hashes it
	//    again); a partial one is never resumed.
	total := p.pins.downloadBytes()
	var done uint64
	shown := time.Now()
	client := downloadClient()
	for _, f := range p.pins.downloads() {
		dst := filepath.Join(dlDir, filepath.FromSlash(f.name))
		if st, err := os.Stat(dst); err == nil && uint64(st.Size()) == f.size {
			done += f.size
			continue
		}
		err := fetch(client, f, dst, job, func(n uint64) {
			done += n
			if time.Since(shown) >= 100*time.Millisecond {
				shown = time.Now()
				p.setState(job, Install{Kind: Downloading, Done: done, Total: total})
			}
		})
		if err != nil {
			return err
		}
	}
	p.setState(job, Install{Kind: Downloading, Done: total, Total: total})

	// 2. Verifying: every file against its pin, as it is on disk now.
	p.setState(job, Install{Kind: Verifying})
	for _, f := range p.pins.downloads() {
		path := filepath.Join(dlDir, filepath.FromSlash(f.name))
		h, err := hashFile(path, job)
		if err != nil {
			return err
		}
		if h != f.sha {
			os.Remove(path)
			return failf("%s didn’t match its pinned checksum. Press Retry.", f.name)
		}
	}

	// 3. Installing, all in staging.
	p.setState(job, Install{Kind: Installing})
	if err := os.MkdirAll(filepath.Join(staging, "tmp"), 0o755); err != nil {
		return failf("%v", err)
	}
	t := time.Now()
	took := func(what string) {
		core.Logf("phonon: %s took %.1fs", what, time.Since(t).Seconds())
		t = time.Now()
	}
	if err := untarGz(filepath.Join(dlDir, filepath.FromSlash(p.pins.python.name)), staging, job); err != nil {
		return err
	}
	took("unpacking the runtime")
	var reqs strings.Builder
	for _, w := range p.pins.wheels {
		fmt.Fprintf(&reqs, "%s==%s --hash=sha256:%s\n", w.name, w.version, w.file.sha)
	}
	if err := os.WriteFile(filepath.Join(staging, "requirements.txt"), []byte(reqs.String()), 0o644); err != nil {
		return failf("%v", err)
	}
	// --no-compile: pip byte-compiles all of torch and transformers one file at a time (9.5
	// min with Windows' scanner); the check below compiles only what Phonon imports.
	if _, err := p.python(staging, []string{"-I", "-m", "pip", "install", "--no-index", "--no-deps", "--require-hashes", "--only-binary", ":all:",
		"--no-compile", "--no-warn-script-location", "--find-links", filepath.Join(dlDir, "wheels"), "-r", filepath.Join(staging, "requirements.txt")}, job, false); err != nil {
		return err
	}
	took("pip")

	model := filepath.Join(staging, "model", modelDir)
	if _, err := p.python(staging, []string{"-I", "-c", unpackPy, filepath.Join(dlDir, p.pins.repo[0].name), model}, job, false); err != nil {
		return err
	}
	took("unpacking the model")
	for _, m := range p.pins.model {
		path := filepath.Join(model, m.name)
		st, err := os.Stat(path)
		if err != nil || uint64(st.Size()) != m.size {
			return failf("Phonon’s model file %s didn’t match its pin.", m.name)
		}
		h, err := hashFile(path, job)
		if err != nil {
			return err
		}
		if h != m.sha {
			return failf("Phonon’s model file %s didn’t match its pin.", m.name)
		}
	}
	lic := filepath.Join(staging, "licenses")
	if err := os.MkdirAll(lic, 0o755); err != nil {
		return failf("%v", err)
	}
	for _, f := range p.pins.repo[1:] {
		b, err := os.ReadFile(filepath.Join(dlDir, f.name))
		if err == nil {
			err = os.WriteFile(filepath.Join(lic, f.name), b, 0o644)
		}
		if err != nil {
			return failf("%v", err)
		}
	}

	// The check: Ready means the runtime loads the model and hears the sample right. Its
	// first run also writes the plane cache, so the user's first recording is warm.
	check := filepath.Join(staging, "check.wav")
	if err := os.WriteFile(check, sampleWav, 0o644); err != nil {
		return failf("%v", err)
	}
	out, err := p.python(staging, transcribeArgs(staging, check), job, false)
	if err != nil {
		return err
	}
	took("the check")
	heard, _, _, perr := parseAnswer(out)
	if perr != nil {
		return failf("Phonon’s check failed: %s", perr.Message())
	}
	if !slices.Equal(words(heard), words(sampleTxt)) {
		return failf("Phonon’s check heard “%s”, not the sample’s words.", heard)
	}
	marker, _ := json.Marshal(map[string]any{"id": phononID, "version": versionLabel(), "revision": revision, "fermion": fermion, "triple": p.pins.triple,
		"pins": p.pins.identity(), "heard": heard})
	if err := os.WriteFile(filepath.Join(staging, "ready.json"), marker, 0o644); err != nil {
		return failf("%v", err)
	}
	if job.Load() {
		return errCancelled
	}
	if err := p.promote(staging); err != nil {
		return failf("%v", err)
	}
	os.RemoveAll(dlDir)
	v := p.settings.Voice()
	m := localModel(staging)
	v.Local = &m
	p.settings.SetVoice(v)
	return nil
}

// promote points `current` at the checked install, then deletes the one it named before.
func (p *Phonon) promote(home string) error {
	p.busy.Lock()
	defer p.busy.Unlock()
	old, tmp := p.current(), filepath.Join(p.root, "current.tmp")
	if err := os.WriteFile(tmp, []byte(filepath.Base(home)), 0o644); err != nil {
		return fmt.Errorf("Couldn’t switch to the new Phonon (%v).", err)
	}
	if err := os.Rename(tmp, filepath.Join(p.root, "current")); err != nil {
		return fmt.Errorf("Couldn’t switch to the new Phonon (%v).", err)
	}
	if old != "" && old != home {
		os.RemoveAll(old)
	}
	return nil
}

// sweep removes folders under installs/ that `current` doesn't name: an interrupted setup's,
// or an old install a scanner kept from being deleted.
func (p *Phonon) sweep() {
	cur := p.current()
	list, _ := os.ReadDir(filepath.Join(p.root, "installs"))
	for _, e := range list {
		if path := filepath.Join(p.root, "installs", e.Name()); path != cur {
			os.RemoveAll(path)
		}
	}
}

// Remove is an error while a setup runs or a transcription reads the files. It forgets the
// install at once (`current` goes), then deletes Hover's own folders here on a worker.
func (p *Phonon) Remove() error {
	p.mu.Lock()
	if p.job != nil {
		p.mu.Unlock()
		return errors.New("Phonon is being set up. Cancel it first.")
	}
	if !p.setup.TryLock() {
		p.mu.Unlock()
		return errors.New("Phonon’s setup is still stopping. Try again in a moment.")
	}
	if !p.busy.TryLock() || p.inUse.Load() > 0 {
		p.setup.Unlock()
		p.mu.Unlock()
		return errors.New("Phonon is transcribing. Try again when it’s done.")
	}
	p.busy.Unlock()
	if err := os.Remove(filepath.Join(p.root, "current")); err != nil && !os.IsNotExist(err) {
		p.setup.Unlock()
		p.mu.Unlock()
		return fmt.Errorf("Couldn’t remove Phonon (%v).", err)
	}
	if why := unsupported(); why != "" {
		p.state = Install{Kind: Unsupported, Message: why}
	} else {
		p.state = Install{Kind: NotInstalled}
	}
	p.setup.Unlock()
	p.mu.Unlock()
	v := p.settings.Voice()
	if v.Local != nil {
		v.Local = nil
		p.settings.SetVoice(v)
	}
	go func() {
		// A Download pressed meanwhile waits: it would build in the folder being deleted.
		p.setup.Lock()
		defer p.setup.Unlock()
		for _, d := range []string{"installs", "downloads"} {
			os.RemoveAll(filepath.Join(p.root, d))
		}
		os.Remove(filepath.Join(p.root, "current.tmp"))
		os.Remove(p.root)
	}()
	p.notify()
	return nil
}

// Speech is non-nil when an install that passed its check is on disk: Ready, or a failed or
// cancelled repair that left the working one alone.
func (p *Phonon) Speech() Speech {
	if h := p.current(); h == "" || !p.usable(h) {
		return nil
	}
	return localSpeech{p}
}

// Shutdown stops a running transcription's process now.
func (p *Phonon) Shutdown() {
	p.engineMu.Lock()
	g := p.engine
	p.engine = nil
	p.engineMu.Unlock()
	if g != nil {
		g.Kill()
	}
}

type localSpeech struct{ p *Phonon }

func (l localSpeech) Transcribe(wav string, cancel *atomic.Bool) (Transcript, *SpeechError) {
	return l.p.transcribe(wav, cancel)
}

func (p *Phonon) transcribe(wav string, cancel *atomic.Bool) (Transcript, *SpeechError) {
	t0 := time.Now()
	p.inUse.Add(1)
	defer p.inUse.Add(-1)
	p.busy.Lock()
	defer p.busy.Unlock()
	home := p.current()
	if home == "" || !p.usable(home) {
		return Transcript{}, speechErr(ErrNotReady, damaged)
	}
	st, err := os.Stat(wav)
	if err != nil {
		return Transcript{}, speechErr(ErrEngine, fmt.Sprintf("The recording is gone (%v).", err))
	}
	if time.Duration(float64(max(st.Size()-44, 0))/32_000*float64(time.Second)) > maxAudio {
		return Transcript{}, speechErr(ErrUnsupported, "Local speech takes up to ten minutes of audio.")
	}
	out, err := p.python(home, transcribeArgs(home, wav), cancel, true)
	if err != nil {
		var f *failure
		switch {
		case errors.Is(err, errCancelled), cancel.Load():
			return Transcript{}, speechErr(ErrCancelled, "")
		case errors.As(err, &f):
			return Transcript{}, speechErr(ErrEngine, f.msg)
		}
		return Transcript{}, speechErr(ErrEngine, err.Error())
	}
	text, truncated, audio, perr := parseAnswer(out)
	if perr != nil {
		return Transcript{}, perr
	}
	return Transcript{Text: text, Truncated: truncated, Audio: audio, Took: time.Since(t0)}, nil
}

// python runs the managed Python in home, offline, and waits; killed when cancel is set.
// engine: a transcription, which Shutdown() may stop.
func (p *Phonon) python(home string, args []string, cancel *atomic.Bool, engine bool) (string, error) {
	cmd := agents.Hidden(pythonExe(home), args...)
	cmd.Dir = home
	tmp := filepath.Join(home, "tmp")
	os.MkdirAll(tmp, 0o755)
	drop := map[string]bool{"PYTHONPATH": true, "PYTHONHOME": true, "PYTHONSTARTUP": true, "PIP_INDEX_URL": true, "PIP_EXTRA_INDEX_URL": true, "PIP_FIND_LINKS": true}
	var env []string
	for _, e := range os.Environ() {
		k, _, _ := strings.Cut(e, "=")
		if !drop[strings.ToUpper(k)] {
			env = append(env, e)
		}
	}
	cmd.Env = append(env, "FERMION_CACHE_DIR="+filepath.Join(home, "cache"), "HF_HOME="+filepath.Join(home, "hf"), "HF_HUB_OFFLINE=1", "TRANSFORMERS_OFFLINE=1",
		"HF_HUB_DISABLE_TELEMETRY=1", "DO_NOT_TRACK=1", "FERMION_QUIET_DEPRECATIONS=1", "PIP_NO_CACHE_DIR=1", "PIP_DISABLE_PIP_VERSION_CHECK=1", "PIP_NO_INPUT=1",
		"PIP_CONFIG_FILE="+nullFile, "PYTHONNOUSERSITE=1", "PYTHONUTF8=1", "TMP="+tmp, "TEMP="+tmp, "TMPDIR="+tmp)
	g, err := agents.Spawn(cmd)
	if err != nil {
		return "", failf("Phonon’s Python didn’t start (%v). %s", err, damaged)
	}
	_, so, se := g.TakePipes()
	outc, errc := make(chan string, 1), make(chan string, 1)
	go func() {
		var b []byte
		if so != nil {
			b, _ = io.ReadAll(so)
		}
		outc <- string(b)
	}()
	go func() {
		var b []byte
		if se != nil {
			b, _ = io.ReadAll(se)
		}
		errc <- string(b[max(len(b)-4096, 0):])
	}()
	if engine {
		p.engineMu.Lock()
		p.engine = g
		p.engineMu.Unlock()
	}
	var code int
	cancelled := false
	for {
		if cancel.Load() {
			g.Kill()
			cancelled = true
			break
		}
		if c, done := g.WaitTimeout(100 * time.Millisecond); done {
			code = c
			break
		}
	}
	if engine {
		p.engineMu.Lock()
		if p.engine == g {
			p.engine = nil
		}
		p.engineMu.Unlock()
	}
	out, stderr := <-outc, <-errc
	g.Close()
	switch {
	case cancelled:
		return "", errCancelled
	case code == 0:
		return out, nil
	}
	return "", &failure{explain(code, stderr)}
}

func transcribeArgs(home, wav string) []string {
	return []string{"-I", "-c", mainPy, "transcribe", "--json", filepath.Join(home, "model", modelDir), wav}
}

// explain is what a failed run said, in a line.
func explain(code int, err string) string {
	if strings.Contains(err, "WinError 126") || strings.Contains(err, "msvcp140") {
		return vcMsg
	}
	last := ""
	lines := strings.Split(err, "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		if l := strings.TrimSpace(lines[i]); l != "" {
			last = l
			break
		}
	}
	if r := []rune(last); len(r) > 300 {
		last = string(r[:300])
	}
	if last == "" {
		return fmt.Sprintf("Phonon stopped (exit %d).", code)
	}
	return fmt.Sprintf("Phonon stopped (exit %d): %s", code, last)
}

// parseAnswer reads `fermion transcribe --json`'s answer: text, truncated, the audio's length.
func parseAnswer(out string) (string, bool, time.Duration, *SpeechError) {
	lines := strings.Split(out, "\n")
	line := ""
	for i := len(lines) - 1; i >= 0; i-- {
		if strings.HasPrefix(strings.TrimLeft(lines[i], " \t"), "{") {
			line = lines[i]
			break
		}
	}
	if line == "" {
		return "", false, 0, speechErr(ErrEngine, "Phonon gave no answer.")
	}
	var v struct {
		Text      *string
		Truncated bool
		Duration  *float64 `json:"duration_seconds"`
	}
	if err := json.Unmarshal([]byte(line), &v); err != nil {
		return "", false, 0, speechErr(ErrEngine, fmt.Sprintf("Phonon’s answer wasn’t readable (%v).", err))
	}
	if v.Text == nil {
		return "", false, 0, speechErr(ErrEngine, "Phonon’s answer had no text.")
	}
	text := strings.TrimSpace(*v.Text)
	if text == "" {
		return "", false, 0, speechErr(ErrEngine, "Phonon heard no words.")
	}
	var d time.Duration
	if v.Duration != nil && *v.Duration >= 0 {
		d = time.Duration(*v.Duration * float64(time.Second))
	}
	return text, v.Truncated, d, nil
}

func downloadClient() *http.Client {
	// No overall limit: a 200 MB wheel on a slow line is fine. A body that stalls for an
	// hour is not.
	tr := http.DefaultTransport.(*http.Transport).Clone()
	tr.ResponseHeaderTimeout = 60 * time.Second
	return &http.Client{Transport: tr}
}

// fetch downloads one pinned file into dst, hashed as it comes; seen gets each chunk's size.
func fetch(client *http.Client, f dl, dst string, cancel *atomic.Bool, seen func(uint64)) error {
	part := dst + ".part"
	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return failf("%v", err)
	}
	err := func() error {
		req, _ := http.NewRequest("GET", f.url, nil)
		req.Header.Set("User-Agent", "Hover")
		resp, err := client.Do(req)
		if err != nil {
			return failf("Couldn’t download %s: %v", f.name, err)
		}
		defer resp.Body.Close()
		if resp.StatusCode >= 400 {
			return failf("Couldn’t download %s: status %d", f.name, resp.StatusCode)
		}
		fh, err := os.Create(part)
		if err != nil {
			return failf("%v", err)
		}
		defer fh.Close()
		out := bufio.NewWriter(fh)
		h, buf := sha256.New(), make([]byte, 256<<10)
		var n uint64
		for {
			if cancel.Load() {
				return errCancelled
			}
			k, rerr := resp.Body.Read(buf)
			if k > 0 {
				n += uint64(k)
				if n > f.size {
					return failf("%s is larger than its pin.", f.name)
				}
				h.Write(buf[:k])
				if _, err := out.Write(buf[:k]); err != nil {
					return failf("%v", err)
				}
				seen(uint64(k))
			}
			if rerr == io.EOF {
				break
			}
			if rerr != nil {
				return failf("Couldn’t download %s: %v", f.name, rerr)
			}
		}
		if err := out.Flush(); err != nil {
			return failf("%v", err)
		}
		if n != f.size || hex.EncodeToString(h.Sum(nil)) != f.sha {
			return failf("%s didn’t match its pinned checksum. Press Retry.", f.name)
		}
		return nil
	}()
	if err != nil {
		os.Remove(part)
		return err
	}
	if err := os.Rename(part, dst); err != nil {
		return failf("%v", err)
	}
	return nil
}

// untarGz unpacks the runtime's .tar.gz into `into`; an entry that would land outside it
// fails setup.
func untarGz(file, into string, cancel *atomic.Bool) error {
	fh, err := os.Open(file)
	if err != nil {
		return failf("%v", err)
	}
	defer fh.Close()
	gz, err := gzip.NewReader(bufio.NewReader(fh))
	if err != nil {
		return failf("%v", err)
	}
	tr := tar.NewReader(gz)
	root := filepath.Clean(into) + string(os.PathSeparator)
	for {
		if cancel.Load() {
			return errCancelled
		}
		hd, err := tr.Next()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return failf("%v", err)
		}
		dst := filepath.Join(into, filepath.FromSlash(hd.Name))
		if !strings.HasPrefix(dst+string(os.PathSeparator), root) {
			return failf("Phonon’s runtime archive has a path outside its folder.")
		}
		switch hd.Typeflag {
		case tar.TypeDir:
			err = os.MkdirAll(dst, 0o755)
		case tar.TypeReg:
			if err = os.MkdirAll(filepath.Dir(dst), 0o755); err == nil {
				var out *os.File
				if out, err = os.OpenFile(dst, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, os.FileMode(hd.Mode)&0o777|0o200); err == nil {
					_, err = io.Copy(out, tr)
					if cerr := out.Close(); err == nil {
						err = cerr
					}
				}
			}
		case tar.TypeSymlink:
			// Python's own links (python3 → python3.12) stay inside the folder.
			target := hd.Linkname
			if !filepath.IsAbs(target) && strings.HasPrefix(filepath.Join(filepath.Dir(dst), target)+string(os.PathSeparator), root) {
				if err = os.MkdirAll(filepath.Dir(dst), 0o755); err == nil {
					os.Remove(dst)
					err = os.Symlink(target, dst)
				}
			}
		case tar.TypeLink:
			src := filepath.Join(into, filepath.FromSlash(hd.Linkname))
			if strings.HasPrefix(src+string(os.PathSeparator), root) {
				if err = os.MkdirAll(filepath.Dir(dst), 0o755); err == nil {
					os.Remove(dst)
					err = os.Link(src, dst)
				}
			}
		}
		if err != nil {
			return failf("%v", err)
		}
	}
}

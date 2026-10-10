package core

// projects.rs: the places voice may start work: the projects the user registered (each a
// folder, the words that name it, whether voice may use it, and its own tool access), the
// default workspace for everything else, and the voice settings themselves. Kept in
// settings.json beside the 2.x keys; 2.x ignores keys it doesn't know.

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
)

// AccessIDs are the ids AgentOptions.WithAccess takes. A new target asks first: being
// registered never grants full access on its own.
var AccessIDs = []string{"full", "risky", "always", "read"}

const NewTargetAccess = "risky"

func isAccessID(a string) bool {
	for _, x := range AccessIDs {
		if x == a {
			return true
		}
	}
	return false
}

func accessOrDefault(v *string) string {
	if v != nil && isAccessID(*v) {
		return *v
	}
	return NewTargetAccess
}

// Project is a registered project.
type Project struct {
	// ID is stable, made once (a GUID's hex); the name and folder can change.
	ID, Name, Folder string
	Aliases          []string
	// Voice: voice may start tasks here.
	Voice  bool
	Access string
}

func NewProject(name, folder string) Project {
	return Project{ID: GUIDN(), Name: strings.TrimSpace(name), Folder: folder, Aliases: []string{}, Voice: true, Access: NewTargetAccess}
}

func (p Project) ToJSON() JSON {
	aliases := make([]JSON, len(p.Aliases))
	for i, a := range p.Aliases {
		aliases[i] = JStr(a)
	}
	return JObj(P("Id", JStr(p.ID)), P("Name", JStr(p.Name)), P("Folder", JStr(p.Folder)), P("Aliases", JArr(aliases...)),
		P("Voice", JBool(p.Voice)), P("Access", JStr(p.Access)))
}

func ProjectFromJSON(v JSON) (Project, error) {
	r := readObj(v)
	p := Project{ID: r.text("Id"), Name: r.text("Name"), Folder: r.text("Folder"), Aliases: []string{}}
	if p.ID == "" {
		p.ID = GUIDN()
	}
	for _, a := range listOf(r, "Aliases", itemText) {
		if a = strings.TrimSpace(a); a != "" {
			p.Aliases = append(p.Aliases, a)
		}
	}
	p.Voice = r.boolOr("Voice", true)
	p.Access = accessOrDefault(r.optText("Access"))
	return p, r.err
}

// Workspace is where a voice task goes when no project is clearly named. A nil Folder is
// the user's home plus "Hover", found when it is needed.
type Workspace struct {
	Folder *string
	Access string
}

func DefaultWorkspaceSetting() Workspace { return Workspace{Access: NewTargetAccess} }

// Path is the folder, the configured one or home + Hover; "" only with no home at all.
func (w Workspace) Path() string {
	if w.Folder != nil && strings.TrimSpace(*w.Folder) != "" {
		return *w.Folder
	}
	return DefaultWorkspacePath()
}

func (w Workspace) ToJSON() JSON {
	return JObj(P("Folder", optStr(w.Folder)), P("Access", JStr(w.Access)))
}

func WorkspaceFromJSON(v JSON) (Workspace, error) {
	r := readObj(v)
	w := Workspace{Folder: r.optText("Folder"), Access: accessOrDefault(r.optText("Access"))}
	if w.Folder != nil && strings.TrimSpace(*w.Folder) == "" {
		w.Folder = nil
	}
	return w, r.err
}

// DefaultWorkspacePath is the user's home plus Hover: C:\Users\<name>\Hover, ~/Hover.
func DefaultWorkspacePath() string {
	if h := Home(); h != "" {
		return filepath.Join(h, "Hover")
	}
	return ""
}

// CleanupProvider is which service tidies a transcript, when cleanup is on.
type CleanupProvider int

const (
	CleanupGemini CleanupProvider = iota
	CleanupOpenAI
	CleanupCustom
)

var cleanupNames = []string{"Gemini", "OpenAI", "Custom"}

func (c CleanupProvider) Name() string { return cleanupNames[c] }

// Base is the OpenAI-compatible base each preset uses (Gemini's from its compatibility
// docs). Custom has the user's own.
func (c CleanupProvider) Base() string {
	switch c {
	case CleanupGemini:
		return "https://generativelanguage.googleapis.com/v1beta/openai"
	case CleanupOpenAI:
		return "https://api.openai.com/v1"
	}
	return ""
}

// Secret is the secret store's name for its key.
func (c CleanupProvider) Secret() string {
	return []string{"cleanup.gemini", "cleanup.openai", "cleanup.custom"}[c]
}

// TranscribeModels are Groq's transcription models (console.groq.com/docs/speech-to-text,
// checked 2026-10-01). Both detect the language when none is given.
var TranscribeModels = []struct{ ID, Name string }{
	{"whisper-large-v3-turbo", "Whisper Large V3 Turbo"}, {"whisper-large-v3", "Whisper Large V3"},
}

const GroqSecret = "voice.groq"

// SpeechMode is where speech becomes text. A file without the key reads as Cloud, so
// nothing is downloaded or changed on its own.
type SpeechMode int

const (
	SpeechLocal SpeechMode = iota
	SpeechCloud
)

var speechNames = []string{"Local", "Cloud"}

func (m SpeechMode) ID() string    { return speechNames[m] }
func (m SpeechMode) Label() string { return []string{"Local (Phonon)", "Cloud (Groq)"}[m] }

// LocalModel is the local model that passed its check: what it is, which pinned release,
// and where it was put. Written only once it is Ready.
type LocalModel struct{ ID, Version, Folder string }

func (l LocalModel) ToJSON() JSON {
	return JObj(P("Id", JStr(l.ID)), P("Version", JStr(l.Version)), P("Folder", JStr(l.Folder)))
}

func LocalModelFromJSON(v JSON) (LocalModel, error) {
	r := readObj(v)
	return LocalModel{r.text("Id"), r.text("Version"), r.text("Folder")}, r.err
}

// VoiceSettings: voice is off until switched on.
type VoiceSettings struct {
	Enabled  bool
	Shortcut Shortcut
	// Microphone is the input device's name; nil is the system's default.
	Microphone *string
	Speech     SpeechMode
	Local      *LocalModel
	// Model is Groq's transcription model (Cloud).
	Model           string
	Cleanup         bool
	CleanupProvider CleanupProvider
	CleanupModel    *string
	// CleanupBase is Custom's base URL (…/v1).
	CleanupBase *string
	// Agent is the agent voice starts tasks with; nil follows the new-task box's tool.
	Agent *AgentTool
	// Countdown is the seconds the preview counts down before it starts the task; 0
	// waits for Start.
	Countdown uint32
	// AuraColor is the listening card's aura, as #RRGGBB; nil is VoiceAuraColor.
	AuraColor *string
	// Hold: hold the shortcut to talk and let go to finish. Off (the default) is a
	// toggle: one press starts listening, the next ends it.
	Hold bool
}

var (
	// VoiceShortcut is Ctrl+Alt+Space, the mockup's.
	VoiceShortcut = Shortcut{Key: 18, Modifiers: ModControl | ModAlt}
	// VoiceCountdowns are the countdowns Settings offers (0 is Off).
	VoiceCountdowns = []uint32{0, 3, 5, 10}
	// VoiceAuraColors are the colours Settings offers; any other is typed in.
	VoiceAuraColors = []struct{ Name, Hex string }{
		{"Cyan", "#1FD5F9"}, {"Violet", "#C4A2FF"}, {"Green", "#4ADE80"}, {"Amber", "#FFB340"}, {"Pink", "#FF6BD5"}, {"White", "#F6F2FF"},
	}
)

const (
	VoiceCountdown = 5
	// VoiceAuraColor is the aura's colour unless one is picked (LiveKit's Aura's own).
	VoiceAuraColor = "#1FD5F9"
)

func DefaultVoiceSettings() VoiceSettings {
	return VoiceSettings{Shortcut: VoiceShortcut, Speech: SpeechCloud, Model: TranscribeModels[0].ID, CleanupProvider: CleanupGemini,
		Countdown: VoiceCountdown}
}

// Aura is the aura's colour now, as #RRGGBB.
func (v VoiceSettings) Aura() string {
	if v.AuraColor != nil {
		return *v.AuraColor
	}
	return VoiceAuraColor
}

// HexColor is #RRGGBB from "#rgb", "rrggbb" and the like (upper case); false if it isn't
// one.
func HexColor(s string) (string, bool) {
	h := strings.TrimLeft(strings.TrimSpace(s), "#")
	for _, c := range h {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F') {
			return "", false
		}
	}
	switch len(h) {
	case 3:
		h = string([]byte{h[0], h[0], h[1], h[1], h[2], h[2]})
	case 6:
	default:
		return "", false
	}
	return "#" + strings.ToUpper(h), true
}

// RGB is the colour's red, green and blue.
func RGB(hex string) [3]uint8 {
	v := uint64(0x1FD5F9)
	if h, ok := HexColor(hex); ok {
		if n, err := strconv.ParseUint(h[1:], 16, 32); err == nil {
			v = n
		}
	}
	return [3]uint8{uint8(v >> 16), uint8(v >> 8), uint8(v)}
}

func (v VoiceSettings) ToJSON() JSON {
	local := JNull
	if v.Local != nil {
		local = v.Local.ToJSON()
	}
	var agent *string
	if v.Agent != nil {
		agent = ptr(v.Agent.ID())
	}
	return JObj(P("Enabled", JBool(v.Enabled)), P("Shortcut", v.Shortcut.ToJSON()), P("Microphone", optStr(v.Microphone)),
		P("Speech", JStr(v.Speech.ID())), P("Local", local), P("Model", JStr(v.Model)), P("Cleanup", JBool(v.Cleanup)),
		P("CleanupProvider", JStr(v.CleanupProvider.Name())), P("CleanupModel", optStr(v.CleanupModel)), P("CleanupBase", optStr(v.CleanupBase)),
		P("Agent", optStr(agent)), P("Countdown", JInt(int64(v.Countdown))), P("AuraColor", optStr(v.AuraColor)), P("HoldToTalk", JBool(v.Hold)))
}

// trimmedText is an optional string, trimmed, with blank as none.
func trimmedText(s *string) *string {
	if s == nil {
		return nil
	}
	t := strings.TrimSpace(*s)
	if t == "" {
		return nil
	}
	return &t
}

func VoiceSettingsFromJSON(v JSON) (VoiceSettings, error) {
	r := readObj(v)
	d := DefaultVoiceSettings()
	out := d
	if m := r.optText("Model"); m != nil {
		for _, t := range TranscribeModels {
			if t.ID == *m {
				out.Model = *m
			}
		}
	}
	out.Enabled = r.boolOr("Enabled", false)
	if x, ok := r.some("Shortcut"); ok && r.err == nil {
		s, err := ShortcutFromJSON(x)
		r.fail(err)
		out.Shortcut = s
	}
	if m := r.optText("Microphone"); m != nil && *m != "" {
		out.Microphone = m
	}
	if x, ok := r.some("Speech"); ok && r.err == nil {
		i, found, err := x.EnumOf(speechNames)
		if err == nil && !found {
			err = errors.New("not a SpeechMode")
		}
		r.fail(err)
		out.Speech = SpeechMode(i)
	}
	if x, ok := r.some("Local"); ok && r.err == nil {
		l, err := LocalModelFromJSON(x)
		r.fail(err)
		out.Local = &l
	}
	out.Cleanup = r.boolOr("Cleanup", false)
	if x, ok := r.some("CleanupProvider"); ok && r.err == nil {
		i, found, err := x.EnumOf(cleanupNames)
		if err == nil && !found {
			err = errors.New("not a CleanupProvider")
		}
		r.fail(err)
		out.CleanupProvider = CleanupProvider(i)
	}
	out.CleanupModel = trimmedText(r.optText("CleanupModel"))
	out.CleanupBase = trimmedText(r.optText("CleanupBase"))
	// An id this build doesn't know follows the new-task tool.
	if t, ok := ParseTool(r.optText("Agent")); ok {
		out.Agent = &t
	}
	// Settings before this had none (3 s, fixed): the new default. A minute at most.
	if x, ok := r.get("Countdown"); ok && x.Kind() == NumKind {
		if n, err := x.I64(); err == nil && n >= 0 && n <= 60 {
			out.Countdown = uint32(n)
		}
	}
	// One that isn't a colour is the default (and a value of the wrong kind is no error).
	if x, ok := r.get("AuraColor"); ok {
		if s, err := x.OptStr(); err == nil && s != nil {
			if h, ok := HexColor(*s); ok {
				out.AuraColor = &h
			}
		}
	}
	// Settings before this were hold to talk; the toggle is the new default for everyone.
	out.Hold = r.boolOr("HoldToTalk", false)
	return out, r.err
}

// ResolveFolder is a folder as the system resolves it: absolute, links followed, and on
// Windows without the \\?\ prefix. An error says why it can't be used.
func ResolveFolder(p string) (string, error) {
	t := strings.TrimSpace(p)
	if t == "" {
		return "", errors.New("No folder is set.")
	}
	if !filepath.IsAbs(t) {
		return "", fmt.Errorf("%s isn’t a full path.", t)
	}
	c, err := filepath.EvalSymlinks(t)
	if err == nil {
		c, err = filepath.Abs(c)
	}
	switch {
	case errors.Is(err, fs.ErrNotExist):
		return "", fmt.Errorf("%s isn’t there any more.", t)
	case errors.Is(err, fs.ErrPermission):
		return "", fmt.Errorf("Hover isn’t allowed to open %s.", t)
	case err != nil:
		return "", fmt.Errorf("%s can’t be opened: %v", t, err)
	}
	if !isDir(c) {
		return "", fmt.Errorf("%s isn’t a folder.", t)
	}
	if _, err := os.ReadDir(c); err != nil {
		return "", fmt.Errorf("Hover can’t read %s: %v", t, err)
	}
	return plainPath(c), nil
}

// plainPath is \\?\C:\x as C:\x and \\?\UNC\h\s as \\h\s, which every tool takes.
func plainPath(p string) string {
	if runtime.GOOS != "windows" {
		return p
	}
	if r, ok := strings.CutPrefix(p, `\\?\UNC\`); ok {
		return `\\` + r
	}
	if r, ok := strings.CutPrefix(p, `\\?\`); ok {
		return r
	}
	return p
}

// SameFolder: their resolved paths are equal, any case on Windows (where the file system
// ignores it), exactly elsewhere. A folder that can't be resolved is compared by its text.
func SameFolder(a, b string) bool {
	r := func(p string) string {
		if x, err := ResolveFolder(p); err == nil {
			return x
		}
		return strings.TrimRight(strings.TrimSpace(p), `\/`)
	}
	x, y := r(a), r(b)
	if runtime.GOOS == "windows" {
		return strings.ToLower(x) == strings.ToLower(y)
	}
	return x == y || sameInode(x, y)
}

// sameInode: on a Mac the default volume ignores case, so two spellings count as one when
// they are the same folder on disk. os.SameFile is device and inode there.
func sameInode(x, y string) bool {
	if runtime.GOOS != "darwin" {
		return false
	}
	a, err1 := os.Stat(x)
	b, err2 := os.Stat(y)
	return err1 == nil && err2 == nil && os.SameFile(a, b)
}

// EnsureFolder makes the default workspace when first needed (never a repository, only
// the folder).
func EnsureFolder(p string) (string, error) {
	if _, err := os.Stat(p); err != nil {
		if err := os.MkdirAll(p, 0o755); err != nil {
			return "", fmt.Errorf("Hover couldn’t make %s: %v", p, err)
		}
	}
	return ResolveFolder(p)
}

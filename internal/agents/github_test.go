package agents

// tests/github.rs: the GitHub CLI's setup against a stand-in gh, and the sign-in, the
// cancel and the install (BrowserAndGitHubTests, ported). Create pull request's tests are
// with the desk's.

import (
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"
)

var signedInLines = []string{"out=github.com", "out=  ✓ Logged in to github.com account octocat (keyring)", "out=  - Active account: true"}

func signedIn(f *fake) {
	f.script("version", "out=gh version 2.102.0 (2026-09-30)", "out=https://github.com/cli/cli/releases/tag/v2.102.0")
	f.script("auth_status", signedInLines...)
}

// MARK: Parsing (BrowserAndGitHubTests.Gh_output_gives_the_device_code_and_the_account)

func TestGhOutputGivesTheDeviceCodeAndTheAccount(t *testing.T) {
	if deref(ParseCode("! First copy your one-time code: 1A2B-3C4D")) != "1A2B-3C4D" {
		t.Error("code")
	}
	if ParseCode("Open this URL to continue in your web browser: https://github.com/login/device") != nil {
		t.Error("no code")
	}
	// A code-shaped word on a line that isn't about the code is not the code.
	if ParseCode("Updating ABCD-1234 in the keyring") != nil {
		t.Error("not about the code")
	}
	if deref(ParseUser("github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  - Active account: true")) != "octocat" {
		t.Error("octocat")
	}
	if deref(ParseUser("✓ Logged in to github.com as mona-lisa (oauth_token)")) != "mona-lisa" {
		t.Error("mona-lisa")
	}
	if ParseUser("You are not logged into any GitHub hosts.") != nil {
		t.Error("nobody")
	}
}

func TestLinuxIsToldTheLineForItsPackageManagerAndNeverRunsIt(t *testing.T) {
	for in, want := range map[string]string{
		"NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n": "sudo apt install gh",
		"ID=linuxmint\nID_LIKE=\"ubuntu debian\"":      "sudo apt install gh",
		"ID=fedora\n": "sudo dnf install gh",
		"ID=rocky\nID_LIKE=\"rhel centos fedora\"": "sudo dnf install gh",
		"ID=arch\n": "sudo pacman -S github-cli",
		"ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"": "sudo zypper install gh",
		"ID=alpine\n": "sudo apk add github-cli",
		"ID=plan9\n":  "",
		"":            "",
	} {
		if got := LinuxInstallLine(in); got != want {
			t.Errorf("%q: %q", in, got)
		}
	}
}

func TestThePlacesGhIsLookedForFitTheSystem(t *testing.T) {
	places := KnownPlaces()
	if len(places) == 0 {
		t.Fatal("none")
	}
	for _, p := range places {
		if !filepath.IsAbs(p) {
			t.Errorf("%s", p)
		}
	}
	switch runtime.GOOS {
	case "windows":
		for _, p := range places {
			if filepath.Base(p) != "gh.exe" {
				t.Error(p)
			}
		}
	case "darwin":
		// A Mac app started from the Dock has no Homebrew on its PATH.
		if !slices.ContainsFunc(places, func(p string) bool { return strings.HasPrefix(p, "/opt/homebrew/bin") }) {
			t.Error(places)
		}
	default:
		if !slices.ContainsFunc(places, func(p string) bool { return strings.HasPrefix(p, "/usr/bin") }) {
			t.Error(places)
		}
	}
	// One click installs where the system has winget (Windows) or Homebrew (a Mac) and
	// says what to do elsewhere.
	cli := NewGitHubCli()
	switch plan := cli.InstallPlan(); {
	case plan.Manual != nil:
		if !strings.Contains(*plan.Manual, "cli.github.com") || cli.CanInstall() || deref(cli.InstallHint()) != *plan.Manual {
			t.Error(*plan.Manual)
		}
	case plan.Winget != "":
		if runtime.GOOS != "windows" || !cli.CanInstall() || cli.InstallHint() != nil {
			t.Error("winget")
		}
	case plan.Homebrew != "":
		if runtime.GOOS != "darwin" || !cli.CanInstall() {
			t.Error("brew")
		}
	}
	if runtime.GOOS == "linux" && cli.InstallPlan().Manual == nil {
		t.Error("Hover never installs with sudo")
	}
}

// MARK: Status

func TestTheStatusComesFromGhAndIsKeptAMinute(t *testing.T) {
	f := newFake(t)
	signedIn(f)
	cli := f.cli()
	s := cli.Check(false)
	if !s.Installed || !s.SignedIn || deref(s.User) != "octocat" || deref(s.Version) != "2.102.0" || s.Hint != "" {
		t.Fatalf("%+v", s)
	}
	if k, ok := cli.Known(); !ok || !reflect.DeepEqual(k, s) {
		t.Error("known")
	}
	calls := len(f.calls())
	// Signed out meanwhile: the kept answer stands until it is asked fresh.
	f.script("auth_status", "err=You are not logged into any GitHub hosts. To log in, run: gh auth login", "exit=1")
	if !reflect.DeepEqual(cli.Check(false), s) || len(f.calls()) != calls {
		t.Error("no new call")
	}
	fresh := cli.Check(true)
	if !fresh.Installed || fresh.SignedIn || fresh.User != nil || fresh.Hint != "Sign in to GitHub to see and open pull requests." || deref(fresh.Version) != "2.102.0" {
		t.Errorf("%+v", fresh)
	}
	if !slices.ContainsFunc(f.calls(), func(c []string) bool { return slices.Equal(c, []string{"auth", "status", "--hostname", "github.com"}) }) {
		t.Error(f.calls())
	}
}

func TestNoGhIsNotInstalled(t *testing.T) {
	d := newDir(t, "nogh")
	cli := NewGitHubCli().With(sp(filepath.Join(d, "gh-is-not-here")), nil)
	s := cli.Check(true)
	if s.Installed || s.SignedIn || s.Hint != "Install the GitHub CLI to see and open pull requests." || cli.Exe() != "" {
		t.Errorf("%+v", s)
	}
}

// MARK: Sign in

func waitIdle(t *testing.T, cli *GitHubCli) {
	waitFor(t, "the setup to end", func() bool { return !cli.Busy() })
}

func countLogins(f *fake) int {
	n := 0
	for _, c := range f.calls() {
		if len(c) > 1 && c[1] == "login" {
			n++
		}
	}
	return n
}

func TestSignInShowsTheCodePressesEnterAndSetsGitUp(t *testing.T) {
	f := newFake(t)
	f.script("version", "out=gh version 2.102.0 (2026-09-30)")
	f.script("auth_status", "err=You are not logged into any GitHub hosts.", "exit=1")
	// gh prints the code and waits for Enter (and the user); afterwards the account is signed in.
	f.script("auth_login",
		"sleep=300", "err=! First copy your one-time code: 1A2B-3C4D", "err=Press Enter to open github.com in your browser...", "sleep=1000", "read",
		"out=✓ Authentication complete.",
		`write=script/auth_status.txt|out=✓ Logged in to github.com account octocat (keyring)\nexit=0`)
	f.script("auth_setup-git", "sleep=300")
	cli := f.cli()
	var mu sync.Mutex
	var seen []GhProgress
	cli.OnChanged(func() { p := cli.Setup(); mu.Lock(); seen = append(seen, p); mu.Unlock() })
	if !cli.Start() {
		t.Fatal("didn't start")
	}
	waitFor(t, "the code", func() bool { return cli.Setup().Code != nil })
	code := cli.Setup()
	if code.Step != SigningIn || deref(code.Code) != "1A2B-3C4D" || deref(code.URL) != DeviceURL || code.Error != nil {
		t.Errorf("%+v", code)
	}
	if code.Line != "Enter the code at github.com/login/device, then come back." || SigningIn.Name() != "signing-in" {
		t.Error(code.Line)
	}
	waitIdle(t, cli)
	if !reflect.DeepEqual(cli.Setup(), GhProgress{}) {
		t.Errorf("finished: nothing left to show: %+v", cli.Setup())
	}
	if k, _ := cli.Known(); !k.SignedIn || deref(k.User) != "octocat" {
		t.Errorf("%+v", k)
	}
	calls := f.calls()
	has := func(want ...string) bool {
		return slices.ContainsFunc(calls, func(c []string) bool { return slices.Equal(c, want) })
	}
	if !has("auth", "login", "--hostname", "github.com", "--git-protocol", "https", "--web") || !has("auth", "setup-git", "--hostname", "github.com") {
		t.Error(calls)
	}
	mu.Lock()
	var lines []string
	for _, p := range seen {
		lines = append(lines, p.Line)
	}
	mu.Unlock()
	if !slices.ContainsFunc(lines, func(l string) bool { return strings.Contains(l, "Asking GitHub for a sign-in code") }) ||
		!slices.ContainsFunc(lines, func(l string) bool { return strings.Contains(l, "Setting up git to use your GitHub sign-in") }) {
		t.Error(lines)
	}
	// Already signed in: nothing more to do, and nothing is started.
	if !cli.Start() {
		t.Fatal("didn't start")
	}
	waitIdle(t, cli)
	if countLogins(f) != 1 {
		t.Error(countLogins(f))
	}
}

func TestASignInThatFailsSaysSoAndALaterClickCanTryAgain(t *testing.T) {
	f := newFake(t)
	f.script("version", "out=gh version 2.102.0")
	f.script("auth_status", "exit=1")
	f.script("auth_login", "err=! First copy your one-time code: AAAA-1111", "err=failed to authenticate", "exit=1")
	cli := f.cli()
	cli.RunSetup()
	p := cli.Setup()
	if deref(p.Error) != "GitHub sign-in didn’t finish." || p.Step != 0 || p.Code != nil || cli.Busy() {
		t.Errorf("%+v", p)
	}
	cli.RunSetup()
	if countLogins(f) != 2 {
		t.Error(countLogins(f))
	}
}

func TestCancelEndsTheSignInAndTheGhItStarted(t *testing.T) {
	f := newFake(t)
	f.script("version", "out=gh version 2.102.0")
	f.script("auth_status", "exit=1")
	f.script("auth_login", "err=! First copy your one-time code: 1A2B-3C4D", "hold")
	cli := f.cli()
	if !cli.Start() {
		t.Fatal("didn't start")
	}
	waitFor(t, "the setup to start", cli.Busy)
	if cli.Start() {
		t.Error("one setup at a time")
	}
	waitFor(t, "the code", func() bool { return cli.Setup().Code != nil })
	waitFor(t, "gh to wait", func() bool { return exists(filepath.Join(f.dir, "alive.txt")) })
	cli.Cancel()
	waitIdle(t, cli)
	if !reflect.DeepEqual(cli.Setup(), GhProgress{}) {
		t.Error("a cancel is not an error")
	}
	// gh is gone with it: its heartbeat has stopped.
	time.Sleep(300 * time.Millisecond)
	a, _ := os.ReadFile(filepath.Join(f.dir, "alive.txt"))
	time.Sleep(300 * time.Millisecond)
	if b, _ := os.ReadFile(filepath.Join(f.dir, "alive.txt")); string(b) != string(a) {
		t.Error("still running")
	}
	if slices.ContainsFunc(f.calls(), func(c []string) bool { return len(c) > 1 && c[1] == "setup-git" }) {
		t.Error("git is not set up after a cancel")
	}
}

// MARK: Install

func TestTheInstallStreamsItsLinesAndThenSignsIn(t *testing.T) {
	f := newFake(t)
	// No gh yet: the stand-in installer (winget's place) makes it.
	gh := f.gh()
	winget := f.also(t, "winget")
	os.Remove(gh)
	f.script("install", "out=Found GitHub CLI [GitHub.cli] Version 2.102.0", "out=Successfully installed", "copy=winget"+exeSuffix+"|gh"+exeSuffix)
	f.script("version", "out=gh version 2.102.0")
	f.script("auth_status", signedInLines...)
	cli := NewGitHubCli().With(&gh, gitEnv(f.dir)).WithInstall(InstallPlan{Winget: winget})
	if !cli.CanInstall() {
		t.Fatal("can't install")
	}
	var mu sync.Mutex
	var seen []GhProgress
	cli.OnChanged(func() { p := cli.Setup(); mu.Lock(); seen = append(seen, p); mu.Unlock() })
	if cli.Check(true).Installed {
		t.Fatal("installed")
	}
	cli.RunSetup()
	if !reflect.DeepEqual(cli.Setup(), GhProgress{}) {
		t.Errorf("%+v", cli.Setup())
	}
	mu.Lock()
	sawStart := slices.ContainsFunc(seen, func(p GhProgress) bool { return p.Step == Installing && p.Line == "Installing the GitHub CLI…" })
	sawLine := slices.ContainsFunc(seen, func(p GhProgress) bool { return p.Step == Installing && p.Line == "Successfully installed" })
	mu.Unlock()
	if !sawStart || !sawLine {
		t.Error(seen)
	}
	if k, _ := cli.Known(); !k.Installed || !k.SignedIn {
		t.Errorf("%+v", k)
	}
	calls := f.calls()
	if calls[0][0] != "install" || !slices.Contains(calls[0], "--id") || calls[0][slices.Index(calls[0], "--id")+1] != "GitHub.cli" {
		t.Error(calls)
	}
	// Signed in already: no sign-in was started.
	if countLogins(f) != 0 {
		t.Error("a sign-in")
	}
}

func TestAnInstallThatFailsSaysWhyAndWithoutAPackageManagerSaysWhatToRun(t *testing.T) {
	f := newFake(t)
	gh := f.gh()
	winget := f.also(t, "winget")
	os.Remove(gh)
	f.script("install", "err=No package found matching input criteria.", "exit=1")
	cli := NewGitHubCli().With(&gh, nil).WithInstall(InstallPlan{Winget: winget})
	cli.RunSetup()
	if deref(cli.Setup().Error) != "Couldn’t install the GitHub CLI: No package found matching input criteria." || cli.Busy() {
		t.Errorf("%+v", cli.Setup())
	}
	// By hand: Hover installs nothing and shows the hint.
	byHand := NewGitHubCli().With(&gh, nil).WithInstall(InstallPlan{Manual: sp("Install it with `sudo apt install gh`.")})
	if byHand.CanInstall() {
		t.Error("can install")
	}
	byHand.RunSetup()
	if deref(byHand.Setup().Error) != "Install it with `sudo apt install gh`." {
		t.Errorf("%+v", byHand.Setup())
	}
}

// MARK: Running a program

func TestAProgramIsStoppedAtItsTimeAndAtItsCapAndTakesItsInputOnStdin(t *testing.T) {
	f := newFake(t)
	cli := f.cli()
	gh := f.gh()
	env := cli.Env()
	f.script("slow_run", "hold")
	start := time.Now()
	r := RunProgram(gh, "", 400*time.Millisecond, 1024, []string{"slow", "run"}, nil, env)
	if r.Code != -1 || r.Err != "Timed out." || time.Since(start) > 5*time.Second {
		t.Errorf("%+v", r)
	}
	// Its output past the cap ends it; what was read stays, and the code is 0.
	f.script("big_out", "flood=100000")
	r = RunProgram(gh, "", 20*time.Second, 10_000, []string{"big", "out"}, nil, env)
	if !r.Capped || r.Code != 0 || len(r.Out) != 10_000 {
		t.Errorf("%v %d %d", r.Capped, r.Code, len(r.Out))
	}
	// Long text goes in on stdin, not on a command line.
	f.script("pr_create", "readall", "out=done")
	body := strings.Repeat("x", 300_000)
	r = RunProgram(gh, "", 20*time.Second, 1024, []string{"pr", "create", "--body-file", "-"}, []byte(body), env)
	if r.Code != 0 || strings.TrimSpace(r.Out) != "done" {
		t.Errorf("%+v", r)
	}
	if s, _ := f.stdinOf("pr_create"); len(s) != 300_000 {
		t.Error(len(s))
	}
	// A program that can't start is a failure with its reason, not a panic.
	r = RunProgram(filepath.Join(f.dir, "nothing-here"), "", 5*time.Second, 1024, nil, nil, env)
	if r.Code != -1 || r.Err == "" {
		t.Errorf("%+v", r)
	}
}

// MARK: Create pull request

func prArgs(title string) CreatePrArgs { return CreatePrArgs{Title: title} }

func TestCreatePullRequestMakesTheBranchCommitsPushesAndOpensIt(t *testing.T) {
	needGit(t)
	f := newFake(t)
	f.script("pr_create", "readall", "out=https://github.com/acme/app/pull/12")
	desk := f.desk()
	r := newRepo(t, "create")
	r.write(t, "a.txt", "two\n")
	a := CreatePrArgs{Title: "Change a", Body: "Made a two.", Branch: sp("hover/change-a"), Commit: true, Draft: true}
	s := r.snap()
	out := desk.CreatePr(&s, a)
	if out.Error != nil || !out.OK {
		t.Fatalf("%+v", out)
	}
	if ocText(out.URL) != "https://github.com/acme/app/pull/12" {
		t.Error(out.URL)
	}
	eqS(t, out.Steps, []string{"Made branch hover/change-a", "Committed the changes", "Pushed hover/change-a to origin"}, "steps")
	if !strings.Contains(git(t, r.remote, "branch", "--list", "hover/change-a"), "hover/change-a") {
		t.Error("the branch was pushed")
	}
	if got := git(t, r.repo, "status", "--porcelain"); got != "" {
		t.Errorf("the change was committed: %q", got)
	}
	// The commit message came in on stdin, with the title first.
	if got := strings.TrimRight(git(t, r.repo, "log", "-1", "--format=%B"), "\r\n "); got != "Change a\n\nCommitted from Hover." {
		t.Errorf("%q", got)
	}
	var gh []string
	for _, c := range f.calls() {
		if c[0] == "pr" && c[1] == "create" {
			gh = c
			break
		}
	}
	// Neither the description nor the commit message is an argument.
	eqS(t, gh, []string{"pr", "create", "--title", "Change a", "--body-file", "-", "--base", "main", "--head", "hover/change-a", "--draft"}, "gh call")
	if in, _ := f.stdinOf("pr_create"); in != "Made a two." {
		t.Errorf("%q", in)
	}
}

func TestAnEmptyDescriptionIsTheTitleAndAPullRequestIsNotADraftUnlessAsked(t *testing.T) {
	needGit(t)
	f := newFake(t)
	f.script("pr_create", "readall", "out=Creating pull request for feature into main in acme/app", "out=", "out=https://github.com/acme/app/pull/5")
	r := newRepo(t, "create-plain")
	git(t, r.repo, "switch", "-q", "-c", "feature")
	s := r.snap()
	out := f.desk().CreatePr(&s, prArgs("Just the title"))
	if out.Error != nil || !out.OK || ocText(out.URL) != "https://github.com/acme/app/pull/5" {
		t.Fatalf("%+v", out)
	}
	eqS(t, out.Steps, []string{"Pushed feature to origin"}, "nothing to commit, no new branch")
	for _, c := range f.calls() {
		if c[0] == "pr" && slices.Contains(c, "--draft") {
			t.Error("a draft")
		}
	}
	if in, _ := f.stdinOf("pr_create"); in != "Just the title" {
		t.Errorf("%q", in)
	}
}

func TestCreatePullRequestSaysNoBeforeItChangesAnything(t *testing.T) {
	needGit(t)
	f := newFake(t)
	f.script("pr_create", "out=https://github.com/acme/app/pull/1")
	desk := f.desk()
	r := newRepo(t, "create-no")
	r.write(t, "a.txt", "two\n")
	failure := func(a CreatePrArgs, snap DeskSnap) string {
		if e := desk.CreatePr(&snap, a).Error; e != nil {
			return *e
		}
		return "it went ahead"
	}

	// Never while the agent works in the folder.
	busy := r.snap()
	busy.Busy = true
	eq(t, failure(prArgs("x"), busy), "Wait for the agent to finish first: it is still working in this folder.", "busy")
	eq(t, failure(prArgs("   "), r.snap()), "Give the pull request a title.", "title")
	// A branch name that could be an option, or isn't a name at all, is refused, as a base is ignored.
	for _, name := range []string{"--evil", "has space", "a..b"} {
		a := prArgs("x")
		a.Branch = sp(name)
		eq(t, failure(a, r.snap()), "That branch name isn’t valid.", name)
	}
	// On the default branch without a new branch there is nothing to open.
	eq(t, failure(prArgs("x"), r.snap()), "The pull request needs a branch other than main.", "default branch")
	// A base that isn't a branch name falls back to the default one.
	a := prArgs("x")
	a.Base = sp("--upload-pack=evil")
	eq(t, failure(a, r.snap()), "The pull request needs a branch other than main.", "bad base")
	// Not a repository.
	plain := newDir(t, "create-plain-dir")
	eq(t, failure(prArgs("x"), DeskSnap{Folder: plain}), "Not a Git repository.", "not a repository")

	eq(t, strings.TrimSpace(git(t, r.repo, "branch", "--list")), "* main", "no branch was made")
	eq(t, strings.TrimSpace(git(t, r.repo, "status", "--porcelain")), "M a.txt", "nothing was committed")
	for _, c := range f.calls() {
		if c[0] == "pr" {
			t.Error("gh was asked")
		}
	}
}

func TestAStepThatFailsComesBackAsItsReasonWithTheStepsBeforeIt(t *testing.T) {
	needGit(t)
	f := newFake(t)
	desk := f.desk()
	r := newRepo(t, "create-fail")
	r.write(t, "a.txt", "two\n")
	a := CreatePrArgs{Title: "Change a", Branch: sp("hover/change-a"), Commit: true}
	s := r.snap()

	// The push fails: the remote is gone.
	git(t, r.repo, "remote", "set-url", "origin", filepath.Join(r.root, "gone.git"))
	out := desk.CreatePr(&s, a)
	if out.OK || out.URL != nil || !strings.HasPrefix(ocText(out.Error), "Couldn’t push the branch: ") {
		t.Errorf("%+v", out)
	}
	eqS(t, out.Steps, []string{"Made branch hover/change-a", "Committed the changes"}, "steps before the push")

	// The push works and gh says no.
	git(t, r.repo, "remote", "set-url", "origin", r.remote)
	f.script("pr_create", "readall", "err=GraphQL: Resource not accessible by personal access token", "exit=1")
	a.Branch = nil
	out = desk.CreatePr(&s, a)
	eq(t, ocText(out.Error), "gh couldn’t open the pull request: GraphQL: Resource not accessible by personal access token", "gh said no")
	eqS(t, out.Steps, []string{"Pushed hover/change-a to origin"}, "already committed and on the branch")

	// No gh at all.
	none := NewDesk(NewGitHubCli().With(sp(filepath.Join(r.root, "no-gh")), gitEnv(f.dir)), FindGit())
	a.Branch = sp("hover/change-a")
	eq(t, ocText(none.CreatePr(&s, a).Error), "Install the GitHub CLI first.", "no gh")
}

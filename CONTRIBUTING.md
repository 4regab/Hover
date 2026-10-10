# Contributing to Hover

Hover is built and run on Windows and Linux, and compiled on macOS. You can fork it, build it
and send changes without access to the maintainers' release machines, secrets or paid
agent accounts.

Start with [docs/development/architecture.md](docs/development/architecture.md) for how
Hover fits together, then [testing.md](docs/development/testing.md) and
[profiling.md](docs/development/profiling.md) for how to test and measure.

## Fork and build

1. Fork `4regab/Hover` on GitHub, then clone your fork:

   ```sh
   git clone https://github.com/<you>/Hover.git
   cd Hover
   git remote add upstream https://github.com/4regab/Hover.git
   ```

2. Pick the base branch. The Go Hover is on `main`; the Rust version it replaced is at the
   tag `rust-final`, and `dotnet` still holds 2.x (.NET). Branch from the one your change is for:

   ```sh
   git fetch upstream
   git switch -c my-change upstream/main
   ```

3. Set up your platform: [Windows](docs/development/windows.md), or on Linux the
   packages in the README's [Build from source](README.md#build-from-source). Then,
   from the repository root:

   ```powershell
   # Windows
   .\build.ps1 run              # build and start Hover
   ```

   ```sh
   # Linux
   make wgpu                    # once: the library the office is drawn with
   make
   WGPU_NATIVE_PATH=lib/libwgpu_native.so ./hover-linux
   ```

Only one copy of Hover runs at a time. Quit an installed Hover before you start your
build, or your build only opens the installed one's window and exits.

To keep your own settings and history out of it, give the build a data folder of
its own: set `HOVER_DATA_DIR` to an empty folder before you start it.

## Keep your fork up to date

```sh
git fetch upstream
git rebase upstream/main     # or: git merge upstream/main
```

Then build again. Nothing updates on its own. An installed Hover changes only when
you install a new build, and a fork changes only when you pull upstream into it.

## Make a focused change

- One problem per pull request. Touch only what the change needs.
- Match the style around your change. Comments say why, not what.
- A new Go module needs its reason in the pull request. Run `go mod tidy`.
- Line endings are CRLF, except Go files, shell scripts, `Makefile`, `VERSION` and
  `packaging/linux/*` (LF). The `.gitattributes` file handles this.
- Don't commit `publish/`, `dist/`, `evidence/` or `hover-linux` output.

## Check it before you send it

Run what applies to your change and say what you ran in the pull request:

- `gofmt -l cmd internal` prints nothing, and `go vet ./cmd/... ./internal/...` passes for
  each system you touched. Go builds for Windows and macOS from any machine:
  `GOOS=windows CGO_ENABLED=0 go vet ./cmd/... ./internal/...` and
  `GOOS=darwin GOARCH=arm64 CGO_ENABLED=0 go vet ./cmd/... ./internal/...`. On Linux add
  `-tags nowayland,nox11,novulkan`.
- For UI changes: render it with `hoverai --shots DIR` and look at the pictures. A
  green build doesn't prove the pixels are right.
- For agent, permission or storage changes: run an agent, or the stand-in agents described
  in [testing.md](docs/development/testing.md).
- For memory or speed changes: before and after numbers from
  [profiling.md](docs/development/profiling.md), on the same machine and settings.

The CI workflow (`.github/workflows/ci.yml`) checks the Go code for Windows, Linux and macOS
on every pull request, builds and drives the Windows app and its installer, and builds the
Linux packages and the Mac app. It does not run the Go tests. It uses only GitHub's own
runners: no secrets, and no agent accounts.

## Pull requests

Fill in the template. In short:

- **Bugs.** Steps to reproduce, what happened, and what should happen. Add the OS,
  the GPU, and the lines from `hover.log` in the data folder.
- **Visual changes.** A screenshot or short video from before and after.
- **Performance changes.** The scenario, the counter (for example private commit on
  Windows, or USS on Linux), before and after medians over at least three runs, and
  the machine.
- **Blocked checks.** Say plainly what you couldn't run and why (for example no Linux
  desktop, or no Cursor account).

## Releases

Releases are made by CI. Every pull request merged to `main` (other than one that only
changes docs) is published as a nightly build: a pre-release with the next patch number, never
the Latest, with the pull requests merged since the last release as its notes. You don't change
`VERSION` or `CHANGELOG.md` for that. The maintainers make a numbered release by raising the
number in `VERSION` (with its `CHANGELOG.md` section) or pushing a `v*` tag.
`.github/workflows/ci.yml` publishes the installers once the builds pass. Forks don't need it:
`.\build.ps1 installer` and `make package` build the same installers locally.

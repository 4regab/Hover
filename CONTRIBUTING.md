# Contributing to Hover

Hover is built and tested on Windows and Linux. You can fork it, build it, test it and
send changes without access to the maintainers' release machines, secrets or paid
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

2. Pick the base branch. The native (Rust) Hover is on `main`; `dotnet`
   still holds 2.x (.NET). Branch from the one your change is for:

   ```sh
   git fetch upstream
   git switch -c my-change upstream/main
   ```

3. Set up your platform: [Windows](docs/development/windows.md), or on Linux the
   packages in the README's [Build from source](README.md#build-from-source). Then,
   from the repository root:

   ```powershell
   # Windows
   .\build.ps1 release run      # build and start Hover
   .\build.ps1 test             # every test in the workspace
   ```

   ```sh
   # Linux
   make                         # release build
   make test                    # every test in the workspace
   ./target/release/hoverai
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
- A new crate needs its reason written in its `Cargo.toml`.
- Line endings are CRLF, except `Makefile` and `packaging/linux/*` (LF). The
  `.gitattributes` file handles this.
- Don't commit `target/`, `publish/`, `dist/` or `evidence/` output.

## Check it before you send it

Run what applies to your change and say what you ran in the pull request:

- `cargo test --release --workspace` on your
  platform.
- On Linux, if you changed Windows code, the Windows compile check:
  `cargo xwin check --release --workspace --all-targets --target x86_64-pc-windows-msvc`
  (needs `rustup target add x86_64-pc-windows-msvc`, `cargo install cargo-xwin` and
  clang 19 or newer). CI checks Windows on Windows, so you can also leave it to CI.
- For UI changes: render it with `hover --shots DIR` and look at the pictures. A
  green build doesn't prove the pixels are right. On Linux with X11, also run
  `hover --selftest DIR`.
- For agent, permission or storage changes: the fake-agent scenarios in
  [testing.md](docs/development/testing.md).
- For memory or speed changes: before and after numbers from
  [profiling.md](docs/development/profiling.md), on the same machine and settings.

The CI workflow (`.github/workflows/ci.yml`) runs the tests on Windows and Linux for
every pull request. It uses only GitHub's own runners and the fake agent: no secrets,
and no agent accounts.

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

Releases are made by the maintainers: a push to `main` that raises
`Cargo.toml`'s version (or a `v*` tag) runs `.github/workflows/ci.yml`, which publishes the
installers once the tests pass. Forks don't need it: `.\build.ps1 installer` and
`make package` build the same installers locally.

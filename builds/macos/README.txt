Hover 3.4.2 for macOS 14 or later, built from this branch with scripts/build-macos.sh.

  Hover-macOS-arm64.zip   Apple silicon (M1 and later)
  Hover-macOS-x64.zip     Intel

Install
  1. Unzip and open Hover.app. It is signed ad hoc, not notarized, so macOS stops it the
     first time: open System Settings → Privacy & Security, scroll to "Hover was blocked",
     click Open Anyway and confirm. (In Terminal, `xattr -dr com.apple.quarantine
     ~/Downloads/Hover.app` does the same.)
  2. Hover offers to move itself to Applications. Say yes.
  3. Settings opens on Get Started. Click Set Up beside each agent you use: Hover installs
     it with its maker's installer and opens its sign-in. A Mac with no Homebrew or Node
     needs nothing else; Node.js, ripgrep and the GitHub CLI come from their own
     releases, pinned to their checksums, into ~/.local, with no password. "This Mac"
     sets up the agent sandbox and git (Apple's Command Line Tools) the same way.
  4. Hover lives in the menu bar and the notch: hover the notch or press Option-N.

Agent desktops (Settings → Computer Use) need macOS 26 on Apple silicon.

SHA-256
  Hover-macOS-arm64.zip  558719e9519f7b86ba73edc3f0a935422ee59458d8de7c1c58c2d1974e0346c4
  Hover-macOS-x64.zip    3f14332ae72a0312c454c10018b4d0598d7bc5dcea906f9ec4b8ebee5d1649fe

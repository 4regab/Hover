Hover 4.20.1 for macOS 14 or later, built from this branch with scripts/build-macos.sh.

  Hover-macOS-arm64.zip   Apple silicon (M1 and later)
  Hover-macOS-x64.zip     Intel

What 4.20.1 fixes
  - Cursor usage is read with Cursor closed (its database was opened in a way that needs
    Cursor running).
  - A new agent starts at once: it no longer waits minutes for its project's desktop to be
    made, for another agent's checkpoint, or fails because its tool is busy in another folder.
  - Failures show as a popup you dismiss (with Edit task / Open chat), not a toast at the top;
    a refused task keeps its words. A backend that stops is started again.

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
  Hover-macOS-arm64.zip  681fac621a88ccec2aa4fb4a814c7c095fe091cfb588cfd5998ff1d38906e0af
  Hover-macOS-x64.zip    005a5c8e46d44bb692ca107285566087992d6bae236bc15591ef4f0384d47323

# macOS feasibility and implementation plan

Historical planning snapshot. Implementation now exists; see MACOS.md for current build and validation instructions.
Inspected 2026-09-30 at commit 689e53fec79a0f7923504322b103c9d8c51b28fa.

## Local setup

Repository: this checkout, origin https://github.com/4regab/Hover.git, branch main.
Device: Apple Silicon (arm64), macOS 27.0.1. Node 26.8.1 and npm 11.19.0 were already installed.
Installed .NET SDK 10.0.401 in ~/.dotnet and added DOTNET_ROOT/PATH to ~/.zprofile for new login shells.
Installed web dependencies with npm ci --prefix web/office; node_modules is excluded locally via .git/info/exclude.
Apple Command Line Tools are present. Full Xcode will be needed for the proposed Xcode app/test workflow.

Verified npm office bundle generation and dotnet build Hover.slnx -c Release:
build succeeded with zero warnings/errors. This produces Windows assemblies, not a Mac app.
The existing test project requires Microsoft.WindowsDesktop.App and cannot run on macOS.
The current npm lockfile reports a moderate esbuild development-server advisory; build.mjs uses bundling, not a dev server. Dependency updates are outside this setup.

## Why this is a port

Hover.csproj targets net10.0-windows10.0.17763.0 with WPF, WinForms and WebView2.
EnableWindowsTargeting allows this Mac to compile Windows code; changing the publish RID to osx-arm64 cannot make WPF run here.
The existing publisher hardcodes win-x64 and Inno Setup; CI runs on a Windows CodeBuild runner.

## Recommended architecture

Add a native Swift/AppKit macOS application, using SwiftUI where useful for settings,
WKWebView for the existing three.js office, and a self-contained .NET 10 helper process
for the existing agent/session/history engine. Keep the Windows host and introduce a shared
plain net10.0 library. This is a recommendation based on the repository, not a tested prototype.

AppKit owns the notch panel, animation/clip, screen geometry, focus, menu-bar item,
dashboard, hotkeys, pickers, notifications and login-item integration. A Swift host avoids
forcing this unusually specific window behavior through a generic cross-platform window API.
The extra language and IPC layer are the main cost. Avalonia with native macOS interop is a
credible alternative if a single C# UI is the priority; it still needs a replacement webview
and platform integration. A full Swift rewrite would discard substantial working agent code.

The helper communicates over versioned, bounded JSON messages on private stdin/stdout pipes.
Use correlation IDs, explicit readiness/shutdown, cancellation, error and state events;
keep logs on stderr. The UI owns Keychain access and supplies the history key over a pipe,
never argv or environment. No listening HTTP port is needed for Hover IPC (OpenCode still
uses its existing authenticated loopback server). Both UI surfaces share one backend.

## Reuse and replacement map

| Existing area | macOS work |
| --- | --- |
| web/office/main.js, page.html, md.js, diagram.js | Reuse scene/UI; replace chrome.webview coupling with a small host transport adapter. Preserve message types and ready/state/visibility behavior. |
| Owl/KiroPage.cs | Port orchestration/message routing; WKScriptMessageHandler receives JS messages, native code delivers state back to JS. Replace WebView2 virtual hosts with a scoped WKURLSchemeHandler; prevent path traversal and access beyond approved session folders. |
| Services/AcpHost.cs, OpenCodeHost.cs, KiroRunner.cs; Owl/KiroSession.cs | Extract into shared library. Preserve approvals, queues, cancellation, reconnect, idle shutdown and session resumption. Replace Windows job cleanup with managed process groups plus a parent-liveness strategy; killing only the helper is insufficient after a UI crash. |
| Services/Agents.cs; Core/Quota.cs, Layout.cs, Palette.cs | Mostly reusable. Audit executable discovery, executable permissions, Unix paths, shell environment and platform credential sources. GUI launch PATH must find Homebrew/user-installed tools. Cursor installer hints/shim paths are Windows-specific. |
| Core/Settings.cs, Shortcut.cs | Remove WPF key enums, DispatcherTimer and Registry coupling. Keep storage schema with platform-neutral shortcuts; inject persistence scheduling and startup integration. |
| Core/Crypto.cs; Owl/AgentHistory.cs | Preserve AES-GCM/history serialization, replace DPAPI with macOS Keychain key storage. Never silently replace an unreadable key. Windows DPAPI history cannot be copied and decrypted on a Mac without an explicit Windows-side export. |
| Core/Paths.cs | Use ~/Library/Application Support/Hover; keep HOVER_DATA_DIR for tests. Preserve the Windows legacy migration in the Windows host. |
| Interop/*; Owl/Notch.cs, Ui.cs, Marks.cs, Bot.cs, Pages.cs, Theme.cs; Services/TrayIcon.cs | Rebuild native UI/integration. Resting panel must not steal focus; expanded office/approval card must accept keyboard input. Return focus correctly after closing. |
| App.xaml.cs | Native app lifecycle, single-instance/dashboard activation, orderly and crash shutdown. |
| build.ps1, installer/Hover.iss, .github/workflows/codebuild.yml | Add separate macOS build/package/sign/notarize job and retain Windows release flow. |

Quota support needs separate validation: Claude currently reads ~/.claude/.credentials.json;
macOS credentials may use Keychain. Cursor's state database location and credential representation
also need verification. Codex session-log parsing and Kiro CLI output parsing can be reused,
subject to actual Mac tool versions. Do not refresh another tool's OAuth tokens.
A historical comment in Crypto.cs mentions a macOS build, but this checkout contains only
a Windows project and no implemented Mac key-storage path.

## Implementation sequence when conversion is authorized

1. Prototype the native transparent panel with the real office bundle and bidirectional
   bridge, using fake sessions. Verify WebGL, clipping, focus, keyboard, Spaces/fullscreen,
   multiple displays, sleep/wake and physical-notch geometry. Use NSScreen safe-area APIs;
   show a top-center island on displays without a physical notch. Resolve WKWebView OGG
   audio support and local-image origin behavior in this prototype.
2. Extract the net10.0 backend and platform interfaces, keeping Windows behavior working.
   Separate portable tests from WPF fixtures; existing linked-source test guidance is a
   useful starting point, not proof the entire backend is already portable.
3. Connect one real agent end to end, including approval/cancel/resume and cleanup; then
   enable all four tools with Mac-specific discovery, status checks and install guidance.
4. Add Keychain-backed history, settings, quotas, menu-bar/dashboard, notifications,
   hotkeys and login launch. Test first run with no tools installed and GUI launch outside a terminal.
5. Package an Apple Silicon .app first. Add a separate osx-x64 backend/app artifact if Intel
   support is wanted; a universal app requires both compatible slices, not renaming an arm64 binary.
6. Sign nested helper/native libraries before the outer bundle using Developer ID,
   enable hardened runtime with only required entitlements, notarize and staple, then
   distribute a DMG or ZIP. Add macOS CI and test the downloaded artifact on a clean Mac.

Release gate: unit tests for shared code, native host integration/UI tests, real-agent
smoke tests, encrypted-history restart checks, crash/orphan-process checks, and signed
artifact validation. Existing Windows E2E tests are documented as stale for the 2.0 office;
they cannot serve as the macOS acceptance suite.

## Primary references

- WPF runs only on Windows: https://learn.microsoft.com/en-us/dotnet/desktop/wpf/overview/
- .NET macOS installation: https://learn.microsoft.com/en-us/dotnet/core/install/macos
- AppKit panel: https://developer.apple.com/documentation/appkit/nspanel
- JS bridge: https://developer.apple.com/documentation/webkit/wkscriptmessagehandler
- Keychain: https://developer.apple.com/documentation/security/keychain-services
- Screen notch geometry: https://developer.apple.com/documentation/appkit/nsscreen/auxiliarytopleftarea
- Signing/notarization: https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- C# alternative: https://docs.avaloniaui.net/docs/platform-specific-guides/macos

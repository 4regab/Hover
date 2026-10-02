using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Net.Http;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.RegularExpressions;
using Hover.Core;

namespace Hover.Services;

/// The GitHub CLI (gh), which the desk's pull request panels read through and Hover's
/// Create pull request runs: whether it is installed and signed in, and a one-click
/// setup for both. Install uses Homebrew where it is, else the official release from
/// github.com/cli/cli (a Mac's universal zip, Linux's tarball) into ~/.local/bin, and
/// winget on Windows. Sign-in is gh's own device flow (`gh auth login --web`): its
/// one-time code is shown for the user to enter at github.com/login/device, then git
/// is set to use gh for GitHub (`gh auth setup-git`), so a push from Hover works.
/// gh keeps its sign-in in the system keychain; the sandboxed agents never read it.
/// No WPF in here.
public static class GitHubCli
{
    public const string DeviceUrl = "https://github.com/login/device";

    /// What Hover knows: installed, signed in and as whom.
    public sealed record Status(bool Installed, bool SignedIn, string? User, string? Version, string Hint);

    /// What a setup is doing ("installing" or "signing-in"), its newest line, the
    /// one-time code while sign-in waits, and why it stopped if it failed.
    public sealed record Progress(string? Step, string Line, string? Code, string? Error);

    public static readonly Progress Idle = new(null, "", null, null);
    private static Progress _progress = Idle;
    private static (DateTime At, Status Value)? _known;
    private static CancellationTokenSource? _running;
    private static readonly object Lock = new();

    /// Raised off the caller's thread when the status or a setup's progress changes.
    public static event Action? Changed;

    private static string Home => Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

    /// gh, from PATH or where its installers put it.
    public static string? Exe()
    {
        if (Quota.OnPath("gh") is { } found) return found;
        var places = OperatingSystem.IsWindows()
            ? new[] { Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "GitHub CLI", "gh.exe"), Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "GitHub CLI", "gh.exe") }
            : new[] { "/opt/homebrew/bin/gh", "/usr/local/bin/gh", Path.Combine(Home, ".local", "bin", "gh"), "/usr/bin/gh" };
        return places.FirstOrDefault(File.Exists);
    }

    public static Progress Setup { get { lock (Lock) return _progress; } }
    public static bool Busy { get { lock (Lock) return _running is not null; } }
    public static Status? Known { get { lock (Lock) return _known?.Value; } }

    private static void Report(Progress p) { lock (Lock) _progress = p; Changed?.Invoke(); }

    /// Installed and signed in, from gh itself. Kept a minute unless fresh.
    public static async Task<Status> Check(bool fresh = false)
    {
        lock (Lock) if (!fresh && _known is { } k && DateTime.UtcNow - k.At < TimeSpan.FromMinutes(1)) return k.Value;
        Status s;
        if (Exe() is not { } gh) s = new(false, false, null, null, "Install the GitHub CLI to see and open pull requests.");
        else
        {
            var (vc, vt) = await Run(gh, TimeSpan.FromSeconds(15), "--version");
            var version = vc == 0 ? Regex.Match(vt, @"\d+\.\d+\.\d+").Value : null;
            var (ac, at) = await Run(gh, TimeSpan.FromSeconds(20), "auth", "status", "--hostname", "github.com");
            var user = ParseUser(at);
            s = ac == 0 ? new(true, true, user, version, "") : new(true, false, null, version, "Sign in to GitHub to see and open pull requests.");
        }
        lock (Lock) _known = (DateTime.UtcNow, s);
        Changed?.Invoke();
        return s;
    }

    /// The account in `gh auth status`'s answer ("Logged in to github.com account X").
    internal static string? ParseUser(string text) =>
        Regex.Match(text, @"Logged in to \S+ (?:account|as) ([A-Za-z0-9-]+)") is { Success: true } m ? m.Groups[1].Value : null;

    /// The one-time code gh's device flow prints ("First copy your one-time code: ABCD-1234").
    internal static string? ParseCode(string line) =>
        Regex.Match(line, @"\b([A-Z0-9]{4}-[A-Z0-9]{4})\b") is { Success: true } m && line.Contains("code", StringComparison.OrdinalIgnoreCase) ? m.Groups[1].Value : null;

    public static void Cancel() { lock (Lock) _running?.Cancel(); }

    /// One click: installs gh if it is missing, then signs in if it isn't.
    public static async Task Run()
    {
        CancellationTokenSource cts;
        lock (Lock)
        {
            if (_running is not null) return;
            _running = cts = new CancellationTokenSource();
        }
        try
        {
            var s = await Check(fresh: true);
            if (!s.Installed)
            {
                Report(new("installing", "Installing the GitHub CLI…", null, null));
                await Install(cts.Token);
                s = await Check(fresh: true);
                if (!s.Installed) throw new SetupError("The install finished, but gh still isn’t found.");
            }
            if (!s.SignedIn)
            {
                await SignIn(cts.Token);
                s = await Check(fresh: true);
                if (!s.SignedIn) throw new SetupError("GitHub sign-in didn’t finish. Try again.");
            }
            Report(Idle);
        }
        catch (OperationCanceledException) { Report(Idle); }
        catch (SetupError e) { Report(new(null, "", null, e.Message)); }
        catch (Exception e) { Report(new(null, "", null, $"GitHub CLI setup stopped: {e.Message}")); }
        finally
        {
            lock (Lock) _running = null;
            cts.Dispose();
            _ = Check(fresh: true);
        }
    }

    private sealed class SetupError(string message) : Exception(message);

    // MARK: Install

    private static async Task Install(CancellationToken ct)
    {
        if (OperatingSystem.IsWindows())
        {
            if (Quota.OnPath("winget") is not { } winget) throw new SetupError("Install the GitHub CLI from cli.github.com (winget isn’t available).");
            await Stream(winget, TimeSpan.FromMinutes(10), ct, "install", "--id", "GitHub.cli", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements");
            return;
        }
        var brew = Quota.OnPath("brew") ?? new[] { "/opt/homebrew/bin/brew", "/usr/local/bin/brew" }.FirstOrDefault(File.Exists);
        if (brew is not null && OperatingSystem.IsMacOS())
        {
            try { await Stream(brew, TimeSpan.FromMinutes(10), ct, "install", "gh"); return; }
            catch (SetupError) when (!ct.IsCancellationRequested) { Report(new("installing", "Homebrew couldn’t install it; downloading it from GitHub…", null, null)); }
        }
        await Download(ct);
    }

    /// The newest release's archive for this system, unpacked to ~/.local/share/gh and
    /// linked from ~/.local/bin/gh (on Hover's PATH).
    private static async Task Download(CancellationToken ct)
    {
        using var http = new HttpClient { Timeout = TimeSpan.FromMinutes(5) };
        http.DefaultRequestHeaders.UserAgent.ParseAdd("Hover");
        Report(new("installing", "Finding the newest GitHub CLI…", null, null));
        using var doc = JsonDocument.Parse(await http.GetStringAsync("https://api.github.com/repos/cli/cli/releases/latest", ct));
        var arch = RuntimeInformation.OSArchitecture == Architecture.Arm64 ? "arm64" : "amd64";
        var want = OperatingSystem.IsMacOS() ? "_macOS_universal.zip" : $"_linux_{arch}.tar.gz";
        var asset = doc.RootElement.GetProperty("assets").EnumerateArray().FirstOrDefault(a => a.GetProperty("name").GetString()?.EndsWith(want, StringComparison.Ordinal) == true);
        if (asset.ValueKind != JsonValueKind.Object) throw new SetupError("No GitHub CLI download for this system. Install it from cli.github.com.");
        var url = asset.GetProperty("browser_download_url").GetString()!;
        if (!url.StartsWith("https://github.com/cli/cli/releases/download/", StringComparison.Ordinal)) throw new SetupError("Unexpected download address for gh.");
        Report(new("installing", $"Downloading {asset.GetProperty("name").GetString()}…", null, null));
        var root = Path.Combine(Home, ".local", "share", "gh-cli");
        var temp = Path.Combine(Path.GetTempPath(), "hover-gh-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(temp);
        try
        {
            var file = Path.Combine(temp, "gh" + (want.EndsWith(".zip") ? ".zip" : ".tar.gz"));
            await using (var src = await http.GetStreamAsync(url, ct))
            await using (var dst = File.Create(file)) await src.CopyToAsync(dst, ct);
            var into = Path.Combine(temp, "x");
            if (file.EndsWith(".zip")) ZipFile.ExtractToDirectory(file, into);
            else
            {
                await using var gz = new GZipStream(File.OpenRead(file), CompressionMode.Decompress);
                await System.Formats.Tar.TarFile.ExtractToDirectoryAsync(gz, into, true, ct);
            }
            var bin = Directory.EnumerateFiles(into, "gh", SearchOption.AllDirectories).FirstOrDefault(p => Path.GetFileName(Path.GetDirectoryName(p)) == "bin")
                ?? throw new SetupError("The download had no gh in it.");
            var top = Path.GetDirectoryName(Path.GetDirectoryName(bin))!;
            if (Directory.Exists(root)) Directory.Delete(root, true);
            Directory.CreateDirectory(Path.GetDirectoryName(root)!);
            Directory.Move(top, root);
            var exe = Path.Combine(root, "bin", "gh");
            if (!OperatingSystem.IsWindows())
                File.SetUnixFileMode(exe, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute | UnixFileMode.GroupRead | UnixFileMode.GroupExecute | UnixFileMode.OtherRead | UnixFileMode.OtherExecute);
            var link = Path.Combine(Home, ".local", "bin", "gh");
            Directory.CreateDirectory(Path.GetDirectoryName(link)!);
            if (File.Exists(link) || new FileInfo(link).LinkTarget is not null) File.Delete(link);
            File.CreateSymbolicLink(link, exe);
        }
        finally { try { Directory.Delete(temp, true); } catch { } }
    }

    // MARK: Sign in

    /// gh's device flow: it prints a one-time code, the user enters it at
    /// github.com/login/device (Hover opens it), and gh waits until they have.
    private static async Task SignIn(CancellationToken ct)
    {
        var gh = Exe() ?? throw new SetupError("gh isn’t installed.");
        Report(new("signing-in", "Asking GitHub for a sign-in code…", null, null));
        var psi = Quota.Hidden(gh, "auth", "login", "--hostname", "github.com", "--git-protocol", "https", "--web");
        psi.Environment["GH_NO_UPDATE_NOTIFIER"] = "1";
        // gh opens the page itself where it can; Hover shows the link and the code too.
        using var p = new Process { StartInfo = psi };
        string? code = null;
        void Line(string? raw)
        {
            if (raw is null) return;
            var l = Quota.StripAnsi(raw).Trim();
            if (l.Length == 0) return;
            code ??= ParseCode(l);
            Report(new("signing-in", code is null ? (l.Length > 140 ? l[..139] + "…" : l) : $"Enter the code at {DeviceUrl.Replace("https://", "")}, then come back.", code, null));
            // "Press Enter to open github.com in your browser..."
            if (l.Contains("Press Enter", StringComparison.OrdinalIgnoreCase)) { try { p.StandardInput.WriteLine(); p.StandardInput.Flush(); } catch { } }
        }
        p.OutputDataReceived += (_, e) => Line(e.Data);
        p.ErrorDataReceived += (_, e) => Line(e.Data);
        p.Start();
        p.BeginOutputReadLine();
        p.BeginErrorReadLine();
        using var limit = CancellationTokenSource.CreateLinkedTokenSource(ct);
        limit.CancelAfter(TimeSpan.FromMinutes(15));
        try { await p.WaitForExitAsync(limit.Token); }
        catch (OperationCanceledException)
        {
            try { p.Kill(entireProcessTree: true); } catch { }
            if (ct.IsCancellationRequested) throw;
            throw new SetupError("GitHub sign-in timed out.");
        }
        if (p.ExitCode != 0) throw new SetupError("GitHub sign-in didn’t finish.");
        Report(new("signing-in", "Setting up git to use your GitHub sign-in…", null, null));
        await Run(gh, TimeSpan.FromSeconds(30), "auth", "setup-git", "--hostname", "github.com");
    }

    // MARK: Running gh

    private static async Task Stream(string exe, TimeSpan timeout, CancellationToken ct, params string[] args)
    {
        var psi = Quota.Hidden(exe, args);
        psi.Environment["HOMEBREW_NO_AUTO_UPDATE"] = "1";
        psi.Environment["NONINTERACTIVE"] = "1";
        using var p = new Process { StartInfo = psi };
        var tail = "";
        void Line(string? raw)
        {
            if (raw is null) return;
            var l = Quota.StripAnsi(raw).Trim();
            if (l.Length == 0) return;
            tail = l;
            Report(new("installing", l.Length > 140 ? l[..139] + "…" : l, null, null));
        }
        p.OutputDataReceived += (_, e) => Line(e.Data);
        p.ErrorDataReceived += (_, e) => Line(e.Data);
        p.Start();
        p.StandardInput.Close();
        p.BeginOutputReadLine();
        p.BeginErrorReadLine();
        using var limit = CancellationTokenSource.CreateLinkedTokenSource(ct);
        limit.CancelAfter(timeout);
        try { await p.WaitForExitAsync(limit.Token); }
        catch (OperationCanceledException)
        {
            try { p.Kill(entireProcessTree: true); } catch { }
            if (ct.IsCancellationRequested) throw;
            throw new SetupError("The install took too long and was stopped.");
        }
        if (p.ExitCode != 0) throw new SetupError($"Couldn’t install the GitHub CLI: {(tail.Length > 0 ? tail : $"exit code {p.ExitCode}")}");
    }

    internal static async Task<(int Code, string Text)> Run(string exe, TimeSpan timeout, params string[] args)
    {
        try
        {
            var psi = Quota.Hidden(exe, args);
            psi.Environment["GH_PROMPT_DISABLED"] = "1";
            psi.Environment["GH_NO_UPDATE_NOTIFIER"] = "1";
            using var p = new Process { StartInfo = psi };
            p.Start();
            p.StandardInput.Close();
            var output = p.StandardOutput.ReadToEndAsync();
            var error = p.StandardError.ReadToEndAsync();
            using var cts = new CancellationTokenSource(timeout);
            try { await p.WaitForExitAsync(cts.Token); }
            catch (OperationCanceledException) { try { p.Kill(entireProcessTree: true); } catch { } return (-1, "Timed out."); }
            return (p.ExitCode, Quota.StripAnsi(await output + "\n" + await error));
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException or InvalidOperationException) { return (-1, e.Message); }
    }
}

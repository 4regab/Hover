using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using Hover.Core;

namespace Hover.Services;

/// Each project's own desktop: a Cua Space (spaces.cua.ai), a VM or container that the
/// agents working in that folder share (each with its own cursor), and the user watches
/// and steps into, instead of the user's own screen. Hover goes through Cua's own `cua` CLI (MIT): `cua spaces
/// create|start|stop|delete` for the Space, `cua mcp --sandbox <space>` as the
/// session's computer-use MCP server (run by Hover outside the agents' sandbox and
/// joined to the agent over the same socket as Hover's browser), `cua sb view` for the
/// live viewer the Screen panel shows, and `cua teleport push` for an app dragged onto
/// the notch. Spaces are local and free; nothing goes through Cua's relay. A project's
/// Space is made when its first agent's run starts (a clone, about 25 s, once the image
/// is on the Mac), stopped when no agent of that project is left in the office, and
/// deleted with the project's last session. No WPF.
public static class Spaces
{
    public const string ServerName = "cua-space";

    /// Off until switched on in Settings → Computer Use; then it replaces Cua Driver on
    /// the user's own desktop.
    public static bool Wanted => Settings.AgentSpaces && Supported;

    /// Cua Spaces needs macOS 26 or later on Apple silicon (or Linux with Docker).
    public static bool Supported => OperatingSystem.IsMacOS() ? Environment.OSVersion.Version.Major >= 26 && System.Runtime.InteropServices.RuntimeInformation.OSArchitecture == System.Runtime.InteropServices.Architecture.Arm64 : OperatingSystem.IsLinux();

    private static string Home => Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

    /// The cua CLI, from PATH or where its installer puts it.
    public static string? Exe() => Quota.OnPath("cua") ?? new[] { Path.Combine(Home, ".local", "bin", "cua"), "/usr/local/bin/cua", "/opt/homebrew/bin/cua" }.FirstOrDefault(File.Exists);

    /// Lume, which runs the macOS VMs: the source of truth for whether one is on, and
    /// how it is sized. Cua's own list has no power state for local VMs.
    public static string? LumeExe() => Quota.OnPath("lume") ?? new[] { Path.Combine(Home, ".local", "bin", "lume"), Path.Combine(Home, ".local", "share", "lume", "lume.app", "Contents", "MacOS", "lume") }.FirstOrDefault(File.Exists);

    /// What a desktop is given, from this Mac's size: enough to be smooth, leaving the
    /// user most of their machine. Cua's default (2 cores, 4 GB, 1024×768) is sluggish
    /// for macOS 26 and blurry in the panel.
    public sealed record Size(int Cpus, int MemoryGb, string Display);
    public static Size Target()
    {
        var gb = (int)(GC.GetGCMemoryInfo().TotalAvailableMemoryBytes / (1L << 30));
        var cores = Environment.ProcessorCount;
        return new(Math.Clamp(cores / 3, 2, 6), gb >= 24 ? 8 : gb >= 16 ? 6 : 4, "1280x800");
    }

    public sealed record VmInfo(bool Exists, bool Running, int Cpus, int MemoryGb, string? Display);
    /// A local VM as Lume sees it (status running/stopped, its size); null when Lume
    /// isn't there or doesn't know it.
    internal static async Task<VmInfo?> Vm(string name)
    {
        if (LumeExe() is not { } lume) return null;
        var (code, text) = await Run(lume, TimeSpan.FromSeconds(15), "get", name, "--format", "json");
        if (code != 0) return text.Contains("not found", StringComparison.OrdinalIgnoreCase) ? new VmInfo(false, false, 0, 0, null) : null;
        return ParseVm(text);
    }
    internal static VmInfo? ParseVm(string json)
    {
        var start = json.IndexOfAny(new[] { '{', '[' });
        if (start < 0) return null;
        try
        {
            using var d = JsonDocument.Parse(json[start..]);
            var e = d.RootElement.ValueKind == JsonValueKind.Array ? d.RootElement.EnumerateArray().FirstOrDefault() : d.RootElement;
            if (e.ValueKind != JsonValueKind.Object) return null;
            var status = (S(e, "status") ?? "").ToLowerInvariant();
            var cpu = e.TryGetProperty("cpuCount", out var c) && c.TryGetInt32(out var ci) ? ci : 0;
            var mem = e.TryGetProperty("memorySize", out var m) && m.TryGetInt64(out var mi) ? (int)(mi / (1L << 30)) : 0;
            return new(true, status is "running" or "booting" or "starting", cpu, mem, S(e, "display"));
        }
        catch (JsonException) { return null; }
    }

    /// The image a new Space starts from: the macOS VM (two at most on a Mac) or Linux.
    public static string Image => Settings.SpaceImage == "linux" ? "linux" : OperatingSystem.IsMacOS() ? "macos:26" : "linux";

    /// The Space a project's agents share: "hover-", the folder's name and a short hash
    /// of its full path, so two folders called "app" get two desktops.
    public static string NameFor(string folder)
    {
        var full = Path.TrimEndingDirectorySeparator(Path.GetFullPath(folder));
        if (!OperatingSystem.IsLinux()) full = full.ToLowerInvariant();
        var hash = Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(full)))[..6].ToLowerInvariant();
        var slug = Regex.Replace(Path.GetFileName(full).ToLowerInvariant(), "[^a-z0-9]+", "-").Trim('-');
        if (slug.Length > 20) slug = slug[..20].TrimEnd('-');
        return "hover-" + (slug.Length > 0 ? slug + "-" : "") + hash;
    }
    public static string IdFor(string folder) => "local:" + NameFor(folder);
    /// The project's name as the desktop shows it.
    public static string Title(string folder) => Path.GetFileName(Path.TrimEndingDirectorySeparator(folder)) is { Length: > 0 } n ? n : folder;

    // MARK: Status and setup

    /// Installed, ready (the image is on the Mac and Spaces answer), and what to do.
    public sealed record Status(bool Installed, bool Ready, string? Version, string Hint, int Running);

    public sealed record Progress(string? Step, string Line, double? Fraction, string? Error);
    public static readonly Progress Idle = new(null, "", null, null);
    private static Progress _progress = Idle;
    private static (DateTime At, Status Value)? _known;
    private static CancellationTokenSource? _setup;
    private static readonly object Lock = new();
    public static event Action? Changed;
    public static Progress Setup { get { lock (Lock) return _progress; } }
    public static Status? Known { get { lock (Lock) return _known?.Value; } }
    public static bool Busy { get { lock (Lock) return _setup is not null; } }
    private static void Report(Progress p) { lock (Lock) _progress = p; Changed?.Invoke(); }

    // Progress comes many times a second while an image downloads; the office is told at
    // most four times a second, which is all a progress bar needs.
    private static Timer? _notify;
    private static int _pending;
    private static void Notify()
    {
        if (Interlocked.Exchange(ref _pending, 1) == 1) return;
        _notify ??= new Timer(_ => { Interlocked.Exchange(ref _pending, 0); Changed?.Invoke(); });
        _notify.Change(250, Timeout.Infinite);
    }

    public static async Task<Status> Check(bool fresh = false)
    {
        lock (Lock) if (!fresh && _known is { } k && DateTime.UtcNow - k.At < TimeSpan.FromMinutes(1)) return k.Value;
        Status s;
        if (!Supported) s = new(false, false, null, "Agent desktops need Cua Spaces, which needs macOS 26 or later on Apple silicon.", 0);
        else if (Exe() is not { } cua) s = new(false, false, null, "Install Cua Spaces to give each agent a desktop of its own.", 0);
        else
        {
            var (vc, vt) = await Run(cua, TimeSpan.FromSeconds(20), "--version");
            var version = vc == 0 ? Regex.Match(vt, @"\d+\.\d+\.\d+").Value : null;
            var (lc, lt) = await Run(cua, TimeSpan.FromSeconds(30), "spaces", "ls", "--json");
            var list = lc == 0 ? ParseList(lt) : new List<SpaceInfo>();
            var ready = lc == 0 && ImagePulled();
            s = new(true, ready, version, lc != 0 ? $"Cua Spaces isn’t answering: {Line(lt) ?? "run cua doctor"}." : ready ? "" : "Prepare the desktop image once (a one-time download).", list.Count(x => x.Name.StartsWith("hover-", StringComparison.Ordinal) && x.Running));
        }
        lock (Lock) _known = (DateTime.UtcNow, s);
        Changed?.Invoke();
        return s;
    }

    // Hover remembers the image it prepared; Cua's cache keeps the image itself.
    private static string Marker => Path.Combine(Paths.Support, "spaces", "prepared-" + Image.Replace(':', '-'));
    private static bool ImagePulled() => File.Exists(Marker);

    public static void Cancel() { lock (Lock) _setup?.Cancel(); }

    /// One click: installs Cua's CLI and Spaces app if missing (Cua's own installer,
    /// no sign-in), sets up the runtime the image needs (Lume for macOS VMs), and makes
    /// and deletes one Space so the image is on the Mac before the first task.
    public static async Task RunSetup()
    {
        CancellationTokenSource cts;
        lock (Lock) { if (_setup is not null) return; _setup = cts = new(); }
        try
        {
            if (!Supported) throw new SetupError("Agent desktops need macOS 26 or later on Apple silicon.");
            if (Exe() is null)
            {
                Report(new("installing", "Installing Cua Spaces…", null, null));
                await Stream("/bin/bash", TimeSpan.FromMinutes(15), cts.Token, null, "-c",
                    "set -o pipefail; curl -fsSL https://cua.ai/install.sh | sh -s -- --yes --select cli,spaces --no-onboarding");
                if (Exe() is null) throw new SetupError("The installer finished, but cua still isn’t found.");
            }
            var cua = Exe()!;
            if (Image.StartsWith("macos", StringComparison.Ordinal))
            {
                Report(new("installing", "Setting up the macOS desktop runtime (Lume)…", null, null));
                await Stream(cua, TimeSpan.FromMinutes(15), cts.Token, null, "runtime", "setup", "lume");
            }
            Report(new("preparing", "Downloading the desktop image (one time; macOS is about 23 GB)…", 0, null));
            var probe = "hover-prepare";
            await Stream(cua, TimeSpan.FromMinutes(60), cts.Token, f => Report(new("preparing", f.Line, f.Fraction, null)), "spaces", "create", Image, "--name", probe, "--json");
            await Run(cua, TimeSpan.FromMinutes(2), "spaces", "delete", "local:" + probe, "--force");
            Directory.CreateDirectory(Path.GetDirectoryName(Marker)!);
            File.WriteAllText(Marker, DateTime.UtcNow.ToString("O"));
            Report(Idle);
        }
        catch (OperationCanceledException) { Report(Idle); }
        catch (SetupError e) { Report(new(null, "", null, e.Message)); }
        catch (Exception e) { Report(new(null, "", null, $"Setup stopped: {e.Message}")); }
        finally { lock (Lock) _setup = null; cts.Dispose(); _ = Check(fresh: true); }
    }

    private sealed class SetupError(string message) : Exception(message);

    // MARK: One Space per session

    public sealed record SpaceInfo(string Id, string Name, bool Running, string? Os);

    internal static List<SpaceInfo> ParseList(string json)
    {
        var list = new List<SpaceInfo>();
        var start = json.IndexOfAny(new[] { '[', '{' });
        if (start < 0) return list;
        try
        {
            using var doc = JsonDocument.Parse(json[start..]);
            var arr = doc.RootElement.ValueKind == JsonValueKind.Array ? doc.RootElement : doc.RootElement.TryGetProperty("spaces", out var sp) ? sp : default;
            if (arr.ValueKind != JsonValueKind.Array) return list;
            foreach (var e in arr.EnumerateArray())
            {
                var id = S(e, "id") ?? ""; var name = S(e, "name") ?? id.Split(':').Last();
                var power = (S(e, "power_state") ?? S(e, "state") ?? "running").ToLowerInvariant();
                list.Add(new(id, name, power is "running" or "ready" or "on", S(e, "os")));
            }
        }
        catch (JsonException) { }
        return list;
    }

    /// What each session's Space is doing, for its desk: "creating", "starting",
    /// "ready", "stopped" or "failed" with a reason.
    public sealed record SpaceState(string Phase, string Line, double? Fraction, string? Error);
    private static readonly ConcurrentDictionary<string, SpaceState> States = new();
    private static readonly ConcurrentDictionary<string, SemaphoreSlim> Gates = new();
    // Each Space by name, and the folder it is for (the bridge only knows the name).
    private static readonly ConcurrentDictionary<string, string> Folders = new();
    public static SpaceState? StateOf(string folder) => States.TryGetValue(NameFor(folder), out var s) ? s : null;
    private static void Set(string folder, SpaceState s)
    {
        var name = NameFor(folder);
        var phaseChanged = !States.TryGetValue(name, out var was) || was.Phase != s.Phase;
        States[name] = s;
        // A new phase at once; progress within one, a few times a second.
        if (phaseChanged) Changed?.Invoke(); else Notify();
    }

    /// The project's Space, made or started before an agent's run, with progress. Two
    /// agents starting together wait for one create. Returns null when it is ready;
    /// else why not (the run then goes on without a desktop).
    public static async Task<string?> Ensure(string folder, CancellationToken ct)
    {
        if (!Wanted || Exe() is not { } cua) return "Agent desktops are off.";
        var key = folder;
        Folders[NameFor(folder)] = folder;
        var gate = Gates.GetOrAdd(NameFor(folder), _ => new SemaphoreSlim(1, 1));
        await gate.WaitAsync(ct);
        try
        {
            var name = NameFor(folder);
            var size = Target();
            // Lume knows whether the VM is on; Cua's list only knows it was made.
            var vm = await Vm(name);
            var (lc, lt) = await Run(cua, TimeSpan.FromSeconds(30), "spaces", "ls", "--json");
            var known = lc == 0 && ParseList(lt).Any(x => x.Id == "local:" + name || x.Name == name);
            if (vm is { Exists: true, Running: true } || vm is null && known)
            {
                Set(key, new("ready", "The project’s desktop is ready.", 1, null)); return null;
            }
            if (vm is { Exists: true } || known)
            {
                // Off: sized up first if Cua made it small (only while it is off).
                if (vm is { Exists: true } v && (v.Cpus < size.Cpus || v.MemoryGb < size.MemoryGb || v.Display != size.Display) && LumeExe() is { } lume)
                {
                    Set(key, new("starting", "Giving the desktop more room…", null, null));
                    var (zc, zt) = await Run(lume, TimeSpan.FromMinutes(2), "set", name, "--cpu", size.Cpus.ToString(), "--memory", size.MemoryGb + "GB", "--display", size.Display);
                    if (zc != 0) Log.Line($"spaces: couldn't size {name}: {Line(zt)}");
                }
                Set(key, new("starting", "Starting the project’s desktop…", null, null));
                var (sc, st) = await Run(cua, TimeSpan.FromMinutes(4), "spaces", "start", "local:" + name, "--json");
                if (sc != 0) { var why = Explain(Line(st) ?? "The desktop didn’t start."); Set(key, new("failed", "", null, why)); return why; }
                Set(key, new("ready", "The project’s desktop is ready.", 1, null)); return null;
            }
            Set(key, new("creating", "Making the project’s desktop…", 0, null));
            string? error = null;
            try
            {
                await Stream(cua, TimeSpan.FromMinutes(ImagePulled() ? 10 : 60), ct, f => Set(key, new("creating", f.Line, f.Fraction, null)),
                    "spaces", "create", Image, "--name", name, "--cpus", size.Cpus.ToString(), "--memory-mb", (size.MemoryGb * 1024).ToString(), "--json");
                // The screen size isn't a create option: set while it is first off.
                if (LumeExe() is { } lume && (await Vm(name))?.Display != size.Display)
                {
                    await Run(cua, TimeSpan.FromMinutes(2), "spaces", "stop", "local:" + name);
                    await Run(lume, TimeSpan.FromMinutes(2), "set", name, "--display", size.Display);
                    await Run(cua, TimeSpan.FromMinutes(4), "spaces", "start", "local:" + name, "--json");
                }
            }
            catch (SetupError e) { error = e.Message; }
            if (error is null) { Set(key, new("ready", "The project’s desktop is ready.", 1, null)); return null; }
            error = Explain(error);
            Set(key, new("failed", "", null, error));
            return error;
        }
        finally { gate.Release(); }
    }

    private static string Explain(string error) =>
        // A Mac runs two macOS VMs at most: say so plainly.
        error.Contains("limit", StringComparison.OrdinalIgnoreCase) ? "This Mac already runs two macOS desktops (Apple’s limit). Remove the agents of another project to free one."
        : error.Contains("insufficient", StringComparison.OrdinalIgnoreCase) || error.Contains("memory", StringComparison.OrdinalIgnoreCase) ? "There isn’t enough free memory or disk for the desktop right now."
        : error;

    /// Off when no agent of the project is left in the office; deleted with the
    /// project's last session (the backend decides when).
    public static Task Stop(string folder) => Wanted && Exe() is { } cua ? Task.Run(async () =>
    {
        Viewers.TryRemove(NameFor(folder), out _);
        await Run(cua, TimeSpan.FromMinutes(2), "spaces", "stop", IdFor(folder));
        Set(folder, new("stopped", "The desktop is off. The project’s next task starts it again.", null, null));
    }) : Task.CompletedTask;
    public static Task Delete(string folder) => Exe() is { } cua ? Task.Run(async () => { await Run(cua, TimeSpan.FromMinutes(3), "spaces", "delete", IdFor(folder), "--force"); States.TryRemove(NameFor(folder), out _); Changed?.Invoke(); }) : Task.CompletedTask;

    /// The live viewer of the session's Space: Cua's own HTML5 viewer, interactive (the
    /// user can step in), with dropped files going to the Space's Downloads.
    // Each desktop's viewer link, kept while its ticket lasts: opening the panel again (or
    // another agent's of the same project) shows the same live view, not a reload.
    private static readonly ConcurrentDictionary<string, (string Url, DateTime Until)> Viewers = new();

    public static async Task<object> Viewer(string key)
    {
        if (Exe() is not { } cua) return new { error = "Cua Spaces isn’t installed." };
        if (Viewers.TryGetValue(NameFor(key), out var cached) && cached.Until > DateTime.UtcNow && StateOf(key) is { Phase: "ready" }) return new { phase = "ready", url = cached.Url };
        // On first: the viewer of a desktop that is off never loads.
        if (StateOf(key) is not { Phase: "creating" or "starting" } && await Ensure(key, CancellationToken.None) is { } why) return new { phase = "failed", error = why };
        if (StateOf(key) is { Phase: not "ready" } st) return new { phase = st.Phase, line = st.Line, fraction = st.Fraction, error = st.Error };
        var (code, text) = await Run(cua, TimeSpan.FromSeconds(45), "sb", "view", IdFor(key), "--no-open", "--ttl", "12h");
        var url = Regex.Match(text, @"https?://[^\s""']+/viewer/#[^\s""']+").Value;
        if (code != 0 || url.Length == 0) return new { error = Line(text) ?? "The desktop’s viewer didn’t open." };
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u) || u.Host is not ("127.0.0.1" or "localhost" or "[::1]") && !u.Host.StartsWith("192.168.", StringComparison.Ordinal) && !u.Host.StartsWith("10.", StringComparison.Ordinal))
            return new { error = "The viewer isn’t on this Mac." };
        Viewers[NameFor(key)] = (url, DateTime.UtcNow.AddHours(11));
        return new { phase = "ready", url };
    }

    /// An app dragged onto the notch, into the session's Space: Cua's teleport (its
    /// tabs and profile; sign-ins only after the user approves with Touch ID).
    public static async Task<object> Teleport(string key, string app, Action<string>? progress)
    {
        if (Exe() is not { } cua) return new { error = "Cua Spaces isn’t installed." };
        if (string.IsNullOrWhiteSpace(app) || app.StartsWith('-') || app.Length > 200) return new { error = "That app can’t be sent." };
        if (await Ensure(key, CancellationToken.None) is { } why) return new { error = why };
        // Teleport's consent (and Touch ID for sign-ins) is shown by Cua's own app; it is
        // opened in the background if it isn't running, never brought to the front.
        if (OperatingSystem.IsMacOS() && !Sandbox.Inside) await Run("/usr/bin/open", TimeSpan.FromSeconds(15), "-g", "-j", "-b", "com.trycua.spaces.macos");
        try
        {
            progress?.Invoke("Approve it in Cua’s window if it asks…");
            await Stream(cua, TimeSpan.FromMinutes(10), CancellationToken.None, f => progress?.Invoke(f.Line.StartsWith("progress ") ? "Sending… " + f.Line[9..].Replace(' ', '/') : f.Line), "teleport", "push", "--app", app, "--sandbox", IdFor(key), "--scope", "tabs", "--progress");
            return new { ok = true };
        }
        catch (SetupError e)
        {
            var m = e.Message;
            if (m.Contains("unsupported", StringComparison.OrdinalIgnoreCase) || m.Contains("no provider", StringComparison.OrdinalIgnoreCase))
                m = $"Cua can’t teleport {app} yet. It moves Chrome, Firefox, Slack, Discord, WhatsApp and a few others.";
            return new { error = m };
        }
    }

    /// Files dropped on a project's desktop in the notch: to its Downloads, through
    /// `cua sb cp` (folders file by file, at most 500 files).
    private static readonly ConcurrentDictionary<string, string> Homes = new();
    public static async Task<object> SendFiles(string key, IReadOnlyList<string> paths)
    {
        if (Exe() is not { } cua) return new { error = "Cua Spaces isn’t installed." };
        if (await Ensure(key, CancellationToken.None) is { } why) return new { error = why };
        var id = IdFor(key);
        if (!Homes.TryGetValue(id, out var home))
        {
            var (hc, ht) = await Run(cua, TimeSpan.FromSeconds(30), "sb", "exec", id, "echo", "$HOME");
            home = hc == 0 ? ht.Replace("\r", "").Split('\n').Select(l => l.Trim()).LastOrDefault(l => l.StartsWith('/')) ?? "" : "";
            if (home.Length == 0) return new { error = "The desktop didn’t answer." };
            Homes[id] = home;
        }
        var files = new List<(string From, string To)>();
        foreach (var p in paths.Take(20))
        {
            if (File.Exists(p)) files.Add((p, Path.GetFileName(p)));
            else if (Directory.Exists(p))
            {
                var root = Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(p)) ?? p;
                foreach (var f in Directory.EnumerateFiles(p, "*", new EnumerationOptions { RecurseSubdirectories = true, IgnoreInaccessible = true, AttributesToSkip = FileAttributes.ReparsePoint | FileAttributes.Hidden }).Take(500 - files.Count))
                    files.Add((f, Path.GetRelativePath(root, f).Replace('\\', '/')));
            }
        }
        var sent = 0;
        foreach (var (from, to) in files)
        {
            var (code, text) = await Run(cua, TimeSpan.FromMinutes(5), "sb", "cp", from, $"{id}:{home}/Downloads/{to}");
            if (code != 0) return new { error = Line(text) ?? "The files didn’t go.", sent };
            sent++;
        }
        return new { ok = true, sent };
    }

    // MARK: The session's computer-use server

    /// The MCP server a session's tool gets: Hover's relay (as for its browser) to a
    /// `cua mcp` Hover runs for that session's Space, outside the agents' sandbox.
    /// The project's Space for a run in that folder: every agent there gets the same
    /// one, each with its own cursor in it.
    public static IReadOnlyList<McpServer> Servers(string folder)
    {
        if (!Wanted || string.IsNullOrEmpty(folder) || Exe() is null) return Array.Empty<McpServer>();
        Folders[NameFor(folder)] = folder;
        return BrowserTool.Bridge("space:" + NameFor(folder), ServerName, Serve);
    }

    /// Computer use inside the Space, and its files; never its shell (the agent has its
    /// own) or Spaces' admin tools.
    internal const string Permissions = "computer:screenshot,computer:click,computer:type,computer:key,computer:scroll,computer:drag,computer:hotkey,computer:window,computer:accessibility,computer:clipboard";

    private static async Task Serve(string token, StreamReader fromAgent, Stream toAgent)
    {
        if (!Folders.TryGetValue(token["space:".Length..], out var key)) return;
        if (await Ensure(key, CancellationToken.None) is { } why) Log.Line($"spaces: {key}: {why}");
        if (Exe() is not { } cua) return;
        var psi = Quota.Hidden(cua, "mcp", "--sandbox", IdFor(key), "--permissions", Permissions);
        psi.Environment["CUA_TELEMETRY"] = "0";
        psi.StandardOutputEncoding = new UTF8Encoding(false);
        using var p = new Process { StartInfo = psi };
        p.Start();
        _ = Task.Run(async () => { try { while (await p.StandardError.ReadLineAsync() is { } l) Log.Line("cua mcp: " + l); } catch { } });
        var up = Task.Run(async () =>
        {
            try { while (await fromAgent.ReadLineAsync() is { } line) { await p.StandardInput.WriteLineAsync(line); await p.StandardInput.FlushAsync(); } }
            catch (IOException) { }
            try { p.StandardInput.Close(); } catch { }
        });
        try
        {
            var buf = new char[16384]; int n;
            var enc = new UTF8Encoding(false);
            while ((n = await p.StandardOutput.ReadAsync(buf)) > 0) { await toAgent.WriteAsync(enc.GetBytes(buf, 0, n)); await toAgent.FlushAsync(); }
        }
        catch (IOException) { }
        finally { try { if (!p.HasExited) p.Kill(entireProcessTree: true); } catch { } }
        await up;
    }

    // MARK: Running cua

    internal sealed record Frame(string Line, double? Fraction);
    private static readonly Regex Percent = new(@"(\d{1,3}(?:\.\d+)?)\s?%", RegexOptions.Compiled);
    private static readonly Regex Of = new(@"\b(\d+)\s+(\d+)\s*$", RegexOptions.Compiled);

    private static async Task Stream(string exe, TimeSpan timeout, CancellationToken ct, Action<Frame>? frame, params string[] args)
    {
        var psi = Quota.Hidden(exe, args);
        psi.Environment["CUA_TELEMETRY"] = "0";
        psi.Environment["CUA_INSTALL_NONINTERACTIVE"] = "1";
        psi.Environment["NONINTERACTIVE"] = "1";
        using var p = new Process { StartInfo = psi };
        var tail = "";
        void Line(string? raw)
        {
            if (raw is null) return;
            var l = Quota.StripAnsi(raw).Trim();
            if (l.Length == 0) return;
            tail = l;
            double? f = null;
            // A JSON progress line ({"phase":..,"fraction":..}), a percentage, or "<sent> <total>".
            if (l.StartsWith('{'))
                try
                {
                    using var d = JsonDocument.Parse(l);
                    if (d.RootElement.TryGetProperty("fraction", out var fr) && fr.ValueKind == JsonValueKind.Number) f = fr.GetDouble();
                    var phase = S(d.RootElement, "phase");
                    l = phase switch { "pulling" => "Downloading the desktop image…", "creating" => "Making the desktop…", "booting" => "Starting it up…", "waiting_for_services" or "connecting" => "Almost ready…", "ready" => "Ready.", _ => phase ?? l };
                }
                catch (JsonException) { }
            else if (Percent.Match(l) is { Success: true } m) f = double.Parse(m.Groups[1].Value, System.Globalization.CultureInfo.InvariantCulture) / 100;
            else if (Of.Match(l) is { Success: true } o && double.TryParse(o.Groups[2].Value, out var tot) && tot > 0) f = double.Parse(o.Groups[1].Value) / tot;
            frame?.Invoke(new(l.Length > 140 ? l[..139] + "…" : l, f));
            if (frame is null) Report(new(Setup.Step ?? "installing", l.Length > 140 ? l[..139] + "…" : l, f, null));
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
            throw new SetupError("It took too long and was stopped.");
        }
        if (p.ExitCode != 0) throw new SetupError(tail.Length > 0 ? tail : $"exit code {p.ExitCode}");
    }

    internal static async Task<(int Code, string Text)> Run(string exe, TimeSpan timeout, params string[] args)
    {
        try
        {
            var psi = Quota.Hidden(exe, args);
            psi.Environment["CUA_TELEMETRY"] = "0";
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

    private static string? Line(string? text) =>
        (text ?? "").Replace("\r", "").Split('\n').Select(l => l.Trim()).LastOrDefault(l => l.Length > 0) is { } l ? (l.Length > 200 ? l[..199] + "…" : l) : null;

    private static string? S(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
}

using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Core;

namespace Hover.Services;

/// An MCP server a session is given, started over stdio by the tool itself.
public sealed record McpServer(string Name, string Command, IReadOnlyList<string> Args);

/// What Hover knows of Cua Driver: installed or not, its version, and (on a Mac)
/// whether CuaDriver.app has the Accessibility and Screen Recording grants it needs.
/// Permissions is "granted", "partial" (Accessibility only), "missing" or "unknown";
/// always "granted" where there are none to give (Windows, Linux).
public sealed record ComputerUseStatus(bool Installed, string Version, string Permissions, string Hint)
{
    public bool Ready => Installed && Permissions is "granted" or "partial";
}

/// Computer use for the agents, through Cua Driver (github.com/trycua/cua, MIT): its
/// `cua-driver mcp` is handed to every session as an MCP server, so an agent can see
/// and drive apps (the app it is building, a browser, a simulator) in the background,
/// without the user's pointer moving or their focus changing. Off until switched on
/// in Settings. Hover never drives anything itself and never passes Cua's own
/// approval-bypass flags; each tool call still goes through the session's tool access
/// (Ask first asks about it in the notch, Read only refuses it).
///
/// On a Mac the grants belong to CuaDriver.app, not to Hover or the agent: the first
/// `cua-driver mcp` starts that app's daemon through LaunchServices in the background
/// and talks through it, so they are given once, to CuaDriver, with `permissions
/// grant`. Install and grant run the maker's own commands. No WPF in here.
public static class ComputerUse
{
    public const string ServerName = "cua-driver";
    public const string Repo = "github.com/trycua/cua";

    /// The cua-driver program, or null when it isn't installed. PATH first; then where
    /// the installers put it, for a Hover started before the install changed PATH.
    public static string? Exe()
    {
        if (Quota.OnPath("cua-driver") is { } found) return found;
        foreach (var path in Fallbacks())
            if (File.Exists(path)) return path;
        return null;
    }

    private static IEnumerable<string> Fallbacks()
    {
        if (OperatingSystem.IsWindows())
        {
            yield return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "Cua", "cua-driver", "bin", "cua-driver.exe");
            yield break;
        }
        yield return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".local", "bin", "cua-driver");
        if (OperatingSystem.IsMacOS()) yield return "/Applications/CuaDriver.app/Contents/MacOS/cua-driver";
    }

    /// The MCP servers a new session gets now: Cua Driver's, when computer use is on
    /// and it is installed; otherwise none. Where perl is (a Mac, Linux) it runs behind
    /// the guard (see Guard), so an agent's computer use never takes the user's pointer,
    /// keyboard or focus.
    public static IReadOnlyList<McpServer> Servers() =>
        Settings.ComputerUse && Exe() is { } exe ? new[] { Server(exe) } : Array.Empty<McpServer>();

    private const string Perl = "/usr/bin/perl";
    /// Where the guard is written: Hover's own folder, which the sandbox lets the agent
    /// read here but not write, so the agent can't edit its way past it.
    internal static string GuardDir => Path.Combine(Paths.Support, "cua");

    private static McpServer Server(string exe)
    {
        if (OperatingSystem.IsWindows() || !File.Exists(Perl)) return new McpServer(ServerName, exe, new[] { "mcp" });
        Directory.CreateDirectory(GuardDir);
        var guard = Path.Combine(GuardDir, "guard.pl");
        if (!File.Exists(guard) || File.ReadAllText(guard) != Guard) File.WriteAllText(guard, Guard);
        return new McpServer(ServerName, Perl, new[] { guard, exe, "mcp" });
    }

    /// Sits between the agent and `cua-driver mcp` and keeps its computer use out of the
    /// user's way: the user goes on working while an agent tests. Cua can act in the
    /// background (AX actions, events posted to one app, its own drawn cursor), but its
    /// ladder ends in "foreground", which fronts the window and moves the real pointer,
    /// and some tools act on the whole desktop. Here every input goes to the app the
    /// agent names, in the background: delivery_mode is taken out of the tool list and
    /// a "foreground" asked for anyway becomes "background"; input on the desktop scope
    /// or with no app named (it would land in whatever the user is typing in) is
    /// refused; so are bringing an app forward, moving or resizing windows, killing
    /// apps, the clipboard, replays and changing Cua's own settings. The initialize
    /// answer tells the agent so. Everything else passes through untouched, a line at a
    /// time (MCP's stdio framing), with blocking writes, as in Sandbox.Relay.
    internal const string Guard = """
#!/usr/bin/perl
# Hover's guard for cua-driver mcp: computer use stays in the background, out of the
# user's way. See ComputerUse.Guard in Hover's source.
use strict; use warnings;
use POSIX qw(:sys_wait_h EAGAIN EINTR);
use IO::Select;
use JSON::PP;
die "usage: guard.pl cua-driver mcp\n" unless @ARGV;
my $json = JSON::PP->new->utf8->canonical;
my %blocked = map { $_ => 1 } qw(bring_to_front set_window_frame kill_app clipboard_read clipboard_write
    replay_trajectory set_config escalate_session browser_prepare install_extension install_ffmpeg);
my %input = map { $_ => 1 } qw(click double_click right_click drag scroll press_key hotkey type_text set_value move_cursor);
my $note = "Hover runs computer use in the background so the user can keep working: every action goes to the app "
    . "you name (pid and window_id, or an element_token), never to the frontmost app, the user's pointer or keyboard. "
    . "Prefer element_token; x/y clicks are posted to the window. Foreground delivery, desktop-scope input, "
    . "bring_to_front, moving windows, killing apps and the clipboard are turned off. Launch apps with launch_app (it "
    . "stays in the background) and check results with get_window_state.";
my %listing;   # ids of the agent's tools/list and initialize requests, to fix their answers

pipe(my $in_r, my $in_w) or die "pipe: $!\n";
pipe(my $out_r, my $out_w) or die "pipe: $!\n";
my $pid = fork() // die "fork: $!\n";
if ($pid == 0) {
    close $in_w; close $out_r;
    open(STDIN, '<&', $in_r) or die "stdin: $!\n";
    open(STDOUT, '>&', $out_w) or die "stdout: $!\n";
    close $in_r; close $out_w;
    exec { $ARGV[0] } @ARGV or die "exec $ARGV[0]: $!\n";
}
close $in_r; close $out_w;
for my $sig (qw(TERM INT HUP)) { $SIG{$sig} = sub { kill $sig, $pid; } }
$SIG{PIPE} = 'IGNORE';

sub put {
    my ($fh, $s) = @_;
    while (length $s) {
        my $n = syswrite($fh, $s);
        if (!defined $n) { return 0 if $! != EAGAIN && $! != EINTR; IO::Select->new($fh)->can_write(1); next; }
        substr($s, 0, $n) = '';
    }
    return 1;
}
sub refuse {
    my ($id, $why) = @_;
    put(\*STDOUT, $json->encode({ jsonrpc => '2.0', id => $id, result => { isError => JSON::PP::true,
        content => [{ type => 'text', text => "$why $note" }] } }) . "\n");
}
sub named { my ($a) = @_; defined $a->{pid} || defined $a->{element_token} || (ref $a->{target} eq 'HASH' && defined $a->{target}{pid}) }
sub desktop { my ($a) = @_; ($a->{scope} // '') eq 'desktop' || (ref $a->{target} eq 'HASH' && defined $a->{target}{display_id}) }

# One message from the agent: undef when it was answered here, else the line to pass on.
sub from_agent {
    my ($m) = @_;
    return 1 unless ref $m eq 'HASH';
    my $method = $m->{method} // '';
    $listing{$m->{id}} = 1 if defined $m->{id} && ($method eq 'tools/list' || $method eq 'initialize');
    return 1 unless $method eq 'tools/call' && ref $m->{params} eq 'HASH';
    my $name = $m->{params}{name} // '';
    my $a = $m->{params}{arguments}; $a = $m->{params}{arguments} = {} unless ref $a eq 'HASH';
    if ($blocked{$name}) { refuse($m->{id}, "$name is turned off in Hover."); return undef; }
    return 1 unless $input{$name};
    if ($name eq 'move_cursor') {
        if (desktop($a)) { refuse($m->{id}, 'Moving the real pointer is turned off in Hover; the agent cursor moves without scope.'); return undef; }
        return 1;
    }
    if (desktop($a) || !named($a)) { refuse($m->{id}, "$name needs the app it acts on (pid and window_id, or an element_token)."); return undef; }
    $a->{delivery_mode} = 'background' if exists $a->{delivery_mode};
    return 2;
}

# One answer from cua-driver: its tool list without what is off, and the note.
sub from_driver {
    my ($m) = @_;
    return 0 unless ref $m eq 'HASH' && defined $m->{id} && delete $listing{$m->{id}} && ref $m->{result} eq 'HASH';
    my $r = $m->{result};
    if (ref $r->{tools} eq 'ARRAY') {
        $r->{tools} = [grep { !$blocked{$_->{name} // ''} } @{$r->{tools}}];
        for my $t (@{$r->{tools}}) {
            my $p = ref $t->{inputSchema} eq 'HASH' ? $t->{inputSchema}{properties} : undef;
            next unless ref $p eq 'HASH';
            delete $p->{delivery_mode};
            $p->{scope}{enum} = ['window'] if $input{$t->{name} // ''} && ref $p->{scope} eq 'HASH';
        }
    }
    $r->{instructions} = join("\n\n", grep { length } ($r->{instructions} // ''), $note) if defined $r->{protocolVersion};
    return 1;
}

my $sel = IO::Select->new(\*STDIN, $out_r);
my ($from_agent, $from_driver) = ('', '');
my $stdin_open = 1;
my $parent = getppid();
while ($sel->count) {
    if (getppid() != $parent) { kill 'TERM', $pid; last; }
    for my $fh ($sel->can_read(1)) {
        my $n = sysread($fh, my $chunk, 65536);
        if (!defined $n) { next if $! == EAGAIN || $! == EINTR; $n = 0; }
        if ($fh == $out_r) {
            if ($n == 0) { $sel->remove($out_r); put(\*STDOUT, $from_driver) if length $from_driver; $from_driver = ''; next; }
            $from_driver .= $chunk;
            while ((my $i = index($from_driver, "\n")) >= 0) {
                my $line = substr($from_driver, 0, $i + 1, '');
                # Only the answers to a listing are read; screenshots pass as they are.
                if (%listing && $line =~ /"id"/) {
                    my $m = eval { $json->decode($line) };
                    $line = $json->encode($m) . "\n" if $m && from_driver($m);
                }
                put(\*STDOUT, $line) or exit 1;
            }
        } else {
            if ($n == 0) { $sel->remove(\*STDIN); close $in_w; $stdin_open = 0; next; }
            $from_agent .= $chunk;
            while ((my $i = index($from_agent, "\n")) >= 0) {
                my $line = substr($from_agent, 0, $i + 1, '');
                my $m = $line =~ /\S/ ? eval { $json->decode($line) } : undef;
                if (ref $m eq 'ARRAY') {
                    # A batch: what is off is answered here, the rest goes on.
                    my @keep = grep { defined from_agent($_) } @$m;
                    next unless @keep;
                    $line = $json->encode(\@keep) . "\n";
                } elsif ($m) {
                    my $k = from_agent($m);
                    next unless defined $k;
                    $line = $json->encode($m) . "\n" if $k == 2;
                }
                put($in_w, $line) or do { $sel->remove(\*STDIN); $stdin_open = 0; last; };
            }
        }
    }
    last if !$sel->exists($out_r) && !$stdin_open;
    last if !$sel->exists($out_r) && waitpid($pid, WNOHANG) > 0;
}
close $in_w if $stdin_open;
waitpid($pid, 0);
exit($? & 127 ? 128 + ($? & 127) : $? >> 8);
""";

    /// The servers as ACP's session/new and session/load take them (stdio: name,
    /// command, args, and an env list, which ACP requires even when empty).
    public static object[] Acp(IReadOnlyList<McpServer> servers) =>
        servers.Select(s => (object)new { name = s.Name, command = s.Command, args = s.Args, env = Array.Empty<object>() }).ToArray();

    /// What tells a running tool its servers changed: when it differs from the one it
    /// started with, the tool is restarted once nothing of it runs (Hover's MCP
    /// servers are fixed per process for OpenCode and per session for ACP).
    public static string Signature(IReadOnlyList<McpServer> servers) =>
        string.Join("\n", servers.Select(s => s.Name + "\0" + s.Command + "\0" + string.Join("\0", s.Args)));

    /// OpenCode's inline config (OPENCODE_CONFIG_CONTENT, applied over the user's and
    /// the project's) with the servers added as local MCP servers. A config already in
    /// that variable is kept and added to; null when there is nothing to set.
    public static string? OpenCodeConfig(IReadOnlyList<McpServer> servers, string? existing)
    {
        if (servers.Count == 0) return existing;
        JsonObject root;
        try { root = (string.IsNullOrWhiteSpace(existing) ? null : JsonNode.Parse(existing) as JsonObject) ?? new JsonObject(); }
        catch (JsonException) { root = new JsonObject(); }
        if (root["mcp"] is not JsonObject mcp) root["mcp"] = mcp = new JsonObject();
        foreach (var s in servers)
            mcp[s.Name] = new JsonObject
            {
                ["type"] = "local",
                ["command"] = new JsonArray(new[] { s.Command }.Concat(s.Args).Select(a => (JsonNode)JsonValue.Create(a)!).ToArray()),
                ["enabled"] = true,
                // Its first call on a Mac may start CuaDriver's daemon; OpenCode's 5 s
                // default for listing tools is short for that.
                ["timeout"] = 30000,
            };
        return root.ToJsonString();
    }

    public static string InstallHint => OperatingSystem.IsWindows()
        ? "Install Cua Driver: irm https://cua.ai/driver/install.ps1 | iex"
        : "Install Cua Driver: /bin/bash -c \"$(curl -fsSL https://cua.ai/driver/install.sh)\"";

    // MARK: Status

    private static (DateTime At, ComputerUseStatus Status)? _known;
    private static Task<ComputerUseStatus>? _checking;
    private static readonly object Lock = new();

    /// The last check, without running one.
    public static ComputerUseStatus? Known
    {
        get { lock (Lock) return _known?.Status; }
    }

    /// Installed, its version and its grants, from cua-driver's own commands. Kept
    /// for five minutes; a check already going is shared.
    public static Task<ComputerUseStatus> Check(bool fresh = false)
    {
        lock (Lock)
        {
            if (!fresh && _known is { } k && DateTime.UtcNow - k.At < TimeSpan.FromMinutes(5)) return Task.FromResult(k.Status);
            if (_checking is { } going) return going;
            return _checking = Task.Run(async () =>
            {
                ComputerUseStatus s;
                try { s = await Look(); }
                catch (Exception e) { s = new(Exe() is not null, "", "unknown", e.Message); }
                lock (Lock) { _known = (DateTime.UtcNow, s); _checking = null; }
                Changed?.Invoke();
                return s;
            });
        }
    }

    private static async Task<ComputerUseStatus> Look()
    {
        if (Exe() is not { } exe) return new(false, "", "unknown", InstallHint);
        var (vc, vt) = await Run(exe, new[] { "--version" }, TimeSpan.FromSeconds(20));
        var version = vc == 0 ? vt.Trim().Split('\n').LastOrDefault()?.Trim() ?? "" : "";
        if (!OperatingSystem.IsMacOS()) return new(true, version, "granted", "");
        var permissions = await Permissions(exe);
        // Only a running daemon can answer for CuaDriver.app. With computer use on,
        // it is started (in the background, as cua-driver itself does) so the answer
        // is real rather than "unknown".
        if (permissions is null && Settings.ComputerUse && !await DaemonRunning(exe))
        {
            await Run("/usr/bin/open", new[] { "-n", "-g", "-a", "CuaDriver", "--args", "serve" }, TimeSpan.FromSeconds(20));
            for (var i = 0; i < 10 && permissions is null; i++)
            {
                await Task.Delay(TimeSpan.FromMilliseconds(500));
                permissions = await Permissions(exe);
            }
        }
        return permissions switch
        {
            (true, true) => new(true, version, "granted", ""),
            (true, false) => new(true, version, "partial", "Screen Recording isn’t granted to CuaDriver, so agents can read and act on windows but not see them."),
            (false, _) => new(true, version, "missing", "CuaDriver needs Accessibility and Screen Recording. Grant them once; agents can’t drive apps until then."),
            null => new(true, version, "unknown", "CuaDriver hasn’t been given Accessibility and Screen Recording yet (or hasn’t been asked). Grant them once."),
        };
    }

    /// Accessibility and Screen Recording, as CuaDriver's daemon reports them; null
    /// when it can't say (no daemon, or its permission gate still waiting).
    private static async Task<(bool Accessibility, bool ScreenRecording)?> Permissions(string exe)
    {
        var (code, text) = await Run(exe, new[] { "permissions", "status", "--json" }, TimeSpan.FromSeconds(20));
        if (code != 0) return null;
        return ParsePermissions(text);
    }

    /// Reads `cua-driver permissions status --json`. An "unknown" answer carries no
    /// booleans (cua-driver leaves them out rather than guess), and is null here.
    internal static (bool Accessibility, bool ScreenRecording)? ParsePermissions(string json)
    {
        var start = json.IndexOf('{');
        if (start < 0) return null;
        try
        {
            using var doc = JsonDocument.Parse(json[start..]);
            var r = doc.RootElement;
            if (r.ValueKind != JsonValueKind.Object || !r.TryGetProperty("accessibility", out var ax) || ax.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
                return null;
            var sr = r.TryGetProperty("screen_recording", out var s) && s.ValueKind == JsonValueKind.True;
            return (ax.ValueKind == JsonValueKind.True, sr);
        }
        catch (JsonException) { return null; }
    }

    /// CuaDriver's daemon is up before a sandboxed agent's cua-driver looks for it: it
    /// can't start the daemon itself from inside the sandbox (no Launch Services), so
    /// Hover does, in the background (-g) as cua-driver would. A Mac's; a no-op elsewhere.
    public static async Task EnsureDaemon()
    {
        if (!OperatingSystem.IsMacOS() || Exe() is not { } exe || await DaemonRunning(exe)) return;
        await Run("/usr/bin/open", new[] { "-n", "-g", "-a", "CuaDriver", "--args", "serve" }, TimeSpan.FromSeconds(20));
        for (var i = 0; i < 20 && !await DaemonRunning(exe); i++) await Task.Delay(TimeSpan.FromMilliseconds(250));
    }

    private static async Task<bool> DaemonRunning(string exe)
    {
        var (code, text) = await Run(exe, new[] { "status" }, TimeSpan.FromSeconds(15));
        return code == 0 && text.Contains("is running", StringComparison.OrdinalIgnoreCase) && !text.Contains("not running", StringComparison.OrdinalIgnoreCase);
    }

    // MARK: Install and grant

    /// What a setup is doing ("installing" or "granting"), its newest line, and why
    /// it stopped if it failed.
    public sealed record Progress(string? Step, string Line, string? Error);

    public static readonly Progress Idle = new(null, "", null);
    private static Progress _progress = Idle;
    private static CancellationTokenSource? _running;

    /// Raised off the caller's thread whenever the status or a setup's progress changes.
    public static event Action? Changed;

    public static Progress Setup
    {
        get { lock (Lock) return _progress; }
    }

    public static bool Busy
    {
        get { lock (Lock) return _running is not null; }
    }

    /// Granting is a Mac's: elsewhere there is nothing to grant.
    public static bool CanGrant => OperatingSystem.IsMacOS();

    private static void Report(Progress p)
    {
        lock (Lock) _progress = p;
        Changed?.Invoke();
    }

    /// Installs Cua Driver with its maker's installer (CuaDriver.app in /Applications
    /// and cua-driver in ~/.local/bin on a Mac; %LOCALAPPDATA%\Programs\Cua on
    /// Windows), then, on a Mac, asks for its grants.
    public static Task Install() => Go("installing", "Installing Cua Driver…", async ct =>
    {
        var (exe, args) = OperatingSystem.IsWindows()
            ? ("powershell.exe", new[] { "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm https://cua.ai/driver/install.ps1 | iex" })
            // Its PATH line isn't added to the user's shell files: ~/.local/bin is on
            // Hover's PATH already, and the tools find cua-driver through Hover.
            : ("/bin/bash", new[] { "-c", "set -o pipefail; curl -fsSL https://cua.ai/driver/install.sh | bash -s -- --no-modify-path" });
        await Stream("installing", exe, args, TimeSpan.FromMinutes(10), "Couldn’t install Cua Driver", ct);
        var s = await Check(fresh: true);
        if (!s.Installed) throw new SetupError("The installer finished, but cua-driver still isn’t found. " + InstallHint);
        if (CanGrant && s.Permissions is not ("granted" or "partial")) await GrantSteps(ct);
    });

    /// Asks macOS for CuaDriver's Accessibility and Screen Recording (its own grant
    /// command; the dialogs name CuaDriver), and waits up to three minutes for them.
    public static Task Grant() => Go("granting", "Approve CuaDriver in the dialogs and in System Settings…", GrantSteps);

    private static async Task GrantSteps(CancellationToken ct)
    {
        if (!CanGrant) return;
        if (Exe() is not { } exe) throw new SetupError(InstallHint);
        Report(new("granting", "Approve CuaDriver in the dialogs and in System Settings → Privacy & Security…", null));
        try { await Stream("granting", exe, new[] { "permissions", "grant" }, TimeSpan.FromMinutes(4), "Permissions weren’t granted", ct); }
        finally { await Check(fresh: true); }
    }

    public static void Cancel()
    {
        lock (Lock) _running?.Cancel();
    }

    private static async Task Go(string step, string line, Func<CancellationToken, Task> work)
    {
        CancellationTokenSource cts;
        lock (Lock)
        {
            if (_running is not null) return;
            _running = cts = new CancellationTokenSource();
        }
        Report(new(step, line, null));
        try { await work(cts.Token); Report(Idle); }
        catch (OperationCanceledException) { Report(Idle); }
        catch (SetupError e) { Report(new(null, "", e.Message)); }
        catch (Exception e) { Report(new(null, "", $"Setup stopped: {e.Message}")); }
        finally
        {
            lock (Lock) _running = null;
            cts.Dispose();
            Changed?.Invoke();
        }
    }

    private sealed class SetupError(string message) : Exception(message);

    /// Runs a step with no stdin, showing its newest line; fails with its last lines.
    private static async Task Stream(string step, string exe, string[] args, TimeSpan timeout, string failure, CancellationToken ct)
    {
        var psi = new ProcessStartInfo(exe, args)
        {
            RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
            UseShellExecute = false, CreateNoWindow = true,
            WorkingDirectory = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        };
        psi.Environment["CI"] = "1";
        psi.Environment["NO_COLOR"] = "1";
        psi.Environment["TERM"] = "dumb";
        using var p = new Process { StartInfo = psi };
        var tail = new Queue<string>();
        void Line(string? raw)
        {
            if (raw is null) return;
            var l = Quota.StripAnsi(raw).Trim();
            if (l.Length == 0) return;
            lock (tail) { tail.Enqueue(l); while (tail.Count > 6) tail.Dequeue(); }
            Report(new(step, l.Length > 120 ? l[..119] + "…" : l, null));
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
            throw new SetupError($"{failure}: it took longer than {timeout.TotalMinutes:0} minutes and was stopped.");
        }
        if (p.ExitCode != 0)
        {
            string last;
            lock (tail) last = string.Join(" · ", tail.TakeLast(2));
            throw new SetupError($"{failure}: {(last.Length > 0 ? last : $"exit code {p.ExitCode}")}");
        }
    }

    /// One hidden run of a short command: its exit code and everything it printed.
    private static async Task<(int Code, string Text)> Run(string exe, string[] args, TimeSpan timeout)
    {
        try
        {
            using var p = new Process { StartInfo = Quota.Hidden(exe, args) };
            p.Start();
            p.StandardInput.Close();
            var output = p.StandardOutput.ReadToEndAsync();
            var error = p.StandardError.ReadToEndAsync();
            using var cts = new CancellationTokenSource(timeout);
            try { await p.WaitForExitAsync(cts.Token); }
            catch (OperationCanceledException)
            {
                try { p.Kill(entireProcessTree: true); } catch { }
                return (-1, "");
            }
            return (p.ExitCode, Quota.StripAnsi(await output + "\n" + await error));
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException or InvalidOperationException)
        {
            return (-1, e.Message);
        }
    }
}

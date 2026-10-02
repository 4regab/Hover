using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Text.RegularExpressions;
using Hover.Core;

namespace Hover.Services;

/// Every agent tool runs inside Anthropic's sandbox-runtime (srt, Apache-2.0,
/// github.com/anthropics/sandbox-runtime), the sandbox Claude Code runs commands in:
/// sandbox-exec with a generated profile on a Mac, bubblewrap on Linux. It works in
/// the background without getting in the way of the person at the computer:
///   - writes only to the folders its sessions work in, the tool's own state and
///     caches, and temp files; nothing else of the user's can be changed;
///   - keys, keychains, mail, messages, browsers' data and other apps' data (Office's
///     included) can't be read, nor Hover's own;
///   - no window server and no Apple Events (srt's macOS profile has neither): no
///     window drawn, no focus taken, no app launched or scripted;
///   - the network only through srt's proxy, to the tool's own service, package
///     registries and GitHub, plus the hosts in allowed-domains.txt.
/// Computer use still works: the agent's cua-driver connects to CuaDriver's daemon,
/// which Hover starts outside the sandbox, over its one socket, and drives apps in the
/// background as it always does.
///
/// The folders are fixed when the tool starts, so a tool started for some is started
/// again (when nothing of it runs) for a session in another. Off with Settings.Sandbox;
/// never nested inside another sandbox (HOVER_SANDBOXED=1). macOS and Linux; srt's
/// Windows support is an alpha that can't reach tools installed for the user, which is
/// where Kiro, Cursor and OpenCode go, so it isn't used there. No WPF in here.
public static class Sandbox
{
    /// The srt release Hover was checked against, and installs.
    public const string Version = "0.0.78";
    public const string Package = "@anthropic-ai/sandbox-runtime";

    public static bool Supported => OperatingSystem.IsMacOS() || OperatingSystem.IsLinux();

    /// Hover itself runs in a sandbox already (scripts/sandbox.sh, the tests): another
    /// one inside it would fail, and the outer one holds.
    public static bool Inside => Environment.GetEnvironmentVariable("HOVER_SANDBOXED") == "1";

    /// Whether tools are to be started in it.
    public static bool Wanted => Settings.Sandbox && Supported && !Inside;

    public static string? Exe()
    {
        if (Quota.OnPath("srt") is { } found) return found;
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        foreach (var p in new[] { "/opt/homebrew/bin/srt", "/usr/local/bin/srt", Path.Combine(home, ".npm-global", "bin", "srt"), Path.Combine(home, ".local", "bin", "srt") })
            if (File.Exists(p)) return p;
        return null;
    }

    private static string? Tool(string name, params string[] fallbacks) =>
        Quota.OnPath(name) ?? fallbacks.FirstOrDefault(File.Exists);

    /// What is missing for the sandbox to run, as one line to show; null when nothing.
    public static string? Missing()
    {
        if (!Wanted) return null;
        var need = new List<string>();
        if (Exe() is null) need.Add($"npm install -g {Package}@{Version}");
        // srt finds the paths it must keep closed with ripgrep (and on Linux needs
        // bubblewrap and socat for the sandbox itself).
        if (Tool("rg", "/opt/homebrew/bin/rg", "/usr/local/bin/rg") is null) need.Add(OperatingSystem.IsMacOS() ? "brew install ripgrep" : "install ripgrep");
        if (OperatingSystem.IsLinux() && Tool("bwrap", "/usr/bin/bwrap") is null) need.Add("install bubblewrap");
        if (OperatingSystem.IsLinux() && Tool("socat", "/usr/bin/socat") is null) need.Add("install socat");
        return need.Count == 0 ? null
            : $"Hover runs agents in a sandbox, which isn’t set up yet: {string.Join(", then ", need)}. (Or turn the sandbox off in Settings.)";
    }

    // MARK: Folders

    private static readonly HashSet<string> Seen = new(StringComparer.Ordinal);

    /// A folder a session works in, so the next start of its tool covers it.
    public static void Remember(string folder)
    {
        if (!KiroRunner.UsableFolder(folder)) return;
        lock (Seen) Seen.Add(Full(folder));
    }

    /// The folders a tool started now gets: the ones sessions use this run, and the
    /// folder new tasks start in.
    public static IReadOnlyList<string> Folders()
    {
        var all = new List<string>();
        lock (Seen) all.AddRange(Seen);
        if (KiroRunner.UsableFolder(Settings.KiroFolder)) all.Add(Full(Settings.KiroFolder!));
        return all.Distinct(StringComparer.Ordinal).Order(StringComparer.Ordinal).ToList();
    }

    /// The folder is one of them, or inside one.
    public static bool Covers(IReadOnlyCollection<string> folders, string folder)
    {
        var f = Full(folder);
        return folders.Any(x => f == x || f.StartsWith(x.TrimEnd('/') + "/", StringComparison.Ordinal));
    }

    private static string Full(string folder) => Path.GetFullPath(folder).TrimEnd('/') is { Length: > 0 } f ? f : "/";

    // MARK: The policy

    private static string Home => Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

    /// Hosts every tool may reach: package registries, GitHub, and the local machine
    /// (dev servers, the tool's own local services).
    internal static readonly string[] DevDomains =
    {
        "localhost", "github.com", "*.github.com", "*.githubusercontent.com", "*.githubassets.com",
        "registry.npmjs.org", "*.npmjs.org", "*.npmjs.com", "registry.yarnpkg.com", "*.yarnpkg.com",
        "pypi.org", "*.pypi.org", "files.pythonhosted.org", "crates.io", "*.crates.io",
        "proxy.golang.org", "sum.golang.org", "api.nuget.org", "*.nuget.org", "rubygems.org", "*.rubygems.org",
        "repo.maven.apache.org", "repo1.maven.org", "services.gradle.org", "plugins.gradle.org",
        "jsr.io", "deno.land", "bun.sh", "nodejs.org",
    };

    /// Each tool's own service (sign-in, models, telemetry it can't run without).
    internal static string[] ToolDomains(AgentTool t) => t switch
    {
        AgentTool.Kiro => new[] { "kiro.dev", "*.kiro.dev", "*.amazonaws.com", "*.awsapps.com", "*.amazoncognito.com", "*.aws.amazon.com", "*.aws.dev" },
        AgentTool.Codex => new[] { "api.openai.com", "*.openai.com", "chatgpt.com", "*.chatgpt.com", "*.oaiusercontent.com" },
        AgentTool.Cursor => new[] { "cursor.com", "*.cursor.com", "cursor.sh", "*.cursor.sh", "*.cursorapi.com" },
        // OpenCode brings the user's own providers.
        AgentTool.OpenCode => new[]
        {
            "opencode.ai", "*.opencode.ai", "models.dev", "api.anthropic.com", "api.openai.com", "openrouter.ai", "*.openrouter.ai",
            "generativelanguage.googleapis.com", "*.githubcopilot.com", "api.x.ai", "api.groq.com", "api.mistral.ai", "api.deepseek.com",
            "api.together.xyz", "api.fireworks.ai", "api.cerebras.ai", "openai.azure.com", "*.openai.azure.com",
        },
        _ => Array.Empty<string>(),
    };

    /// The user's own: one host per line in allowed-domains.txt (written, explained,
    /// the first time it's needed), # for comments.
    internal static string ExtraFile => Path.Combine(Paths.Support, "sandbox", "allowed-domains.txt");

    private static readonly Regex Host = new(@"^(\*\.)?[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+(:\d{1,5})?$|^localhost(:\d{1,5})?$", RegexOptions.Compiled);

    internal static IEnumerable<string> Extra()
    {
        try
        {
            if (!File.Exists(ExtraFile))
            {
                Directory.CreateDirectory(Path.GetDirectoryName(ExtraFile)!);
                File.WriteAllText(ExtraFile,
                    "# Hosts Hover's sandboxed agents may reach, beyond their own service, package\n" +
                    "# registries and GitHub. One per line, like docs.example.com or *.example.com.\n" +
                    "# A tool picks a change up when it next starts.\n");
            }
            return File.ReadAllLines(ExtraFile).Select(l => l.Split('#')[0].Trim()).Where(l => Host.IsMatch(l)).ToList();
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return Array.Empty<string>(); }
    }

    /// Where a tool keeps its own sign-in, settings, logs and caches: writable.
    internal static IEnumerable<string> ToolState(AgentTool t)
    {
        IEnumerable<string> own = t switch
        {
            AgentTool.Kiro => new[] { "~/.kiro", "~/.aws/sso", "~/.aws/cli", "~/Library/Application Support/kiro-cli", "~/.local/share/kiro-cli", "~/.config/kiro-cli" },
            AgentTool.Codex => new[] { "~/.codex" },
            AgentTool.Cursor => new[] { "~/.cursor", "~/.config/cursor", "~/Library/Application Support/Cursor", "~/.local/share/cursor-agent" },
            AgentTool.OpenCode => new[] { "~/.local/share/opencode", "~/.local/state/opencode", "~/.config/opencode", "~/.cache/opencode" },
            _ => Array.Empty<string>(),
        };
        // What builds and package managers the agents run write to.
        var shared = new[]
        {
            "~/.cache", "~/.npm", "~/.local/state", "~/Library/Caches", "~/Library/Logs", "~/.nuget", "~/.dotnet", "~/.cargo/registry",
            "~/.cargo/git", "~/go/pkg", "~/.gradle", "~/.m2", "~/.bun", "~/.yarn", "~/.pnpm-store", "~/Library/pnpm", "~/.deno",
        };
        return own.Concat(shared).Select(p => p.StartsWith("~/", StringComparison.Ordinal) ? Path.Combine(Home, p[2..]) : p);
    }

    /// What no tool may read.
    internal static IEnumerable<string> Private()
    {
        var p = new List<string>
        {
            "~/.ssh", "~/.gnupg", "~/.netrc", "~/.config/gh", "~/.docker/config.json", "~/.kube", "~/.password-store",
            // Not ~/Library/Keychains: a file keychain is opened in-process (Cursor keeps
            // its sign-in there), and its items stay locked behind securityd and their
            // own access lists.
            "~/Library/Mail", "~/Library/Messages", "~/Library/Safari", "~/Library/Cookies",
            "~/Library/Containers", "~/Library/Group Containers", "~/Library/Calendars", "~/Library/Application Support/AddressBook",
            "~/Library/Application Support/com.apple.TCC", "~/Library/Application Support/Google/Chrome",
            "~/Library/Application Support/BraveSoftware", "~/Library/Application Support/Microsoft Edge",
            "~/Library/Application Support/Firefox", "~/Library/Application Support/Arc", "~/.mozilla", "~/.config/google-chrome",
        };
        var hover = Full(Paths.Support);
        return p.Select(x => Path.Combine(Home, x[2..])).Append(hover);
    }

    /// srt's settings for a tool started for these folders.
    internal static string Config(AgentTool tool, IReadOnlyList<string> folders, string temp, IReadOnlyList<string> sockets)
    {
        var darwinTemp = OperatingSystem.IsMacOS() ? DarwinTemp() : null;
        var config = new Dictionary<string, object?>
        {
            ["network"] = new Dictionary<string, object?>
            {
                ["allowedDomains"] = ToolDomains(tool).Concat(DevDomains).Concat(Extra()).Distinct(StringComparer.OrdinalIgnoreCase).ToList(),
                ["deniedDomains"] = Array.Empty<string>(),
                // Dev servers the agent starts, and test runs against them.
                ["allowLocalBinding"] = true,
                ["allowUnixSockets"] = sockets,
            },
            ["filesystem"] = new Dictionary<string, object?>
            {
                ["denyRead"] = Private().ToList(),
                // Images pasted into a prompt are kept in Hover's folder, and the prompt
                // names them for the agent to read; Cua Driver runs behind Hover's guard.
                // Hover's browser relay too (Services.BrowserTool), readable, not writable.
                ["allowRead"] = folders.Append(Path.Combine(Full(Paths.Support), "kiro-images")).Append(Full(ComputerUse.GuardDir)).Append(Full(BrowserTool.Dir)).ToList(),
                ["allowWrite"] = folders.Concat(ToolState(tool)).Append(temp).Concat(darwinTemp is null ? Array.Empty<string>() : new[] { darwinTemp }).ToList(),
                // A folder's .git/hooks and .git/config, shell rc files and the like are
                // closed by srt itself.
                ["denyWrite"] = Array.Empty<string>(),
            },
            ["allowAppleEvents"] = false,
            // macOS's certificate service: without it .NET, Go (gh) and Security-
            // framework TLS can't verify a certificate, and the tools can't sign in.
            ["enableWeakerNetworkIsolation"] = OperatingSystem.IsMacOS(),
            // Terminals the agent opens for commands.
            ["allowPty"] = true,
        };
        return JsonSerializer.Serialize(config, new JsonSerializerOptions { WriteIndented = true });
    }

    private static string? DarwinTemp()
    {
        // Apple's own tools (sips, xcrun, codesign) write here whatever TMPDIR says.
        var t = Environment.GetEnvironmentVariable("TMPDIR");
        return t is { Length: > 0 } && t.StartsWith("/var/folders/", StringComparison.Ordinal) ? t.TrimEnd('/') : null;
    }

    /// CuaDriver's daemon socket, which the agent's cua-driver talks to.
    internal static string CuaSocket => Path.Combine(Home, "Library", "Caches", "cua-driver", "cua-driver.sock");

    // MARK: Starting a tool in it

    private const string Perl = "/usr/bin/perl";
    /// srt (Node) hands the tool its own stdio, non-blocking, so a write bigger than
    /// the pipe's 64 KB buffer fails with EAGAIN and the tool dies. Cua Driver's tool
    /// list is 67 KB, so with computer use on Kiro quit at its first session
    /// ("failed to forward the v3 engine's output to ACP stdout"), and cua-driver
    /// itself with "os error 35". This relay runs the tool on blocking pipes of its own
    /// and copies them across, waiting when a pipe is full. It ends when the tool does,
    /// and closes the tool's stdin when Hover closes its own.
    internal const string Relay = """
#!/usr/bin/perl
# Runs a tool with pipes of its own and copies them to this process's stdin/stdout.
# srt (Node) shares its stdio with the tool and makes it non-blocking, so a tool's
# write larger than the pipe's 64 KB buffer fails with EAGAIN and the tool dies
# (Kiro: "failed to forward the v3 engine's output"; cua-driver: os error 35). Here
# every read and write waits for its fd, so nothing is lost and nothing fails.
use strict; use warnings;
use POSIX qw(:sys_wait_h EAGAIN EINTR);
use IO::Select;
die "usage: relay.pl tool [args...]\n" unless @ARGV;
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
# A stop for this process is a stop for the tool.
for my $sig (qw(TERM INT HUP)) { $SIG{$sig} = sub { kill $sig, $pid; } }
$SIG{PIPE} = 'IGNORE';
my $sel = IO::Select->new(\*STDIN, $out_r);
my ($to_tool, $to_host) = ('', '');
my $stdin_open = 1;
sub flush_to {
    my ($fh, $buf) = @_;
    while (length $$buf) {
        my $n = syswrite($fh, $$buf);
        if (!defined $n) {
            return 0 if $! != EAGAIN && $! != EINTR;
            IO::Select->new($fh)->can_write(1);
            next;
        }
        substr($$buf, 0, $n) = '';
    }
    return 1;
}
my $parent = getppid();
while ($sel->count) {
    # Whoever started it went away (srt killed): the tool goes too.
    if (getppid() != $parent) { kill 'TERM', $pid; last; }
    my @ready = $sel->can_read(1);
    for my $fh (@ready) {
        my $n = sysread($fh, my $chunk, 65536);
        if (!defined $n) { next if $! == EAGAIN || $! == EINTR; $n = 0; }
        if ($fh == $out_r) {
            if ($n == 0) { $sel->remove($out_r); next; }
            $to_host .= $chunk;
            flush_to(\*STDOUT, \$to_host) or exit 1;
        } else {
            if ($n == 0) { $sel->remove(\*STDIN); close $in_w; $stdin_open = 0; next; }
            $to_tool .= $chunk;
            flush_to($in_w, \$to_tool) or do { $sel->remove(\*STDIN); $stdin_open = 0; };
        }
    }
    last if !$sel->exists($out_r) && !$stdin_open;
    last if !$sel->exists($out_r) && waitpid($pid, WNOHANG) > 0;
}
close $in_w if $stdin_open;
waitpid($pid, 0);
exit($? & 127 ? 128 + ($? & 127) : $? >> 8);
""";


    /// The tool's start, inside srt when the sandbox is wanted: srt with its settings,
    /// then the tool with its own arguments and environment. Unchanged otherwise.
    /// Temp files and Unix sockets go in a short folder of the tool's own (sockets
    /// have a 104-byte path limit), the only place sockets work besides CuaDriver's.
    public static ProcessStartInfo Wrap(ProcessStartInfo psi, AgentTool tool, IReadOnlyList<string> folders)
    {
        if (!Wanted) return psi;
        var srt = Exe() ?? throw new InvalidOperationException(Missing() ?? "srt isn’t installed.");
        var dir = Path.Combine(Paths.Support, "sandbox");
        Directory.CreateDirectory(dir);
        var root = Environment.GetEnvironmentVariable("HOVER_SANDBOX_TMP") is { Length: > 0 } r ? r
            : OperatingSystem.IsMacOS() ? "/private/tmp/claude" : Path.Combine(Path.GetTempPath(), "claude");
        var temp = Path.Combine(root, $"hover-{Agents.Id(tool)}");
        Directory.CreateDirectory(temp);
        var sockets = new List<string> { temp };
        if (OperatingSystem.IsMacOS() && Settings.ComputerUse) sockets.Add(CuaSocket);
        // Hover's browser: the relay the agent's tool starts talks to Hover over this one socket.
        if (BrowserTool.Available && Settings.AgentBrowser || Spaces.Wanted) sockets.Add(BrowserTool.SocketPath);
        var file = Path.Combine(dir, $"{Agents.Id(tool)}.json");
        File.WriteAllText(file, Config(tool, folders, temp, sockets));
        if (!OperatingSystem.IsWindows()) File.SetUnixFileMode(file, UnixFileMode.UserRead | UnixFileMode.UserWrite);

        // srt quotes each argument and runs them with bash -c; env puts the tool's
        // temp folder back, which srt points at its own. The relay gives the tool pipes
        // of its own (see Relay).
        var args = new List<string> { "--settings", file, "--", "/usr/bin/env", "TMPDIR=" + temp + "/" };
        if (File.Exists(Perl)) { var relay = Path.Combine(temp, "relay.pl"); File.WriteAllText(relay, Relay); args.AddRange(new[] { Perl, relay }); }
        args.Add(psi.FileName);
        args.AddRange(psi.ArgumentList);
        var boxed = Quota.Hidden(srt, args.ToArray());
        boxed.WorkingDirectory = psi.WorkingDirectory;
        foreach (var (k, v) in psi.Environment) boxed.Environment[k] = v;
        // A dotnet build the agent runs stays in one process: MSBuild's worker nodes
        // talk over sockets in /tmp that the sandbox can't open without opening every
        // socket there (ssh-agent's among them). The variable tells the agent why.
        boxed.Environment["HOVER_SANDBOXED"] = "1";
        boxed.Environment["MSBUILDDISABLENODEREUSE"] = "1";
        boxed.Environment["DOTNET_CLI_USE_MSBUILD_SERVER"] = "0";
        boxed.Environment["UseSharedCompilation"] = "false";
        Log.Line($"sandbox: {Agents.Name(tool)} starts in srt for {folders.Count} folder(s)");
        return boxed;
    }
}

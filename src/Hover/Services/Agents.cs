using System.Diagnostics;
using System.IO;
using Hover.Core;

namespace Hover.Services;

/// The coding agents the office can hand a task to. Kiro, Codex and Cursor speak ACP
/// (the Agent Client Protocol: JSON-RPC over stdin and stdout), Codex through its ACP
/// adapter. OpenCode runs as its own local server (opencode serve), spoken to over
/// HTTP and its event stream, as T3 Code does. New tools go at the end: the names are
/// saved in settings and history.
public enum AgentTool { Kiro, Codex, Cursor, OpenCode }

/// Whether a tool can take a task now. Hint says what to do when it can't.
public sealed record AgentReady(bool Installed, bool SignedIn, string Hint)
{
    public bool Ok => Installed && SignedIn;
}

/// How to find, start and check each tool. No WPF in here.
public static class Agents
{
    public static readonly IReadOnlyList<AgentTool> All = new[] { AgentTool.Kiro, AgentTool.Codex, AgentTool.Cursor, AgentTool.OpenCode };

    public static string Name(AgentTool t) => t.ToString();
    public static string Id(AgentTool t) => t.ToString().ToLowerInvariant();

    public static AgentTool? Parse(string? id) => All.Cast<AgentTool?>().FirstOrDefault(t => Id(t!.Value) == id);

    /// The oldest OpenCode whose server API Hover was checked against (T3 Code's floor).
    public const string OpenCodeMinVersion = "1.14.19";

    /// The program that runs the tool, or null when it isn't installed.
    public static string? Exe(AgentTool t) => t switch
    {
        AgentTool.Kiro => Quota.OnPath("kiro-cli"),
        AgentTool.Codex => Quota.OnPath("codex-acp"),
        // Cursor's installer puts it here and adds the folder to PATH, but a Hover
        // started before the install has the old PATH. Its "agent" alias is not used:
        // other tools (Grok) install an "agent" too.
        AgentTool.Cursor => File.Exists(CursorShim) ? CursorShim : Quota.OnPath("cursor-agent"),
        AgentTool.OpenCode => OpenCodeExe(),
        _ => null,
    };

    private static string CursorShim => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "cursor-agent", "cursor-agent.cmd");

    /// OpenCode's own exe. npm installs a .cmd shim that runs it; going to the exe it
    /// points at saves a cmd.exe per server and keeps the server Hover's direct child.
    private static string? OpenCodeExe()
    {
        var found = Quota.OnPath("opencode");
        if (found is null || !found.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase)) return found;
        var exe = Path.Combine(Path.GetDirectoryName(found)!, "node_modules", "opencode-ai", "bin", "opencode.exe");
        return File.Exists(exe) ? exe : found;
    }

    public static string[] Arguments(AgentTool t) => t switch
    {
        // v3 is the engine with ACP sessions that load; "cli" keeps the sign-in inside
        // kiro-cli rather than asking Hover for tokens.
        AgentTool.Kiro => new[] { "acp", "--agent-engine", "v3", "--auth-method", "cli" },
        AgentTool.Codex => Array.Empty<string>(),
        AgentTool.Cursor => new[] { "acp" },
        // Local only: this address, a port the system picks, and never announced on
        // the network (mDNS), whatever the user's opencode config says.
        AgentTool.OpenCode => new[] { "serve", "--hostname=127.0.0.1", "--port=0", "--mdns=false" },
        _ => Array.Empty<string>(),
    };

    /// What it takes to install the tool, for the greyed-out choice.
    public static string InstallHint(AgentTool t) => t switch
    {
        AgentTool.Kiro => "Install kiro-cli from kiro.dev/cli.",
        AgentTool.Codex => "Install Codex and its ACP adapter: npm i -g @openai/codex @agentclientprotocol/codex-acp",
        AgentTool.Cursor => "Install the Cursor CLI: irm 'https://cursor.com/install?win32=true' | iex",
        AgentTool.OpenCode => $"Install OpenCode {OpenCodeMinVersion} or newer from opencode.ai.",
        _ => "",
    };

    public static string SignInHint(AgentTool t) => t switch
    {
        AgentTool.Kiro => "Sign in: run “kiro-cli login” in a terminal.",
        AgentTool.Codex => "Sign in: run “codex login” in a terminal.",
        AgentTool.Cursor => "Sign in: run “cursor-agent login” in a terminal.",
        // OpenCode keeps its own providers: API keys, cloud sign-ins, local models.
        AgentTool.OpenCode => "Add a model provider: run “opencode auth login”, or set one up in your opencode config.",
        _ => "",
    };

    /// Read only holds for Kiro (writes wait for an approval Hover refuses), Cursor
    /// (its Ask mode) and OpenCode (session rules its server enforces). Codex's
    /// read-only mode leans on a sandbox it doesn't have on Windows, so there it wrote
    /// files anyway; it isn't offered.
    public static bool ReadOnlyWorks(AgentTool t) => t != AgentTool.Codex || !OperatingSystem.IsWindows();

    private static readonly Dictionary<AgentTool, (DateTime At, AgentReady Ready)> Checked = new();

    /// The last check, if any, without running one.
    public static AgentReady? Known(AgentTool t)
    {
        lock (Checked) return Checked.TryGetValue(t, out var c) ? c.Ready : null;
    }

    private static readonly Dictionary<AgentTool, Task<AgentReady>> Asking = new();

    /// Installed, and signed in, by the tool's own status command. Kept for five
    /// minutes, so opening the office doesn't start three programs each time, and a
    /// check already under way is shared rather than run twice.
    public static Task<AgentReady> Check(AgentTool t, bool fresh = false)
    {
        lock (Checked)
        {
            if (!fresh && Checked.TryGetValue(t, out var c) && DateTime.UtcNow - c.At < TimeSpan.FromMinutes(5)) return Task.FromResult(c.Ready);
            if (Asking.TryGetValue(t, out var going)) return going;
            return Asking[t] = Task.Run(async () =>
            {
                AgentReady ready;
                try { ready = await Look(t); }
                catch (Exception e) { ready = new(Exe(t) is not null, false, e.Message); }
                lock (Checked) { Checked[t] = (DateTime.UtcNow, ready); Asking.Remove(t); }
                return ready;
            });
        }
    }

    private static async Task<AgentReady> Look(AgentTool t)
    {
        if (Exe(t) is not { } exe) return new(false, false, InstallHint(t));
        if (t == AgentTool.OpenCode)
        {
            // Having no sign-in doesn't mean it can't run: API keys in the environment
            // and local models count too. The version is all that is checked here; a
            // provider that can't answer shows up as the task's own error.
            var (vc, vt) = await Ask(exe, new[] { "--version" });
            var version = vt.Trim().Split('\n').LastOrDefault()?.Trim() ?? "";
            if (vc != 0 || !Version.TryParse(version.Split('-')[0], out var have)) return new(true, false, $"Couldn’t read OpenCode’s version. {InstallHint(t)}");
            return have < Version.Parse(OpenCodeMinVersion)
                ? new(true, false, $"OpenCode {version} is too old for Hover. {InstallHint(t)}")
                : new(true, true, "");
        }
        var (cmd, args) = t switch
        {
            AgentTool.Kiro => (exe, new[] { "whoami" }),
            AgentTool.Codex => (Quota.OnPath("codex"), new[] { "login", "status" }),
            _ => (exe, new[] { "status" }),
        };
        // The adapter can carry its own Codex; without the CLI there is nothing to ask.
        if (cmd is null) return new(true, true, "");
        var (code, text) = await Ask(cmd, args);
        var lower = text.ToLowerInvariant();
        var signedIn = code == 0 && !lower.Contains("not logged in") && !lower.Contains("not signed in") && !lower.Contains("logged out");
        return new(true, signedIn, signedIn ? "" : SignInHint(t));
    }

    private static async Task<(int Code, string Text)> Ask(string exe, string[] args)
    {
        try
        {
            using var p = new Process { StartInfo = Quota.Hidden(exe, args) };
            p.Start();
            p.StandardInput.Close();
            var output = p.StandardOutput.ReadToEndAsync();
            var error = p.StandardError.ReadToEndAsync();
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(20));
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

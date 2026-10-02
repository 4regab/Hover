using System.Diagnostics;
using System.IO;
using System.Text;
using Hover.Core;

namespace Hover.Services;

/// One click from "not installed" to "ready": installs what a tool is missing with
/// its maker's own installer, then opens its own sign-in. Nothing here signs in for
/// the user or touches a tool's credentials: sign-in is the tool's command in a
/// Terminal window (each one's login is interactive in its own way: a license choice,
/// a pasted code, a browser), and Hover only watches the tool's status command until
/// it says yes. No WPF; macOS only for now (the Windows installers differ).
public static class AgentSetup
{
    /// What a setup is doing: "installing" or "signing-in", the installer's last line,
    /// and why it stopped if it failed.
    public sealed record Progress(string? Step, string Line, string? Error);

    public static readonly Progress Idle = new(null, "", null);

    /// One install step: what it is called and the bash command that does it.
    public sealed record Step(string Title, string Command);

    private static readonly Dictionary<AgentTool, Progress> State = new();
    private static readonly Dictionary<AgentTool, CancellationTokenSource> Running = new();
    // npm and the vendors' installers write shared folders (~/.local/bin, the npm
    // prefix); one at a time.
    private static readonly SemaphoreSlim Gate = new(1, 1);

    /// Raised (off the caller's thread) whenever a tool's progress changes.
    public static event Action<AgentTool>? Changed;

    public static bool Supported(AgentTool t) => OperatingSystem.IsMacOS();

    public static Progress Of(AgentTool t)
    {
        lock (State) return State.TryGetValue(t, out var p) ? p : Idle;
    }

    private static void Set(AgentTool t, Progress p)
    {
        lock (State) State[t] = p;
        Changed?.Invoke(t);
    }

    private static string Home => Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

    /// npm packages go to ~/.local (bins in ~/.local/bin, already on Hover's PATH), so
    /// a Homebrew or system Node never needs sudo, and the prefix is pinned to the npm
    /// that installs them (as T3 Code pins its npm updates).
    private static string Npm(params string[] packages) =>
        $"npm install --global --no-fund --no-audit --prefix \"$HOME/.local\" {string.Join(' ', packages)}";

    /// What a tool still needs, in order; empty when everything is there.
    public static IReadOnlyList<Step> Plan(AgentTool t)
    {
        var steps = new List<Step>();
        bool Has(string name) => Quota.OnPath(name) is not null;
        var npmPackages = new List<string>();
        switch (t)
        {
            case AgentTool.Codex:
                if (!Has("codex")) npmPackages.Add("@openai/codex");
                if (!Has("codex-acp")) npmPackages.Add("@agentclientprotocol/codex-acp");
                break;
            case AgentTool.Kiro:
                if (!Has("kiro-cli")) steps.Add(new("Installing Kiro CLI", "curl -fsSL https://cli.kiro.dev/install | bash"));
                break;
            case AgentTool.Cursor:
                if (!Has("cursor-agent")) steps.Add(new("Installing the Cursor CLI", "curl -fsS https://cursor.com/install | bash"));
                break;
            case AgentTool.OpenCode:
                if (!Has("opencode")) steps.Add(new("Installing OpenCode", "curl -fsSL https://opencode.ai/install | bash"));
                break;
        }
        var node = new Step("Installing Node.js", "brew install node");
        if (npmPackages.Count > 0)
        {
            // The adapters are Node programs; Homebrew's Node when there is no Node yet.
            if (!Has("npm") && Has("brew")) steps.Add(node);
            steps.Add(new(npmPackages.Count > 1 ? "Installing Codex and its ACP adapter" : "Installing Codex's ACP adapter", Npm(npmPackages.ToArray())));
        }
        // Every tool runs in the sandbox (Services.Sandbox): srt, a Node program at the
        // version Hover was checked against, and ripgrep, which srt needs on a Mac.
        if (Sandbox.Wanted)
        {
            if (Sandbox.Exe() is null)
            {
                if (!Has("npm") && Has("brew") && !steps.Contains(node)) steps.Add(node);
                steps.Add(new("Installing the agent sandbox (srt)", Npm($"{Sandbox.Package}@{Sandbox.Version}")));
            }
            if (!Has("rg") && !File.Exists("/opt/homebrew/bin/rg") && !File.Exists("/usr/local/bin/rg") && Has("brew"))
                steps.Add(new("Installing ripgrep for the sandbox", "brew install ripgrep"));
        }
        return steps;
    }

    /// The tool's own sign-in, run in Terminal.
    public static string? SignInCommand(AgentTool t) => t switch
    {
        AgentTool.Codex => "codex login",
        AgentTool.Kiro => "kiro-cli login",
        AgentTool.Cursor => "cursor-agent login",
        AgentTool.OpenCode => "opencode auth login",
        _ => null,
    };

    /// Installs what is missing, then opens sign-in if the tool still isn't signed in.
    /// One run per tool; a second click while one runs does nothing.
    public static async Task Run(AgentTool t, Func<string, Task>? openFile = null)
    {
        if (!Supported(t)) { Set(t, new(null, "", "One-click setup is available on macOS.")); return; }
        CancellationTokenSource cts;
        lock (Running)
        {
            if (Running.ContainsKey(t)) return;
            Running[t] = cts = new CancellationTokenSource();
        }
        try
        {
            var plan = Plan(t);
            if (plan.Count > 0)
            {
                if (plan.Any(s => s.Command.StartsWith("npm ")) && Quota.OnPath("npm") is null && !plan.Any(s => s.Command.StartsWith("brew ")))
                    throw new SetupError("Node.js is needed for this tool's ACP adapter. Install Node.js from nodejs.org, then try again.");
                Set(t, new("installing", "Waiting for another install to finish…", null));
                await Gate.WaitAsync(cts.Token);
                try
                {
                    foreach (var step in plan)
                    {
                        Set(t, new("installing", step.Title + "…", null));
                        await Shell(t, step, cts.Token);
                    }
                }
                finally { Gate.Release(); }
            }
            var ready = await Agents.Check(t, fresh: true);
            if (!ready.Installed) throw new SetupError($"The installer finished, but {Agents.Name(t)} still isn't found. {Agents.InstallHint(t)}");
            if (!ready.SignedIn) await SignIn(t, openFile, cts.Token);
            else Set(t, Idle);
        }
        catch (OperationCanceledException) { Set(t, Idle); }
        catch (SetupError e) { Set(t, new(null, "", e.Message)); }
        catch (Exception e) { Set(t, new(null, "", $"Setup stopped: {e.Message}")); }
        finally
        {
            lock (Running) Running.Remove(t);
            cts.Dispose();
        }
    }

    public static void Cancel(AgentTool t)
    {
        lock (Running) if (Running.TryGetValue(t, out var c)) c.Cancel();
    }

    public static bool Busy(AgentTool t)
    {
        lock (Running) return Running.ContainsKey(t);
    }

    private sealed class SetupError(string message) : Exception(message);

    /// Opens the tool's own login in Terminal and waits (up to ten minutes) for its
    /// status command to say signed in.
    private static async Task SignIn(AgentTool t, Func<string, Task>? openFile, CancellationToken ct)
    {
        if (SignInCommand(t) is not { } command) { Set(t, Idle); return; }
        var dir = Path.Combine(Paths.Support, "setup");
        Directory.CreateDirectory(dir);
        var script = Path.Combine(dir, $"sign-in-{Agents.Id(t)}.command");
        // The script carries Hover's PATH (from the login shell) so Terminal finds the
        // tool just installed even before a new shell would.
        var path = Environment.GetEnvironmentVariable("PATH") ?? "/usr/bin:/bin";
        var text = new StringBuilder()
            .Append("#!/bin/bash\n")
            .Append("export PATH=").Append(Quote(path)).Append('\n')
            .Append("clear\n")
            .Append("printf '\\n  Hover · Sign in to ").Append(Agents.Name(t)).Append("\\n\\n'\n")
            .Append(command).Append('\n')
            .Append("printf '\\n  Done. You can close this window; Hover picks it up by itself.\\n\\n'\n")
            .ToString();
        await File.WriteAllTextAsync(script, text, ct);
        if (!OperatingSystem.IsWindows()) File.SetUnixFileMode(script, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        Set(t, new("signing-in", "Finish signing in in the Terminal window and your browser…", null));
        if (openFile is not null) await openFile(script);
        else
        {
            using var open = Process.Start(new ProcessStartInfo("/usr/bin/open", new[] { "-a", "Terminal", script }) { UseShellExecute = false });
            if (open is not null) await open.WaitForExitAsync(ct);
        }
        var until = DateTime.UtcNow.AddMinutes(10);
        while (DateTime.UtcNow < until)
        {
            await Task.Delay(TimeSpan.FromSeconds(3), ct);
            if ((await Agents.Check(t, fresh: true)).SignedIn) { Set(t, Idle); return; }
        }
        Set(t, new(null, "", $"Not signed in yet. Click Sign in to try again, or run “{command}” in a terminal."));
    }

    private static string Quote(string s) => "'" + s.Replace("'", "'\\''") + "'";

    /// Runs one step with bash, no stdin (an installer that prompts gets EOF and its
    /// default), showing its newest line. Fails with the step's last lines.
    private static async Task Shell(AgentTool t, Step step, CancellationToken ct)
    {
        var psi = new ProcessStartInfo("/bin/bash", new[] { "-c", "set -o pipefail; " + step.Command })
        {
            RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
            UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = Home,
        };
        // Installers that draw progress bars or colours fall back to plain lines.
        psi.Environment["CI"] = "1";
        psi.Environment["NO_COLOR"] = "1";
        psi.Environment["TERM"] = "dumb";
        psi.Environment["HOMEBREW_NO_AUTO_UPDATE"] = "1";
        using var p = new Process { StartInfo = psi };
        var tail = new Queue<string>();
        void Line(string? raw)
        {
            if (raw is null) return;
            var line = Quota.StripAnsi(raw).Trim();
            if (line.Length == 0) return;
            lock (tail) { tail.Enqueue(line); while (tail.Count > 6) tail.Dequeue(); }
            Set(t, new("installing", line.Length > 120 ? line[..119] + "…" : line, null));
        }
        p.OutputDataReceived += (_, e) => Line(e.Data);
        p.ErrorDataReceived += (_, e) => Line(e.Data);
        p.Start();
        p.StandardInput.Close();
        p.BeginOutputReadLine(); p.BeginErrorReadLine();
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeout.CancelAfter(TimeSpan.FromMinutes(10));
        try { await p.WaitForExitAsync(timeout.Token); }
        catch (OperationCanceledException)
        {
            try { p.Kill(entireProcessTree: true); } catch { }
            if (ct.IsCancellationRequested) throw;
            throw new SetupError($"{step.Title} took more than ten minutes and was stopped.");
        }
        if (p.ExitCode != 0)
        {
            string last;
            lock (tail) last = string.Join(" · ", tail.TakeLast(2));
            throw new SetupError($"{step.Title.Replace("Installing", "Couldn’t install")}: {(last.Length > 0 ? last : $"exit code {p.ExitCode}")}");
        }
    }
}

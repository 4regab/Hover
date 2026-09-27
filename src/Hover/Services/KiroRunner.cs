using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using Hover.Core;

namespace Hover.Services;

public enum KiroState { Idle, Running, Completed, Failed, Cancelled }

/// What Kiro is broadly busy with, read from its tool calls. The Kiro page turns it
/// into the ghost's mood and a few words; it is never shown as a log.
public enum KiroPhase { Starting, Thinking, Planning, Reading, Searching, Editing, Running, Writing, Working }

/// How a run ended. Text is Kiro's final answer when it completed, and a readable
/// reason when it failed or was stopped.
public sealed record KiroResult(KiroState State, string Text, int? ExitCode = null);

/// Runs one Kiro CLI task headlessly in a folder, as a hidden child of Hover:
///
///   kiro-cli chat --no-interactive --trust-all-tools --agent-engine v3 --output-format stream-json
///
/// with the prompt written to stdin, never put on the command line, and the folder as
/// the working directory. Full tool access is on because nobody is there to approve
/// a tool call; the Kiro page's first-use note and its folder-first flow are the
/// boundary instead. No WPF in here, so it can be driven by a stand-in kiro-cli.
public static class KiroRunner
{
    public static readonly IReadOnlyList<string> Arguments = new[]
    {
        "chat", "--no-interactive", "--trust-all-tools", "--agent-engine", "v3", "--output-format", "stream-json",
    };

    /// A folder Kiro can work in: a full path to a directory that is there now.
    public static bool UsableFolder(string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return false;
        try { return Path.IsPathFullyQualified(path) && Directory.Exists(path); }
        catch (ArgumentException) { return false; }
    }

    /// Runs the task to the end, or until ct is cancelled, which kills kiro-cli and
    /// everything it started. Phases are reported through progress as they change.
    /// Never throws: every way it can go wrong comes back as a Failed result.
    public static async Task<KiroResult> Run(string folder, string prompt, IProgress<KiroPhase>? progress,
        CancellationToken ct, string? exe = null)
    {
        if (!UsableFolder(folder)) return new(KiroState.Failed, "That folder isn’t there any more. Choose another one.");
        if (string.IsNullOrWhiteSpace(prompt)) return new(KiroState.Failed, "Tell Kiro what to do first.");
        exe ??= Quota.OnPath("kiro-cli");
        if (exe is null) return new(KiroState.Failed, "kiro-cli isn’t installed or isn’t on PATH. Install it from kiro.dev/cli, then try again.");

        var psi = Quota.Hidden(exe, Arguments.ToArray());
        psi.WorkingDirectory = folder;
        using var p = new Process { StartInfo = psi };
        var stream = new KiroStream();
        try { p.Start(); }
        catch (Exception e) { return new(KiroState.Failed, $"kiro-cli couldn’t start: {e.Message}"); }
        progress?.Report(KiroPhase.Starting);

        // Registered only once the process exists; the kill runs straight from Cancel,
        // so a Hover that is quitting doesn't leave Kiro working on its own.
        using var stop = ct.Register(() => Kill(p));
        var stderr = ReadCapped(p.StandardError, 32 * 1024);
        try
        {
            // One trailing newline, as a prompt piped in from a shell would end.
            await p.StandardInput.WriteAsync(prompt.Trim() + "\n");
            p.StandardInput.Close();
        }
        catch (IOException) { /* it exited before reading; its exit code says why */ }

        var stdout = Task.Run(async () =>
        {
            while (await p.StandardOutput.ReadLineAsync() is { } line)
                if (stream.Feed(line) is { } phase) progress?.Report(phase);
        });
        await p.WaitForExitAsync(CancellationToken.None);
        // A grandchild (an MCP server) can hold the pipes open after kiro-cli has
        // gone. Its output is not wanted; the run is over when kiro-cli is.
        await Task.WhenAny(Task.WhenAll(stdout, stderr), Task.Delay(TimeSpan.FromSeconds(3)));
        var err = stderr.IsCompletedSuccessfully ? stderr.Result : "";
        return stream.Outcome(p.ExitCode, ct.IsCancellationRequested, err);
    }

    private static void Kill(Process p)
    {
        try { if (!p.HasExited) p.Kill(entireProcessTree: true); }
        catch { /* already gone */ }
    }

    private static async Task<string> ReadCapped(StreamReader r, int max)
    {
        var sb = new StringBuilder();
        var buf = new char[4096];
        int n;
        while ((n = await r.ReadAsync(buf)) > 0)
        {
            sb.Append(buf, 0, n);
            // Keep the end: that is where a CLI says why it gave up.
            if (sb.Length > max) sb.Remove(0, sb.Length - max);
        }
        return sb.ToString();
    }
}

/// Reads kiro-cli's stream-json output: one JSON object per line, the run's ACP
/// events (runStarted, sessionUpdate, runFinished with its finalText, runError, and
/// a final interruption record). The format is not documented, so every field is
/// optional here and anything unrecognised is skipped; the exit code has the last
/// word on success.
public sealed class KiroStream
{
    private readonly StringBuilder _said = new();
    private readonly Queue<string> _plain = new();

    public KiroPhase Phase { get; private set; } = KiroPhase.Starting;
    public string? FinalText { get; private set; }
    public string? StopReason { get; private set; }
    public string? Error { get; private set; }
    public bool Interrupted { get; private set; }
    public bool Finished { get; private set; }

    /// Everything Kiro has said so far, from its message chunks.
    public string Said => _said.ToString();

    /// One line of output. Returns the new phase when it changed.
    public KiroPhase? Feed(string line)
    {
        line = line.Trim();
        if (line.Length == 0) return null;
        if (!line.StartsWith('{')) { Keep(line); return null; }
        var before = Phase;
        try
        {
            using var doc = JsonDocument.Parse(line);
            if (doc.RootElement.ValueKind != JsonValueKind.Object) return null;
            Read(doc.RootElement);
        }
        catch (JsonException) { Keep(line); return null; }
        return Phase != before ? Phase : null;
    }

    private void Keep(string line)
    {
        _plain.Enqueue(Quota.StripAnsi(line));
        while (_plain.Count > 12) _plain.Dequeue();
    }

    private void Read(JsonElement root)
    {
        var (name, body) = Envelope(root);
        var n = name.ToLowerInvariant();
        if (n.Contains("error")) Error ??= Message(body) ?? Message(root) ?? "Kiro reported an error.";
        if (n.Contains("interrupt") || n.Contains("cancel")) Interrupted = true;
        if (n.Contains("finish") || n.Contains("complete")) Finished = true;
        if ((Str(body, "finalText") ?? Str(root, "finalText")) is { } final) FinalText = final;
        if ((Str(body, "stopReason") ?? Str(root, "stopReason")) is { } reason)
        {
            StopReason = reason;
            if (reason == "cancelled") Interrupted = true;
        }
        if (FindUpdate(root, 0) is { } update) Update(update);
    }

    /// The event's name and its payload, from {"type": ..., "data": {...}} or the
    /// like, or from an object with one key: {"runFinished": {...}}.
    private static (string Name, JsonElement Body) Envelope(JsonElement root)
    {
        foreach (var key in new[] { "type", "event", "method" })
            if (Str(root, key) is { } name)
            {
                foreach (var inner in new[] { "data", "payload", "params" })
                    if (root.TryGetProperty(inner, out var b) && b.ValueKind == JsonValueKind.Object) return (name, b);
                return (name, root);
            }
        var props = root.EnumerateObject().ToList();
        if (props.Count == 1 && props[0].Value.ValueKind == JsonValueKind.Object) return (props[0].Name, props[0].Value);
        return ("", root);
    }

    private static JsonElement? FindUpdate(JsonElement e, int depth)
    {
        if (e.ValueKind != JsonValueKind.Object || depth > 5) return null;
        if (Str(e, "sessionUpdate") is not null) return e;
        foreach (var p in e.EnumerateObject())
            if (FindUpdate(p.Value, depth + 1) is { } hit) return hit;
        return null;
    }

    private void Update(JsonElement u)
    {
        switch (Str(u, "sessionUpdate"))
        {
            case "agent_message_chunk":
                if (u.TryGetProperty("content", out var c)) AppendText(c);
                Phase = KiroPhase.Writing;
                break;
            case "agent_thought_chunk":
                Phase = KiroPhase.Thinking;
                break;
            case "plan":
                Phase = KiroPhase.Planning;
                break;
            case "tool_call":
            case "tool_call_update":
            case "tool_call_chunk":
                if (ToolPhase(Str(u, "kind"), Str(u, "title")) is { } phase) Phase = phase;
                break;
        }
    }

    private void AppendText(JsonElement content)
    {
        if (content.ValueKind == JsonValueKind.Array)
            foreach (var part in content.EnumerateArray()) AppendText(part);
        else if (Str(content, "text") is { } t) _said.Append(t);
    }

    /// ACP's tool kinds, with the title as a fallback for a tool that gives none.
    internal static KiroPhase? ToolPhase(string? kind, string? title)
    {
        switch (kind)
        {
            case "read": return KiroPhase.Reading;
            case "edit" or "delete" or "move": return KiroPhase.Editing;
            case "execute": return KiroPhase.Running;
            case "search" or "fetch": return KiroPhase.Searching;
            case "think": return KiroPhase.Thinking;
        }
        var t = (title ?? "").ToLowerInvariant();
        if (t.Length == 0) return kind is null ? null : KiroPhase.Working;
        if (t.Contains("read")) return KiroPhase.Reading;
        if (t.Contains("write") || t.Contains("edit") || t.Contains("replace") || t.Contains("creat")) return KiroPhase.Editing;
        if (t.Contains("grep") || t.Contains("glob") || t.Contains("search") || t.Contains("find") || t.Contains("fetch")) return KiroPhase.Searching;
        if (t.Contains("shell") || t.Contains("bash") || t.Contains("command") || t.Contains("run")) return KiroPhase.Running;
        return KiroPhase.Working;
    }

    /// The run's result once kiro-cli has exited.
    public KiroResult Outcome(int exitCode, bool cancelled, string stderr)
    {
        var said = Clip((FinalText ?? Said).Trim());
        if (cancelled || Interrupted)
            return new(KiroState.Cancelled, said.Length > 0 ? said : "Stopped before Kiro finished.", exitCode);
        if (StopReason == "refusal") return new(KiroState.Failed, "Kiro declined this request.", exitCode);
        if (exitCode == 0 && Error is null)
            return new(KiroState.Completed, said.Length > 0 ? said : "Done. Kiro didn’t leave a summary.", exitCode);
        return new(KiroState.Failed, Explain(exitCode, stderr), exitCode);
    }

    private string Explain(int exitCode, string stderr)
    {
        var text = Quota.StripAnsi(stderr + "\n" + string.Join("\n", _plain));
        var lower = text.ToLowerInvariant();
        if (lower.Contains("kiro-cli login") || lower.Contains("not logged in") || lower.Contains("login required") ||
            lower.Contains("authentication"))
            return "Kiro needs you to sign in. Run “kiro-cli login” in a terminal, then try again.";
        if (Error is not null) return Clip(Error);
        if (exitCode == 3) return "An MCP server Kiro depends on didn’t start.";
        var lines = text.Split('\n').Select(l => l.Trim()).Where(l => l.Length > 0).TakeLast(3).ToList();
        if (lines.Count > 0) return Clip(string.Join("\n", lines), 600);
        return $"kiro-cli stopped with exit code {exitCode}.";
    }

    private static string Clip(string s, int max = 20000) => s.Length <= max ? s : s[..max] + "…";

    private static string? Message(JsonElement e)
    {
        if (e.ValueKind != JsonValueKind.Object) return null;
        if (Str(e, "message") is { Length: > 0 } m) return m;
        if (e.TryGetProperty("error", out var err))
        {
            if (err.ValueKind == JsonValueKind.String && err.GetString() is { Length: > 0 } s) return s;
            if (Message(err) is { } inner) return inner;
        }
        return null;
    }

    private static string? Str(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
}

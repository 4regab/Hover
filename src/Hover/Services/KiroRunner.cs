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

/// One thing Kiro did in a run, from a tool call: its ACP kind (read, edit, execute,
/// search...), its title, the file or command it was about, and how it went
/// (in_progress, completed, failed).
public sealed record KiroStep(string Id, string Kind, string Title, string? Target, string Status);

/// Detail from a run as it goes, beside its phase: a step that started or ended, how
/// full Kiro's context is (0 to 100), and Kiro's own session id, which lets a reply
/// carry on the same conversation (kiro-cli chat --resume-id).
public sealed record KiroEvent(KiroStep? Step = null, double? Context = null, string? SessionId = null);

/// How one agent's runs are set up, from its page in Settings. Null model or effort
/// leaves the tool's own default. Read only refuses whatever would change a file or run
/// a command. IdleMinutes is how long the tool's process stays up with nothing to do.
/// Agent (a Kiro agent, sent as its mode) and RequireMcp are Kiro's alone. HideSteps
/// keeps the tools it runs out of the chat (they are still kept).
public sealed record AgentOptions(string? Model = null, string? Effort = null, bool ReadOnly = false,
    int IdleMinutes = 5, string? Agent = null, bool RequireMcp = false, bool HideSteps = false)
{
    public static readonly AgentOptions Default = new();
    public static readonly int[] IdleChoices = { 5, 15 };
}

/// What Hover knows about kiro-cli without starting it: its models as a fallback
/// before a run has listed them, the agents on disk, and whether a folder can be used.
/// The runs themselves go through AcpHost.
public static class KiroRunner
{
    /// Settings → Kiro's models until a run has listed Kiro's own. Auto lets Kiro pick.
    public static readonly IReadOnlyList<(string Id, string Name)> Models = new[]
    {
        ("auto", "Auto"), ("claude-opus-5.5", "Claude Opus 5.5"), ("claude-opus-5", "Claude Opus 5"),
        ("claude-sonnet-5", "Claude Sonnet 5"), ("claude-opus-4.8", "Claude Opus 4.8"), ("claude-sonnet-4.6", "Claude Sonnet 4.6"),
        ("claude-haiku-4.5", "Claude Haiku 4.5"), ("gpt-5.6-sol", "GPT-5.6 Sol"), ("gpt-5.6-terra", "GPT-5.6 Terra"),
        ("gpt-5.6-luna", "GPT-5.6 Luna"), ("deepseek-3.2", "DeepSeek 3.2"), ("minimax-m2.5", "MiniMax M2.5"),
        ("glm-5", "GLM-5"), ("qwen3-coder-next", "Qwen3 Coder Next"),
    };

    // Names from files on disk are only offered when they are plain.
    private static bool Plain(string? s) => !string.IsNullOrEmpty(s) && s.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_' or '.');

    /// The agents kiro-cli can run with: the user's in ~/.kiro/agents and the
    /// project's in <folder>/.kiro/agents, by the name in each file. Under ACP an agent
    /// is a session mode, so Kiro's own modes (plan, spec...) come from the run instead.
    public static IReadOnlyList<string> Agents(string? folder)
    {
        var names = new List<string>();
        var dirs = new List<string> { Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".kiro", "agents") };
        if (UsableFolder(folder)) dirs.Add(Path.Combine(folder!, ".kiro", "agents"));
        foreach (var dir in dirs.Where(Directory.Exists))
            foreach (var file in Directory.EnumerateFiles(dir, "*.json"))
            {
                var name = Path.GetFileNameWithoutExtension(file);
                try
                {
                    using var doc = JsonDocument.Parse(File.ReadAllText(file));
                    if (doc.RootElement.ValueKind == JsonValueKind.Object && doc.RootElement.TryGetProperty("name", out var n) &&
                        n.ValueKind == JsonValueKind.String && n.GetString() is { Length: > 0 } s) name = s;
                }
                catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException) { }
                if (Plain(name) && !names.Contains(name)) names.Add(name);
            }
        return names;
    }

    /// A folder an agent can work in: a full path to a directory that is there now.
    public static bool UsableFolder(string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return false;
        try { return Path.IsPathFullyQualified(path) && Directory.Exists(path); }
        catch (ArgumentException) { return false; }
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
    private const int SaidLimit = 64 * 1024;

    /// Who is talking, for the messages a result carries.
    public string Name { get; init; } = "Kiro";

    public KiroPhase Phase { get; private set; } = KiroPhase.Starting;
    public string? FinalText { get; private set; }
    public string? StopReason { get; private set; }
    public string? Error { get; private set; }
    public bool Interrupted { get; private set; }
    public bool Finished { get; private set; }
    /// Kiro's id for this conversation, for a reply to resume.
    public string? SessionId { get; private set; }
    /// How full Kiro's context is, 0 to 100, as it last said.
    public double? Context { get; private set; }

    private readonly List<KiroEvent> _events = new();
    private readonly Dictionary<string, KiroStep> _steps = new();

    /// The steps, context and session id seen since the last call.
    public IReadOnlyList<KiroEvent> Drain()
    {
        if (_events.Count == 0) return Array.Empty<KiroEvent>();
        var e = _events.ToArray();
        _events.Clear();
        return e;
    }

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
        if ((Str(body, "sessionId") ?? Str(root, "sessionId")) is { Length: > 0 } id && id != SessionId)
        {
            SessionId = id;
            _events.Add(new KiroEvent(SessionId: id));
        }
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
                // Text after a tool call, or under a new message id, is a new message;
                // the answer is the last one (Codex, for one, says a warning first).
                var mid = Str(u, "messageId");
                // Codex marks its answer (final_answer) apart from what it says first.
                var final = u.TryGetProperty("_meta", out var cm) && cm.ValueKind == JsonValueKind.Object &&
                            cm.TryGetProperty("codex", out var cx) && Str(cx, "phase") == "final_answer";
                if (_afterTool || (mid is not null && _message is not null && mid != _message) || (final && !_final)) { _said.Clear(); _afterTool = false; }
                _final |= final;
                if (mid is not null) _message = mid;
                if (u.TryGetProperty("content", out var c)) AppendText(c);
                Phase = KiroPhase.Writing;
                break;
            case "usage_update":
                // Codex: {"used": tokens, "size": the context window}.
                if (u.TryGetProperty("used", out var used) && used.ValueKind == JsonValueKind.Number &&
                    u.TryGetProperty("size", out var size) && size.ValueKind == JsonValueKind.Number && size.GetDouble() > 0)
                    SetContext(used.GetDouble() * 100 / size.GetDouble());
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
                if (_said.Length > 0) _afterTool = true;
                Step(u);
                break;
            case "session_info_update":
                // {"_meta":{"kiro":{"contextUsage":{"usagePercentage":3.37}}}}
                if (u.TryGetProperty("_meta", out var meta) && meta.ValueKind == JsonValueKind.Object &&
                    meta.TryGetProperty("kiro", out var kiro) && kiro.ValueKind == JsonValueKind.Object &&
                    kiro.TryGetProperty("contextUsage", out var usage) && usage.ValueKind == JsonValueKind.Object &&
                    usage.TryGetProperty("usagePercentage", out var pct) && pct.ValueKind == JsonValueKind.Number)
                    SetContext(pct.GetDouble());
                break;
        }
    }

    private bool _afterTool;
    private string? _message;
    private bool _final;

    private void SetContext(double pct)
    {
        var v = Math.Clamp(pct, 0, 100);
        if (Context is not { } old || Math.Abs(old - v) >= 0.5) { Context = v; _events.Add(new KiroEvent(Context: v)); }
    }

    /// A tool call starts a step; its updates carry the status. The first one names it.
    private void Step(JsonElement u)
    {
        if (Str(u, "toolCallId") is not { Length: > 0 } id) return;
        var status = Str(u, "status") ?? "in_progress";
        if (_steps.TryGetValue(id, out var known))
        {
            if (known.Status == status) return;
            known = known with { Status = status, Title = Str(u, "title") ?? known.Title };
        }
        else
        {
            string? target = null;
            if (u.TryGetProperty("locations", out var locs) && locs.ValueKind == JsonValueKind.Array)
                foreach (var l in locs.EnumerateArray()) { target = Str(l, "path"); if (target is not null) break; }
            if (target is null && u.TryGetProperty("rawInput", out var raw))
                target = Str(raw, "command") ?? Str(raw, "path") ?? Str(raw, "pattern") ?? Str(raw, "query") ?? Str(raw, "url");
            known = new KiroStep(id, Str(u, "kind") ?? "other", Str(u, "title") ?? "Working", target, status);
        }
        _steps[id] = known;
        _events.Add(new KiroEvent(Step: known));
    }

    private void AppendText(JsonElement content)
    {
        if (content.ValueKind == JsonValueKind.Array)
            foreach (var part in content.EnumerateArray()) AppendText(part);
        else if (Str(content, "text") is { } t)
        {
            _said.Append(t);
            // A long run can say a lot; only the end is ever shown.
            if (_said.Length > SaidLimit) _said.Remove(0, _said.Length - SaidLimit);
        }
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
            return new(KiroState.Cancelled, said.Length > 0 ? said : $"Stopped before {Name} finished.", exitCode);
        if (StopReason == "refusal") return new(KiroState.Failed, $"{Name} declined this request.", exitCode);
        if (exitCode == 0 && Error is null)
            return new(KiroState.Completed, said.Length > 0 ? said : $"Done. {Name} didn’t leave a summary.", exitCode);
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

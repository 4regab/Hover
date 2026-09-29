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
/// (in_progress, completed, failed). An edit carries how many lines it adds and
/// removes and a short preview of the change ("- old", "+ new", "  context"); a
/// command carries the end of its output and its exit code. Ms is how long it took.
public sealed record KiroStep(string Id, string Kind, string Title, string? Target, string Status,
    int Added = 0, int Removed = 0, string? Diff = null, string? Output = null, int? Exit = null, double? Ms = null);

/// Detail from a run as it goes, beside its phase: a step that started or ended, how
/// full Kiro's context is (0 to 100), and Kiro's own session id, which lets a reply
/// carry on the same conversation (kiro-cli chat --resume-id). Credits is what a turn
/// cost, as Kiro says at its end.
public sealed record KiroEvent(KiroStep? Step = null, double? Context = null, string? SessionId = null, double? Credits = null);

/// How one agent's runs are set up, from its page in Settings. Null model or effort
/// leaves the tool's own default. Read only refuses whatever would change a file or run
/// a command. IdleMinutes is how long the tool's process stays up with nothing to do.
/// Agent (a Kiro agent, sent as its mode) and RequireMcp are Kiro's alone. HideSteps
/// keeps the tools it runs out of the chat (they are still kept). Approval is when the
/// agent stops to ask the user first; read only overrules it.
public sealed record AgentOptions(string? Model = null, string? Effort = null, bool ReadOnly = false,
    int IdleMinutes = 5, string? Agent = null, bool RequireMcp = false, bool HideSteps = false,
    AgentApproval Approval = AgentApproval.Autopilot)
{
    public static readonly AgentOptions Default = new();
    public static readonly int[] IdleChoices = { 5, 15 };

    /// A session's own tool access, picked when it started: full, risky (ask first),
    /// always (ask always) or read. Anything else keeps the tool's setting.
    public AgentOptions WithAccess(string? access) => access switch
    {
        "full" => this with { ReadOnly = false, Approval = AgentApproval.Autopilot },
        "risky" => this with { ReadOnly = false, Approval = AgentApproval.Risky },
        "always" => this with { ReadOnly = false, Approval = AgentApproval.Always },
        "read" => this with { ReadOnly = true },
        _ => this,
    };

    /// The id WithAccess takes for these options.
    public string AccessId(bool readOnlyWorks) => ReadOnly && readOnlyWorks ? "read"
        : Approval switch { AgentApproval.Risky => "risky", AgentApproval.Always => "always", _ => "full" };
}

/// When an agent with full access stops to ask. Autopilot never asks (what 2.0 did).
/// Risky asks for commands, deletes, moves, the network and anything outside the
/// folder, and lets reading, searching and editing inside it go ahead. Always asks
/// before anything but reading and searching.
public enum AgentApproval { Autopilot, Risky, Always }

/// A tool call an agent is waiting on the user for (ACP session/request_permission),
/// told the way the notch and the office show it. Kind is ACP's (execute, edit,
/// delete...). Command is the command line, Path the file (relative to the folder when
/// inside it), Preview a few lines of the change, with +/- before each, and Added and
/// Removed how many lines it changes. Reason is Hover's own few words on why it asks;
/// Danger marks what can't be taken back easily.
public sealed record AgentAsk(string Id, string Kind, string Title, string? Command, string? Path, string? Preview,
    int Added, int Removed, string Reason, bool Danger, IReadOnlyList<AgentQuestion>? Questions = null)
{
    /// A question for the user to answer (Kind "question"), not a tool call to allow.
    public bool IsQuestion => Questions is { Count: > 0 };
}

/// One question an agent asks the user (OpenCode's question tool): a short header,
/// the question, its choices, whether several may be picked, and whether the user
/// may type an answer of their own.
public sealed record AgentQuestion(string Header, string Question, IReadOnlyList<(string Label, string Description)> Options,
    bool Multiple, bool Custom);

/// The user's answer to an AgentAsk. Trust allows this one and the same again for the
/// rest of the session; TrustAll allows everything the session asks from now on.
public enum AskAnswer { Allow, Trust, TrustAll, Deny }

/// What a tool can really do, so the office only shows what works.
public sealed record AgentCaps(bool Questions, bool ReadOnly, bool Resume, string EffortLabel);

/// One agent tool as Hover runs it: an ACP server (Kiro, Codex, Cursor) or OpenCode's
/// own server. Shared by all of that tool's sessions; the sessions and the views only
/// ever see this. No WPF in here.
public interface IAgentRuntime
{
    AgentTool Tool { get; }
    AgentCaps Caps { get; }
    /// The tool's process is up.
    bool Alive { get; }
    /// The models, efforts and modes it offers, whenever they are read. Off the UI thread.
    event Action<AgentTool, IReadOnlyList<AcpOption>>? OptionsSeen;
    /// Asks the user about a tool call, for the tool's own session id named first.
    Func<string, AgentAsk, CancellationToken, Task<AskAnswer>>? Asking { get; set; }
    /// Asks the user a question the agent has (AgentAsk.Questions). The answer is each
    /// question's picked labels, in order; null when the user skipped it.
    Func<string, AgentAsk, CancellationToken, Task<IReadOnlyList<IReadOnlyList<string>>?>>? Questioning { get; set; }
    /// Runs one turn: a new conversation, or the one resume names. Never throws.
    Task<KiroResult> Run(string folder, string prompt, IProgress<KiroPhase>? progress, CancellationToken ct,
        string? resume = null, IProgress<KiroEvent>? events = null, string? access = null);
    /// End the tool's process now. Runs still going fail; the next one starts it again.
    void Shutdown(string why = "shut down");
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

    /// A tool call seen so far, by its id. Not thread-safe: read it where Feed runs.
    internal KiroStep? StepOf(string id) => _steps.TryGetValue(id, out var s) ? s : null;

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
                // At a turn's end: {"_meta":{"kiro":{"kind":"turn_completion",
                // "promptTurnSummaries":[{"unit":"credit","usage":0.087}]}}}.
                if (meta.ValueKind == JsonValueKind.Object && meta.TryGetProperty("kiro", out var done) && done.ValueKind == JsonValueKind.Object &&
                    Str(done, "kind") == "turn_completion" && done.TryGetProperty("promptTurnSummaries", out var sums) && sums.ValueKind == JsonValueKind.Array)
                {
                    double? credits = null;
                    foreach (var sum in sums.EnumerateArray())
                        if (Str(sum, "unit") == "credit" && sum.TryGetProperty("usage", out var use) && use.ValueKind == JsonValueKind.Number)
                            credits = (credits ?? 0) + use.GetDouble();
                    if (credits is { } spent) _events.Add(new KiroEvent(Credits: spent));
                }
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

    private readonly Dictionary<string, DateTime> _began = new();

    /// A tool call starts a step; its updates carry the status, and at the end the
    /// change it made (ACP diff content) or what the command printed (rawOutput).
    private void Step(JsonElement u)
    {
        if (Str(u, "toolCallId") is not { Length: > 0 } id) return;
        var status = Str(u, "status");
        if (!_steps.TryGetValue(id, out var known))
        {
            known = new KiroStep(id, Str(u, "kind") ?? "other", Str(u, "title") ?? "Working", Target(u), status ?? "in_progress");
            _began[id] = DateTime.UtcNow;
        }
        var next = known with { Status = status ?? known.Status, Title = Str(u, "title") ?? known.Title, Target = known.Target ?? Target(u) };
        if (DiffOf(u) is { } d) next = next with { Added = d.Added, Removed = d.Removed, Diff = d.Preview };
        if (next.Kind == "execute")
        {
            if (OutputOf(u, out var exit) is { } o) next = next with { Output = o };
            if (exit is not null) next = next with { Exit = exit };
        }
        if (next.Status is "completed" or "failed" && known.Ms is null && _began.TryGetValue(id, out var t0))
            next = next with { Ms = (DateTime.UtcNow - t0).TotalMilliseconds };
        if (_steps.ContainsKey(id) && next == known) return;
        _steps[id] = next;
        _events.Add(new KiroEvent(Step: next));
    }

    private static string? Target(JsonElement u)
    {
        string? target = null;
        if (u.TryGetProperty("locations", out var locs) && locs.ValueKind == JsonValueKind.Array)
            foreach (var l in locs.EnumerateArray()) { target = Str(l, "path"); if (target is { Length: > 0 }) break; }
        if (target is not { Length: > 0 } && u.TryGetProperty("rawInput", out var raw))
            target = Str(raw, "command") ?? Str(raw, "path") ?? Str(raw, "file_path") ?? Str(raw, "pattern") ?? Str(raw, "query") ?? Str(raw, "url");
        return target is { Length: > 0 } ? target : null;
    }

    /// The change in a tool call's diff content: lines added and removed, and the
    /// changed part with a line of context before it, up to a dozen lines.
    internal static (int Added, int Removed, string Preview)? DiffOf(JsonElement u)
    {
        if (!u.TryGetProperty("content", out var content) || content.ValueKind != JsonValueKind.Array) return null;
        int added = 0, removed = 0;
        var lines = new List<string>();
        foreach (var item in content.EnumerateArray())
        {
            if (Str(item, "type") != "diff") continue;
            var oldText = Str(item, "oldText");
            var newText = Str(item, "newText") ?? "";
            // Kiro sends an empty diff while the edit is still pending.
            if (string.IsNullOrEmpty(oldText) && newText.Length == 0) continue;
            var a = oldText is null ? new List<string>() : oldText.Replace("\r", "").TrimEnd('\n').Split('\n').ToList();
            var b = newText.Replace("\r", "").TrimEnd('\n').Split('\n').ToList();
            if (oldText is null or "") a.Clear();
            // What is the same at both ends is not the change.
            var head = 0;
            while (head < a.Count && head < b.Count && a[head] == b[head]) head++;
            var tail = 0;
            while (tail < a.Count - head && tail < b.Count - head && a[^(tail + 1)] == b[^(tail + 1)]) tail++;
            var gone = a.Skip(head).Take(a.Count - head - tail).ToList();
            var came = b.Skip(head).Take(b.Count - head - tail).ToList();
            removed += gone.Count;
            added += came.Count;
            if (lines.Count >= 12) continue;
            if (head > 0 && a[head - 1].Trim().Length > 0) lines.Add("  " + Clip(a[head - 1].TrimEnd(), 160));
            lines.AddRange(gone.Take(6).Select(x => "- " + Clip(x.TrimEnd(), 160)));
            lines.AddRange(came.Take(12 - Math.Min(12, lines.Count)).Select(x => "+ " + Clip(x.TrimEnd(), 160)));
        }
        return added + removed == 0 ? null : (added, removed, string.Join("\n", lines.Take(12)));
    }

    /// The end of what a command printed, and its exit code, from rawOutput: Kiro's
    /// {output, exitCode}, Codex's {formatted_output, exit_code}, or plain text.
    internal static string? OutputOf(JsonElement u, out int? exit)
    {
        exit = null;
        string? text = null;
        if (u.TryGetProperty("rawOutput", out var ro))
        {
            if (ro.ValueKind == JsonValueKind.String) text = ro.GetString();
            else if (ro.ValueKind == JsonValueKind.Object)
            {
                text = Str(ro, "formatted_output") ?? Str(ro, "output") ?? Str(ro, "aggregated_output") ?? Str(ro, "stdout");
                if (Str(ro, "stderr") is { Length: > 0 } err) text = text is { Length: > 0 } ? text + "\n" + err : err;
                foreach (var n in new[] { "exitCode", "exit_code" })
                    if (ro.TryGetProperty(n, out var ec) && ec.ValueKind == JsonValueKind.Number && ec.TryGetInt32(out var v)) exit = v;
            }
        }
        if (text is null) return null;
        var rows = Quota.StripAnsi(text).Replace("\r", "").Split('\n').Select(l => l.TrimEnd()).ToList();
        while (rows.Count > 0 && rows[^1].Length == 0) rows.RemoveAt(rows.Count - 1);
        while (rows.Count > 0 && rows[0].Length == 0) rows.RemoveAt(0);
        return rows.Count == 0 ? null : string.Join("\n", rows.TakeLast(10).Select(l => Clip(l, 200)));
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

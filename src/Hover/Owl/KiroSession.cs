using Hover.Services;

namespace Hover.Owl;

/// One prompt in a session and what came of it: the steps Kiro took and its answer.
public sealed class KiroTurn
{
    public KiroTurn(string prompt, IReadOnlyList<string>? images = null)
    {
        Prompt = prompt;
        Images = images ?? Array.Empty<string>();
    }

    public string Prompt { get; }
    /// Pictures pasted with the prompt, as files Kiro can read.
    public IReadOnlyList<string> Images { get; }

    /// What Kiro is sent: the prompt, then the pictures' paths for it to look at.
    internal string Text => Images.Count == 0 ? Prompt
        : (Prompt.Length > 0 ? Prompt : "Look at the attached image.") + "\n\n" +
          string.Join("\n", Images.Select(p => "Attached image (read it from this file): " + p));
    public List<KiroStep> Steps { get; } = new();
    public KiroResult? Result { get; internal set; }
    /// Sent while the turn before was still running; it starts when that one ends.
    public bool Queued { get; internal set; }
    public DateTime StartedAt { get; internal set; }
    /// When Kiro first did something other than start up.
    public DateTime? WokeAt { get; internal set; }
    public DateTime? EndedAt { get; internal set; }
    /// What the turn cost, in the tool's credits, when it says (Kiro does).
    public double? Credits { get; internal set; }
}

/// One Kiro session: a folder, the first prompt, and every reply after it, each a
/// turn. A reply carries on Kiro's own conversation (kiro-cli --resume-id). Lives on
/// the UI thread: Changed is raised there, and the runner's news arrives there
/// through Progress.
public sealed class KiroSession
{
    /// Runs one turn. Resume is Kiro's session id from the turn before, if any.
    public delegate Task<KiroResult> RunTask(string folder, string prompt, IProgress<KiroPhase> progress, CancellationToken ct,
        string? resume, IProgress<KiroEvent>? events);

    private static int _ids;
    private readonly RunTask _run;
    private readonly Func<DateTime> _now;
    private readonly List<KiroTurn> _turns = new();
    private CancellationTokenSource? _cts;

    public KiroSession(RunTask? run = null, Func<DateTime>? now = null)
    {
        _run = run ?? ((_, _, _, _, _, _) => Task.FromResult(new KiroResult(KiroState.Failed, "No agent to run it.")));
        _now = now ?? (() => DateTime.Now);
    }

    public int Id { get; } = Interlocked.Increment(ref _ids);
    /// The tool the session's turns go to.
    public AgentTool Tool { get; internal set; }
    public KiroState State { get; private set; } = KiroState.Idle;
    public KiroPhase Phase { get; private set; } = KiroPhase.Starting;
    public string Folder { get; private set; } = "";
    /// Oldest first; queued replies at the end.
    public IReadOnlyList<KiroTurn> Turns => _turns;
    /// The turn running now, or the last one that ran.
    public KiroTurn? Current => _turns.LastOrDefault(t => !t.Queued);
    /// The first prompt: what the session is about.
    public string Prompt => _turns.Count > 0 ? _turns[0].Prompt : "";
    public KiroResult? Result => Current?.Result;
    public DateTime StartedAt => Current?.StartedAt ?? default;
    public DateTime? EndedAt => Current?.EndedAt;
    public TimeSpan Elapsed => (EndedAt ?? _now()) - StartedAt;
    /// Kiro's id for the conversation, once the first turn has told it.
    public string? KiroId { get; private set; }
    /// How full Kiro's context is, 0 to 100, when known.
    public double? Context { get; private set; }
    /// The desk and the bot the office gives this session, 0 to MaxKept - 1.
    public int Seat { get; internal set; }
    public int Bot { get; internal set; }
    /// The session's lasting name, in the history on disk.
    public string Key { get; private set; } = Guid.NewGuid().ToString("N");
    /// Deleted by the user: nothing about it is saved again.
    internal bool Deleted { get; set; }
    /// The tool access picked when the session started (AgentOptions.WithAccess);
    /// null keeps the tool's setting.
    public string? Access { get; internal set; }

    /// The session as the history keeps it.
    public SavedSession Snapshot() => new(Key, Tool, Folder, Title, KiroId, Context,
        _turns.Select(t => new SavedTurn(t.Prompt, t.Images, t.Steps.ToList(), t.Result?.State, t.Result?.Text, t.StartedAt, t.WokeAt, t.EndedAt, t.Credits)).ToList(),
        _now(), Access);

    /// A new session made to carry on a saved one: its turns, its folder and its
    /// tool's conversation id, so the next reply resumes that conversation. A turn that
    /// was cut short by Hover closing reads as stopped.
    public void Restore(SavedSession s)
    {
        if (State != KiroState.Idle || _turns.Count > 0) return;
        Key = s.Key;
        Tool = s.Tool;
        Folder = s.Folder;
        KiroId = s.AcpId;
        Context = s.Context;
        Access = s.Access;
        foreach (var t in s.Turns)
        {
            var turn = new KiroTurn(t.Prompt, t.Images) { StartedAt = t.StartedAt, WokeAt = t.WokeAt, EndedAt = t.EndedAt ?? t.StartedAt, Credits = t.Credits };
            turn.Steps.AddRange(t.Steps);
            turn.Result = new KiroResult(t.State ?? KiroState.Cancelled, t.Text ?? "Stopped when Hover closed.");
            _turns.Add(turn);
        }
        State = _turns.Count > 0 ? _turns[^1].Result!.State : KiroState.Cancelled;
    }

    /// The prompt's first line, short enough for a label.
    public string Title
    {
        get
        {
            var line = Prompt.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).FirstOrDefault() ?? "";
            return line.Length > 60 ? line[..59] + "…" : line;
        }
    }

    public event Action? Changed;
    /// A turn ended; the argument is how.
    public event Action<KiroResult>? Ended;

    public bool Busy => State == KiroState.Running;

    private readonly List<(AgentAsk Ask, TaskCompletionSource<AskAnswer>? Done, TaskCompletionSource<IReadOnlyList<IReadOnlyList<string>>?>? Answers)> _asks = new();

    /// What the agent is waiting on the user for, oldest first; empty when nothing.
    public IReadOnlyList<AgentAsk> Asks => _asks.Select(a => a.Ask).ToList();
    public AgentAsk? Asking => _asks.Count > 0 ? _asks[0].Ask : null;
    public bool Waiting => _asks.Count > 0;

    /// The agent asks the user about a tool call. The answer comes from Answer(); a
    /// run that is stopped (or the token) withdraws the question with Deny.
    public Task<AskAnswer> Ask(AgentAsk ask, CancellationToken ct = default)
    {
        var done = new TaskCompletionSource<AskAnswer>(TaskCreationOptions.RunContinuationsAsynchronously);
        if (!Busy) { done.SetResult(AskAnswer.Deny); return done.Task; }
        _asks.Add((ask, done, null));
        Withdraw(ask, ct);
        Changed?.Invoke();
        return done.Task;
    }

    /// The agent asks the user a question (AgentAsk.Questions). The answer comes from
    /// AnswerQuestion(): each question's picked labels, in order. Null when the user
    /// skipped it, or it was withdrawn.
    public Task<IReadOnlyList<IReadOnlyList<string>>?> AskQuestion(AgentAsk ask, CancellationToken ct = default)
    {
        var done = new TaskCompletionSource<IReadOnlyList<IReadOnlyList<string>>?>(TaskCreationOptions.RunContinuationsAsynchronously);
        if (!Busy || !ask.IsQuestion) { done.SetResult(null); return done.Task; }
        _asks.Add((ask, null, done));
        Withdraw(ask, ct);
        Changed?.Invoke();
        return done.Task;
    }

    private void Withdraw(AgentAsk ask, CancellationToken ct)
    {
        if (!ct.CanBeCanceled) return;
        var ui = SynchronizationContext.Current;
        ct.Register(() => { if (ui is null) Answer(ask.Id, AskAnswer.Deny); else ui.Post(_ => Answer(ask.Id, AskAnswer.Deny), null); });
    }

    /// Answer what the agent asked. False when it isn't waiting on that one. Deny on a
    /// question skips it.
    public bool Answer(string id, AskAnswer answer)
    {
        var i = _asks.FindIndex(a => a.Ask.Id == id);
        if (i < 0) return false;
        var (_, done, answers) = _asks[i];
        _asks.RemoveAt(i);
        done?.TrySetResult(answer);
        answers?.TrySetResult(null);
        Changed?.Invoke();
        return true;
    }

    /// Answer a question the agent asked: the labels picked (or typed) for each of its
    /// questions. False when it isn't waiting on that one, or the answers don't fit.
    public bool AnswerQuestion(string id, IReadOnlyList<IReadOnlyList<string>> picked)
    {
        var i = _asks.FindIndex(a => a.Ask.Id == id && a.Answers is not null);
        if (i < 0 || picked.Count != _asks[i].Ask.Questions!.Count || picked.All(p => p.Count == 0)) return false;
        var (_, _, answers) = _asks[i];
        _asks.RemoveAt(i);
        answers!.TrySetResult(picked);
        Changed?.Invoke();
        return true;
    }

    private void DenyAll()
    {
        if (_asks.Count == 0) return;
        foreach (var (_, done, answers) in _asks) { done?.TrySetResult(AskAnswer.Deny); answers?.TrySetResult(null); }
        _asks.Clear();
    }

    /// Start the session with its first prompt. False, and nothing happens, when it
    /// has started already or the folder or prompt can't be used.
    public bool Start(string folder, string prompt, IReadOnlyList<string>? images = null)
    {
        if (State != KiroState.Idle || !KiroRunner.UsableFolder(folder) || !Usable(prompt, images)) return false;
        Folder = folder;
        var t = new KiroTurn(prompt.Trim(), images);
        _turns.Add(t);
        Begin(t);
        return true;
    }

    /// Reply in the session. While a turn runs the reply waits and starts when it
    /// ends. False when the session hasn't started or there is nothing to send.
    public bool Reply(string text, IReadOnlyList<string>? images = null)
    {
        if (State == KiroState.Idle || !Usable(text, images)) return false;
        var t = new KiroTurn(text.Trim(), images) { Queued = Busy };
        _turns.Add(t);
        if (t.Queued) Changed?.Invoke();
        else Begin(t);
        return true;
    }

    private static bool Usable(string text, IReadOnlyList<string>? images) => !string.IsNullOrWhiteSpace(text) || images is { Count: > 0 };

    private void Begin(KiroTurn t)
    {
        t.Queued = false;
        t.StartedAt = _now();
        Phase = KiroPhase.Starting;
        State = KiroState.Running;
        _cts = new CancellationTokenSource();
        Changed?.Invoke();
        _ = Go(t, _cts);
    }

    private async Task Go(KiroTurn turn, CancellationTokenSource cts)
    {
        var progress = new Progress<KiroPhase>(p =>
        {
            if (!Busy || Phase == p) return;
            Phase = p;
            if (p != KiroPhase.Starting) turn.WokeAt ??= _now();
            Changed?.Invoke();
        });
        var events = new Progress<KiroEvent>(e =>
        {
            if (e.SessionId is { } id) KiroId = id;
            if (e.Context is { } c) Context = c;
            if (e.Credits is { } credits) turn.Credits = credits;
            if (e.Step is { } step)
            {
                var i = turn.Steps.FindIndex(x => x.Id == step.Id);
                if (i >= 0) turn.Steps[i] = step; else turn.Steps.Add(step);
                turn.WokeAt ??= _now();
            }
            Changed?.Invoke();
        });
        KiroResult r;
        try { r = await _run(Folder, turn.Text, progress, cts.Token, KiroId, events); }
        catch (Exception e) { r = new KiroResult(KiroState.Failed, e.Message); }
        if (cts.IsCancellationRequested && r.State != KiroState.Completed) r = r with { State = KiroState.Cancelled };
        // A question the run left behind has nobody to answer it now.
        DenyAll();
        _cts = null;
        cts.Dispose();
        turn.Result = r;
        turn.EndedAt = _now();
        State = r.State;
        Core.Log.Line($"{Tool.ToString().ToLowerInvariant()} run {Id} turn {_turns.IndexOf(turn) + 1} {r.State.ToString().ToLowerInvariant()} after {Elapsed.TotalSeconds:0}s (exit {r.ExitCode?.ToString() ?? "-"})");
        // A stop drops the replies that were waiting; otherwise the next one goes.
        var next = _turns.FirstOrDefault(t => t.Queued);
        if (next is not null && r.State == KiroState.Cancelled)
        {
            foreach (var q in _turns.Where(t => t.Queued).ToList())
            {
                q.Queued = false;
                q.StartedAt = _now();
                q.EndedAt = q.StartedAt;
                q.Result = new KiroResult(KiroState.Cancelled, "Not sent: the run before it was stopped.");
            }
            next = null;
        }
        Changed?.Invoke();
        Ended?.Invoke(r);
        if (next is not null) Begin(next);
    }

    /// Stop the turn that runs. Kiro and whatever it started are killed, and replies
    /// waiting behind it are not sent.
    public void Stop()
    {
        if (!Busy) return;
        if (_asks.Count > 0) { DenyAll(); Changed?.Invoke(); }
        try { _cts?.Cancel(); }
        catch (ObjectDisposedException) { }
    }
}

/// Every Kiro session the page knows about, shared by the notch and the app window.
/// Several run side by side, each its own kiro-cli in its own folder. Each of those
/// takes a few hundred MB while it works, so only MaxRunning run at once (the user's
/// pick, one to MaxKept), and only the last MaxKept are kept (one per desk in the
/// office); the oldest finished one makes way for a new one.
public sealed class KiroSessions
{
    public const int DefaultRunning = 3, MaxKept = 6;

    /// How many run at once. A run already going is never stopped by a lower number.
    public int MaxRunning
    {
        get => _maxRunning;
        set => _maxRunning = Math.Clamp(value, 1, MaxKept);
    }
    private int _maxRunning = DefaultRunning;

    private readonly List<KiroSession> _all = new();
    private readonly Func<AgentTool, KiroSession> _make;

    public KiroSessions(Func<KiroSession>? make = null) : this(_ => (make ?? (() => new KiroSession()))()) { }

    /// Make gives a session whose turns go to that tool.
    public KiroSessions(Func<AgentTool, KiroSession> make) => _make = make;

    /// Where every session is kept once it has started, until the user deletes it.
    public AgentHistory? History { get; init; }

    /// Oldest first.
    public IReadOnlyList<KiroSession> All => _all;
    public int Running => _all.Count(s => s.Busy);
    public bool CanStart => Running < MaxRunning;

    /// The session the page last opened.
    public KiroSession? Selected { get; private set; }

    /// Any session changed, or one came or went.
    public event Action? Changed;
    public event Action<KiroSession, KiroResult>? Ended;

    public KiroSession? Start(string folder, string prompt, IReadOnlyList<string>? images = null) => Start(AgentTool.Kiro, folder, prompt, images);

    public KiroSession? Start(AgentTool tool, string folder, string prompt, IReadOnlyList<string>? images = null, string? access = null)
    {
        if (!CanStart || !KiroRunner.UsableFolder(folder) || (string.IsNullOrWhiteSpace(prompt) && images is not { Count: > 0 })) return null;
        if (!FreeDesk()) return null;
        var s = _make(tool);
        s.Tool = tool;
        s.Access = access;
        Seat(s);
        if (!s.Start(folder, prompt, images))
        {
            Remove(s);
            return null;
        }
        Save(s);
        Selected = s;
        Changed?.Invoke();
        return s;
    }

    /// A seventh session needs a desk: the oldest finished one gives up its own. It
    /// stays in the history.
    private bool FreeDesk()
    {
        if (_all.Count >= MaxKept && _all.FirstOrDefault(x => !x.Busy) is { } old) Remove(old);
        return _all.Count < MaxKept;
    }

    private void Seat(KiroSession s)
    {
        s.Seat = Enumerable.Range(0, MaxKept).First(i => _all.All(x => x.Seat != i));
        s.Bot = Enumerable.Range(0, MaxKept).First(i => _all.All(x => x.Bot != i));
        s.Changed += OnChanged;
        s.Ended += r => { Save(s); Ended?.Invoke(s, r); };
        _all.Add(s);
    }

    private void Save(KiroSession s)
    {
        if (!s.Deleted && s.Turns.Count > 0) History?.Save(s.Snapshot());
    }

    /// A reply in a session. False when it would start an agent beyond the cap.
    public bool Reply(KiroSession s, string text, IReadOnlyList<string>? images = null)
    {
        if (!_all.Contains(s) || (!s.Busy && !CanStart)) return false;
        if (!s.Reply(text, images)) return false;
        Save(s);
        return true;
    }

    /// The session a history entry is, at a desk: the one already there, or the saved
    /// one brought back to a free desk. Null when it can't be read or every desk is busy.
    public KiroSession? Wake(string key)
    {
        if (_all.FirstOrDefault(x => x.Key == key) is { } here) return here;
        if (History?.Load(key) is not { } saved || !FreeDesk()) return null;
        var s = _make(saved.Tool);
        s.Restore(saved);
        Seat(s);
        Changed?.Invoke();
        return s;
    }

    /// The history entry's record, whether or not it is at a desk now.
    public SavedSession? Saved(string key) =>
        _all.FirstOrDefault(x => x.Key == key)?.Snapshot() ?? History?.Load(key);

    private void OnChanged() => Changed?.Invoke();

    private void Remove(KiroSession s)
    {
        s.Changed -= OnChanged;
        _all.Remove(s);
        if (ReferenceEquals(Selected, s)) Selected = null;
    }

    public void Select(KiroSession? s)
    {
        if (s is not null && !_all.Contains(s)) return;
        if (ReferenceEquals(Selected, s)) return;
        Selected = s;
        Changed?.Invoke();
    }

    /// Take a finished session out of the office. It stays in the history.
    public void Dismiss(KiroSession s)
    {
        if (s.Busy || !_all.Contains(s)) return;
        Remove(s);
        Changed?.Invoke();
    }

    /// The user deleted a session: a run of it is stopped, and it leaves the office
    /// and the history.
    public void Delete(string key)
    {
        if (_all.FirstOrDefault(x => x.Key == key) is { } s)
        {
            s.Deleted = true;
            s.Stop();
            Remove(s);
        }
        History?.Delete(key);
        Changed?.Invoke();
    }

    public void StopAll()
    {
        foreach (var s in _all.ToList()) s.Stop();
    }

    /// Something the page shows changed outside a run, like the first-use note.
    public void RaiseChanged() => Changed?.Invoke();
}

/// What the notch and the office say about a session, in a few words: what its agent
/// is doing (a verb, and the file or command it is about), and what it asks for. No WPF.
public static class AgentWords
{
    /// The verb and its object: ("Editing", "refresh.ts"), ("Running", "npm test"),
    /// ("Thinking", "").
    public static (string Verb, string Object) Activity(KiroSession s)
    {
        if (s.State != KiroState.Running)
            return (s.State switch
            {
                KiroState.Completed => "Done", KiroState.Failed => "Couldn’t finish", KiroState.Cancelled => "Stopped", _ => "Ready",
            }, "");
        if (s.Phase == KiroPhase.Starting) return ("Starting", Agents.Name(s.Tool));
        var steps = s.Current?.Steps;
        var step = steps?.LastOrDefault(x => x.Status is "in_progress" or "pending");
        if (step is null && s.Phase is KiroPhase.Reading or KiroPhase.Searching or KiroPhase.Editing or KiroPhase.Running) step = steps?.LastOrDefault();
        if (step is null)
            return (s.Phase switch
            {
                KiroPhase.Thinking => "Thinking", KiroPhase.Planning => "Making a plan", KiroPhase.Writing => "Writing it up", _ => "Working",
            }, "");
        // An MCP tool: its server, then its name ("playwriter › execute").
        if (step.Kind is "other" && Mcp(step.Title) is var (server, tool))
            return server is null ? ("Using an MCP tool", "") : ($"{server} ›", tool);
        var verb = step.Kind switch
        {
            "read" => "Reading", "edit" => "Editing", "delete" => "Deleting", "move" => "Moving", "execute" => "Running",
            "search" => "Searching", "fetch" => "Fetching", "think" => "Thinking",
            _ => KiroStream.ToolPhase(step.Kind, step.Title) switch
            {
                KiroPhase.Reading => "Reading", KiroPhase.Editing => "Editing", KiroPhase.Running => "Running",
                KiroPhase.Searching => "Searching", _ => "Working",
            },
        };
        // A tool of the agent's own that says nothing of what it does: its name
        // ("Update Session Information") rather than "Working".
        if (verb == "Working" && Short(step.Target) is null && step.Title is { Length: > 0 } title && title != "Working")
            return (title.Length > 32 ? title[..31] + "…" : title, "");
        return (verb, Short(step.Target) ?? "");
    }

    private static readonly System.Text.RegularExpressions.Regex KiroMcp = new(@"^@([\w.-]+)/([\w.-]+)$");
    private static readonly System.Text.RegularExpressions.Regex CursorMcp = new(@"^([\w.-]+):\s+([\w.-]+)$");

    /// An MCP tool's server and name, from the title its agent gives the call: Kiro's
    /// "@playwriter/execute", Cursor's "playwriter: execute". Cursor's "MCP: tool"
    /// names neither: the server is null. Null when the title isn't an MCP tool's.
    public static (string? Server, string Tool)? Mcp(string? title)
    {
        if (string.IsNullOrWhiteSpace(title)) return null;
        var m = KiroMcp.Match(title.Trim());
        if (!m.Success) m = CursorMcp.Match(title.Trim());
        if (!m.Success) return null;
        var server = m.Groups[1].Value;
        return server.Equals("mcp", StringComparison.OrdinalIgnoreCase) ? (null, m.Groups[2].Value) : (server, m.Groups[2].Value);
    }

    /// A tool's name as the notch says it: "playwriter › execute" for an MCP tool,
    /// otherwise the title the agent gave it.
    public static string ToolName(string title) => Mcp(title) is var (server, tool) ? (server is null ? "an MCP tool" : $"{server} › {tool}") : title;

    /// A file's name, or a command's program and first word, short enough for the notch.
    public static string? Short(string? target)
    {
        if (string.IsNullOrWhiteSpace(target)) return null;
        var t = target.Trim().Replace('\n', ' ');
        if (t.Contains(' '))
        {
            var words = t.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            var head = System.IO.Path.GetFileName(words[0].Trim('"', '\'')) + (words.Length > 1 ? " " + words[1] : "");
            return head.Length > 26 ? head[..25] + "…" : head;
        }
        var name = System.IO.Path.GetFileName(t.TrimEnd('\\', '/').Replace('\\', '/'));
        if (name.Length == 0) name = t;
        return name.Length > 28 ? name[..27] + "…" : name;
    }

    /// The question in one line: ("Wants to run", "npm install").
    public static (string Verb, string Object) AskLine(AgentAsk a) => a.Kind switch
    {
        "question" => ("Asks you", a.Title),
        "execute" => ("Wants to run", Short(a.Command) ?? "a command"),
        "edit" => ("Wants to edit", Short(a.Path) ?? "a file"),
        "delete" => ("Wants to delete", Short(a.Path) ?? "files"),
        "move" => ("Wants to move", Short(a.Path) ?? "files"),
        "fetch" => ("Wants to go online", ""),
        _ => ("Wants to use", ToolName(a.Title)),
    };

    /// The question as its card's title.
    public static string AskTitle(AgentAsk a) => a.Kind switch
    {
        "question" => a.Questions is { Count: > 1 } q ? $"Asks you {q.Count} questions" : "Asks you a question",
        "execute" => "Wants to run a command",
        "edit" => $"Wants to edit {Short(a.Path) ?? "a file"}",
        "delete" => $"Wants to delete {Short(a.Path) ?? "files"}",
        "move" => $"Wants to move {Short(a.Path) ?? "files"}",
        "fetch" => "Wants to use the network",
        _ => $"Wants to use {ToolName(a.Title)}",
    };

    /// The word on the button that allows it.
    public static string AskAllow(AgentAsk a) => a.Kind switch
    {
        "execute" => "Run", "edit" => "Allow edit", "delete" => "Delete", "move" => "Move", "question" => "Answer", _ => "Allow",
    };

    /// Why it asks, in a few words, or "" when that only says again what the title
    /// says ("Wants to run a command": "Runs a command"). Network, outside the folder,
    /// what can't be taken back and how many lines change are kept.
    public static string AskWhy(AgentAsk a)
    {
        var lines = a.Added + a.Removed > 0 && a.Kind != "edit" ? $"+{a.Added} −{a.Removed}" : "";
        var reason = a.Reason is "Runs a command" or "Uses a tool" or "Edits a file" or "Deletes files" or "Moves or renames files" or "Uses the network" ? "" : a.Reason;
        return reason.Length > 0 && lines.Length > 0 ? reason + " · " + lines : reason + lines;
    }
}

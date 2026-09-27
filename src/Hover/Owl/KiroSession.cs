using Hover.Services;

namespace Hover.Owl;

/// One Kiro task: its folder, prompt, state and result. Lives on the UI thread:
/// Changed is raised there, and the runner's phases arrive there through Progress.
public sealed class KiroSession
{
    public delegate Task<KiroResult> RunTask(string folder, string prompt, IProgress<KiroPhase> progress, CancellationToken ct);

    private static int _ids;
    private readonly RunTask _run;
    private readonly Func<DateTime> _now;
    private CancellationTokenSource? _cts;

    public KiroSession(RunTask? run = null, Func<DateTime>? now = null)
    {
        _run = run ?? ((f, p, pr, ct) => KiroRunner.Run(f, p, pr, ct));
        _now = now ?? (() => DateTime.Now);
    }

    public int Id { get; } = Interlocked.Increment(ref _ids);
    public KiroState State { get; private set; } = KiroState.Idle;
    public KiroPhase Phase { get; private set; } = KiroPhase.Starting;
    public string Prompt { get; private set; } = "";
    public string Folder { get; private set; } = "";
    public KiroResult? Result { get; private set; }
    public DateTime StartedAt { get; private set; }
    public DateTime? EndedAt { get; private set; }
    public TimeSpan Elapsed => (EndedAt ?? _now()) - StartedAt;

    /// The prompt's first line, short enough for a label under a ghost.
    public string Title
    {
        get
        {
            var line = Prompt.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).FirstOrDefault() ?? "";
            return line.Length > 60 ? line[..59] + "…" : line;
        }
    }

    public event Action? Changed;
    /// The run ended; the argument is how.
    public event Action<KiroResult>? Ended;

    public bool Busy => State == KiroState.Running;

    /// Start the task. False, and nothing happens, when it has run already or the
    /// folder or prompt can't be used.
    public bool Start(string folder, string prompt)
    {
        if (State != KiroState.Idle || !KiroRunner.UsableFolder(folder) || string.IsNullOrWhiteSpace(prompt)) return false;
        Folder = folder;
        Prompt = prompt.Trim();
        Phase = KiroPhase.Starting;
        StartedAt = _now();
        State = KiroState.Running;
        _cts = new CancellationTokenSource();
        Changed?.Invoke();
        _ = Go(_cts);
        return true;
    }

    private async Task Go(CancellationTokenSource cts)
    {
        var progress = new Progress<KiroPhase>(p =>
        {
            if (!Busy || Phase == p) return;
            Phase = p;
            Changed?.Invoke();
        });
        KiroResult r;
        try { r = await _run(Folder, Prompt, progress, cts.Token); }
        catch (Exception e) { r = new KiroResult(KiroState.Failed, e.Message); }
        if (cts.IsCancellationRequested && r.State != KiroState.Completed) r = r with { State = KiroState.Cancelled };
        _cts = null;
        cts.Dispose();
        Result = r;
        State = r.State;
        EndedAt = _now();
        Core.Log.Line($"kiro run {Id} {r.State.ToString().ToLowerInvariant()} after {Elapsed.TotalSeconds:0}s (exit {r.ExitCode?.ToString() ?? "-"})");
        Changed?.Invoke();
        Ended?.Invoke(r);
    }

    /// Stop the task. Kiro and whatever it started are killed.
    public void Stop()
    {
        if (!Busy) return;
        try { _cts?.Cancel(); }
        catch (ObjectDisposedException) { }
    }
}

/// Every Kiro task the page knows about, shared by the notch and the app window.
/// Several run side by side, each its own kiro-cli in its own folder. Each of those
/// takes a few hundred MB while it works, so only MaxRunning run at once, and only
/// the last MaxKept are kept; the oldest finished one makes way for a new one.
public sealed class KiroSessions
{
    public const int MaxRunning = 3, MaxKept = 6;

    private readonly List<KiroSession> _all = new();
    private readonly Func<KiroSession> _make;

    public KiroSessions(Func<KiroSession>? make = null) => _make = make ?? (() => new KiroSession());

    /// Oldest first, as they stand on the stage.
    public IReadOnlyList<KiroSession> All => _all;
    public int Running => _all.Count(s => s.Busy);
    public bool CanStart => Running < MaxRunning;

    /// The session both views show in detail; null shows the prompt for a new one.
    public KiroSession? Selected { get; private set; }

    /// A half-written prompt, kept while the views are rebuilt.
    public string Draft { get; set; } = "";

    /// Any session changed, or one came or went.
    public event Action? Changed;
    public event Action<KiroSession, KiroResult>? Ended;

    public KiroSession? Start(string folder, string prompt) => Add(folder, prompt, _all.Count);

    /// The same task again, in its folder, in the old one's place on the stage.
    public KiroSession? Again(KiroSession old)
    {
        if (old.Busy) return null;
        var at = _all.IndexOf(old);
        if (at < 0) return null;
        var fresh = Add(old.Folder, old.Prompt, at);
        if (fresh is not null)
        {
            _all.Remove(old);
            Changed?.Invoke();
        }
        return fresh;
    }

    private KiroSession? Add(string folder, string prompt, int at)
    {
        if (!CanStart || !KiroRunner.UsableFolder(folder) || string.IsNullOrWhiteSpace(prompt)) return null;
        var s = _make();
        s.Changed += OnChanged;
        s.Ended += r => Ended?.Invoke(s, r);
        _all.Insert(at, s);
        if (!s.Start(folder, prompt))
        {
            _all.Remove(s);
            return null;
        }
        Trim();
        Selected = s;
        Changed?.Invoke();
        return s;
    }

    private void OnChanged() => Changed?.Invoke();

    private void Trim()
    {
        while (_all.Count > MaxKept && _all.FirstOrDefault(x => !x.Busy) is { } old)
        {
            old.Changed -= OnChanged;
            _all.Remove(old);
            if (ReferenceEquals(Selected, old)) Selected = null;
        }
    }

    public void Select(KiroSession? s)
    {
        if (s is not null && !_all.Contains(s)) return;
        if (ReferenceEquals(Selected, s)) return;
        Selected = s;
        Changed?.Invoke();
    }

    /// Take a finished session off the stage.
    public void Dismiss(KiroSession s)
    {
        if (s.Busy || !_all.Remove(s)) return;
        s.Changed -= OnChanged;
        if (ReferenceEquals(Selected, s)) Selected = null;
        Changed?.Invoke();
    }

    public void StopAll()
    {
        foreach (var s in _all.ToList()) s.Stop();
    }

    /// Something the page shows changed outside a run: the folder, or the first-use
    /// note. Both views redraw.
    public void RaiseChanged() => Changed?.Invoke();
}

using Hover.Services;

namespace Hover.Owl;

/// The one Kiro task Hover runs at a time, shared by the notch and the app window so
/// both show the same run. Lives on the UI thread: Changed is raised there, and the
/// runner's phases arrive there through Progress.
public sealed class KiroSession
{
    public delegate Task<KiroResult> RunTask(string folder, string prompt, IProgress<KiroPhase> progress, CancellationToken ct);

    private readonly RunTask _run;
    private readonly Func<DateTime> _now;
    private CancellationTokenSource? _cts;

    public KiroSession(RunTask? run = null, Func<DateTime>? now = null)
    {
        _run = run ?? ((f, p, pr, ct) => KiroRunner.Run(f, p, pr, ct));
        _now = now ?? (() => DateTime.Now);
    }

    public KiroState State { get; private set; } = KiroState.Idle;
    public KiroPhase Phase { get; private set; } = KiroPhase.Starting;
    /// The task and folder of the run in progress or the last one.
    public string Prompt { get; private set; } = "";
    public string Folder { get; private set; } = "";
    public KiroResult? Result { get; private set; }
    public DateTime StartedAt { get; private set; }
    public DateTime? EndedAt { get; private set; }
    public TimeSpan Elapsed => (EndedAt ?? _now()) - StartedAt;

    /// A half-written prompt, kept while the views are rebuilt (a theme change, the
    /// other window) so it isn't lost.
    public string Draft { get; set; } = "";

    public event Action? Changed;
    /// A run ended; the argument is how.
    public event Action<KiroResult>? Ended;

    public bool Busy => State == KiroState.Running;

    /// Start a task. False, and nothing happens, while another is running or when
    /// the folder or prompt can't be used.
    public bool Start(string folder, string prompt)
    {
        if (Busy || !KiroRunner.UsableFolder(folder) || string.IsNullOrWhiteSpace(prompt)) return false;
        Folder = folder;
        Prompt = prompt.Trim();
        Result = null;
        Phase = KiroPhase.Starting;
        StartedAt = _now();
        EndedAt = null;
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
            if (!ReferenceEquals(cts, _cts) || Phase == p) return;
            Phase = p;
            Changed?.Invoke();
        });
        KiroResult r;
        try { r = await _run(Folder, Prompt, progress, cts.Token); }
        catch (Exception e) { r = new KiroResult(KiroState.Failed, e.Message); }
        if (cts.IsCancellationRequested && r.State != KiroState.Completed) r = r with { State = KiroState.Cancelled };
        cts.Dispose();
        if (!ReferenceEquals(cts, _cts)) return;
        _cts = null;
        Result = r;
        State = r.State;
        EndedAt = _now();
        Core.Log.Line($"kiro run {r.State.ToString().ToLowerInvariant()} after {Elapsed.TotalSeconds:0}s (exit {r.ExitCode?.ToString() ?? "-"})");
        Changed?.Invoke();
        Ended?.Invoke(r);
    }

    /// Stop the running task. Kiro and whatever it started are killed.
    public void Stop()
    {
        if (!Busy) return;
        try { _cts?.Cancel(); }
        catch (ObjectDisposedException) { }
    }

    /// Something the Kiro page shows changed outside a run: the folder, or the
    /// first-use note. Both views redraw.
    public void RaiseChanged() => Changed?.Invoke();

    /// Back to a fresh prompt after a run has ended.
    public void Reset()
    {
        if (Busy) return;
        State = KiroState.Idle;
        Result = null;
        Changed?.Invoke();
    }
}

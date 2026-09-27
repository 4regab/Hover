namespace Hover.Owl;

/// The focus timer: a countdown with a limit, or a stopwatch. Time is measured from
/// a clock rather than counted in ticks, so a stalled UI thread cannot lose seconds.
/// Every stretch of running time is handed to Credited as it ends, which is what
/// Insights adds up — paused time is never counted.
public sealed class FocusTimer
{
    public enum Phase { Ready, Running, Paused }

    private readonly Func<DateTime> _now;
    private TimeSpan _banked;
    private DateTime? _since;

    public FocusTimer(Func<DateTime>? now = null) => _now = now ?? (() => DateTime.Now);

    public Phase State { get; private set; } = Phase.Ready;
    public bool Stopwatch { get; private set; }
    public TimeSpan Duration { get; private set; } = TimeSpan.FromMinutes(25);
    /// The task being focused on, if any.
    public string? TaskId { get; private set; }

    public event Action<DateTime, TimeSpan>? Credited;
    public event Action? Changed;

    public TimeSpan Elapsed => _banked + (_since is { } s ? _now() - s : TimeSpan.Zero);
    public TimeSpan Remaining => Duration - Elapsed is var r && r > TimeSpan.Zero ? r : TimeSpan.Zero;
    public bool Finished => !Stopwatch && State != Phase.Ready && Elapsed >= Duration;
    /// 0..1 through the countdown; 0 for a stopwatch.
    public double Progress => Stopwatch || Duration <= TimeSpan.Zero ? 0
        : Math.Clamp(Elapsed.TotalSeconds / Duration.TotalSeconds, 0, 1);

    /// What the clock face shows.
    public TimeSpan Shown => Stopwatch ? Elapsed : State == Phase.Ready ? Duration : Remaining;

    public void Configure(TimeSpan duration, bool stopwatch = false)
    {
        Duration = duration > TimeSpan.Zero ? duration : TimeSpan.FromMinutes(25);
        Stopwatch = stopwatch;
        Changed?.Invoke();
    }

    public void Attach(string? taskId)
    {
        TaskId = taskId;
        Changed?.Invoke();
    }

    public void Start()
    {
        if (State == Phase.Running) return;
        if (State == Phase.Ready) _banked = TimeSpan.Zero;
        _since = _now();
        State = Phase.Running;
        Changed?.Invoke();
    }

    public void Pause()
    {
        if (State != Phase.Running) return;
        // State first: a Credited listener may end the session, and must not have
        // it put back to Paused afterwards.
        State = Phase.Paused;
        Bank();
        Changed?.Invoke();
    }

    public void Resume() => Start();

    public void Toggle()
    {
        if (State == Phase.Running) Pause();
        else Start();
    }

    public void AddFive()
    {
        Duration += TimeSpan.FromMinutes(5);
        Changed?.Invoke();
    }

    /// End the session. Returns the active time it held.
    public TimeSpan Stop()
    {
        if (State == Phase.Running) Bank();
        var total = _banked;
        _banked = TimeSpan.Zero;
        _since = null;
        State = Phase.Ready;
        TaskId = null;
        Changed?.Invoke();
        return total;
    }

    private void Bank()
    {
        if (_since is not { } s) return;
        var now = _now();
        // A countdown stops counting at its limit even if nobody noticed it end.
        var span = now - s;
        if (!Stopwatch && _banked + span > Duration) span = Duration - _banked;
        // Cleared before Credited runs: a listener that ends the session from inside
        // it must find nothing left to bank, or the same stretch is counted twice.
        _since = null;
        if (span <= TimeSpan.Zero) return;
        _banked += span;
        Credited?.Invoke(s, span);
    }

    /// The face text. A countdown rounds up (it reads 25:00 until a whole second has
    /// gone), a stopwatch rounds down.
    public string Text => Format(Shown, roundUp: !Stopwatch);

    public static string Format(TimeSpan t, bool roundUp = true)
    {
        var secs = roundUp ? (int)Math.Ceiling(t.TotalSeconds - 0.0001) : (int)Math.Floor(t.TotalSeconds);
        if (secs < 0) secs = 0;
        return secs >= 3600
            ? $"{secs / 3600}:{secs / 60 % 60:00}:{secs % 60:00}"
            : $"{secs / 60:00}:{secs % 60:00}";
    }
}

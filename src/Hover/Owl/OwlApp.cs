using System.Windows.Threading;
using Hover.Core;
using Microsoft.Win32;

namespace Hover.Owl;

/// The workspace's shared state: one planner, one focus timer, today's calendar.
/// Every view (the notch panel and the dashboard window) draws from here, so they
/// stay in step without talking to each other.
public static class OwlApp
{
    public static Planner Planner => Planner.Shared;
    public static FocusTimer Timer { get; } = new();

    /// Once a second, for clock faces.
    public static event Action? Tick;
    public static event Action? EventsChanged;
    /// Midnight passed: every view rebuilds for the new day.
    public static event Action? DayChanged;

    /// The card layout changed. The argument is the view that changed it, if any.
    public static event Action<object?>? LayoutChanged;
    public static void RaiseLayoutChanged(object? source) => LayoutChanged?.Invoke(source);

    /// The latest usage reading for each quota the notch shows, and when it was taken.
    public static event Action? QuotasChanged;
    public static IReadOnlyDictionary<string, (QuotaReading Reading, DateTime At)> Quotas => _quotas;
    private static readonly Dictionary<string, (QuotaReading Reading, DateTime At)> _quotas = new();
    private static readonly HashSet<string> _quotaBusy = new();
    private static readonly TimeSpan QuotaEvery = TimeSpan.FromMinutes(5);

    public static IReadOnlyList<CalEvent> Events { get; private set; } = Array.Empty<CalEvent>();
    /// Null while nothing has been fetched; otherwise the last error, or "".
    public static string? CalendarError { get; private set; }
    public static bool CalendarBusy { get; private set; }

    // Host hooks, set by NotchHost.
    public static Action<string, string>? Notify { get; set; }
    public static Action? OpenDashboard { get; set; }
    public static Action? Collapse { get; set; }
    public static Action? ShowWorkspace { get; set; }
    /// The dashboard window, opened on its Settings tab.
    public static Action? OpenSettings { get; set; }
    /// A workspace preference changed that the notch draws from.
    public static Action? SettingsChanged { get; set; }

    private static DispatcherTimer? _tick;
    private static bool _ending;
    private static DateOnly _day;
    private static string? _ics;
    private static DateTime _fetched = DateTime.MinValue;
    private static readonly TimeSpan FetchEvery = TimeSpan.FromMinutes(15);

    public static void Start()
    {
        Timer.Credited += (start, span) => Planner.AddFocus(start, span);
        ResetDuration();
        _day = Planner.Today;
        Planner.Changed += DropStaleSession;
        if (Planner.RollOver()) Planner.Save();

        // Timers stop while the PC sleeps, so Insights only ever counts time at the desk.
        SystemEvents.PowerModeChanged += (_, e) =>
        {
            if (e.Mode == PowerModes.Suspend) Dispatch(Timer.Pause);
        };

        // Normal, not the default Background: WPF holds Background work back while
        // any input is waiting in the queue, and under UI Automation traffic that
        // froze the clock for ten seconds at a time. A focus timer must not stall.
        _tick = new DispatcherTimer(DispatcherPriority.Normal) { Interval = TimeSpan.FromSeconds(1) };
        _tick.Tick += (_, _) => OnTick();
        _tick.Start();
        _ = RefreshCalendar();
    }

    private static void Dispatch(Action a) => System.Windows.Application.Current?.Dispatcher.BeginInvoke(a);

    private static void OnTick()
    {
        if (Timer.Finished)
        {
            var task = Planner.Find(Timer.TaskId);
            EndSession();
            Notify?.Invoke("Time's up", task is null ? "Your focus session is complete." : $"Focus on “{task.Title}” is complete.");
        }

        foreach (var r in Planner.TakeDueReminders())
            Notify?.Invoke("Reminder", r.Title);

        if (Planner.Today != _day)
        {
            _day = Planner.Today;
            if (Planner.RollOver()) Planner.Save();
            Reparse();
            DayChanged?.Invoke();
        }

        if (Planner.Data.CalendarSource.Length > 0 && !CalendarBusy && DateTime.Now - _fetched > FetchEvery)
            _ = RefreshCalendar();

        RefreshQuotas();
        Tick?.Invoke();
    }

    // MARK: Quotas

    /// Read each quota the notch shows once it is five minutes old — or now, when
    /// forced (switched on, or Refresh in Settings). Readings for quotas switched off
    /// are dropped so a stale number never comes back with the switch. While the
    /// notch is not always shown nothing displays them, so nothing is read either.
    public static void RefreshQuotas(bool force = false)
    {
        var on = NotchItem.Quotas.Where(Settings.HasNotchItem).ToList();
        var dropped = _quotas.Keys.Where(k => !on.Contains(k)).ToList();
        foreach (var k in dropped) _quotas.Remove(k);
        if (dropped.Count > 0) QuotasChanged?.Invoke();
        if (!force && !Settings.ShowIdleNotch) return;

        foreach (var id in on)
        {
            if (_quotaBusy.Contains(id)) continue;
            if (!force && _quotas.TryGetValue(id, out var q) && DateTime.Now - q.At < QuotaEvery) continue;
            _quotaBusy.Add(id);
            _ = ReadQuota(id);
        }
    }

    private static async Task ReadQuota(string id)
    {
        QuotaReading r;
        try
        {
            // Off the UI thread entirely: the PATH walk, the log scan and the SQLite
            // read are all synchronous.
            r = id switch
            {
                NotchItem.Kiro => await Task.Run(() => Quota.Kiro(CancellationToken.None)),
                NotchItem.Codex => await Task.Run(() => Quota.Codex(DateTime.Now)),
                _ => await Task.Run(() => Quota.Cursor(CancellationToken.None)),
            };
        }
        catch (Exception e)
        {
            r = QuotaReading.Fail(e.Message);
        }
        if (!r.Ok) Log.Line($"quota {id}: {r.Detail}");
        // Resumes on the UI thread: the await was started from the dispatcher.
        _quotaBusy.Remove(id);
        if (!Settings.HasNotchItem(id)) return;
        _quotas[id] = (r, DateTime.Now);
        QuotasChanged?.Invoke();
    }

    /// A session whose task was finished or deleted elsewhere ends with it.
    private static void DropStaleSession()
    {
        if (Timer.TaskId is { } id && Planner.Find(id) is not { Done: false })
            EndSession();
    }

    // MARK: Focus

    public static void StartFocus(PlanTask task)
    {
        if (Timer.State != FocusTimer.Phase.Ready) EndSession();
        // A task with no limit of its own keeps the stopwatch if that is what is set.
        Timer.Configure(TimeSpan.FromMinutes(task.LimitMinutes ?? Planner.Data.DefaultFocusMinutes),
            task.LimitMinutes is null && Timer.Stopwatch);
        Timer.Attach(task.Id);
        Timer.Start();
    }

    /// The check next to the timer: the task is done and the session ends.
    public static void CompleteFocus()
    {
        var id = Timer.TaskId;
        EndSession();
        if (id is not null) Planner.SetDone(id, true);
    }

    public static void EndSession()
    {
        // Stopping credits focus time, which saves the planner, which raises Changed,
        // which lands back in DropStaleSession while this session is still ending.
        if (_ending) return;
        _ending = true;
        try
        {
            Timer.Stop();
            ResetDuration();
        }
        finally { _ending = false; }
    }

    public static void ResetDuration() =>
        Timer.Configure(TimeSpan.FromMinutes(Planner.Data.DefaultFocusMinutes), Timer.Stopwatch);

    /// Set the length from the timer's own "Set time".
    public static void SetDuration(int minutes, bool start)
    {
        Timer.Configure(TimeSpan.FromMinutes(minutes));
        if (Timer.TaskId is { } id) Planner.SetLimit(id, minutes);
        if (start && Timer.State != FocusTimer.Phase.Running) Timer.Start();
    }

    // MARK: Calendar

    public static async Task RefreshCalendar()
    {
        var source = Planner.Data.CalendarSource;
        if (source.Length == 0)
        {
            _ics = null;
            CalendarError = null;
            Events = Array.Empty<CalEvent>();
            EventsChanged?.Invoke();
            return;
        }
        CalendarBusy = true;
        EventsChanged?.Invoke();
        try
        {
            _ics = await Calendar.Fetch(source);
            CalendarError = "";
        }
        catch (Exception e)
        {
            Log.Line($"calendar fetch failed — {e.Message}");
            CalendarError = e.Message;
        }
        finally
        {
            _fetched = DateTime.Now;
            CalendarBusy = false;
        }
        Reparse();
    }

    private static void Reparse()
    {
        try
        {
            Events = _ics is null ? Array.Empty<CalEvent>() : Calendar.For(_ics, Planner.Today);
        }
        catch (Exception e)
        {
            Log.Line($"calendar parse failed — {e.Message}");
            CalendarError = "The calendar file could not be read.";
            Events = Array.Empty<CalEvent>();
        }
        EventsChanged?.Invoke();
    }

    /// Called as the app quits, so a running session's time is not lost.
    public static void Shutdown() => Timer.Pause();
}

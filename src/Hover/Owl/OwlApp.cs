using System.Windows.Threading;
using Hover.Core;

namespace Hover.Owl;

/// The shared state: the agents, their sessions, and the quota readings. Every view
/// (the notch and the app window) draws from here, so they stay in step without
/// talking to each other.
public static class OwlApp
{
    /// Each agent tool's ACP process, shared by all of its sessions and shut down when
    /// it has been idle for the time in its settings.
    public static IReadOnlyDictionary<Services.AgentTool, Services.AcpHost> Agents { get; } =
        Services.Agents.All.ToDictionary(t => t, t =>
        {
            var host = new Services.AcpHost(t, () => Settings.AgentOptions(t));
            // What the tool offers (models, efforts) fills in its settings page.
            host.OptionsSeen += (tool, offers) => Dispatch(() => Settings.SetAgentOffers(tool, offers));
            // A question goes to the session whose conversation it is, on the UI thread,
            // where the notch and the office show it. One nobody holds is turned down.
            host.Asking = (sid, ask, ct) =>
            {
                var answer = new TaskCompletionSource<Services.AskAnswer>(TaskCreationOptions.RunContinuationsAsynchronously);
                var app = System.Windows.Application.Current;
                if (app is null) { answer.SetResult(Services.AskAnswer.Deny); return answer.Task; }
                app.Dispatcher.BeginInvoke(() =>
                {
                    // Kiro is made after Agents; by the time a tool asks, it is there.
                    var s = Kiro?.All.FirstOrDefault(x => x.Tool == t && x.KiroId == sid && x.Busy);
                    if (s is null) { answer.TrySetResult(Services.AskAnswer.Deny); return; }
                    Log.Line($"{Services.Agents.Id(t)} run {s.Id} asks: {ask.Kind} ({ask.Reason})");
                    s.Ask(ask, ct).ContinueWith(a => answer.TrySetResult(a.Result), TaskScheduler.Default);
                });
                return answer.Task;
            };
            return host;
        });

    /// The office's runs, several at once, each with Kiro, Codex or Cursor.
    public static KiroSessions Kiro { get; } = new(tool => new KiroSession((f, p, pr, ct, resume, events) =>
        Agents[tool].Run(f, p, pr, ct, resume, events)))
    {
        // Every session, sealed, until the user deletes it; the office's bookshelf lists them.
        History = new AgentHistory(System.IO.Path.Combine(Paths.Support, "agents")),
    };

    /// Kiro tasks that ended while no Kiro page was in view. The resting notch keeps
    /// saying so until one is looked at.
    public static int KiroUnseen { get; private set; }

    /// The tool of those unseen ends, or null when they were different tools.
    public static string? KiroUnseenTool { get; private set; }

    /// The latest of those ends, for the notch: which tool, the task, how it went and
    /// how long it took.
    public static (Services.AgentTool Tool, string Title, Services.KiroState State, TimeSpan Took)? KiroUnseenLast { get; private set; }

    /// A Kiro page came into view: the ends it announced have been seen.
    public static void KiroSeen()
    {
        if (KiroUnseen == 0) return;
        KiroUnseen = 0;
        KiroUnseenLast = null;
        Kiro.RaiseChanged();
    }

    /// The latest usage reading for each quota the notch shows, and when it was taken.
    public static event Action? QuotasChanged;
    public static void RaiseQuotasChanged() => QuotasChanged?.Invoke();
    public static IReadOnlyDictionary<string, (QuotaReading Reading, DateTime At)> Quotas => _quotas;
    private static readonly Dictionary<string, (QuotaReading Reading, DateTime At)> _quotas = new();
    private static readonly HashSet<string> _quotaBusy = new();
    private static readonly TimeSpan QuotaEvery = TimeSpan.FromMinutes(5);

    // Host hooks, set by NotchManager.
    public static Action<string, string>? Notify { get; set; }
    public static Action? OpenDashboard { get; set; }
    public static Action? Collapse { get; set; }
    /// The shortcut and the tray: open the notch, or close it.
    public static Action? ShowOffice { get; set; }
    /// The app window, opened on Settings.
    public static Action? OpenSettings { get; set; }
    /// A preference changed that the notch draws from.
    public static Action? SettingsChanged { get; set; }

    private static DispatcherTimer? _tick;

    public static void Start()
    {
        DropPlanner();

        // Normal, not the default Background: WPF holds Background work back while
        // any input is waiting in the queue, and under UI Automation traffic that
        // held timers back for ten seconds at a time.
        _tick = new DispatcherTimer(DispatcherPriority.Normal) { Interval = TimeSpan.FromSeconds(30) };
        _tick.Tick += (_, _) => RefreshQuotas();
        _tick.Start();
        RefreshQuotas();

        // A Kiro task can take minutes; the notch has usually been folded away by the
        // time it ends, so the end is announced, unless the office is in view and it
        // was seen happening.
        Kiro.Ended += (s, r) =>
        {
            if (KiroPage.Watching) return;
            KiroUnseen++;
            var who = Services.Agents.Name(s.Tool);
            KiroUnseenTool = KiroUnseen == 1 || KiroUnseenTool == who ? who : null;
            KiroUnseenLast = (s.Tool, s.Title, r.State, s.Elapsed);
            Notify?.Invoke((r.State switch
            {
                Services.KiroState.Completed => $"{who} is done",
                Services.KiroState.Cancelled => $"{who} stopped",
                _ => $"{who} couldn't finish",
            }) + ": " + s.Title, FirstLine(KiroText.Plain(r.Text)));
        };
    }

    /// Tasks, the notepad, focus time and the calendar address went with the
    /// workspace in 2.0, and the user chose to have them deleted, not kept. The key
    /// (note.key) stays: the agents' history is sealed with it.
    public static void DropPlanner()
    {
        try
        {
            foreach (var f in System.IO.Directory.EnumerateFiles(Paths.Support, "planner.dat*"))
            {
                System.IO.File.Delete(f);
                Log.Line($"removed {System.IO.Path.GetFileName(f)} (the workspace is gone)");
            }
        }
        catch (Exception e) when (e is System.IO.IOException or UnauthorizedAccessException)
        {
            Log.Line($"couldn't remove the old planner - {e.Message}");
        }
    }

    private static string FirstLine(string text)
    {
        var line = text.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).FirstOrDefault() ?? "";
        line = line.TrimStart('#', ' ', '*');
        return line.Length > 120 ? line[..119] + "…" : line;
    }

    private static void Dispatch(Action a) => System.Windows.Application.Current?.Dispatcher.BeginInvoke(a);

    // MARK: Quotas

    /// Read each quota that is switched on once it is five minutes old — or now, when
    /// forced (switched on, or Refresh in Settings). Readings for quotas switched off
    /// are dropped so a stale number never comes back with the switch.
    public static void RefreshQuotas(bool force = false)
    {
        var on = NotchItem.Quotas.Where(Settings.HasNotchItem).ToList();
        var dropped = _quotas.Keys.Where(k => !on.Contains(k)).ToList();
        foreach (var k in dropped) _quotas.Remove(k);
        if (dropped.Count > 0) QuotasChanged?.Invoke();

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
                NotchItem.Claude => await Task.Run(() => Quota.Claude(DateTime.Now, CancellationToken.None)),
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

    /// Called as the app quits. A running task is stopped rather than left working
    /// with nobody watching.
    public static void Shutdown()
    {
        Kiro.StopAll();
        foreach (var host in Agents.Values) host.Shutdown("Hover quit");
        Kiro.History?.Flush();
    }
}

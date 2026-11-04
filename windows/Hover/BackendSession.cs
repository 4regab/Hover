using Hover.Host;
using Hover.Host.Protocol;

namespace Hover;

/// <summary>
/// The app's one backend: opens the history key, starts hover-backend through Hover.Host
/// and says in words how it is doing. Later phases hand the backend's messages to the
/// notch, the office and the chat from here.
/// </summary>
public sealed class BackendSession : IAsyncDisposable
{
    BackendClient? client;
    bool stopping;

    /// <summary>One line for the dashboard: starting, ready, or why not.</summary>
    public string Status { get; private set; } = "Starting the agent backend…";

    /// <summary>What is wrong with the backend, or null while it starts or runs well. The app window shows it in its title bar and shows nothing otherwise.</summary>
    public string? Problem { get; private set; }

    /// <summary>True once the backend answered <c>initialize</c>.</summary>
    public bool Ready { get; private set; }

    public event Action? Changed;

    /// <summary>A message from the backend. Raised on the backend reader's thread, not the UI thread.</summary>
    public event Action<BackendEvent>? Event;

    /// <summary>Sends a command; nothing happens while there is no backend.</summary>
    public void Send(BackendCommand command) => client?.Send(command);

    public async Task StartAsync(string dataDir)
    {
        // Null is "no key this run": the backend still starts, with the history off, and
        // nothing is sealed that the next run could not open (HistoryKey says why in the log).
        var key = HistoryKey.LoadOrCreate(dataDir, new DpapiKeyGuard());

        var exe = BackendOptions.BesideApp();
        if (!File.Exists(exe))
        {
            Update($"The agent backend is missing: {exe}", problem: true);
            return;
        }

        try
        {
            client = BackendClient.Start(new BackendOptions { ExecutablePath = exe, DataDir = dataDir });
            HostLog.Line($"backend started (pid {client.ProcessId})");
            client.Event += OnEvent;
            // A backend that ends by itself leaves the app without agents: say what to do. (The backend's own words, when it gave a reason, are
            // kept: it said them just before.)
            client.Exited += code =>
            {
                if (stopping) return;
                if (Problem is not null && failureSaid) return;
                Update($"The agent backend stopped ({code}). Quit Hover from its tray menu and open it again.", problem: true);
            };
            // The Discord status is the app's (Hover.Settings' DiscordPresence), so the backend runs no client of its own; Kiro's
            // credits by day are kept from the start, for Settings → Kiro.
            await client.InitializeAsync(key, hostDiscord: true, credits: true);
            Ready = true;
            // The settings the notch follows, and the first look at the tools and the quotas.
            client.Send(new SettingsCommand());
            client.Send(new ReadyCommand());
            Update(key is null ? "The agent backend is ready. The history is off this run: its key could not be read." : "The agent backend is ready.", problem: key is null);
        }
        catch (Exception e) when (e is BackendException or InvalidOperationException or System.ComponentModel.Win32Exception)
        {
            Update($"The agent backend did not start: {e.Message}", problem: true);
        }
    }

    bool failureSaid;

    void OnEvent(BackendEvent e)
    {
        if (e is ToastEvent toast) HostLog.Line($"backend toast: {toast.Text}");
        // The backend gives up with a reason (a history it cannot open, say) and then waits to be let go: the words are the bar's.
        if (e is BackendFailureEvent failure && !stopping)
        {
            failureSaid = true;
            Update(failure.Text, problem: true);
        }
        Event?.Invoke(e);
    }

    void Update(string status, bool problem = false)
    {
        HostLog.Line($"backend: {status}");
        Status = status;
        Problem = problem ? status : null;
        Changed?.Invoke();
    }

    /// <summary>Quits the backend in order, so it writes the history and the settings first.</summary>
    public async ValueTask DisposeAsync()
    {
        stopping = true;
        if (client is null) return;
        var code = await client.StopAsync(TimeSpan.FromSeconds(10));
        HostLog.Line($"backend stopped (exit code {code})");
        await client.DisposeAsync();
        client = null;
    }
}

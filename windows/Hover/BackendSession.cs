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

    /// <summary>One line for the dashboard: starting, ready, or why not.</summary>
    public string Status { get; private set; } = "Starting the agent backend…";

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
            Update($"The agent backend is missing: {exe}");
            return;
        }

        try
        {
            client = BackendClient.Start(new BackendOptions { ExecutablePath = exe, DataDir = dataDir });
            HostLog.Line($"backend started (pid {client.ProcessId})");
            client.Event += OnEvent;
            client.Exited += code => Update($"The agent backend stopped ({code}).");
            await client.InitializeAsync(key);
            Ready = true;
            // The settings the notch follows, and the first look at the tools and the quotas.
            client.Send(new SettingsCommand());
            client.Send(new ReadyCommand());
            Update(key is null ? "The agent backend is ready. The history is off this run: its key could not be read." : "The agent backend is ready.");
        }
        catch (Exception e) when (e is BackendException or InvalidOperationException or System.ComponentModel.Win32Exception)
        {
            Update($"The agent backend did not start: {e.Message}");
        }
    }

    void OnEvent(BackendEvent e)
    {
        if (e is ToastEvent toast) HostLog.Line($"backend toast: {toast.Text}");
        Event?.Invoke(e);
    }

    void Update(string status)
    {
        HostLog.Line($"backend: {status}");
        Status = status;
        Changed?.Invoke();
    }

    /// <summary>Quits the backend in order, so it writes the history and the settings first.</summary>
    public async ValueTask DisposeAsync()
    {
        if (client is null) return;
        await client.StopAsync(TimeSpan.FromSeconds(10));
        await client.DisposeAsync();
        client = null;
    }
}

using Hover.Host;
using Hover.Host.Protocol;
using Hover.Settings;

namespace Hover.Sharing;

/// <summary>
/// Hover on the Discord status, driven by the backend's messages: which agents are at work (<c>state</c>) and whether the switch in
/// Settings → Integrations is on (<c>settings</c>, <c>preferences</c>). The pipe and the rules are <see cref="DiscordPresence"/>. The
/// backend is told at <c>initialize</c> not to run a Discord client of its own (<c>hostDiscord</c>), so the status shows once.
/// </summary>
sealed class DiscordHost : IDisposable
{
    readonly BackendSession backend;
    readonly DiscordPresence presence;
    readonly Action<BackendEvent> handler;
    volatile bool on;
    volatile string[] tools = [];

    public DiscordHost(BackendSession backend)
    {
        this.backend = backend;
        presence = new DiscordPresence(() => on ? tools : null, log: HostLog.Line);
        handler = OnEvent;
        backend.Event += handler;
    }

    /// <summary>The tools with a task at work, each once, in the order they started (<c>discordWorking</c>).</summary>
    public static string[] Working(StateEvent state)
    {
        var names = new List<string>();
        foreach (var s in state.Sessions)
        {
            if (s.Stage is not ("working" or "waking")) continue;
            var name = Tools.ById(s.Tool)?.Name ?? s.Tool;
            if (!names.Contains(name)) names.Add(name);
        }
        return [.. names];
    }

    void OnEvent(BackendEvent e)
    {
        switch (e)
        {
            case SettingsEvent s:
                on = SettingsData.From(s.Model).DiscordPresence;
                break;
            case PreferencesEvent p:
                on = p.DiscordPresence;
                break;
            case StateEvent st:
                tools = Working(st);
                break;
            default:
                return;
        }
        presence.Wake();
    }

    public void Dispose()
    {
        backend.Event -= handler;
        presence.Dispose();
    }
}

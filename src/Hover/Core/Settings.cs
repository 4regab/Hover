using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Windows.Input;
using System.Windows.Threading;
using Microsoft.Win32;

namespace Hover.Core;

public enum Appearance { System, Light, Dark }

/// How big the notch opens. Each is capped by the display it opens on.
public enum WorkspaceSize { Default, Small, Large, ExtraLarge }

/// The handful of preferences, in one JSON file in Hover's folder.
/// Writes are debounced through Save(), which every setter calls. Keys an older
/// build wrote (the notes deck's) are ignored on load and dropped on the next save.
public static class Settings
{
    private sealed class Model
    {
        public bool HoverOpensWorkspace { get; set; } = true;
        public List<string>? NotchItems { get; set; }
        public Appearance Appearance { get; set; } = Appearance.System;
        public SavedTheme? Theme { get; set; }
        public WorkspaceSize WorkspaceSize { get; set; }
        public string? KiroFolder { get; set; }
        public bool KiroNoticeSeen { get; set; }
        public string? KiroModel { get; set; }
        public string KiroEffort { get; set; } = "high";
        public string? KiroAgent { get; set; }
        public bool KiroReadOnly { get; set; }
        public bool KiroRequireMcp { get; set; }
        public int KiroIdleMinutes { get; set; } = 5;
        public bool KiroHideSteps { get; set; }
        public Services.AgentApproval KiroApproval { get; set; }
        /// Codex's and Cursor's settings, by tool id. Kiro's are the fields above.
        public Dictionary<string, Services.AgentOptions>? Agents { get; set; }
        /// What each tool last offered (models, efforts, modes), for its settings page.
        public Dictionary<string, List<Services.AcpOption>>? AgentOffers { get; set; }
        public string? AgentTool { get; set; }

        // Option-N on a Mac. Alt+N here also means "Insert" in Office and "File name"
        // in file dialogs; while Hover runs, it opens the notch instead.
        public Shortcut ScWorkspace { get; set; } = new(ModifierKeys.Alt, Key.N);
    }

    private static readonly JsonSerializerOptions Json = new()
    {
        WriteIndented = true,
        Converters = { new JsonStringEnumConverter() },
    };

    private static readonly Model M = Load();

    private static Model Load()
    {
        try
        {
            if (File.Exists(Paths.SettingsFile))
            {
                var m = JsonSerializer.Deserialize<Model>(File.ReadAllText(Paths.SettingsFile), Json);
                if (m is not null) return m;
            }
        }
        catch (Exception e)
        {
            Log.Line($"settings load failed — {e.Message}");
        }
        return new Model();
    }

    private static DispatcherTimer? _writeBack;

    /// Every setter calls this, several at a time — so the write itself waits for the value to settle. Flush() forces it
    /// out when the app is closing.
    public static void Save()
    {
        _writeBack?.Stop();
        _writeBack = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(400) };
        _writeBack.Tick += (_, _) => Flush();
        _writeBack.Start();
    }

    public static void Flush()
    {
        _writeBack?.Stop();
        _writeBack = null;
        try
        {
            File.WriteAllText(Paths.SettingsFile, JsonSerializer.Serialize(M, Json));
        }
        catch (Exception e)
        {
            Log.Line($"settings save failed — {e.Message}");
        }
    }

    /// Resting the pointer on the notch opens it. Off, it takes the
    /// shortcut or a click — the top edge is where maximised browsers keep their tabs.
    public static bool HoverOpensWorkspace
    {
        get => M.HoverOpensWorkspace;
        set { M.HoverOpensWorkspace = value; Save(); }
    }

    /// What the resting notch shows, in order. See NotchItem for the ids. The quota
    /// items are off until switched on, since each reads another app's sign-in.
    public static IReadOnlyList<string> NotchItems
    {
        // Ids an older build saved (the focus timer's) are left out.
        get => M.NotchItems = NotchItem.All.Where((M.NotchItems ?? new List<string>()).Contains).ToList();
        set { M.NotchItems = NotchItem.All.Where(value.Contains).ToList(); Save(); }
    }

    public static bool HasNotchItem(string id) => NotchItems.Contains(id);

    /// Light, dark, or whatever Windows is set to. Applies to Hover's own theme.
    public static Appearance Appearance
    {
        get => M.Appearance;
        set { M.Appearance = value; Save(); }
    }

    /// A theme taken from a VS Code colour theme, or null for Hover's own.
    public static SavedTheme? Theme
    {
        get => M.Theme;
        set { M.Theme = value; Save(); }
    }

    public static WorkspaceSize WorkspaceSize
    {
        get => M.WorkspaceSize;
        set { M.WorkspaceSize = value; Save(); }
    }

    public static void SetNotchItem(string id, bool on)
    {
        var set = NotchItems.ToHashSet();
        if (on) set.Add(id); else set.Remove(id);
        NotchItems = set.ToList();
    }

    public static Shortcut ScWorkspace { get => M.ScWorkspace; set { M.ScWorkspace = value; Save(); } }

    /// The project folder the Kiro page last ran in, as picked. It is kept even when
    /// it has gone missing; Services.KiroRunner.UsableFolder decides whether it can
    /// still be used, and the page asks for another rather than fall back to one.
    public static string? KiroFolder
    {
        get => M.KiroFolder;
        set { M.KiroFolder = string.IsNullOrWhiteSpace(value) ? null : value; Save(); }
    }

    /// The Kiro page's one-time note about full tool access has been read.
    public static bool KiroNoticeSeen
    {
        get => M.KiroNoticeSeen;
        set { M.KiroNoticeSeen = value; Save(); }
    }

    /// How an agent's runs are set up (Settings → Kiro, Codex, Cursor). Read when a
    /// run starts.
    public static Services.AgentOptions AgentOptions(Services.AgentTool t)
    {
        if (t == Services.AgentTool.Kiro)
            return new(M.KiroModel, M.KiroEffort, M.KiroReadOnly, M.KiroIdleMinutes, M.KiroAgent, M.KiroRequireMcp, M.KiroHideSteps, M.KiroApproval);
        return M.Agents?.GetValueOrDefault(Services.Agents.Id(t)) ?? Services.AgentOptions.Default;
    }

    public static void SetAgentOptions(Services.AgentTool t, Services.AgentOptions value)
    {
        value = value with
        {
            Model = value.Model is null or "auto" ? null : value.Model,
            Agent = string.IsNullOrWhiteSpace(value.Agent) ? null : value.Agent,
            IdleMinutes = Services.AgentOptions.IdleChoices.Contains(value.IdleMinutes) ? value.IdleMinutes : Services.AgentOptions.IdleChoices[0],
        };
        if (t == Services.AgentTool.Kiro)
        {
            M.KiroModel = value.Model;
            M.KiroEffort = value.Effort ?? "high";
            M.KiroAgent = value.Agent;
            M.KiroReadOnly = value.ReadOnly;
            M.KiroRequireMcp = value.RequireMcp;
            M.KiroIdleMinutes = value.IdleMinutes;
            M.KiroHideSteps = value.HideSteps;
            M.KiroApproval = value.Approval;
        }
        // An agent is Kiro's (the fields above) and OpenCode's (Build, Plan, the user's own).
        else (M.Agents ??= new())[Services.Agents.Id(t)] = value with { Agent = t == Services.AgentTool.OpenCode ? value.Agent : null, RequireMcp = false };
        Save();
    }

    /// The models, efforts and modes the tool offered the last time it ran.
    public static IReadOnlyList<Services.AcpOption> AgentOffers(Services.AgentTool t) =>
        M.AgentOffers?.GetValueOrDefault(Services.Agents.Id(t)) ?? new List<Services.AcpOption>();

    public static void SetAgentOffers(Services.AgentTool t, IReadOnlyList<Services.AcpOption> offers)
    {
        var id = Services.Agents.Id(t);
        var old = M.AgentOffers?.GetValueOrDefault(id);
        // Every turn reports them; only a change is written.
        if (old is not null && JsonSerializer.Serialize(old) == JsonSerializer.Serialize(offers)) return;
        (M.AgentOffers ??= new())[id] = offers.ToList();
        Save();
    }

    /// The tool the last new task went to.
    public static Services.AgentTool AgentTool
    {
        get => Services.Agents.Parse(M.AgentTool) ?? Services.AgentTool.Kiro;
        set { M.AgentTool = Services.Agents.Id(value); Save(); }
    }

    // MARK: Launch at login — HKCU Run, no elevation needed

    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string RunValue = "Hover";

    public static bool LaunchAtLogin
    {
        get
        {
            try
            {
                using var key = Registry.CurrentUser.OpenSubKey(RunKey);
                return key?.GetValue(RunValue) is string s && s.Length > 0;
            }
            catch { return false; }
        }
        set
        {
            try
            {
                using var key = Registry.CurrentUser.CreateSubKey(RunKey);
                if (key is null) return;
                if (value)
                {
                    var exe = Environment.ProcessPath;
                    if (exe is null) return;
                    key.SetValue(RunValue, $"\"{exe}\"");
                }
                else key.DeleteValue(RunValue, throwOnMissingValue: false);
            }
            catch (Exception e)
            {
                Log.Line($"launch-at-login toggle failed — {e.Message}");
            }
        }
    }
}

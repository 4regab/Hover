using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Windows.Input;
using System.Windows.Threading;
using Microsoft.Win32;

namespace Hover.Core;

public enum Appearance { System, Light, Dark }

/// How big the workspace opens. Each is capped by the display it opens on.
public enum WorkspaceSize { Default, Small, Large, ExtraLarge }

/// A button in the workspace header that opens a terminal and runs Command there,
/// in Folder (the user's folder when empty). Icon is an icon name, Color an accent
/// name (see Ui.AccentNamed), so the button follows the theme.
public sealed record LaunchButton(string Name, string Command, string Icon, string Color, string? Folder = null);

/// The handful of preferences, in one JSON file beside the planner.
/// Writes are debounced through Save(), which every setter calls. Keys an older
/// build wrote (the notes deck's) are ignored on load and dropped on the next save.
public static class Settings
{
    private sealed class Model
    {
        public bool HoverOpensWorkspace { get; set; } = true;
        public List<string>? NotchItems { get; set; }
        public bool QuotasOnNotch { get; set; }
        public Appearance Appearance { get; set; } = Appearance.System;
        public SavedTheme? Theme { get; set; }
        public WorkspaceSize WorkspaceSize { get; set; }
        public List<LaunchButton>? Buttons { get; set; }
        public List<CardSlot>? Cards { get; set; }
        public string? KiroFolder { get; set; }
        public bool KiroNoticeSeen { get; set; }

        // Option-N on a Mac. Alt+N here also means "Insert" in Office and "File name"
        // in file dialogs; while Hover runs, it opens the workspace instead.
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

    /// Every setter calls this, and a splitter calls its setter on every pixel of the
    /// drag — so the write itself waits for the value to settle. Flush() forces it
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

    /// Resting the pointer on the notch opens the workspace. Off, it takes the
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
        get => M.NotchItems ??= new List<string> { NotchItem.Timer };
        set { M.NotchItems = NotchItem.All.Where(value.Contains).ToList(); Save(); }
    }

    public static bool HasNotchItem(string id) => NotchItems.Contains(id);

    /// The AI quotas that are switched on always show in the workspace header. On,
    /// they also stay on the resting notch.
    public static bool QuotasOnNotch
    {
        get => M.QuotasOnNotch;
        set { M.QuotasOnNotch = value; Save(); }
    }

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

    /// The header's command buttons, in order.
    public static IReadOnlyList<LaunchButton> Buttons
    {
        get => M.Buttons ??= new List<LaunchButton>();
        set { M.Buttons = value.ToList(); Save(); }
    }

    public static void SetNotchItem(string id, bool on)
    {
        var set = NotchItems.ToHashSet();
        if (on) set.Add(id); else set.Remove(id);
        NotchItems = set.ToList();
    }

    /// The workspace cards: which show, in what order, and how wide.
    public static IReadOnlyList<CardSlot> Cards
    {
        get => M.Cards = CardLayout.Normalize(M.Cards);
        set { M.Cards = CardLayout.Normalize(value); Save(); }
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

using System.Text.Json;
using Hover.Host.Protocol;

namespace Hover.Settings;

/// <summary>What Settings asks of the app that shows it (<c>app.Host</c>). Everything is called on the thread that draws.</summary>
public interface ISettingsEnv
{
    void Send(BackendCommand command);

    /// <summary>Launch at login is outside settings.json: the Run value of this user, which only the app can write (the backend is hover-backend.exe).</summary>
    bool LaunchAtLogin();

    void SetLaunchAtLogin(bool on);

    bool SystemDark();

    void Quit();

    /// <summary>Asks for a folder with the system's dialog; <paramref name="done"/> gets it, or null when the dialog was cancelled.</summary>
    void ChooseFolder(Action<string?> done);

    /// <summary>Asks for a VS Code theme file (.json or .jsonc).</summary>
    void ChooseThemeFile(Action<string?> done);

    /// <summary>Opens a file in the user's editor (Kiro's mcp.json).</summary>
    void OpenInEditor(string path);

    /// <summary>The input devices by name.</summary>
    IReadOnlyList<string> Microphones();

    /// <summary>Try it: the button went down / came up.</summary>
    void TrialPress();

    void TrialRelease();
}

/// <summary>A picker's menu while it is open: the row that asked, its options, and where.</summary>
public sealed record OpenMenu(string Id, IReadOnlyList<Opt> Options, float X, float Y);

/// <summary>
/// The page's state and what each click in it does: <c>app.Pane</c> and view.go's handlers. It follows the backend's
/// messages, builds the open section's blocks, and turns a click, a key or a pick into commands. It draws nothing.
/// </summary>
public sealed partial class SettingsController
{
    readonly ISettingsEnv env;
    SettingsData data = SettingsData.Default;
    PreferencesEvent? prefs;
    IReadOnlyList<ToolInfo> tools = [];
    readonly Dictionary<string, QuotaValue> quotas = [];
    readonly Dictionary<string, bool> secrets = [];
    bool secretsKept = true;
    CreditsView? credits;
    IReadOnlyList<string> kiroAgents = [];
    string kiroAgentsFor = "";
    string workspacePath = "";
    readonly List<InstalledTheme> installed = [];
    List<ThemeListing> listed = [];
    bool askedThemes;
    readonly Dictionary<string, AppPalette> themePalettes = [];
    readonly HashSet<string> askedPalettes = [];
    IReadOnlyList<Block>? blocks;
    DateTime micsAt = DateTime.MinValue;

    public SettingsController(ISettingsEnv env) => this.env = env;

    /// <summary>The page, or the palette, changed: draw it again.</summary>
    public event Action? Changed;

    /// <summary>The palette Settings and the app window use changed (a theme, the appearance, the system's light or dark).</summary>
    public event Action<AppPalette>? PaletteChanged;

    public Section Section { get; private set; } = Section.General;

    /// <summary>The project open in Projects (its id), "" for the list.</summary>
    public string Project { get; private set; } = "";

    public OpenMenu? Menu { get; private set; }
    public string ImportStatus { get; private set; } = "";

    /// <summary>The last action's message, by the id of its control.</summary>
    public (string Id, string Text)? Note { get; private set; }

    /// <summary>The notch shortcut or the voice one is recording (the next chord is taken).</summary>
    public bool Recording { get; private set; }

    public bool RecordingVoice { get; private set; }

    /// <summary>The recorder's words ("Press keys…", the modifier hint), or null when the field shows the shortcut.</summary>
    public string? RecorderField { get; private set; }

    public AppPalette Palette { get; private set; } = Palettes.HoverDark;
    public SettingsData Data => data;
    public Live Live { get; } = new();

    /// <summary>What the last failure said (a toast); the painter shows it for a moment.</summary>
    public string? ToastText { get; private set; }

    public DateTime ToastAt { get; private set; }

    public IReadOnlyList<InstalledTheme> Installed => installed;

    /// <summary>Whether a key by that name is kept (the page never knows more of it).</summary>
    public bool HasSecret(string name) => secrets.TryGetValue(name, out var has) && has;

    // What the page changed and the backend has not answered yet. A message that was on its way before the change must not
    // put the old value back (and a second change made at once would be built on it), so the change is laid over every model
    // that comes until the model has it, or a few seconds have passed (the backend turned it down or kept it in other words).
    readonly Dictionary<string, (Func<SettingsData, SettingsData> Apply, Func<SettingsData, bool> Done, DateTime Until)> held = [];

    /// <summary>Shows a change at once and keeps showing it until the backend's model has it.</summary>
    void Hold(string key, Func<SettingsData, SettingsData> apply, Func<SettingsData, bool> done)
    {
        held[key] = (apply, done, DateTime.UtcNow.AddSeconds(3));
        data = apply(data);
    }

    SettingsData Held(SettingsData parsed)
    {
        foreach (var key in held.Keys.ToList())
        {
            var (apply, done, until) = held[key];
            if (done(parsed) || DateTime.UtcNow > until) held.Remove(key);
            else parsed = apply(parsed);
        }
        return parsed;
    }

    void Dirty()
    {
        blocks = null;
        Changed?.Invoke();
    }

    /// <summary>The open section's blocks, built again after anything that changes them.</summary>
    public IReadOnlyList<Block> Blocks => blocks ??= BuildBlocks();

    IReadOnlyList<Block> BuildBlocks()
    {
        var voiceShortcut = data.Voice.Shortcut.Label;
        var notch = data.Notch.Label;
        var field = RecorderField;
        var input = new PageInput
        {
            Settings = data,
            LaunchAtLogin = env.LaunchAtLogin(),
            Shortcut = Recording && !RecordingVoice && field is not null ? field : notch,
            VoiceShortcut = Recording && RecordingVoice && field is not null ? field : voiceShortcut,
            Reading = id => quotas.TryGetValue(id, out var q) ? q : null,
            Ready = ReadyOf,
            EffortLabel = t => tools.FirstOrDefault(x => x.Id == t.Id) is { EffortLabel: { Length: > 0 } l } ? l : t.Id == "opencode" ? "Variant" : "Effort",
            Installed = installed,
            PaletteOf = t => themePalettes.TryGetValue(t.ToJson().ToJsonString(), out var p) ? p : null,
            SystemDark = env.SystemDark(),
            ImportStatus = ImportStatus,
            KiroAgents = kiroAgents,
            HasSecret = HasSecret,
            SecretsKept = secretsKept,
            Project = Project,
            Note = Note,
            Live = Live,
            Credits = credits,
            WorkspacePath = workspacePath,
        };
        return PageBuilder.Build(Section, input);
    }

    ToolReady? ReadyOf(ToolDef t) => tools.FirstOrDefault(x => x.Id == t.Id) is { } k ? new ToolReady(k.CheckedYet, k.Ready, k.Hint) : null;

    // MARK: Messages from the backend

    /// <summary>Asks for what the pages show. Call it once the backend is ready.</summary>
    public void Start()
    {
        env.Send(new SettingsCommand());
        env.Send(new ProjectsCommand());
        foreach (var n in new[] { PageBuilder.GroqSecret, "cleanup.gemini", "cleanup.openai", "cleanup.custom" }) env.Send(new SecretCommand("has", n));
        env.Send(new CreditsCommand());
        env.Send(VoiceCommandsFor.Phonon);
        Palette = PaletteForSettings();
    }

    /// <summary>Takes a message from the backend. Call it on the thread that draws.</summary>
    public void Accept(BackendEvent message)
    {
        var e = SettingsEvents.Read(message);
        switch (e)
        {
            case SettingsEvent s:
                data = Held(SettingsData.From(s.Model));
                if (Project != "" && data.Project(Project) is null) Project = "";
                AskKiroAgents();
                AskThemePalette();
                Apply();
                Dirty();
                break;
            case PreferencesEvent p:
                prefs = p;
                Dirty();
                break;
            case StateEvent st:
                tools = st.Tools;
                Dirty();
                break;
            case QuotasEvent q:
                foreach (var (id, v) in q.Values) quotas[id] = v;
                Dirty();
                break;
            case ToastEvent t:
                ToastText = t.Text;
                ToastAt = DateTime.UtcNow;
                Dirty();
                break;
            case ProjectsEvent p:
                OnProjects(p);
                break;
            case SecretEvent s:
                OnSecret(s);
                break;
            case CreditsEvent c:
                if (c.Ok && c.Ready) credits = CreditsView.From(c.View);
                Dirty();
                break;
            case KiroAgentsEvent k:
                if (k.Ok && k.Folder == kiroAgentsFor) kiroAgents = k.Agents;
                Dirty();
                break;
            case ThemesEvent t:
                OnThemes(t);
                break;
            case ThemeReadEvent r:
                OnThemeRead(r);
                break;
            case PaletteEvent p:
                OnPalette(p);
                break;
            case McpEvent m:
                OnMcp(m);
                break;
            case UnknownEvent { Type: "phonon" } u:
                OnPhonon(u.Raw);
                break;
            case UnknownEvent { Type: "groqCheck" } u:
                Live.GroqCheck = u.Raw.TryGetProperty("said", out var said) && said.ValueKind == JsonValueKind.String ? said.GetString() : "";
                Dirty();
                break;
        }
    }

    void OnSecret(SecretEvent s)
    {
        if (s.Name is null) return;
        if (!s.Ok)
        {
            // The error never holds the key. It goes under the field that asked.
            Note = (SecretFieldFor(s.Name), s.Error ?? "The key couldn’t be saved.");
            Dirty();
            return;
        }
        secrets[s.Name] = s.Has;
        secretsKept = s.Persistent;
        Dirty();
    }

    string SecretFieldFor(string name) => name == PageBuilder.GroqSecret ? "VoiceGroqKey" : "VoiceCleanupKey";

    void OnProjects(ProjectsEvent p)
    {
        if (p.DefaultWorkspace?.Path is { } path) workspacePath = path;
        if (!p.Ok && p.Error is not null)
        {
            Note = (ProjectNoteId(p.Op), p.Error);
            Dirty();
            return;
        }
        // The list is in the settings model too: after a change the model is read again, so every page shows the same.
        if (p.Op is not (null or "list")) env.Send(new SettingsCommand());
        Dirty();
    }

    string ProjectNoteId(string? op) => op switch { "add" => "ProjectAdd", "default" => lastDefaultControl, "remove" => "ProjectRemove", _ => lastProjectControl };

    string lastProjectControl = "ProjectName";
    string lastDefaultControl = "DefaultFolder";

    void AskKiroAgents()
    {
        var folder = data.KiroFolder ?? "";
        if (folder == kiroAgentsFor && kiroAgentsAsked) return;
        kiroAgentsFor = folder;
        kiroAgentsAsked = true;
        env.Send(new KiroAgentsCommand(folder == "" ? null : folder));
    }

    bool kiroAgentsAsked;

    // MARK: The palette

    AppPalette PaletteForSettings()
    {
        if (data.Theme is { } t) return themePalettes.TryGetValue(t.ToJson().ToJsonString(), out var p) ? p : Palettes.Hover(t.Dark);
        var dark = data.Appearance switch { "Light" => false, "Dark" => true, _ => env.SystemDark() };
        return Palettes.Hover(dark);
    }

    /// <summary>Works out the palette again (the system's dark mode changed, or a theme or the appearance did) and tells whoever draws.</summary>
    public void Apply()
    {
        var palette = PaletteForSettings();
        if (palette == Palette) return;
        Palette = palette;
        PaletteChanged?.Invoke(palette);
    }

    void AskThemePalette()
    {
        if (data.Theme is not { } t) return;
        var key = t.ToJson().ToJsonString();
        if (themePalettes.ContainsKey(key) || !askedPalettes.Add(key)) return;
        env.Send(new ThemePaletteCommand(t.ToJson()));
        pendingTheme = key;
    }

    string? pendingTheme;

    void OnPalette(PaletteEvent p)
    {
        if (!p.Ok || Palettes.Parse(p.Palette) is not { } palette) return;
        if (pendingTheme is { } key)
        {
            themePalettes[key] = palette;
            pendingTheme = null;
        }
        Apply();
        Dirty();
    }

    // MARK: Themes

    void OnThemes(ThemesEvent t)
    {
        if (!t.Ok) return;
        listed = t.Themes;
        foreach (var l in listed.Take(200)) env.Send(new ThemeReadCommand(l.Path));
    }

    void OnThemeRead(ThemeReadEvent r)
    {
        if (importPath is not null && r.Path == importPath)
        {
            importPath = null;
            if (r.Ok && ThemeData.From(r.Theme) is { } theme)
            {
                if (Palettes.Parse(r.Palette) is { } palette) themePalettes[theme.ToJson().ToJsonString()] = palette;
                ApplyTheme(theme);
            }
            else
            {
                ImportStatus = "That file has no VS Code theme colours in it.";
                Dirty();
            }
            return;
        }
        if (!r.Ok || r.Path is null) return;
        var l = listed.FirstOrDefault(x => x.Path == r.Path);
        if (l is null || ThemeData.From(r.Theme) is not { } t || Palettes.Parse(r.Palette) is not { } p) return;
        // A theme file the backend could not read is left out, as the Go page leaves it out.
        installed.RemoveAll(x => x.Path == l.Path);
        installed.Add(new InstalledTheme(l.Label, l.Path, l.Dark, l.From, t, p));
        // The backend's order (by label) is the page's.
        var order = listed.Select((x, k) => (x.Path, k)).ToDictionary(x => x.Path, x => x.k);
        installed.Sort((a, b) => order.GetValueOrDefault(a.Path, int.MaxValue).CompareTo(order.GetValueOrDefault(b.Path, int.MaxValue)));
        Dirty();
    }

    string? importPath;

    void ApplyTheme(ThemeData? theme)
    {
        ImportStatus = "";
        env.Send(new SetSettingCommand("Theme", theme?.ToJson()));
    }

    // MARK: Opening

    public void Open(Section section)
    {
        Section = section;
        Menu = null;
        Project = "";
        Note = null;
        Live.Mcp.Form = null;
        Live.Mcp.Confirm = null;
        Live.Mcp.Notice = null;
        OnOpened();
        Dirty();
    }

    /// <summary>Opens a project's own page in Projects.</summary>
    public void OpenProject(string id)
    {
        Section = Section.Projects;
        Project = id;
        Dirty();
    }

    void OnOpened()
    {
        switch (Section)
        {
            case Section.General when !askedThemes:
                askedThemes = true;
                env.Send(new ThemesCommand());
                break;
            case Section.Voice:
                RefreshMics();
                env.Send(VoiceCommandsFor.Phonon);
                break;
            case Section.Kiro:
                env.Send(new McpCommand());
                env.Send(new CreditsCommand());
                AskKiroAgents();
                break;
            case Section.Integrations:
                env.Send(new RefreshCommand());
                break;
        }
    }

    /// <summary>Reads the microphones (at most every ten seconds) when the Voice page opens.</summary>
    public void RefreshMics()
    {
        if (DateTime.UtcNow - micsAt < TimeSpan.FromSeconds(10)) return;
        micsAt = DateTime.UtcNow;
        var mics = env.Microphones();
        if (!mics.SequenceEqual(Live.Mics))
        {
            Live.Mics = mics;
            Dirty();
        }
    }

    /// <summary>Draws the page again (something the app knows changed: Phonon, Try it, a shortcut's error).</summary>
    public void Refresh() => Dirty();
}

/// <summary>The voice commands Settings sends, by name (Hover.Voice's own are in that library; this one needs no reference to it).</summary>
static class VoiceCommandsFor
{
    public static RawCommand Phonon => new("phonon", new System.Text.Json.Nodes.JsonObject { ["op"] = "get" });

    public static RawCommand PhononOp(string op) => new("phonon", new System.Text.Json.Nodes.JsonObject { ["op"] = op });

    public static RawCommand GroqCheck => new("groqCheck");
}

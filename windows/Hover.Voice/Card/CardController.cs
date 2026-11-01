using System.Text.Json.Nodes;
using Hover.Host.Protocol;

namespace Hover.Voice;

/// <summary>What the card asks of the app, by name: start, cancel, toggleCloud, retry, settings, unattach (N), edit (S), pick (S), openMenu (N), pickModel (S), pickEffort (S), pickOpt (S), search (S), searchEnter.</summary>
public readonly record struct CardAct(string Name, string S = "", int N = 0);

/// <summary>
/// The card's behaviour with no window: the menus (agent, model and effort, folder, Kiro Web
/// repository), the screenshot feedback, the amber glow of a turned-away press, and what each
/// button sends. It is the part of <c>internal/shell/voice.go</c> that is not drawing; the painter
/// draws <see cref="View"/> and tells it about clicks and keys.
/// </summary>
public sealed class VoiceCardController : IDisposable
{
    readonly VoiceService service;
    readonly object gate = new();
    readonly Action playChime;
    StateEvent? state;
    ProjectsEvent? projects;
    string holdError = "";
    int menu;
    long menuId;
    string repoQuery = "";
    string note = "";
    bool flash, busy;
    long shotSeen;
    DateTime listAsked = DateTime.MinValue, projectsAsked = DateTime.MinValue;
    bool listed, listing;
    List<string> repos = [];
    string reposError = "";
    Timer? holdErrorTimer, noteTimer, flashTimer, busyTimer;
    bool disposed;

    /// <summary>The card needs redrawing. Raised on whatever thread the change came on.</summary>
    public event Action? Changed;

    /// <summary>Settings → Voice was asked for (the card's Settings button).</summary>
    public event Action? SettingsRequested;

    /// <summary>Something to say while dictating, where there is no card: the screenshot note.</summary>
    public event Action<string>? Toast;

    public VoiceCardController(VoiceService service, Action? playChime = null)
    {
        this.service = service;
        this.playChime = playChime ?? (() => Chime.Play());
        service.Changed += OnVoice;
        service.Busy += OnBusy;
    }

    /// <summary>Hand every backend message to this too: the tools, the projects and the repositories are read from them.</summary>
    public void OnBackend(BackendEvent e)
    {
        switch (e)
        {
            case StateEvent s:
                lock (gate) state = s;
                break;
            case ProjectsEvent p when p.Ok:
                lock (gate) projects = p;
                break;
            case CloudReposEvent r:
                lock (gate)
                {
                    if (!r.Done)
                    {
                        // The list saved last time, shown while Kiro is asked again.
                        if (r.Ok) (listed, repos, reposError) = (true, [.. r.Repos], "");
                    }
                    else
                    {
                        listing = false;
                        if (r.Ok) (listed, repos, reposError) = (true, [.. r.Repos], "");
                        else if (!(listed && reposError.Length == 0)) (listed, repos, reposError) = (true, [], r.Error ?? "Kiro couldn’t list your repositories.");
                    }
                }
                break;
            default:
                return;
        }
        Changed?.Invoke();
    }

    // MARK: What is shown

    /// <summary>The shortcut could not be taken (the Go app's words), or can again (empty). Shown once, for 12 s, or until dismissed.</summary>
    public string HoldError
    {
        get { lock (gate) return holdError; }
        set
        {
            lock (gate)
            {
                if (holdError == value) return;
                holdError = value;
                holdErrorTimer?.Dispose();
                holdErrorTimer = value.Length == 0 ? null : new Timer(_ => { lock (gate) holdError = ""; Changed?.Invoke(); }, null, TimeSpan.FromSeconds(12), Timeout.InfiniteTimeSpan);
            }
            Changed?.Invoke();
        }
    }

    public int Menu { get { lock (gate) return OpenMenu(service.State); } }

    VoicePreview? MenuPreview(VoiceState st) =>
        st.Stage is VoiceStage.Preview or VoiceStage.Editing && st.Preview is { Trial: false } p ? p : null;

    /// <summary>The open menu, or 0 when the interaction it was opened for has gone.</summary>
    int OpenMenu(VoiceState st) => MenuPreview(st) is { } p && menuId == p.Id ? menu : 0;

    CardContext Context(VoiceState st)
    {
        var tools = state?.Tools ?? [];
        return new CardContext
        {
            Settings = service.Settings,
            Tools = tools,
            Sessions = state?.Sessions ?? [],
            Projects = projects?.Projects ?? [],
            DefaultWorkspace = projects?.DefaultWorkspace?.Path ?? "",
            HoldError = holdError,
            Trial = service.Trial,
        };
    }

    /// <summary>The card for the stage the backend last sent, with the menu that is open on it. Null when nothing shows.</summary>
    public VoiceView? View
    {
        get
        {
            var st = service.State;
            return ViewOf(st);
        }
    }

    /// <summary>The view of any stage (the pictures draw one the backend never sent).</summary>
    public VoiceView? ViewOf(VoiceState st)
    {
        lock (gate)
        {
            var c = Context(st);
            var card = VoiceCards.Build(st, c);
            if (card is null) return null;
            var view = new VoiceView
            {
                Card = card,
                Level = st.Stage == VoiceStage.Recording ? (float)st.Level : 0,
                Busy = busy,
                Shots = st.Shots,
                Note = note,
                Flash = flash,
                AuraColor = c.Settings.AuraColor,
                Tools = ToolsOf(st, c),
            };
            return MenuOf(view, st, c);
        }
    }

    IReadOnlyList<CardTool> ToolsOf(VoiceState st, CardContext c)
    {
        if (st.Stage == VoiceStage.ChooseAgent && st.Pending is { } pend) return pend.Tools.Select(t => new CardTool(t, VoiceCards.ToolName(c, t))).ToList();
        if (MenuPreview(st) is { } p && OpenMenu(st) == 1)
        {
            var ready = st.ReadyTools.ToList();
            if (ready.Count == 0) ready.Add(p.Tool);
            return ready.Select(t => new CardTool(t, VoiceCards.ToolName(c, t))).ToList();
        }
        return [];
    }

    VoiceView MenuOf(VoiceView view, VoiceState st, CardContext c)
    {
        var which = OpenMenu(st);
        if (MenuPreview(st) is not { } p || which == 0) return view;
        switch (which)
        {
            case 1:
                return view with
                {
                    Menu = 1,
                    MenuHead = "AGENT FOR THIS TASK",
                    MenuNote = "Agents that are installed and signed in. The default is in Settings → Voice.",
                };
            case 2:
            {
                var tool = c.Tools.FirstOrDefault(t => t.Id == p.Tool);
                var models = tool?.Models ?? [];
                var cur = tool?.Model ?? (models.Count > 0 ? models[0].Id : "");
                var levels = models.FirstOrDefault(m => m.Id == cur)?.Levels is { Count: > 0 } l ? l : tool?.Efforts ?? [];
                var effort = tool?.Effort;
                var shown = effort is not null && levels.Contains(effort) ? effort : null;
                var name = VoiceCards.ToolName(c, p.Tool);
                // The heading stays for Auto, which says it picks the effort itself; a model with none listed has none.
                var head = levels.Count == 0 && !string.Equals(cur, "auto", StringComparison.OrdinalIgnoreCase) ? "" : (tool?.EffortLabel ?? "Effort").ToUpperInvariant();
                return view with
                {
                    Menu = 2,
                    MenuHead = (name + " model").ToUpperInvariant(),
                    Models = models.Select(m => new MenuOpt(m.Id, VoiceCards.ShortModel(m.Name), m.Id == cur)).ToList(),
                    EffortHead = head,
                    Efforts = levels.Select(e => new MenuOpt(e, VoiceCards.EffortWord(e), e == shown)).ToList(),
                    MenuNote = "Used by " + name + " from its next turn.",
                };
            }
            case 3:
            {
                var atHome = c.DefaultWorkspace.Length > 0 && Folders.Same(c.DefaultWorkspace, p.Folder);
                var rows = new List<MenuOpt> { new("", "Default workspace", atHome) };
                rows.AddRange(st.FolderChoices.Select(f => new MenuOpt(f.Id, f.Name, !atHome && Folders.Same(f.Folder, p.Folder))));
                return view with { Menu = 3, MenuHead = "FOLDER FOR THIS TASK", Opts = rows, MenuNote = "Your voice projects. Add more in Settings → Projects." };
            }
            case 4:
            {
                var (list, repoNote) = ConnectedRepos();
                var matches = list.Any(r => RepoMatches(r, repoQuery));
                if (list.Count > 0 && !matches) repoNote = string.Concat("No repository matches “", repoQuery.Trim(), "”. ", repoNote).TrimEnd(' ');
                var rows = new List<MenuOpt>
                {
                    new("", "This folder’s repository", p.RepoKind == "folder"),
                    new("-", "Empty workspace", p.RepoKind == "empty"),
                };
                rows.AddRange(list.Where(r => RepoMatches(r, repoQuery)).Select(r => new MenuOpt(r, r, p.RepoKind == "named" && p.RepoName == r)));
                return view with { Menu = 4, MenuHead = "REPOSITORY FOR KIRO WEB", Opts = rows, MenuNote = repoNote };
            }
        }
        return view;
    }

    /// <summary>The repositories typed so far can be found in the menu.</summary>
    public static bool RepoMatches(string name, string query)
    {
        var q = query.Trim();
        return q.Length == 0 || name.Contains(q, StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>The connected repositories Kiro listed (none until it has), and what to say instead while there are none.</summary>
    (List<string> Repos, string Note) ConnectedRepos()
    {
        if (listed && reposError.Length == 0 && repos.Count > 0) return (repos, listing ? "Checking for new repositories…" : "");
        if (listing) return ([], "Loading your connected repositories…");
        if (listed && reposError.Length > 0) return ([], reposError);
        if (listed) return ([], "No GitHub repositories are connected. Connect GitHub in Kiro Web.");
        return ([], "");
    }

    // MARK: The backend

    void OnVoice(VoiceState st)
    {
        string? toast = null;
        var chime = false;
        lock (gate)
        {
            // A new interaction: what the projects and the default workspace are is read again.
            if (st.Stage == VoiceStage.Recording && DateTime.UtcNow - projectsAsked > TimeSpan.FromSeconds(5))
            {
                projectsAsked = DateTime.UtcNow;
                service.Send(new RawCommand("projects", new JsonObject { ["op"] = "list" }));
            }
            // The card's menu belongs to the interaction it was opened on.
            if (MenuPreview(st) is not { } p || p.Id != menuId) (menu, menuId) = (0, 0);
            if (st.ShotCount != shotSeen)
            {
                shotSeen = st.ShotCount;
                if (st.ShotMessage == "Screenshot attached") chime = true;
                if (service.Dictating) toast = st.ShotMessage;
                else
                {
                    note = st.ShotMessage;
                    if (chime) SetFlash();
                    noteTimer?.Dispose();
                    noteTimer = new Timer(_ => { lock (gate) note = ""; Changed?.Invoke(); }, null, TimeSpan.FromMilliseconds(1800), Timeout.InfiniteTimeSpan);
                }
            }
        }
        if (chime) playChime();
        if (toast is not null) Toast?.Invoke(toast);
        Changed?.Invoke();
    }

    void SetFlash()
    {
        flash = true;
        flashTimer?.Dispose();
        flashTimer = new Timer(_ => { lock (gate) flash = false; Changed?.Invoke(); }, null, TimeSpan.FromMilliseconds(90), Timeout.InfiniteTimeSpan);
    }

    void OnBusy()
    {
        lock (gate)
        {
            busy = true;
            busyTimer?.Dispose();
            busyTimer = new Timer(_ => { lock (gate) busy = false; Changed?.Invoke(); }, null, TimeSpan.FromMilliseconds(600), Timeout.InfiniteTimeSpan);
        }
        Changed?.Invoke();
    }

    // MARK: The card's buttons and keys

    /// <summary>Esc closes an open menu, then cancels. Enter starts once Start can.</summary>
    public void Key(bool escape, bool shift = false)
    {
        var st = service.State;
        if (escape)
        {
            Act(OpenMenuOf(st) != 0 ? new CardAct("openMenu", "", 0) : new CardAct("cancel"));
            return;
        }
        if (!shift && st.Stage is VoiceStage.Preview or VoiceStage.Editing && VoiceCards.Build(st, Context(st))?.CanStart == true) Act(new CardAct("start"));
    }

    int OpenMenuOf(VoiceState st)
    {
        lock (gate) return OpenMenu(st);
    }

    /// <summary>One thing the card asked for.</summary>
    public void Act(CardAct a)
    {
        var st = service.State;
        switch (a.Name)
        {
            case "unattach":
                service.Send(VoiceCommands.Unattach(Math.Max(a.N, 0)));
                break;
            case "start":
                service.Send(VoiceCommands.StartNow);
                break;
            case "toggleCloud":
                lock (gate) (menu, menuId) = (0, 0);
                service.Send(VoiceCommands.ToggleCloud);
                break;
            case "cancel":
                if (st.Stage == VoiceStage.Idle) HoldError = "";
                else service.Cancel();
                break;
            case "edit":
                service.Send(VoiceCommands.Edit(a.S));
                break;
            case "retry":
                service.Send(VoiceCommands.Retry);
                break;
            case "pick":
                if (st.Stage == VoiceStage.ChooseAgent) service.Send(VoiceCommands.ChooseAgent(a.S));
                else
                {
                    lock (gate) (menu, menuId) = (0, 0);
                    service.Send(VoiceCommands.ChangeAgent(a.S));
                }
                break;
            case "openMenu":
                OpenMenuTo(a.N, st);
                break;
            case "pickModel":
                if (MenuPreview(st) is { } pm)
                {
                    // The model and effort picks are the tool's own from then on, as the office's new-task box writes them.
                    var tool = state?.Tools.FirstOrDefault(t => t.Id == pm.Tool);
                    service.Send(new SetModelCommand(pm.Tool, a.S.Length == 0 ? null : a.S, tool?.Effort));
                    lock (gate) (menu, menuId) = (0, 0);
                    service.Send(VoiceCommands.ChangeAgent(pm.Tool));
                }
                break;
            case "pickEffort":
                if (MenuPreview(st) is { } pe)
                {
                    var tool = state?.Tools.FirstOrDefault(t => t.Id == pe.Tool);
                    service.Send(new SetModelCommand(pe.Tool, tool?.Model, a.S));
                    service.Send(VoiceCommands.Hold);
                }
                break;
            case "pickOpt":
                PickOption(a.S, st);
                break;
            case "search":
                lock (gate) repoQuery = a.S;
                MaybeAskAgain(a.S);
                break;
            case "searchEnter":
                SearchEnter(st);
                break;
            case "settings":
                HoldError = "";
                service.Send(VoiceCommands.Dismiss);
                if (st.Stage == VoiceStage.ChooseAgent) service.Cancel();
                SettingsRequested?.Invoke();
                break;
        }
        Changed?.Invoke();
    }

    void OpenMenuTo(int which, VoiceState st)
    {
        var p = MenuPreview(st);
        if (p is null)
        {
            lock (gate) (menu, menuId) = (0, 0);
            return;
        }
        bool ask;
        lock (gate)
        {
            (menu, menuId) = (which, p.Id);
            repoQuery = "";
            ask = which == 4 && !listing;
            if (ask) (listing, listAsked) = (true, DateTime.UtcNow);
        }
        // Opening a menu stops the countdown for good, so it cannot start the task mid-choice.
        if (which != 0) service.Send(VoiceCommands.Hold);
        if (ask) service.Send(new RawCommand("cloudRepos"));
    }

    void PickOption(string id, VoiceState st)
    {
        if (MenuPreview(st) is null) return;
        int which;
        lock (gate) which = menu;
        switch (which)
        {
            case 3:
                service.Send(VoiceCommands.ChangeFolder(id.Length == 0 ? null : id));
                break;
            case 4:
                service.Send(id switch
                {
                    "" => VoiceCommands.ChangeRepo("folder"),
                    "-" => VoiceCommands.ChangeRepo("empty"),
                    _ => VoiceCommands.ChangeRepo("named", id),
                });
                break;
            default:
                return;
        }
        lock (gate) (menu, menuId, repoQuery) = (0, 0, "");
    }

    void SearchEnter(VoiceState st)
    {
        string query;
        List<string> list;
        lock (gate)
        {
            if (MenuPreview(st) is null || menu != 4) return;
            query = repoQuery;
            list = ConnectedRepos().Repos;
        }
        var first = list.FirstOrDefault(r => query.Trim().Length > 0 && RepoMatches(r, query));
        if (first is null) return;
        service.Send(VoiceCommands.ChangeRepo("named", first));
        lock (gate) (menu, menuId, repoQuery) = (0, 0, "");
    }

    /// <summary>A search that matches none of the repositories asks Kiro again (at most every 15 s): the repository may have been made since the list was.</summary>
    void MaybeAskAgain(string query)
    {
        lock (gate)
        {
            if (query.Trim().Length == 0 || DateTime.UtcNow - listAsked <= TimeSpan.FromSeconds(15) || !listed || reposError.Length > 0) return;
            if (repos.Any(r => RepoMatches(r, query))) return;
            (listing, listAsked) = (true, DateTime.UtcNow);
        }
        service.Send(new RawCommand("cloudRepos"));
    }

    /// <summary>The text typed in the repository menu's search box.</summary>
    public string RepoQuery { get { lock (gate) return repoQuery; } }

    public void Dispose()
    {
        lock (gate)
        {
            if (disposed) return;
            disposed = true;
            holdErrorTimer?.Dispose();
            noteTimer?.Dispose();
            flashTimer?.Dispose();
            busyTimer?.Dispose();
        }
        service.Changed -= OnVoice;
        service.Busy -= OnBusy;
    }
}

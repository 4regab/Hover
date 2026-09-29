using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;
using Hover.Core;
using Hover.Services;
using Microsoft.Web.WebView2.Core;
using Microsoft.Web.WebView2.Wpf;

namespace Hover.Owl;

/// The Kiro page: the agent office. Every session is a bot at its own desk; a click
/// on a bot (or its note on the wall board) opens the session, where it can be
/// answered, stopped or deleted. The circle at the bottom left starts a new task: a
/// click shows the agents' logos, a pick opens the box for that agent.
///
/// The office is a three.js scene (web/office, built into Assets/kiro-office.html)
/// shown in WebView2. It is the composition control, because the notch's window is
/// layered and a windowed WebView2 can't draw in one. The page only draws: the
/// sessions live in OwlApp.Kiro, the page is sent their state as JSON and posts back
/// what the user asked for. The WebView2 is made when the page shows and dropped a
/// little while after it hides, since it holds 100 MB or so while it lives.
///
/// The first visit explains that Kiro runs with full tool access; that note is shown
/// once, natively, before the office.
internal sealed class KiroPage
{
    public FrameworkElement Root => _root;

    private static KiroSessions Sessions => OwlApp.Kiro;
    private static bool _historyHooked;

    /// The Kiro pages on screen right now (the notch's and the app window's).
    private static readonly HashSet<KiroPage> InView = new();

    /// A Kiro page is in view, so a task ending is seen happening and needs no alert.
    // ponytail: a window behind other windows still counts as in view; only a
    // minimised one does not.
    public static bool Watching => InView.Any(p => Window.GetWindow(p._root) is not { WindowState: WindowState.Minimized });

    private const string Host = "hover.office";
    private static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };

    private readonly Grid _root = new();
    private readonly DispatcherTimer _drop = new() { Interval = TimeSpan.FromSeconds(30) };
    private readonly DispatcherTimer _push = new(DispatcherPriority.Normal) { Interval = TimeSpan.FromMilliseconds(120) };
    // In the app window rather than the notch: the page leaves out its close button.
    private readonly bool _window;
    private readonly Action _openSettings;
    private WebView2CompositionControl? _web;
    private bool _ready, _notice;
    private DateTime _madeAt;
    /// The chat open in the office, shared by both views and kept while the page is dropped.
    private static int? _open;
    private static readonly HashSet<KiroPage> Live = new();

    /// Open a session's chat in the office: now in a page that is up, and in the next
    /// page made (the notch's is made as it opens).
    public static void Reveal(int id)
    {
        _open = id;
        foreach (var p in Live)
            if (p._ready) p._web?.CoreWebView2?.PostWebMessageAsJson(JsonSerializer.Serialize(new { type = "reveal", id }, Json));
    }

    public KiroPage(bool window, Action openSettings)
    {
        _window = window;
        _openSettings = openSettings;
        AutomationProperties.SetAutomationId(_root, "KiroPage");
        _drop.Tick += (_, _) => { _drop.Stop(); DropWeb(); };
        // Changes come in bursts (a phase, a step, the context); one send covers them.
        _push.Tick += (_, _) => { _push.Stop(); Push(); };
        _root.Loaded += (_, _) =>
        {
            if (Sessions.History is { } h && !_historyHooked) { _historyHooked = true; h.Changed += () => _historyVersion++; }
            Sessions.Changed += OnChanged;
            Live.Add(this);
            Build();
        };
        _root.Unloaded += (_, _) =>
        {
            Sessions.Changed -= OnChanged;
            Live.Remove(this);
            InView.Remove(this);
            _push.Stop();
            DropWeb();
        };
        _root.IsVisibleChanged += (_, _) =>
        {
            if (!_root.IsVisible) { InView.Remove(this); Shown(false); _drop.Start(); return; }
            InView.Add(this);
            OwlApp.KiroSeen();
            _drop.Stop();
            if (!_notice && _web is null) Build();
            else Shown(true);
        };
    }

    /// Hidden, the page stops its music and WebView2 gives back what memory it can
    /// until it is shown again or dropped.
    private void Shown(bool on)
    {
        if (_web?.CoreWebView2 is not { } core) return;
        try
        {
            core.MemoryUsageTargetLevel = on ? CoreWebView2MemoryUsageTargetLevel.Normal : CoreWebView2MemoryUsageTargetLevel.Low;
            if (_ready) core.PostWebMessageAsJson(JsonSerializer.Serialize(new { type = "visible", on }, Json));
        }
        catch (InvalidOperationException) { }
    }

    /// The shortcut opened the notch: keys go to the office.
    public void Focus() => _web?.Focus();

    private void OnChanged()
    {
        if (_notice != !Settings.KiroNoticeSeen) { Build(); return; }
        if (_ready && !_push.IsEnabled) _push.Start();
    }

    private void Build()
    {
        _notice = !Settings.KiroNoticeSeen;
        if (_notice)
        {
            DropWeb();
            _root.Children.Clear();
            // The note is a card; in the notch it keeps clear of the shape's curves.
            _root.Children.Add(Notice().Margin(_window ? 0 : 10, _window ? 0 : 10, _window ? 0 : 10, _window ? 0 : 10));
            return;
        }
        if (_web is not null) return;
        _root.Children.Clear();
        _ = MakeWeb();
    }

    // MARK: The office

    private static Task<CoreWebView2Environment>? _environment;

    /// One WebView2 environment for every office (the notch's and the app window's),
    /// so they share one browser process and one GPU process.
    private static Task<CoreWebView2Environment> SharedEnvironment() =>
        _environment ??= CoreWebView2Environment.CreateAsync(null, Path.Combine(Paths.Support, "WebView2"));

    private async Task MakeWeb()
    {
        _madeAt = DateTime.UtcNow;
        var web = new WebView2CompositionControl
        {
            DefaultBackgroundColor = System.Drawing.Color.FromArgb(255, 0x0B, 0x08, 0x10),
            Focusable = true,
        };
        AutomationProperties.SetAutomationId(web, "KiroOffice");
        _web = web;
        // Edge to edge: in the notch its shape clips it, in the app window the title
        // bar sits over it. Shown once the office has drawn, so its first frame never flashes.
        _root.Children.Add(new Border { Child = web, Opacity = 0 });
        try
        {
            try { await web.EnsureCoreWebView2Async(await SharedEnvironment()); }
            catch (Exception e) when (e is System.Runtime.InteropServices.COMException or InvalidOperationException)
            {
                // The shared environment can go stale (its browser crashed or was
                // killed); a fresh one gets one more try.
                Log.Line($"kiro office: webview2 environment retry - {e.Message}");
                _environment = null;
                if (!ReferenceEquals(_web, web)) return;
                await web.EnsureCoreWebView2Async(await SharedEnvironment());
            }
        }
        catch (Exception e) when (e is WebView2RuntimeNotFoundException or System.Runtime.InteropServices.COMException or InvalidOperationException)
        {
            Log.Line($"kiro office: webview2 unavailable - {e.Message}");
            _environment = null;
            if (ReferenceEquals(_web, web)) { _web = null; _root.Children.Clear(); _root.Children.Add(Missing()); }
            return;
        }
        if (!ReferenceEquals(_web, web)) return;   // dropped while it started
        var core = web.CoreWebView2;
        var s = core.Settings;
        s.AreDevToolsEnabled = System.Diagnostics.Debugger.IsAttached;
        s.AreDefaultContextMenusEnabled = false;
        s.IsZoomControlEnabled = false;
        s.IsStatusBarEnabled = false;
        s.AreBrowserAcceleratorKeysEnabled = false;
        s.IsPasswordAutosaveEnabled = s.IsGeneralAutofillEnabled = false;
        core.SetVirtualHostNameToFolderMapping(Host, OfficeFolder(), CoreWebView2HostResourceAccessKind.DenyCors);
        core.SetVirtualHostNameToFolderMapping(ImagesHost, ImagesFolder(), CoreWebView2HostResourceAccessKind.DenyCors);
        // The page is Hover's own and goes nowhere else.
        core.NavigationStarting += (_, e) => { if (!e.Uri.StartsWith($"https://{Host}/", StringComparison.Ordinal)) e.Cancel = true; };
        core.NewWindowRequested += (_, e) => e.Handled = true;
        core.WebMessageReceived += OnMessage;
        core.ProcessFailed += (_, e) =>
        {
            Log.Line($"kiro office: webview2 process failed ({e.ProcessFailedKind})");
            _root.Dispatcher.BeginInvoke(() => { DropWeb(); if (_root.IsVisible) Build(); });
        };
        core.Navigate($"https://{Host}/kiro-office.html");
    }

    private void DropWeb()
    {
        _ready = false;
        _mapped.Clear();
        _historySent = 0;
        if (_web is null) return;
        var web = _web;
        _web = null;
        _root.Children.Clear();
        // A WebView2 that failed half way can throw on the way out; the page just goes.
        try { web.Dispose(); }
        catch (Exception e) { Log.Line($"kiro office: webview2 dispose - {e.Message}"); }
    }

    /// The page and its music come out of the exe into Hover's folder, where WebView2
    /// reads them.
    private static string OfficeFolder()
    {
        var dir = Path.Combine(Paths.Support, "office");
        Directory.CreateDirectory(dir);
        Extract("Hover.KiroOffice.html", Path.Combine(dir, "kiro-office.html"));
        Extract("Hover.OfficeBeats.ogg", Path.Combine(dir, "office-beats.ogg"));
        return dir;
    }

    private static void Extract(string resource, string file)
    {
        using var res = typeof(KiroPage).Assembly.GetManifestResourceStream(resource)
            ?? throw new InvalidOperationException($"{resource} is missing from the build.");
        try
        {
            // Rewritten only when the build's copy differs, so a normal open writes nothing.
            if (File.Exists(file) && new FileInfo(file).Length == res.Length)
            {
                using var mem = new MemoryStream();
                res.CopyTo(mem);
                if (File.ReadAllBytes(file).AsSpan().SequenceEqual(mem.ToArray())) return;
                res.Position = 0;
            }
            using var fs = File.Create(file);
            res.CopyTo(fs);
        }
        catch (IOException e) { Log.Line($"kiro office: couldn't write {Path.GetFileName(file)} - {e.Message}"); }
    }

    private void OnMessage(object? sender, CoreWebView2WebMessageReceivedEventArgs e)
    {
        if (!e.Source.StartsWith($"https://{Host}/", StringComparison.Ordinal)) return;
        JsonElement m;
        try { m = JsonDocument.Parse(e.WebMessageAsJson).RootElement; }
        catch (JsonException) { return; }
        var type = Str(m, "type");
        var session = m.TryGetProperty("id", out var idv) && idv.ValueKind == JsonValueKind.Number
            ? Sessions.All.FirstOrDefault(x => x.Id == idv.GetInt32()) : null;
        switch (type)
        {
            case "ready":
                _ready = true;
                Log.Line($"kiro office: ready in {(DateTime.UtcNow - _madeAt).TotalMilliseconds:0} ms");
                Push();
                _ = CheckTools();
                // After the first state has been drawn: a short fade, not a pop.
                if (_web?.Parent is Border shown)
                    shown.BeginAnimation(UIElement.OpacityProperty,
                        new System.Windows.Media.Animation.DoubleAnimation(1, TimeSpan.FromMilliseconds(Animator.Still ? 0 : 180)) { BeginTime = TimeSpan.FromMilliseconds(60) });
                break;
            case "new":
                var folder = Str(m, "folder");
                var prompt = Str(m, "prompt") ?? "";
                if (!KiroRunner.UsableFolder(folder)) { Say("toast", "That folder isn’t there any more. Choose another one."); break; }
                Settings.KiroFolder = folder;
                var tool = Agents.Parse(Str(m, "tool")) ?? Settings.AgentTool;
                if (Agents.Known(tool) is { Ok: false } not) { Say("toast", not.Hint); _ = CheckTools(fresh: true); break; }
                Settings.AgentTool = tool;
                // The access picked in the new-task box, for this session only.
                var access = Str(m, "access");
                if (access is not ("full" or "risky" or "always" or "read") || (access == "read" && !Agents.ReadOnlyWorks(tool))) access = null;
                if (Sessions.Start(tool, folder!, prompt, SaveImages(m), access) is null)
                    Say("toast", Sessions.CanStart ? "Couldn’t start that task." : $"{KiroSessions.MaxRunning} tasks are running. Start another when one is done.");
                break;
            case "reply":
                // A reply to a session in the history brings it back to a desk first.
                if (session is null && Str(m, "key") is { } wakeKey)
                {
                    session = Sessions.Wake(wakeKey);
                    if (session is null) { Say("toast", "Every desk is busy. Try again when a task is done."); break; }
                }
                if (session is not null && !Sessions.Reply(session, Str(m, "text") ?? "", SaveImages(m)))
                    Say("toast", $"{KiroSessions.MaxRunning} tasks are running. Reply when one is done.");
                break;
            case "stop":
                session?.Stop();
                break;
            case "answer":
                // The office answered what the agent asked: over its head, or in its chat.
                // A question's answer is the picked labels, one list per question.
                if (session is not null && Str(m, "ask") is { } qId && m.TryGetProperty("answers", out var chosen) && chosen.ValueKind == JsonValueKind.Array)
                {
                    var lists = chosen.EnumerateArray().Select(a => (IReadOnlyList<string>)(a.ValueKind == JsonValueKind.Array
                        ? a.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.String).Select(x => x.GetString()!.Trim()).Where(x => x.Length is > 0 and <= 4000).ToList()
                        : new List<string>())).ToList();
                    Log.Line($"{Agents.Id(session.Tool)} run {session.Id}: answered a question from the office");
                    if (!session.AnswerQuestion(qId, lists)) Say("toast", "Pick an answer first.");
                }
                else if (session is not null && Str(m, "ask") is { } askId)
                {
                    var how = Str(m, "answer") switch
                    {
                        "allow" => AskAnswer.Allow, "trust" => AskAnswer.Trust, "trustAll" => AskAnswer.TrustAll, _ => AskAnswer.Deny,
                    };
                    Log.Line($"{Agents.Id(session.Tool)} run {session.Id}: {how.ToString().ToLowerInvariant()} from the office");
                    session.Answer(askId, how);
                }
                break;
            case "remove":
                if (session is not null) Sessions.Dismiss(session);
                break;
            case "delete":
                if ((session?.Key ?? Str(m, "key")) is { } deleteKey) Sessions.Delete(deleteKey);
                break;
            case "history":
                // One saved session, whole, for the chat to show.
                if (Str(m, "key") is { } historyKey && Sessions.Saved(historyKey) is { } saved)
                {
                    var view = new KiroSession();
                    view.Restore(saved);
                    _web?.CoreWebView2?.PostWebMessageAsJson(JsonSerializer.Serialize(new { type = "transcript", session = State(view) }, Json));
                }
                break;
            case "setModel":
                // The composer's pick is the tool's default from now on, as in Settings.
                if (Agents.Parse(Str(m, "tool")) is { } modelTool)
                {
                    var o = Settings.AgentOptions(modelTool);
                    Settings.SetAgentOptions(modelTool, o with { Model = Str(m, "model") ?? o.Model, Effort = Str(m, "effort") ?? o.Effort });
                    Push();
                }
                break;
            case "link":
                // Links in answers open in the browser, never in the office.
                if (Uri.TryCreate(Str(m, "url"), UriKind.Absolute, out var url) && url.Scheme is "https" or "http")
                    try { System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(url.AbsoluteUri) { UseShellExecute = true }); }
                    catch (Exception ex) { Log.Line($"kiro office: couldn't open a link - {ex.Message}"); }
                break;
            case "open":
                Sessions.Select(session);
                _open = session?.Id;
                break;
            case "close":
                _open = null;
                break;
            case "pickFolder":
                if (PickFolder(Str(m, "folder")) is { } picked) Say("folder", picked);
                break;
            case "settings":
                _openSettings();
                break;
            case "fold":
                OwlApp.Collapse?.Invoke();
                break;
        }
    }

    private void Say(string type, string text) =>
        _web?.CoreWebView2?.PostWebMessageAsJson(JsonSerializer.Serialize(new { type, text }, Json));

    // MARK: Pasted images

    private const int MaxImages = 4, MaxImageBytes = 8 * 1024 * 1024;
    private const string ImagesHost = "hover.images";

    /// Where pasted images are kept, for Kiro to read and the office to show. Older
    /// than two weeks are cleared the first time it's asked for in a run of Hover.
    // ponytail: a fixed age, not tied to the sessions; a session older than that
    // shows its pictures as missing, which is fine for a history of six.
    private static string ImagesFolder()
    {
        var dir = Path.Combine(Paths.Support, "kiro-images");
        Directory.CreateDirectory(dir);
        if (!_imagesSwept)
        {
            _imagesSwept = true;
            foreach (var f in Directory.EnumerateFiles(dir))
                try { if (File.GetLastWriteTimeUtc(f) < DateTime.UtcNow.AddDays(-14)) File.Delete(f); }
                catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
        return dir;
    }
    private static bool _imagesSwept;

    /// The images a message carries, as data: URLs, saved as files. Only PNG, JPEG,
    /// GIF and WebP, up to MaxImages of MaxImageBytes each; anything else is skipped.
    private static IReadOnlyList<string> SaveImages(JsonElement m)
    {
        var saved = new List<string>();
        if (!m.TryGetProperty("images", out var list) || list.ValueKind != JsonValueKind.Array) return saved;
        foreach (var item in list.EnumerateArray())
        {
            if (saved.Count >= MaxImages || item.ValueKind != JsonValueKind.String) break;
            var url = item.GetString() ?? "";
            var comma = url.IndexOf(',');
            if (!url.StartsWith("data:image/", StringComparison.Ordinal) || comma < 0 || !url[..comma].EndsWith(";base64", StringComparison.Ordinal)) continue;
            var ext = url[11..url.IndexOf(';')] switch { "png" => ".png", "jpeg" => ".jpg", "gif" => ".gif", "webp" => ".webp", _ => null };
            if (ext is null || (url.Length - comma) * 3 / 4 > MaxImageBytes) continue;
            byte[] bytes;
            try { bytes = Convert.FromBase64String(url[(comma + 1)..]); }
            catch (FormatException) { continue; }
            var file = Path.Combine(ImagesFolder(), $"{DateTime.Now:yyyyMMdd-HHmmss}-{Guid.NewGuid():N}"[..24] + ext);
            try { File.WriteAllBytes(file, bytes); saved.Add(file); }
            catch (IOException e) { Log.Line($"kiro office: couldn't save a pasted image - {e.Message}"); }
        }
        return saved;
    }

    /// Whether each tool is installed and signed in, for the new task's tool picker.
    /// Checked when the office opens (kept five minutes), and again after a failed start.
    private async Task CheckTools(bool fresh = false)
    {
        await Task.WhenAll(Agents.All.Select(t => Agents.Check(t, fresh)));
        Push();
    }

    /// Everything the office draws, as one message.
    private void Push()
    {
        if (!_ready || _web?.CoreWebView2 is not { } core) return;
        var state = new
        {
            type = "state",
            window = _window,
            canStart = Sessions.CanStart,
            maxRunning = KiroSessions.MaxRunning,
            folder = KiroRunner.UsableFolder(Settings.KiroFolder) ? Settings.KiroFolder : null,
            tool = Agents.Id(Settings.AgentTool),
            // The session whose chat was open, so a page made again opens it again.
            open = _open,
            tools = Agents.All.Select(t => new
            {
                id = Agents.Id(t),
                name = Agents.Name(t),
                // Unknown until checked; the picker offers it meanwhile.
                ready = Agents.Known(t)?.Ok ?? true,
                hint = Agents.Known(t)?.Hint ?? "",
                // The tool access a new task starts with, unless the box picks another.
                access = Settings.AgentOptions(t).AccessId(Agents.ReadOnlyWorks(t)),
                readOnly = Agents.ReadOnlyWorks(t),
                hideSteps = Settings.AgentOptions(t).HideSteps,
                // The composer's model and effort picks: what the tool offered last,
                // Kiro's own list before it has run. A model with levels of its own
                // (OpenCode's variants) takes those instead of the tool's efforts.
                models = Models(t).Select(x => new { id = x.Id, name = x.Name, levels = x.Levels }).ToList(),
                model = Settings.AgentOptions(t).Model ?? Models(t).FirstOrDefault().Id,
                efforts = Offer(t, "thought_level", "effortLevel", "reasoning_effort", "effort")?.Choices.Select(c => c.Value).ToList() ?? new List<string>(),
                effort = Settings.AgentOptions(t).Effort ?? Offer(t, "thought_level", "effortLevel", "reasoning_effort", "effort")?.Current,
                effortLabel = OwlApp.Agents[t].Caps.EffortLabel,
                questions = OwlApp.Agents[t].Caps.Questions,
            }).ToList(),
            sessions = Sessions.All.Select(State).ToList(),
            // The whole history only when it changed since this page last had it.
            history = _historySent == _historyVersion ? null : History(),
        };
        _historySent = _historyVersion;
        core.PostWebMessageAsJson(JsonSerializer.Serialize(state, Json));
    }

    private static AcpOption? Offer(AgentTool t, string category, params string[] ids)
    {
        var offers = Settings.AgentOffers(t);
        return offers.FirstOrDefault(x => x.Category == category) ?? offers.FirstOrDefault(x => ids.Contains(x.Id));
    }

    /// The models the composer offers; the first, when it isn't the tool's own
    /// "auto", is preceded by a Default that sends none.
    private static List<(string Id, string Name, IReadOnlyList<string>? Levels)> Models(AgentTool t)
    {
        var list = Offer(t, "model", "model")?.Choices.Select(c => (c.Value, c.Name, c.Levels)).ToList()
                   ?? (t == AgentTool.Kiro ? KiroRunner.Models.Select(x => (x.Id, x.Name, (IReadOnlyList<string>?)null)).ToList() : new());
        if (list.Count == 0 || !(list[0].Item1 == "auto" || list[0].Item1.StartsWith("default", StringComparison.Ordinal))) list.Insert(0, ("", "Default", null));
        return list;
    }

    // The history version the page has, and the one there is now.
    private static int _historyVersion = 1;
    private int _historySent;

    private static List<object> History() => (Sessions.History?.Entries ?? Array.Empty<HistoryEntry>()).Select(e => (object)new
    {
        key = e.Key,
        tool = Agents.Id(e.Tool),
        title = e.Title,
        folder = e.Folder,
        at = Ms(e.Updated),
        stage = Stage(e.State, KiroPhase.Working),
        turns = e.Turns,
    }).ToList();

    /// A session's folder as a web host, so images an answer points at in it can show.
    /// Only for folders that are there; one host per session.
    private string? FilesHost(KiroSession s)
    {
        if (_web?.CoreWebView2 is not { } core || !KiroRunner.UsableFolder(s.Folder)) return null;
        var host = $"f{s.Key[..12]}.hover";
        if (_mapped.Add(host))
            try { core.SetVirtualHostNameToFolderMapping(host, s.Folder, CoreWebView2HostResourceAccessKind.DenyCors); }
            catch (ArgumentException) { _mapped.Remove(host); return null; }
        return host;
    }
    private readonly HashSet<string> _mapped = new();

    private object State(KiroSession s)
    {
        var cur = s.Current;
        var steps = cur?.Steps ?? new List<KiroStep>();
        var lastStep = steps.LastOrDefault();
        return new
        {
            id = s.Id,
            key = s.Key,
            files = FilesHost(s),
            tool = Agents.Id(s.Tool),
            bot = s.Bot,
            seat = s.Seat,
            title = s.Title,
            folder = s.Folder,
            ctx = s.Context is { } c ? (int?)Math.Round(c) : null,
            // The session's own tool access, or the tool's setting.
            access = s.Access ?? Settings.AgentOptions(s.Tool).AccessId(Agents.ReadOnlyWorks(s.Tool)),
            stage = s.Waiting ? "waiting" : Stage(s.State, s.Phase),
            act = Act(s.Phase),
            // What the agent is waiting on the user for, and how many more are behind it.
            ask = s.Asking is { } a ? new
            {
                id = a.Id,
                kind = a.Kind,
                title = AgentWords.AskTitle(a),
                line = AgentWords.AskLine(a) is var (verb, obj) ? (verb + " " + obj).Trim() : "",
                command = a.Command,
                path = a.Path,
                preview = a.Preview,
                added = a.Added,
                removed = a.Removed,
                reason = a.Reason,
                danger = a.Danger,
                allow = AgentWords.AskAllow(a),
                more = s.Asks.Count - 1,
                // A question's own choices, which the office shows as buttons.
                questions = a.Questions?.Select(q => new
                {
                    header = q.Header,
                    question = q.Question,
                    options = q.Options.Select(o => new { label = o.Label, description = o.Description }).ToList(),
                    multiple = q.Multiple,
                    custom = q.Custom,
                }).ToList(),
            } : null,
            pose = Pose(s.Phase),
            file = lastStep is null ? "" : Short(lastStep.Target) ?? "",
            turns = s.Turns.Select(t => new
            {
                prompt = t.Prompt,
                images = t.Images.Select(p => $"https://{ImagesHost}/{Uri.EscapeDataString(Path.GetFileName(p))}").ToList(),
                queued = t.Queued,
                stage = t.Result is { } r ? Stage(r.State, KiroPhase.Working) : t.Queued ? "queued" : s.Waiting ? "waiting" : Stage(s.State, s.Phase),
                steps = t.Steps.Select(x => Row(x, s.Folder)).ToList(),
                // Markdown as the tool wrote it; the page renders it.
                answer = t.Result is { } res ? res.Text : "",
                t0 = Ms(t.StartedAt),
                woke = t.WokeAt is { } w ? (w - t.StartedAt).TotalSeconds : (double?)null,
                took = t.EndedAt is { } end ? (end - t.StartedAt).TotalMilliseconds : (double?)null,
            }).ToList(),
        };
    }

    private static long Ms(DateTime t) => t == default ? 0 : new DateTimeOffset(t).ToUnixTimeMilliseconds();

    private static string Stage(KiroState state, KiroPhase phase) => state switch
    {
        KiroState.Running => phase == KiroPhase.Starting ? "waking" : "working",
        KiroState.Completed => "done",
        KiroState.Failed => "failed",
        KiroState.Cancelled => "stopped",
        _ => "waking",
    };

    private static string Act(KiroPhase p) => p switch
    {
        KiroPhase.Thinking or KiroPhase.Planning or KiroPhase.Starting => "Thinking",
        KiroPhase.Reading => "Reading",
        KiroPhase.Searching => "Searching",
        KiroPhase.Editing => "Editing",
        KiroPhase.Running => "Running",
        KiroPhase.Writing => "Writing",
        _ => "Working",
    };

    /// How the bot sits for a phase: the office has four ways of working.
    private static string Pose(KiroPhase p) => p switch
    {
        KiroPhase.Reading or KiroPhase.Searching => "Reading",
        KiroPhase.Editing or KiroPhase.Writing or KiroPhase.Working => "Editing",
        KiroPhase.Running => "Running",
        _ => "Thinking",
    };

    /// A step as the chat's timeline shows it: its kind's icon, a verb, and the file
    /// (its name bright, its folder dim) or the command it was about, with the change
    /// it made or what the command printed, and how it went.
    private static object Row(KiroStep x, string folder)
    {
        var icon = x.Kind switch { "read" => "read", "edit" or "delete" or "move" => "edit", "execute" => "run", "search" or "fetch" => "search", _ => "think" };
        var verb = x.Kind switch
        {
            "read" => "Read", "edit" => "Edited", "delete" => "Deleted", "move" => "Moved",
            "execute" => "Ran", "search" => "Searched", "fetch" => "Fetched", _ => null,
        };
        var target = Relative(x.Target, folder);
        string? name = null, dir = null, cmd = null;
        if (x.Kind is "execute" or "search") cmd = target ?? (verb is null ? null : x.Title);
        else if (target is not null && x.Kind is "read" or "edit" or "delete" or "move")
        {
            var t = target.Replace('\\', '/');
            var i = t.LastIndexOf('/');
            (name, dir) = i < 0 ? (t, null) : (t[(i + 1)..], t[..i]);
        }
        else if (target is not null) cmd = target;
        return new
        {
            k = icon,
            verb = verb ?? x.Title,
            name,
            dir,
            cmd,
            status = x.Status,
            add = x.Added,
            del = x.Removed,
            diff = x.Diff,
            @out = x.Output,
            exit = x.Exit,
            ms = x.Ms,
        };
    }

    private static string? Relative(string? target, string folder)
    {
        if (string.IsNullOrWhiteSpace(target)) return null;
        var t = target.Trim().Replace('\n', ' ');
        var root = folder.TrimEnd('\\', '/') + "\\";
        if (t.Replace('/', '\\').StartsWith(root, StringComparison.OrdinalIgnoreCase)) t = t[root.Length..].Replace('\\', '/');
        return t.Length > 90 ? t[..89] + "…" : t;
    }

    private static string? Short(string? target)
    {
        if (string.IsNullOrWhiteSpace(target)) return null;
        var t = target.Trim();
        if (t.Contains(' ') || t.Length > 40) return t.Length > 28 ? t[..27] + "…" : t;   // a command
        return Path.GetFileName(t.TrimEnd('\\', '/'));
    }

    private static string? Str(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    /// The folder a new task works in. Starts where the last one did.
    internal static string? PickFolder(string? from = null)
    {
        var current = KiroRunner.UsableFolder(from) ? from : Settings.KiroFolder;
        var dlg = new Microsoft.Win32.OpenFolderDialog
        {
            Title = "Choose the folder the agent works in",
            InitialDirectory = KiroRunner.UsableFolder(current) ? current : Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        };
        if (dlg.ShowDialog() != true || !KiroRunner.UsableFolder(dlg.FolderName)) return null;
        Settings.KiroFolder = dlg.FolderName;
        return dlg.FolderName;
    }

    /// Settings → Kiro's folder picker.
    internal static void ChooseFolder()
    {
        if (PickFolder() is not null) Sessions.RaiseChanged();
    }

    // MARK: First visit, and a PC without WebView2

    private FrameworkElement Notice()
    {
        var g = new Grid { Margin = new Thickness(24, 18, 28, 18), VerticalAlignment = VerticalAlignment.Center, MaxWidth = 720 };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.Children.Add(new BotGlyph { Width = 110, Height = 110, Margin = new Thickness(0, 0, 26, 0), VerticalAlignment = VerticalAlignment.Center });

        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        var title = Ui.Text("Before an agent starts", 19, Ui.Ink, FontWeights.SemiBold);
        title.FontFamily = Ui.Display;
        AutomationProperties.SetAutomationId(title, "KiroNotice");
        text.Children.Add(title);
        text.Children.Add(Para("Kiro, Codex, Cursor and OpenCode work on their own here, with full access to their tools. They can edit files " +
                               "and run commands in the project folder you choose, without stopping to ask.", 13.5, Ui.Ink).Margin(0, 8, 0, 0));
        text.Children.Add(Para("So pick the folder with care, and keep it under version control, so you can look over what " +
                               "changed and undo it if you need to. In Settings each can be made to ask first, in the notch, or (all but Codex) only read. OpenCode's own deny rules always hold.", 12.5, Ui.InkDim).Margin(0, 6, 0, 0));
        var ok = Ui.Button("OwlBlueButton", "Got it", "KiroNoticeOk", "Got it", () =>
        {
            Settings.KiroNoticeSeen = true;
            // The other view (notch or app window) may be showing the note too.
            Sessions.RaiseChanged();
        });
        ok.HorizontalAlignment = HorizontalAlignment.Left;
        ok.Padding = new Thickness(18, 6, 18, 6);
        text.Children.Add(ok.Margin(0, 14, 0, 0));
        Grid.SetColumn(text, 1);
        g.Children.Add(text);
        return Ui.Card(g);
    }

    private static FrameworkElement Missing()
    {
        var s = new StackPanel { VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center, MaxWidth = 460 };
        s.Children.Add(new BotGlyph { Width = 70, Height = 70, HorizontalAlignment = HorizontalAlignment.Center });
        var t = Ui.Text("The office needs Microsoft Edge WebView2", 15, Ui.Ink, FontWeights.SemiBold);
        t.HorizontalAlignment = HorizontalAlignment.Center;
        AutomationProperties.SetAutomationId(t, "KiroNoWebView");
        s.Children.Add(t.Margin(0, 14, 0, 0));
        var p = Para("It comes with Windows 11 and most Windows 10 PCs. Install the WebView2 Runtime from Microsoft, then open this page again.", 12.5, Ui.InkDim);
        p.TextAlignment = TextAlignment.Center;
        s.Children.Add(p.Margin(0, 6, 0, 0));
        return Ui.Card(s);
    }

    private static TextBlock Para(string s, double size, Brush fg)
    {
        var t = Ui.Text(s, size, fg);
        t.TextWrapping = TextWrapping.Wrap;
        t.TextTrimming = TextTrimming.None;
        return t;
    }
}

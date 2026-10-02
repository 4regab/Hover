using System.Collections.Concurrent;
using System.Text.Json;
using Hover.Core;
using Hover.Owl;
using Hover.Services;
using Hover.Backend;

// All session callbacks and commands run on one context, just as they do in WPF.
// stdin EOF means the native host died: shut down every runtime before exiting.
using var loop = new EventLoop();
SynchronizationContext.SetSynchronizationContext(loop);
Backend? backend = null;
var input = Task.Run(async () =>
{
    while (await Console.In.ReadLineAsync() is { } line)
    {
        if (line.Length > 48 * 1024 * 1024) break;
        try
        {
            var command = JsonDocument.Parse(line).RootElement.Clone();
            loop.Post(_ =>
            {
                try
                {
                    if (backend is null)
                    {
                        if (Backend.Str(command, "type") != "initialize") throw new InvalidOperationException("Initialize the backend first.");
                        Crypto.InitializeKey(Convert.FromBase64String(Backend.Str(command, "key") ?? ""));
                        backend = new Backend(loop);
                    }
                    else backend.Handle(command);
                }
                catch (Exception e) { Backend.Send(new { type = backend is null ? "backendFailure" : "toast", text = e.Message }); }
            }, null);
        }
        catch (JsonException) { Backend.Send(new { type = "toast", text = "Invalid host message." }); }
    }
    loop.Post(_ => { backend?.Shutdown(); loop.Complete(); }, null);
});
loop.Run();

internal sealed class EventLoop : SynchronizationContext, IDisposable
{
    private readonly BlockingCollection<(SendOrPostCallback, object?)> queue = new();
    public override void Post(SendOrPostCallback d, object? state) { try { queue.Add((d, state)); } catch (InvalidOperationException) { } }
    public override SynchronizationContext CreateCopy() => this;
    public void Run() { foreach (var (callback, state) in queue.GetConsumingEnumerable()) callback(state); }
    public void Complete() => queue.CompleteAdding();
    public void Dispose() => queue.Dispose();
}

internal sealed class Backend
{
    private readonly EventLoop loop;
    private readonly Dictionary<AgentTool, IAgentRuntime> runtimes;
    private readonly KiroSessions sessions;
    private readonly OfficeState state;
    private readonly Timer quotaTimer;
    private readonly Dictionary<string, object> quotas = new();
    private bool checking, pushing, closing, readingQuotas;
    private TaskCompletionSource<string?>? claudeCredentials;
    private static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
    public static string? Str(JsonElement m, string k) => m.ValueKind == JsonValueKind.Object && m.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
    public static void Send(object value) { lock (Json) { Console.WriteLine(JsonSerializer.Serialize(value, Json)); Console.Out.Flush(); } }
    public Backend(EventLoop loop)
    {
        this.loop = loop;
        // Do not let an invalid Keychain key turn existing encrypted history into
        // an empty index that a later session would overwrite.
        var index = Path.Combine(Paths.Support, "agents", "index.dat");
        if (File.Exists(index))
        {
            var plain = Crypto.Open(File.ReadAllBytes(index));
            if (string.IsNullOrEmpty(plain)) throw new InvalidOperationException("The history key cannot decrypt existing history.");
            using var verified = JsonDocument.Parse(plain);
        }
        runtimes = Agents.All.ToDictionary(t => t, t => t == AgentTool.OpenCode
            ? (IAgentRuntime)new OpenCodeHost(() => Settings.AgentOptions(t)) : new AcpHost(t, () => Settings.AgentOptions(t)));
        sessions = new KiroSessions(tool =>
        {
            KiroSession? session = null;
            // The session's key names it to the MCP servers made for it (Hover's browser).
            session = new KiroSession(async (f, p, pr, ct, resume, events) =>
            {
                // Its own desktop (a Cua Space) is made or started before it starts.
                if (Spaces.Wanted && session?.Key is { } key) { pr?.Report(KiroPhase.Starting); await Spaces.Ensure(key, ct); }
                return await runtimes[tool].Run(f, p, pr, ct, resume, events, session?.Access, session?.Key);
            });
            return session;
        }) { History = new AgentHistory(Path.Combine(Paths.Support, "agents")), MaxRunning = Settings.MaxRunning };
        foreach (var (tool, runtime) in runtimes)
        {
            runtime.OptionsSeen += (t, options) => loop.Post(_ => { Settings.SetAgentOffers(t, options); Push(); }, null);
            runtime.Asking = (sid, ask, ct) => Ask(tool, sid, AskAnswer.Deny, s => s.Ask(ask, ct));
            runtime.Questioning = (sid, ask, ct) => Ask<IReadOnlyList<IReadOnlyList<string>>?>(tool, sid, null, s => s.AskQuestion(ask, ct));
        }
        state = new OfficeState(sessions, runtimes);
        // The Mac app drives a browser per session; agents get it as an MCP server.
        BrowserTool.Available = Environment.GetEnvironmentVariable("HOVER_NO_BROWSER") != "1";
        BrowserTool.ToHost = Send;
        BrowserTool.Resolve = tag =>
        {
            var found = new TaskCompletionSource<int?>(TaskCreationOptions.RunContinuationsAsynchronously);
            loop.Post(_ =>
            {
                // OpenCode has one server for all its sessions: the one at work answers.
                var s = tag == "opencode"
                    ? sessions.All.Where(x => x.Tool == AgentTool.OpenCode && x.Busy).OrderByDescending(x => x.Current?.StartedAt).FirstOrDefault()
                    : sessions.All.FirstOrDefault(x => x.Key == tag);
                found.TrySetResult(s?.Id);
            }, null);
            return found.Task;
        };
        GitHubCli.Changed += () => loop.Post(_ => { if (!closing) SendGitHub(); }, null);
        Spaces.Changed += () => loop.Post(_ => { if (!closing) { SendSpaces(); Push(); } }, null);
        AgentSetup.Changed += _ => loop.Post(_ => Push(), null);
        ComputerUse.Changed += () => loop.Post(_ => { if (!closing) SendComputerUse(); }, null);
        sessions.Changed += Push;
        sessions.History.Changed += () => loop.Post(_ => Push(), null);
        // The tool and outcome let the native island show the tool's logo with a badge.
        sessions.Ended += (s, r) => Send(new { type = "ended", title = $"{Agents.Name(s.Tool)}: {s.Title}", text = r.State.ToString(), tool = Agents.Id(s.Tool), task = s.Title, ok = r.State == KiroState.Completed });
        quotaTimer = new Timer(_ => loop.Post(_ => _ = RefreshQuotas(), null), null, TimeSpan.FromMinutes(5), TimeSpan.FromMinutes(5));
        Send(new { type = "initialized", version = 1 });
    }
    private Task<T> Ask<T>(AgentTool tool, string sid, T fallback, Func<KiroSession, Task<T>> ask)
    {
        var result = new TaskCompletionSource<T>(TaskCreationOptions.RunContinuationsAsynchronously);
        loop.Post(async _ =>
        {
            var s = sessions.All.FirstOrDefault(x => x.Tool == tool && x.KiroId == sid && x.Busy);
            try { result.TrySetResult(s is null ? fallback : await ask(s)); }
            catch { result.TrySetResult(fallback); }
        }, null);
        return result.Task;
    }
    private void Push()
    {
        if (pushing || closing) return;
        pushing = true;
        loop.Post(_ => { pushing = false; if (!closing) Send(state.Snapshot()); }, null);
    }
    private async Task Check()
    {
        if (checking) return;
        checking = true;
        try { await Task.WhenAll(Agents.All.Select(t => Agents.Check(t, true))); }
        finally { checking = false; Push(); }
    }
    public void Handle(JsonElement m)
    {
        var type = Str(m, "type");
        var s = m.TryGetProperty("id", out var id) && id.TryGetInt32(out var n) ? sessions.All.FirstOrDefault(x => x.Id == n) : null;
        switch (type)
        {
            case "ready": Push(); _ = Check(); _ = RefreshQuotas(); if (Settings.ComputerUse) _ = ComputerUse.Check(); break;
            case "new":
                var folder = Str(m, "folder"); var tool = Agents.Parse(Str(m, "tool")) ?? Settings.AgentTool;
                if (!KiroRunner.UsableFolder(folder)) throw new InvalidOperationException("Choose an existing project folder.");
                if (!Settings.KiroNoticeSeen) throw new InvalidOperationException("Review agent access in Settings before starting your first task.");
                if (Agents.Known(tool)?.Ok != true) throw new InvalidOperationException(Agents.Known(tool)?.Hint ?? "The tool is not ready yet.");
                var access = Str(m, "access");
                if (access is not ("full" or "risky" or "always" or "read")) access = Settings.AgentOptions(tool).AccessId(Agents.ReadOnlyWorks(tool));
                Settings.KiroFolder = folder; Settings.AgentTool = tool;
                if (sessions.Start(tool, folder!, Str(m, "prompt") ?? "", SaveImages(m), access) is null) throw new InvalidOperationException("All available desks are busy, or the prompt is empty.");
                break;
            case "reply":
                s ??= Str(m, "key") is { } key ? sessions.Wake(key) : null;
                if (s is null || !sessions.Reply(s, Str(m, "text") ?? "", SaveImages(m))) throw new InvalidOperationException("Could not send this reply. A desk may be busy.");
                break;
            case "stop": s?.Stop(); break;
            case "answer":
                if (s is null || Str(m, "ask") is not { } askId) break;
                if (m.TryGetProperty("answers", out var answers) && answers.ValueKind == JsonValueKind.Array)
                {
                    var lists = answers.EnumerateArray().Select(a => (IReadOnlyList<string>)a.EnumerateArray().Select(x => x.GetString() ?? "").Where(x => x.Length <= 4000).ToList()).ToList();
                    if (!s.AnswerQuestion(askId, lists)) throw new InvalidOperationException("Choose an answer first.");
                }
                else s.Answer(askId, Str(m, "answer") switch { "allow" => AskAnswer.Allow, "trust" => AskAnswer.Trust, "trustAll" => AskAnswer.TrustAll, _ => AskAnswer.Deny });
                break;
            case "delete": if ((s?.Key ?? Str(m, "key")) is { } deleteKey) { sessions.Delete(deleteKey); _ = Spaces.Delete(deleteKey); } break;
            case "remove": if (s is not null) { _ = Spaces.Stop(s.Key); sessions.Dismiss(s); } break;
            case "history":
                if (Str(m, "key") is { } historyKey && sessions.Saved(historyKey) is { } saved)
                { var view = new KiroSession(); view.Restore(saved); Send(new { type = "transcript", session = state.Session(view) }); }
                break;
            case "open": sessions.Select(s); break;
            // The desk menu's panels: read here, answered when git or gh are done.
            case "desk":
                if (s is null) break;
                var (deskId, what, arg) = (s.Id, Str(m, "what"), Str(m, "arg"));
                _ = DeskInfo.Answer(s, what, arg).ContinueWith(t => Send(new
                {
                    type = "desk", id = deskId, what, arg,
                    data = t.IsCompletedSuccessfully ? t.Result : new { error = t.Exception?.GetBaseException().Message ?? "Couldn’t read that." },
                }), TaskScheduler.Default);
                break;
            // Hover's browser answered a call (BrowserTool).
            case "browserResult": BrowserTool.Complete(m); break;
            // The desk's buttons that change the folder: Create pull request.
            case "deskAction":
                if (s is null) break;
                var (actionId, action) = (s.Id, Str(m, "what"));
                if (action != "prCreate") break;
                _ = DeskInfo.CreatePr(s, m.TryGetProperty("args", out var actionArgs) ? actionArgs.Clone() : default).ContinueWith(t => Send(new
                {
                    type = "deskAction", id = actionId, what = action,
                    data = t.IsCompletedSuccessfully ? t.Result : new { error = t.Exception?.GetBaseException().Message ?? "That didn’t work." },
                }), TaskScheduler.Default);
                break;
            // The session's own desktop (a Cua Space): its live viewer, an app teleported
            // into it from the notch, files dropped on it.
            case "spaceView":
                if (s is null) break;
                var viewId = s.Id;
                // Opening the panel makes or starts the Space when the session has none running.
                if (Spaces.Wanted && Spaces.StateOf(s.Key) is null or { Phase: "failed" or "stopped" }) { var k2 = s.Key; _ = Task.Run(() => Spaces.Ensure(k2, CancellationToken.None)); }
                _ = Spaces.Viewer(s.Key).ContinueWith(t => Send(new { type = "space", id = viewId, data = t.IsCompletedSuccessfully ? t.Result : new { error = "The desktop’s viewer didn’t open." } }), TaskScheduler.Default);
                break;
            case "teleport":
                if (s is null || Str(m, "app") is not { } app) break;
                var (tpId, tpTitle) = (s.Id, s.Title);
                Send(new { type = "teleport", id = tpId, phase = "sending", app, line = $"Sending {app} to its desktop…" });
                _ = Spaces.Teleport(s.Key, app, line => Send(new { type = "teleport", id = tpId, phase = "sending", app, line })).ContinueWith(t => Send(new
                {
                    type = "teleport", id = tpId, app, phase = "done",
                    data = t.IsCompletedSuccessfully ? t.Result : new { error = "The app didn’t go." },
                }), TaskScheduler.Default);
                break;
            case "spaceFiles":
                if (s is null || !m.TryGetProperty("paths", out var fp) || fp.ValueKind != JsonValueKind.Array) break;
                var filesId = s.Id;
                _ = Spaces.SendFiles(s.Key, fp.EnumerateArray().Select(x => x.GetString() ?? "").Where(x => x.Length > 0).ToList())
                    .ContinueWith(t => Send(new { type = "teleport", id = filesId, phase = "done", app = "files", data = t.IsCompletedSuccessfully ? t.Result : new { error = "The files didn’t go." } }), TaskScheduler.Default);
                break;
            case "spaces":
                switch (Str(m, "step"))
                {
                    case "setup": _ = Spaces.RunSetup(); break;
                    case "cancel": Spaces.Cancel(); break;
                    default: SendSpaces(); _ = Spaces.Check(fresh: true); break;
                }
                break;
            // The GitHub CLI: what is known, a fresh check, and its one-click setup.
            case "gh":
                switch (Str(m, "step"))
                {
                    case "setup": _ = GitHubCli.Run(); break;
                    case "cancel": GitHubCli.Cancel(); break;
                    default: SendGitHub(); _ = GitHubCli.Check(fresh: true); break;
                }
                break;
            case "setModel":
                if (Agents.Parse(Str(m, "tool")) is { } modelTool)
                    Settings.SetAgentOptions(modelTool, Settings.AgentOptions(modelTool) with { Model = Str(m, "model"), Effort = Str(m, "effort") });
                Push(); break;
            case "getSettings": SendPreferences(); break;
            case "saveSettings": SavePreferences(m); SendPreferences(); Push(); _ = RefreshQuotas(); break;
            case "refresh": _ = Check(); _ = RefreshQuotas(); if (Settings.ComputerUse) _ = ComputerUse.Check(fresh: true); break;
            // Settings → Computer Use: what is known now, then a fresh check.
            case "computerUse": SendComputerUse(); _ = ComputerUse.Check(fresh: true); break;
            case "computerUseSetup":
                switch (Str(m, "step"))
                {
                    case "install": _ = ComputerUse.Install(); break;
                    case "grant": _ = ComputerUse.Grant(); break;
                    case "cancel": ComputerUse.Cancel(); break;
                }
                SendComputerUse(); break;
            case "setup":
                if (Agents.Parse(Str(m, "tool")) is not { } setupTool) break;
                if (Str(m, "step") == "cancel") { AgentSetup.Cancel(setupTool); break; }
                _ = Task.Run(async () =>
                {
                    await AgentSetup.Run(setupTool);
                    loop.Post(_ => { Push(); _ = RefreshQuotas(); }, null);
                });
                Push(); break;
            case "claudeCredentials": claudeCredentials?.TrySetResult(Str(m, "json")); break;
            case "shutdown": Shutdown(); loop.Complete(); break;
        }
    }
    private static void SendSpaces()
    {
        var k = Spaces.Known;
        var p = Spaces.Setup;
        Send(new
        {
            type = "spaces", on = Settings.AgentSpaces, image = Settings.SpaceImage, supported = Spaces.Supported, @checked = k is not null,
            installed = k?.Installed ?? false, ready = k?.Ready ?? false, version = k?.Version, hint = k?.Hint ?? "", running = k?.Running ?? 0,
            step = p.Step, line = p.Line, fraction = p.Fraction, error = p.Error, busy = Spaces.Busy,
        });
    }
    private static void SendGitHub()
    {
        var s = GitHubCli.Known;
        var p = GitHubCli.Setup;
        Send(new
        {
            type = "gh", @checked = s is not null, installed = s?.Installed ?? false, signedIn = s?.SignedIn ?? false, user = s?.User, version = s?.Version,
            step = p.Step, line = p.Line, code = p.Code, error = p.Error, busy = GitHubCli.Busy, url = GitHubCli.DeviceUrl,
        });
    }
    private void SendPreferences() => Send(new
    {
        type = "preferences", maxRunning = Settings.MaxRunning, hover = Settings.HoverOpensWorkspace,
        noticeSeen = Settings.KiroNoticeSeen, quotaItems = Settings.NotchItems, computerUse = Settings.ComputerUse, sandbox = Settings.Sandbox, agentBrowser = Settings.AgentBrowser,
        agentSpaces = Settings.AgentSpaces, spaceImage = Settings.SpaceImage, spacesSupported = Spaces.Supported,
        tools = Agents.All.Select(t => new { id = Agents.Id(t), access = Settings.AgentOptions(t).AccessId(true), idle = Settings.AgentOptions(t).IdleMinutes, hideSteps = Settings.AgentOptions(t).HideSteps })
    });
    /// Cua Driver as Settings → Computer Use shows it: installed, its grants, and a
    /// setup's progress. Checked is false until a check has finished.
    private static void SendComputerUse()
    {
        var s = ComputerUse.Known;
        var p = ComputerUse.Setup;
        Send(new
        {
            type = "computerUse", on = Settings.ComputerUse, @checked = s is not null,
            installed = s?.Installed ?? false, version = s?.Version ?? "", permissions = s?.Permissions ?? "unknown",
            ready = s?.Ready ?? false, hint = s?.Hint ?? "", canGrant = ComputerUse.CanGrant, installHint = ComputerUse.InstallHint,
            step = p.Step, line = p.Line, error = p.Error, busy = ComputerUse.Busy,
        });
    }
    private void SavePreferences(JsonElement m)
    {
        if (m.TryGetProperty("maxRunning", out var max) && max.TryGetInt32(out var cap)) sessions.MaxRunning = Settings.MaxRunning = cap;
        if (m.TryGetProperty("hover", out var hover)) Settings.HoverOpensWorkspace = hover.GetBoolean();
        if (m.TryGetProperty("noticeSeen", out var notice) && notice.GetBoolean()) Settings.KiroNoticeSeen = true;
        // Every tool picks it up from its next session (a running one when it is next idle).
        if (m.TryGetProperty("computerUse", out var cua) && cua.ValueKind is JsonValueKind.True or JsonValueKind.False && cua.GetBoolean() != Settings.ComputerUse)
        {
            Settings.ComputerUse = cua.GetBoolean();
            if (Settings.ComputerUse) _ = ComputerUse.Check(fresh: true);
        }
        // Each tool picks it up when it next starts (AcpHost restarts an idle one), and
        // what it needs installed is checked again.
        if (m.TryGetProperty("sandbox", out var box) && box.ValueKind is JsonValueKind.True or JsonValueKind.False && box.GetBoolean() != Settings.Sandbox)
        {
            Settings.Sandbox = box.GetBoolean();
            _ = Check();
        }
        if (m.TryGetProperty("agentSpaces", out var spaces) && spaces.ValueKind is JsonValueKind.True or JsonValueKind.False) { Settings.AgentSpaces = spaces.GetBoolean(); _ = Spaces.Check(fresh: true); }
        if (Str(m, "spaceImage") is { } image) Settings.SpaceImage = image;
        // Each session gets it from its next run (BrowserTool).
        if (m.TryGetProperty("agentBrowser", out var browser) && browser.ValueKind is JsonValueKind.True or JsonValueKind.False) Settings.AgentBrowser = browser.GetBoolean();
        if (m.TryGetProperty("quotaItems", out var items)) Settings.NotchItems = items.EnumerateArray().Select(x => x.GetString() ?? "").Where(NotchItem.Quotas.Contains).ToList();
        if (m.TryGetProperty("tools", out var tools)) foreach (var t in tools.EnumerateArray())
            if (Agents.Parse(Str(t, "id")) is { } tool)
            {
                var o = Settings.AgentOptions(tool).WithAccess(Str(t, "access"));
                if (t.TryGetProperty("idle", out var idle) && idle.TryGetInt32(out var minutes)) o = o with { IdleMinutes = minutes };
                if (t.TryGetProperty("hideSteps", out var hide)) o = o with { HideSteps = hide.GetBoolean() };
                Settings.SetAgentOptions(tool, o);
            }
    }
    private async Task RefreshQuotas()
    {
        if (readingQuotas || closing) return;
        readingQuotas = true;
        try
        {
            quotas.Clear();
            foreach (var id in NotchItem.Quotas.Where(Settings.HasNotchItem))
            {
                string? credentials = null;
                if (id == NotchItem.Claude && OperatingSystem.IsMacOS())
                {
                    claudeCredentials = new(TaskCreationOptions.RunContinuationsAsynchronously);
                    Send(new { type = "readClaudeCredentials" });
                    try { credentials = await claudeCredentials.Task.WaitAsync(TimeSpan.FromSeconds(15)); }
                    catch (TimeoutException) { }
                    finally { claudeCredentials = null; }
                }
                var q = await Task.Run(async () => id switch
                {
                    NotchItem.Codex => Quota.Codex(DateTime.Now), NotchItem.Kiro => await Quota.Kiro(CancellationToken.None),
                    NotchItem.Claude => await Quota.Claude(DateTime.Now, CancellationToken.None, credentials), _ => await Quota.Cursor(CancellationToken.None),
                });
                quotas[id] = new { ok = q.Ok, used = q.Used, detail = q.Detail };
            }
            Send(new { type = "quotas", values = quotas });
        }
        finally { readingQuotas = false; }
    }
    private static IReadOnlyList<string> SaveImages(JsonElement m)
    {
        var saved = new List<string>();
        if (!m.TryGetProperty("images", out var images) || images.ValueKind != JsonValueKind.Array) return saved;
        var dir = Path.Combine(Paths.Support, "kiro-images"); Directory.CreateDirectory(dir);
        foreach (var image in images.EnumerateArray().Take(4))
        {
            var data = image.ValueKind == JsonValueKind.String ? image.GetString() : Str(image, "data");
            if (data is null || data.Length > 12 * 1024 * 1024) continue;
            var comma = data.IndexOf(','); if (comma < 0) continue;
            var ext = data[..comma] switch { "data:image/png;base64" => ".png", "data:image/jpeg;base64" => ".jpg", "data:image/webp;base64" => ".webp", "data:image/gif;base64" => ".gif", _ => null };
            if (ext is null) continue;
            var bytes = Convert.FromBase64String(data[(comma + 1)..]); if (bytes.Length > 8 * 1024 * 1024) continue;
            var path = Path.Combine(dir, Guid.NewGuid().ToString("N") + ext); File.WriteAllBytes(path, bytes); saved.Add(path);
        }
        return saved;
    }
    public void Shutdown()
    {
        if (closing) return; closing = true;
        quotaTimer.Dispose(); BrowserTool.Stop(); sessions.StopAll();
        // The agents' desktops are turned off with Hover (their disks stay); a few
        // seconds at most, so quitting never hangs on them.
        if (Spaces.Wanted) Task.WaitAll(sessions.All.Select(x => Spaces.Stop(x.Key)).ToArray(), TimeSpan.FromSeconds(6)); foreach (var runtime in runtimes.Values) runtime.Shutdown("Hover quit");
        sessions.History?.Flush(); Settings.Flush();
    }
}

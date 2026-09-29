using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Core;

namespace Hover.Services;

/// A running OpenCode server: where it listens, the password it was started with,
/// and how to end it.
public sealed class OpenCodeLink(Uri url, string password, Action kill, Func<string>? errors = null, Task? exited = null)
{
    public Uri Url { get; } = url;
    public string Password { get; } = password;
    public Action Kill { get; } = kill;
    public Func<string> Errors { get; } = errors ?? (() => "");
    /// Ends when the server's process does.
    public Task Exited { get; } = exited ?? Task.Delay(Timeout.Infinite);
}

/// OpenCode as T3 Code runs it: one Hover-owned "opencode serve" on 127.0.0.1, a
/// port the system picks, no mDNS, and a password made for that process only (passed
/// in its environment, sent as Basic auth, never on a command line or in a URL). Every
/// request names the session's folder, so one server serves every folder with that
/// folder's opencode config, agents, skills and MCP servers. OpenCode keeps its own
/// providers (API keys, cloud sign-ins, local models); Hover never sees them.
///
/// A turn follows T3's order: subscribe to the events first, then send the prompt
/// (prompt_async, with a message id Hover makes), and count the turn done only once
/// the server went idle after it saw that message or went busy for it. An idle left
/// over from before, or from another session, never ends it. When the event stream
/// drops, the turn says so, reconnects and reads the session's state and messages
/// back, since missed events aren't replayed. A prompt whose sending can't be
/// confirmed is looked up by its id, never sent twice.
///
/// Shut down after the idle time in its settings, like the ACP tools. No WPF in here.
public sealed class OpenCodeHost : IAgentRuntime
{
    private readonly Func<AgentOptions> _options;
    private readonly Func<CancellationToken, Task<OpenCodeLink?>> _connect;
    private readonly SemaphoreSlim _gate = new(1, 1);
    private readonly ConcurrentDictionary<string, Turn> _turns = new();
    private readonly ConcurrentDictionary<string, ConcurrentDictionary<string, bool>> _trusted = new();
    private readonly ConcurrentDictionary<string, JsonObject> _inventory = new();
    private readonly ConcurrentDictionary<string, byte> _stuck = new();
    private readonly Timer _idle;
    private OpenCodeLink? _link;
    private HttpClient? _http;
    private int _busy;

    public OpenCodeHost(Func<AgentOptions> options, Func<CancellationToken, Task<OpenCodeLink?>>? connect = null)
    {
        _options = options;
        _connect = connect ?? Launch;
        _idle = new Timer(_ => { if (Volatile.Read(ref _busy) == 0) Shutdown("idle"); });
    }

    public AgentTool Tool => AgentTool.OpenCode;
    private const string Name = "OpenCode";
    public AgentCaps Caps { get; } = new(Questions: true, ReadOnly: true, Resume: true, EffortLabel: "Variant");
    public bool Alive => _link is not null;
    public event Action<AgentTool, IReadOnlyList<AcpOption>>? OptionsSeen;
    public Func<string, AgentAsk, CancellationToken, Task<AskAnswer>>? Asking { get; set; }
    public Func<string, AgentAsk, CancellationToken, Task<IReadOnlyList<IReadOnlyList<string>>?>>? Questioning { get; set; }

    /// How long a startup, a prompt's sending and the first event wait. T3 waits 30 s
    /// for the server; here it took 39 s on a cold start (its plugins load first), so 90.
    internal static TimeSpan StartTimeout = TimeSpan.FromSeconds(90), SendTimeout = TimeSpan.FromSeconds(10),
        ConnectTimeout = TimeSpan.FromSeconds(10), StopGrace = TimeSpan.FromSeconds(8), Quiet = TimeSpan.FromSeconds(20);

    private sealed class OpenCodeError(HttpStatusCode? status, string message) : Exception(message)
    {
        public HttpStatusCode? Status { get; } = status;
    }

    /// One turn: the session it is in, the message it sent, and what it has seen.
    private sealed class Turn(string folder, AgentOptions options, IProgress<KiroPhase>? progress, IProgress<KiroEvent>? events, CancellationToken token)
    {
        public string Folder { get; } = folder;
        public AgentOptions Options { get; } = options;
        public IProgress<KiroPhase>? Progress { get; } = progress;
        public IProgress<KiroEvent>? Events { get; } = events;
        public CancellationToken Token { get; } = token;
        public string Sid = "";
        public string MessageId = "";
        /// This session and the subagents' sessions it started.
        public readonly ConcurrentDictionary<string, byte> Related = new();
        public readonly TaskCompletionSource<KiroResult> Done = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public readonly TaskCompletionSource Connected = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public readonly CancellationTokenSource Stream = new();
        public volatile bool Accepted, UserSeen, BusySeen, Stopping, Refused, IdleEarly, Connects;
        public volatile string? Error, Retry;
        public DateTime LastEvent = DateTime.UtcNow;
        public int IdleConfirms;
        /// Requests being asked about now, and those answered, so neither is asked twice.
        public readonly ConcurrentDictionary<string, CancellationTokenSource> Open = new();
        public readonly ConcurrentDictionary<string, byte> Resolved = new();
        public readonly object Lock = new();
        // The answer: text parts by message, in order, from this turn's assistant messages.
        public readonly List<string> Messages = new();
        public readonly Dictionary<string, List<string>> PartOrder = new();
        public readonly Dictionary<string, string> Text = new();
        public readonly Dictionary<string, string> Roles = new();
        public readonly Dictionary<string, KiroStep> Steps = new();
        public readonly Dictionary<string, DateTime> Began = new();
        public KiroPhase Phase = KiroPhase.Starting;
        public double? Context;

        public string Said
        {
            get
            {
                lock (Lock)
                    for (var i = Messages.Count - 1; i >= 0; i--)
                        if (PartOrder.TryGetValue(Messages[i], out var parts))
                        {
                            var t = string.Concat(parts.Select(p => Text.GetValueOrDefault(p, ""))).Trim();
                            if (t.Length > 0) return t;
                        }
                return "";
            }
        }

        public void SetPhase(KiroPhase p)
        {
            if (Phase == p) return;
            Phase = p;
            Progress?.Report(p);
        }
    }

    // MARK: A run

    public async Task<KiroResult> Run(string folder, string prompt, IProgress<KiroPhase>? progress, CancellationToken ct,
        string? resume = null, IProgress<KiroEvent>? events = null, string? access = null)
    {
        if (!KiroRunner.UsableFolder(folder)) return new(KiroState.Failed, "That folder isn’t there any more. Choose another one.");
        if (string.IsNullOrWhiteSpace(prompt)) return new(KiroState.Failed, $"Tell {Name} what to do first.");
        var o = _options().WithAccess(access);
        Interlocked.Increment(ref _busy);
        _idle.Change(Timeout.Infinite, Timeout.Infinite);
        var turn = new Turn(folder, o, progress, events, ct);
        Task? pump = null;
        try
        {
            progress?.Report(KiroPhase.Starting);
            await Start(ct);
            var inv = await Inventory(folder, ct);
            var model = PickModel(inv, o.Model, out var modelError);
            if (modelError is not null) return new(KiroState.Failed, modelError);
            var agent = o.Agent;
            var agents = await Get("/agent", folder, ct) as JsonArray ?? new JsonArray();
            if (agent is not null && !agents.Any(a => Str(a, "name") == agent))
                return new(KiroState.Failed, $"OpenCode has no agent named “{agent}” for this folder. Pick another in Settings → OpenCode.");
            var rules = Rules(o, agents, agent ?? Str(await Get("/config", folder, ct), "default_agent") ?? "build");

            if (resume is { Length: > 0 })
            {
                if (_stuck.ContainsKey(resume))
                {
                    // The last stop wasn't confirmed: a busy session isn't sent more.
                    var st = await Get("/session/status", folder, ct);
                    if (st?[resume] is JsonObject s && Str(s, "type") is "busy" or "retry")
                        return new(KiroState.Failed, "OpenCode is still stopping the last run of this conversation. Try again in a moment.");
                    _stuck.TryRemove(resume, out _);
                }
                try { await Get($"/session/{Uri.EscapeDataString(resume)}", folder, ct); }
                catch (OpenCodeError e) when (e.Status == HttpStatusCode.NotFound)
                {
                    // No quiet new conversation in its place: the transcript stays, the user decides.
                    return new(KiroState.Failed, "OpenCode no longer has this conversation, so it can’t carry on from here. The chat above is kept. Start a new task to go on.");
                }
                // Resuming skips session create, so the rules are set again.
                await Send(HttpMethod.Patch, $"/session/{Uri.EscapeDataString(resume)}", folder, new JsonObject { ["permission"] = rules }, ct);
                turn.Sid = resume;
            }
            else
            {
                var created = await Send(HttpMethod.Post, "/session", folder, new JsonObject { ["title"] = Title(prompt), ["permission"] = rules }, ct);
                turn.Sid = Str(created, "id") ?? throw new OpenCodeError(null, "OpenCode didn’t start a session.");
            }
            turn.Related[turn.Sid] = 0;
            _turns[turn.Sid] = turn;
            events?.Report(new KiroEvent(SessionId: turn.Sid));

            // Events first, then the prompt: nothing it does is missed.
            pump = Pump(turn);
            if (await Task.WhenAny(turn.Connected.Task, Task.Delay(ConnectTimeout, ct)) != turn.Connected.Task)
            {
                ct.ThrowIfCancellationRequested();
                return new(KiroState.Failed, "OpenCode’s event stream didn’t connect. Try again.");
            }
            _ = RecoverAsks(turn);

            turn.MessageId = NewMessageId();
            var body = new JsonObject { ["messageID"] = turn.MessageId, ["parts"] = new JsonArray(new JsonObject { ["type"] = "text", ["text"] = prompt.Trim() }) };
            if (model is { } m)
            {
                body["model"] = new JsonObject { ["providerID"] = m.Provider, ["modelID"] = m.Model };
                // Only a variant this model has; never one made up.
                if (o.Effort is { } v && m.Variants.Contains(v)) body["variant"] = v;
            }
            if (agent is not null) body["agent"] = agent;
            using (ct.Register(() => _ = Stop(turn)))
            {
                await Submit(turn, body);
                var r = await turn.Done.Task;
                return r;
            }
        }
        catch (OperationCanceledException) { return Stopped(turn); }
        catch (OpenCodeError e) { return ct.IsCancellationRequested ? Stopped(turn) : new(KiroState.Failed, Explain(e.Message)); }
        catch (HttpRequestException e) { return ct.IsCancellationRequested ? Stopped(turn) : new(KiroState.Failed, $"Couldn’t reach OpenCode: {e.Message}"); }
        finally
        {
            turn.Done.TrySetResult(Stopped(turn));
            try { turn.Stream.Cancel(); } catch (ObjectDisposedException) { }
            foreach (var open in turn.Open.Values) try { open.Cancel(); } catch (ObjectDisposedException) { }
            if (turn.Sid.Length > 0 && _turns.TryGetValue(turn.Sid, out var t) && ReferenceEquals(t, turn)) _turns.TryRemove(turn.Sid, out _);
            if (pump is not null) try { await pump; } catch { }
            if (Interlocked.Decrement(ref _busy) == 0 && _link is not null)
                _idle.Change(TimeSpan.FromMinutes(Math.Max(1, _options().IdleMinutes)), Timeout.InfiniteTimeSpan);
        }
    }

    private static string Title(string prompt)
    {
        var line = prompt.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).FirstOrDefault() ?? "Hover task";
        return line.Length > 60 ? line[..59] + "…" : line;
    }

    private KiroResult Stopped(Turn t)
    {
        var said = t.Said;
        return new(KiroState.Cancelled, said.Length > 0 ? said : $"Stopped before {Name} finished.");
    }

    /// Sends the prompt once. When the answer is lost (a timeout, a dropped
    /// connection), the message is looked up by its id instead of sent again.
    private async Task Submit(Turn turn, JsonObject body)
    {
        try
        {
            await Send(HttpMethod.Post, $"/session/{Uri.EscapeDataString(turn.Sid)}/prompt_async", turn.Folder, body, turn.Token, SendTimeout);
            turn.Accepted = true;
            // Started: the model has the prompt. Its own news may have come first.
            if (turn.Phase == KiroPhase.Starting) turn.SetPhase(KiroPhase.Thinking);
            // It may have gone idle before the answer to the prompt came back.
            if (turn.IdleEarly) _ = Reconcile(turn, "idle while sending");
            return;
        }
        catch (OpenCodeError e) when (e.Status is { } s && (int)s is >= 400 and < 500)
        {
            // Refused outright: it wasn't taken.
            turn.Done.TrySetResult(new(KiroState.Failed, Explain(e.Message)));
            return;
        }
        catch (Exception e) when (e is OpenCodeError or HttpRequestException or TaskCanceledException && !turn.Token.IsCancellationRequested)
        {
            Log.Line($"opencode: prompt for {turn.Sid} unconfirmed - {e.Message}");
        }
        if (await MessageExists(turn)) { turn.Accepted = true; turn.UserSeen = true; _ = Reconcile(turn, "prompt found"); return; }
        turn.Done.TrySetResult(new(KiroState.Failed,
            "Hover couldn’t confirm OpenCode got the task, and it isn’t in the conversation, so it wasn’t sent again. Send it again when you’re ready."));
    }

    private async Task<bool> MessageExists(Turn turn)
    {
        try
        {
            var m = await Get($"/session/{Uri.EscapeDataString(turn.Sid)}/message/{Uri.EscapeDataString(turn.MessageId)}", turn.Folder, CancellationToken.None, TimeSpan.FromSeconds(5));
            return Str(m?["info"], "id") == turn.MessageId;
        }
        catch (Exception e) when (e is OpenCodeError or HttpRequestException or TaskCanceledException) { return false; }
    }

    /// Stop: OpenCode is asked to abort, questions still open are withdrawn, and the
    /// turn ends as stopped once it goes idle, or after a grace period. One that is
    /// still busy then is marked, and not sent more until it has stopped.
    private async Task Stop(Turn turn)
    {
        if (turn.Stopping || turn.Sid.Length == 0) return;
        turn.Stopping = true;
        foreach (var open in turn.Open.Values) try { open.Cancel(); } catch (ObjectDisposedException) { }
        try { await Send(HttpMethod.Post, $"/session/{Uri.EscapeDataString(turn.Sid)}/abort", turn.Folder, null, CancellationToken.None, TimeSpan.FromSeconds(5)); }
        catch (Exception e) when (e is OpenCodeError or HttpRequestException or TaskCanceledException) { Log.Line($"opencode: abort {turn.Sid} - {e.Message}"); }
        if (await Task.WhenAny(turn.Done.Task, Task.Delay(StopGrace)) != turn.Done.Task)
        {
            _stuck[turn.Sid] = 0;
            Log.Line($"opencode: {turn.Sid} didn't stop within {StopGrace.TotalSeconds:0}s");
            // Only this run uses the server: end it, and with it the run.
            if (Volatile.Read(ref _busy) == 1) Shutdown("didn't stop when asked");
            turn.Done.TrySetResult(Stopped(turn));
        }
    }

    private string Explain(string message)
    {
        var lower = message.ToLowerInvariant();
        if (lower.Contains("api key") || lower.Contains("unauthorized") || lower.Contains("authentication") || lower.Contains("not authenticated"))
            return $"{message}\n\n{Agents.SignInHint(AgentTool.OpenCode)}";
        return message.Length > 600 ? message[..599] + "…" : message;
    }

    // MARK: Models, variants, agents

    internal readonly record struct Pick(string Provider, string Model, IReadOnlyList<string> Variants, double? Limit);

    /// The model the settings name, exactly as OpenCode names it ("provider/model",
    /// where the model part may itself have slashes). Null model: OpenCode's default.
    internal static Pick? PickModel(JsonObject inv, string? wanted, out string? error)
    {
        error = null;
        if (wanted is null) return null;
        var slash = wanted.IndexOf('/');
        if (slash > 0 && inv["providers"] is JsonArray providers)
            foreach (var p in providers)
                if (Str(p, "id") == wanted[..slash] && p?["models"]?[wanted[(slash + 1)..]] is JsonObject model)
                    return new(wanted[..slash], wanted[(slash + 1)..], Variants(model), Num(model["limit"], "context"));
        error = $"OpenCode doesn’t offer “{wanted}” any more. Pick another model in the model menu.";
        return null;
    }

    private static List<string> Variants(JsonObject model) =>
        model["variants"] is JsonObject v ? v.Select(kv => kv.Key).ToList() : new();

    /// The folder's providers and models, read once per server and folder, and passed
    /// on as the tool's offers: every model with its own variants, and the agents.
    private async Task<JsonObject> Inventory(string folder, CancellationToken ct)
    {
        if (_inventory.TryGetValue(folder, out var known)) return known;
        var inv = (await Get("/config/providers", folder, ct))?.AsObject() ?? new JsonObject();
        var agents = (await Get("/agent", folder, ct))?.AsArray() ?? new JsonArray();
        _inventory[folder] = inv;
        OptionsSeen?.Invoke(Tool, Offers(inv, agents));
        return inv;
    }

    internal static List<AcpOption> Offers(JsonObject inv, JsonArray agents)
    {
        var models = new List<AcpChoice>();
        if (inv["providers"] is JsonArray providers)
            foreach (var p in providers.OfType<JsonObject>())
            {
                var pid = Str(p, "id");
                if (pid is null || p["models"] is not JsonObject list) continue;
                var pname = Str(p, "name") ?? pid;
                foreach (var (mid, mv) in list)
                    if (mv is JsonObject model)
                        models.Add(new AcpChoice($"{pid}/{mid}", $"{Str(model, "name") ?? mid} · {pname}", Variants(model)));
            }
        var modes = agents.OfType<JsonObject>()
            .Where(a => Str(a, "mode") is "primary" or "all" && a["hidden"]?.GetValueKind() != JsonValueKind.True && Str(a, "name") is { Length: > 0 })
            .Select(a => new AcpChoice(Str(a, "name")!, Cap(Str(a, "name")!))).ToList();
        return new() { new AcpOption("model", "model", null, models), new AcpOption("agent", "mode", null, modes) };
    }

    private static string Cap(string s) => s.Length == 0 ? s : char.ToUpperInvariant(s[0]) + s[1..];

    // MARK: Permission rules

    /// The session's rules for the tool access picked. OpenCode applies the last rule
    /// that matches, so the agent's own deny rules (the user's config and the agent's,
    /// Plan's no-edit for one) go after Hover's and always win: Full never undoes a
    /// deny. Read only is enforced by the server, not by trusting the agent's name.
    internal static JsonArray Rules(AgentOptions o, JsonArray agents, string agent)
    {
        static JsonObject R(string permission, string pattern, string action) => new() { ["permission"] = permission, ["pattern"] = pattern, ["action"] = action };
        var rules = new List<JsonObject>();
        if (o.ReadOnly)
        {
            // Everything but reading asks, and Hover turns every ask down in read only
            // (Permission), so the server runs no edit, command, subagent, MCP or custom
            // tool, and nothing outside the folder. They are asked about rather than
            // denied: a deny hides the tool, and OpenCode's free models refused a
            // request whose tools didn't look like OpenCode's own.
            rules.Add(R("*", "*", "ask"));
            foreach (var p in new[] { "read", "glob", "grep", "list", "lsp", "codesearch", "webfetch", "websearch", "todoread", "todowrite", "skill", "question" })
                rules.Add(R(p, "*", "allow"));
            rules.Add(R("read", "*.env", "deny"));
            rules.Add(R("read", "*.env.*", "deny"));
            foreach (var p in new[] { "edit", "bash", "task", "external_directory", "doom_loop" })
                rules.Add(R(p, "*", "ask"));
        }
        else if (o.Approval == AgentApproval.Autopilot)
        {
            rules.Add(R("*", "*", "allow"));
            rules.Add(R("external_directory", "*", "allow"));
            // OpenCode's own safety stop for a tool called over and over stays.
            rules.Add(R("doom_loop", "*", "ask"));
        }
        else
        {
            // T3's Supervised set, with edits in the folder let through for Ask first.
            rules.Add(R("*", "*", "ask"));
            foreach (var p in new[] { "read", "glob", "grep", "list", "lsp", "skill", "todoread", "todowrite", "question" })
                rules.Add(R(p, "*", "allow"));
            rules.Add(R("read", "*.env", "ask"));
            rules.Add(R("read", "*.env.*", "ask"));
            rules.Add(R("read", "*.env.example", "allow"));
            rules.Add(R("edit", "*", o.Approval == AgentApproval.Risky ? "allow" : "ask"));
            foreach (var p in new[] { "bash", "webfetch", "websearch", "codesearch", "external_directory", "doom_loop", "task" })
                rules.Add(R(p, "*", "ask"));
        }
        var chosen = agents.OfType<JsonObject>().FirstOrDefault(a => Str(a, "name") == agent);
        if (chosen?["permission"] is JsonArray own)
        {
            var list = own.OfType<JsonObject>().ToList();
            // Only a deny that is the agent's own last word: OpenCode's defaults deny
            // the question tool and allow it again further down, and that allow wins.
            for (var i = 0; i < list.Count; i++)
                if (Str(list[i], "action") == "deny" && Str(list[i], "permission") is { } p && Str(list[i], "pattern") is { } pat &&
                    !list.Skip(i + 1).Any(r => Str(r, "action") != "deny" && Matches(p, Str(r, "permission")) && Matches(pat, Str(r, "pattern"))))
                    rules.Add(R(p, pat, "deny"));
        }
        return new JsonArray(rules.ToArray<JsonNode?>());
    }

    /// OpenCode's wildcard: * is any run of characters, the rest is literal.
    internal static bool Matches(string value, string? pattern) =>
        pattern is not null && System.Text.RegularExpressions.Regex.IsMatch(value,
            "^" + System.Text.RegularExpressions.Regex.Escape(pattern).Replace("\\*", ".*") + "$", System.Text.RegularExpressions.RegexOptions.Singleline);

    // MARK: The event stream

    private async Task Pump(Turn turn)
    {
        var backoff = TimeSpan.FromMilliseconds(250);
        var watch = Watch(turn);
        while (!turn.Done.Task.IsCompleted && !turn.Stream.IsCancellationRequested)
        {
            try
            {
                var http = _http ?? throw new OpenCodeError(null, "OpenCode stopped.");
                using var req = new HttpRequestMessage(HttpMethod.Get, Url("/event", turn.Folder));
                using var res = await http.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, turn.Stream.Token);
                if (!res.IsSuccessStatusCode) throw new OpenCodeError(res.StatusCode, $"OpenCode’s event stream answered {(int)res.StatusCode}.");
                using var reader = new StreamReader(await res.Content.ReadAsStreamAsync(turn.Stream.Token), new UTF8Encoding(false));
                var data = new StringBuilder();
                while (await reader.ReadLineAsync(turn.Stream.Token) is { } line)
                {
                    if (line.Length == 0)
                    {
                        if (data.Length > 0) { Handle(turn, data.ToString()); data.Clear(); }
                        continue;
                    }
                    if (line.StartsWith("data:", StringComparison.Ordinal))
                    {
                        if (data.Length > 0) data.Append('\n');
                        data.Append(line.AsSpan(5).TrimStart());
                        // A runaway event can't grow without end.
                        if (data.Length > 8 * 1024 * 1024) data.Clear();
                    }
                }
                if (data.Length > 0) Handle(turn, data.ToString());
                backoff = TimeSpan.FromMilliseconds(250);
            }
            catch (OperationCanceledException) when (turn.Stream.IsCancellationRequested) { break; }
            catch (Exception e) when (e is IOException or HttpRequestException or OpenCodeError or OperationCanceledException)
            {
                if (_link is null) { turn.Done.TrySetResult(turn.Stopping ? Stopped(turn) : new(KiroState.Failed, "OpenCode stopped unexpectedly.")); break; }
                Log.Line($"opencode: event stream for {turn.Sid} dropped - {e.Message}");
            }
            if (turn.Done.Task.IsCompleted || turn.Stream.IsCancellationRequested) break;
            // Reconnecting: the bot keeps its pose, and the state is read back once in.
            try { await Task.Delay(backoff, turn.Stream.Token); } catch (OperationCanceledException) { break; }
            backoff = TimeSpan.FromMilliseconds(Math.Min(5000, backoff.TotalMilliseconds * 2));
        }
        try { await watch; } catch (OperationCanceledException) { }
    }

    private void Handle(Turn turn, string data)
    {
        JsonNode? e;
        try { e = JsonNode.Parse(data); }
        catch (JsonException) { return; }
        if (e is not JsonObject ev || Str(ev, "type") is not { } type) return;
        turn.LastEvent = DateTime.UtcNow;
        if (type == "server.connected")
        {
            var again = turn.Connects;
            turn.Connects = true;
            turn.Connected.TrySetResult();
            // Events missed while away aren't sent again: read the state back.
            if (again) _ = Reconcile(turn, "reconnected");
            return;
        }
        try { Apply(turn, type, ev["properties"] as JsonObject ?? new JsonObject()); }
        catch (Exception x) when (x is InvalidOperationException or FormatException or KeyNotFoundException)
        {
            Log.Line($"opencode: skipped a {type} event - {x.Message}");
        }
    }

    /// One event, as it touches this turn. Parts and deltas are merged by their ids,
    /// so a delta and the whole part after it never say the same words twice.
    private void Apply(Turn turn, string type, JsonObject p)
    {
        var sid = Str(p, "sessionID");
        switch (type)
        {
            case "session.created":
            case "session.updated":
                if (p["info"] is JsonObject info && Str(info, "parentID") is { } parent && turn.Related.ContainsKey(parent) && Str(info, "id") is { } child)
                    turn.Related[child] = 0;
                return;
            case "permission.asked":
                if (sid is not null && turn.Related.ContainsKey(sid)) _ = Permission(turn, p);
                return;
            case "question.asked":
                if (sid is not null && turn.Related.ContainsKey(sid)) _ = Question(turn, p);
                return;
            case "permission.replied":
            case "question.replied":
            case "question.rejected":
                // Answered elsewhere (another client) or by Hover: its card goes.
                if (Str(p, "requestID") is { } rid)
                {
                    turn.Resolved[rid] = 0;
                    if (turn.Open.TryRemove(rid, out var open)) try { open.Cancel(); } catch (ObjectDisposedException) { }
                }
                return;
        }
        if (sid != turn.Sid) return;
        switch (type)
        {
            case "message.updated":
                if (p["info"] is not JsonObject msg || Str(msg, "id") is not { } mid) return;
                var role = Str(msg, "role") ?? "";
                lock (turn.Lock) turn.Roles[mid] = role;
                if (role == "user" && mid == turn.MessageId) turn.UserSeen = true;
                if (role == "assistant" && Mine(turn, mid, Str(msg, "parentID")))
                {
                    lock (turn.Lock) if (!turn.Messages.Contains(mid)) turn.Messages.Add(mid);
                    if (msg["error"] is JsonObject err && ErrorText(err) is { } why && Str(err, "name") != "MessageAbortedError") turn.Error = why;
                    Usage(turn, msg);
                }
                return;
            case "message.part.updated":
                if (p["part"] is JsonObject part) Part(turn, part);
                return;
            case "message.part.delta":
                if (Str(p, "field") != "text" || Str(p, "partID") is not { } pid || Str(p, "delta") is not { Length: > 0 } delta) return;
                lock (turn.Lock)
                {
                    if (!turn.Text.ContainsKey(pid)) return;
                    turn.Text[pid] += delta;
                }
                turn.SetPhase(KiroPhase.Writing);
                return;
            case "session.status":
                var status = p["status"] as JsonObject;
                switch (Str(status, "type"))
                {
                    case "busy":
                        if (turn.MessageId.Length > 0) turn.BusySeen = true;
                        turn.IdleConfirms = 0;
                        break;
                    case "retry":
                        if (turn.MessageId.Length > 0) turn.BusySeen = true;
                        turn.Retry = Str(status, "message");
                        break;
                    case "idle":
                        Idle(turn);
                        break;
                }
                return;
            case "session.idle":
                Idle(turn);
                return;
            case "session.error":
                var error = p["error"] as JsonObject;
                if (Str(error, "name") == "MessageAbortedError")
                {
                    if (turn.Stopping) turn.Done.TrySetResult(Stopped(turn));
                    return;
                }
                if (!turn.Accepted) return;
                turn.Error = ErrorText(error) ?? turn.Retry ?? "OpenCode reported an error.";
                turn.Done.TrySetResult(turn.Stopping ? Stopped(turn) : new(KiroState.Failed, Explain(turn.Error)));
                return;
        }
    }

    /// An assistant message answers this turn's prompt, or came after it.
    private static bool Mine(Turn turn, string messageId, string? parentId) =>
        turn.MessageId.Length > 0 && (parentId == turn.MessageId || string.CompareOrdinal(messageId, turn.MessageId) > 0);

    /// Only an idle after the server took this prompt (it saw the message, or went
    /// busy for it) ends the turn. Anything else is looked into instead.
    private void Idle(Turn turn)
    {
        if (!turn.Accepted) { if (turn.MessageId.Length > 0 && (turn.UserSeen || turn.BusySeen)) turn.IdleEarly = true; return; }
        if (turn.Stopping) { turn.Done.TrySetResult(Stopped(turn)); return; }
        if (turn.UserSeen || turn.BusySeen) { Finish(turn); return; }
        _ = Reconcile(turn, "idle before the prompt was seen");
    }

    private void Finish(Turn turn)
    {
        var said = turn.Said;
        if (turn.Error is { } err) { turn.Done.TrySetResult(new(KiroState.Failed, Explain(err))); return; }
        if (turn.Refused && said.Length == 0)
        {
            turn.Done.TrySetResult(new(KiroState.Failed, "OpenCode wanted to change files or run a command, and it is set to read only (Settings → OpenCode)."));
            return;
        }
        // The model's own words can claim it did what read only refused.
        if (turn.Refused) said += "\n\n*Hover has OpenCode set to read only, so the changes or commands it tried were refused.*";
        turn.Done.TrySetResult(new(KiroState.Completed, said.Length > 0 ? said : "Done. OpenCode didn’t leave a summary."));
    }

    private void Part(Turn turn, JsonObject part)
    {
        if (Str(part, "id") is not { } id || Str(part, "messageID") is not { } mid) return;
        string? role;
        lock (turn.Lock) role = turn.Roles.GetValueOrDefault(mid);
        if (role == "user" || mid == turn.MessageId || !Mine(turn, mid, null)) return;
        lock (turn.Lock) if (!turn.Messages.Contains(mid)) turn.Messages.Add(mid);
        switch (Str(part, "type"))
        {
            case "text":
                if (part["synthetic"]?.GetValueKind() == JsonValueKind.True) return;
                lock (turn.Lock)
                {
                    if (!turn.PartOrder.TryGetValue(mid, out var order)) turn.PartOrder[mid] = order = new();
                    if (!order.Contains(id)) order.Add(id);
                    // The whole part replaces what the deltas built: never twice.
                    turn.Text[id] = Str(part, "text") ?? "";
                }
                turn.SetPhase(KiroPhase.Writing);
                return;
            case "reasoning":
                turn.SetPhase(KiroPhase.Thinking);
                return;
            case "tool":
                ToolPart(turn, part);
                return;
            case "step-finish":
                if (part["tokens"] is JsonObject tokens) Tokens(turn, tokens, null);
                return;
        }
    }

    private void ToolPart(Turn turn, JsonObject part)
    {
        var tool = Str(part, "tool") ?? "tool";
        // The question itself shows as the question's card.
        if (tool == "question" || Str(part, "callID") is not { } call) return;
        var state = part["state"] as JsonObject;
        var status = Str(state, "status") switch { "completed" => "completed", "error" => "failed", _ => "in_progress" };
        var input = state?["input"] as JsonObject;
        var kind = Kind(tool);
        var target = Str(input, "filePath") ?? Str(input, "path") ?? Str(input, "command") ?? Str(input, "pattern") ?? Str(input, "url") ?? Str(input, "query");
        var title = Str(state, "title") is { Length: > 0 } t ? t : Cap(tool);
        KiroStep next;
        lock (turn.Lock)
        {
            if (!turn.Steps.TryGetValue(call, out var known)) { known = new KiroStep(call, kind, title, target, status); turn.Began[call] = DateTime.UtcNow; }
            next = known with { Status = status, Title = title, Target = known.Target ?? target };
            if (kind == "edit" && input is not null && Change(input) is { } d) next = next with { Added = d.Added, Removed = d.Removed, Diff = d.Preview };
            if (kind == "execute" && status != "in_progress")
            {
                var output = Str(state, "output") ?? Str(state, "error");
                if (output is not null) next = next with { Output = KiroStream.OutputOf(JsonDocument.Parse(JsonSerializer.Serialize(new { rawOutput = output })).RootElement, out _) };
                if (Num(state?["metadata"], "exit") is { } exit) next = next with { Exit = (int)exit };
            }
            if (status != "in_progress" && known.Ms is null && turn.Began.TryGetValue(call, out var t0)) next = next with { Ms = (DateTime.UtcNow - t0).TotalMilliseconds };
            if (turn.Steps.ContainsKey(call) && next == known) return;
            turn.Steps[call] = next;
        }
        if (status == "in_progress" && KiroStream.ToolPhase(kind, title) is { } phase) turn.SetPhase(phase);
        turn.Events?.Report(new KiroEvent(Step: next));
    }

    /// OpenCode's tools as ACP's kinds, which the office draws.
    internal static string Kind(string tool) => tool switch
    {
        "read" => "read",
        "write" or "edit" or "multiedit" or "patch" or "apply_patch" => "edit",
        "bash" or "shell" => "execute",
        "glob" or "grep" or "list" or "codesearch" => "search",
        "webfetch" or "websearch" => "fetch",
        "todowrite" or "todoread" => "think",
        _ => "other",
    };

    /// An edit's lines added and removed, from its old and new text.
    private static (int Added, int Removed, string Preview)? Change(JsonObject input)
    {
        var oldText = Str(input, "oldString");
        var newText = Str(input, "newString") ?? Str(input, "content");
        if (newText is null) return null;
        var diff = new { content = new[] { new { type = "diff", oldText, newText } } };
        return KiroStream.DiffOf(JsonDocument.Parse(JsonSerializer.Serialize(diff)).RootElement);
    }

    private void Usage(Turn turn, JsonObject msg)
    {
        if (msg["tokens"] is JsonObject tokens && Str(msg, "providerID") is { } pid && Str(msg, "modelID") is { } mid)
            Tokens(turn, tokens, $"{pid}/{mid}");
    }

    private readonly ConcurrentDictionary<string, string> _lastModel = new();

    /// How full the context is: the tokens of the last request over the model's window.
    private void Tokens(Turn turn, JsonObject tokens, string? model)
    {
        if (model is not null) _lastModel[turn.Sid] = model;
        else model = _lastModel.GetValueOrDefault(turn.Sid);
        if (model is null || !_inventory.TryGetValue(turn.Folder, out var inv) || PickModel(inv, model, out _) is not { Limit: > 0 } m) return;
        var used = Num(tokens, "input") + Num(tokens, "output") + Num(tokens["cache"], "read") + Num(tokens["cache"], "write");
        if (used is not > 0) return;
        var pct = Math.Clamp(used.Value * 100 / m.Limit!.Value, 0, 100);
        if (turn.Context is { } old && Math.Abs(old - pct) < 0.5) return;
        turn.Context = pct;
        turn.Events?.Report(new KiroEvent(Context: pct));
    }

    // MARK: Looking into the state

    /// No event for a while: the state and messages are read from the server.
    private async Task Watch(Turn turn)
    {
        while (!turn.Done.Task.IsCompleted && !turn.Stream.IsCancellationRequested)
        {
            await Task.Delay(TimeSpan.FromSeconds(3), turn.Stream.Token);
            if (turn.Accepted && DateTime.UtcNow - turn.LastEvent > Quiet) await Reconcile(turn, "quiet");
        }
    }

    private readonly SemaphoreSlim _reconciling = new(1, 1);

    /// What the server says now: busy carries on; idle with this prompt in the
    /// conversation ends the turn with the messages read back; a prompt that never
    /// arrived, after a few looks, fails rather than hang.
    private async Task Reconcile(Turn turn, string why)
    {
        if (!await _reconciling.WaitAsync(0)) return;
        try
        {
            if (turn.Done.Task.IsCompleted || !turn.Accepted) return;
            Log.Line($"opencode: reading {turn.Sid} back ({why})");
            var st = await Get("/session/status", turn.Folder, turn.Stream.Token, TimeSpan.FromSeconds(5));
            var type = Str(st?[turn.Sid], "type") ?? "idle";
            if (type is "busy" or "retry") { turn.BusySeen = true; turn.IdleConfirms = 0; turn.LastEvent = DateTime.UtcNow; return; }
            if (!turn.UserSeen && await MessageExists(turn)) turn.UserSeen = true;
            await ReadBack(turn);
            await RecoverAsks(turn);
            if (turn.UserSeen && (turn.BusySeen || ++turn.IdleConfirms >= 2)) { Finish(turn); return; }
            if (!turn.UserSeen && ++turn.IdleConfirms >= 5)
                turn.Done.TrySetResult(new(KiroState.Failed, "OpenCode took the task but never started it. Send it again when you’re ready."));
        }
        catch (Exception e) when (e is OpenCodeError or HttpRequestException or OperationCanceledException)
        {
            Log.Line($"opencode: couldn't read {turn.Sid} back - {e.Message}");
        }
        finally { _reconciling.Release(); }
    }

    /// This turn's messages from the server, merged by id with what the events built.
    private async Task ReadBack(Turn turn)
    {
        var list = await Get($"/session/{Uri.EscapeDataString(turn.Sid)}/message", turn.Folder, turn.Stream.Token, TimeSpan.FromSeconds(10));
        if (list is not JsonArray messages) return;
        foreach (var m in messages.OfType<JsonObject>())
        {
            if (m["info"] is not JsonObject info) continue;
            Apply(turn, "message.updated", new JsonObject { ["sessionID"] = turn.Sid, ["info"] = info.DeepClone() });
            if (m["parts"] is JsonArray parts)
                foreach (var part in parts.OfType<JsonObject>()) Part(turn, part.DeepClone().AsObject());
        }
    }

    /// Requests left waiting (from before a reconnect, or from a run Hover wasn't
    /// watching) are asked about now; the ones already open or answered aren't.
    private async Task RecoverAsks(Turn turn)
    {
        try
        {
            foreach (var p in (await Get("/permission", turn.Folder, turn.Stream.Token, TimeSpan.FromSeconds(5)))?.AsArray().OfType<JsonObject>() ?? Enumerable.Empty<JsonObject>())
                if (Str(p, "sessionID") is { } s && turn.Related.ContainsKey(s)) _ = Permission(turn, p);
            foreach (var q in (await Get("/question", turn.Folder, turn.Stream.Token, TimeSpan.FromSeconds(5)))?.AsArray().OfType<JsonObject>() ?? Enumerable.Empty<JsonObject>())
                if (Str(q, "sessionID") is { } s && turn.Related.ContainsKey(s)) _ = Question(turn, q);
        }
        catch (Exception e) when (e is OpenCodeError or HttpRequestException or OperationCanceledException)
        {
            Log.Line($"opencode: couldn't read waiting requests - {e.Message}");
        }
    }

    // MARK: Approvals and questions

    /// Read only turns every request down (its rules should leave none). Otherwise
    /// what OpenCode asks is asked of the user: its rules already let through what the
    /// access allows, and a request it sends on purpose is never answered yes for the
    /// user. Trust is Hover's, for this session, and each yes is OpenCode's "once":
    /// its "always" can outlast the session.
    private async Task Permission(Turn turn, JsonObject req)
    {
        if (Str(req, "id") is not { } id || turn.Resolved.ContainsKey(id)) return;
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(turn.Token);
        if (!turn.Open.TryAdd(id, cts)) return;
        string reply;
        string? message = null;
        try
        {
            var ask = Describe(req, turn.Folder);
            var trusted = _trusted.GetOrAdd(turn.Sid, _ => new());
            var key = ask.Kind + ":" + (ask.Command ?? ask.Path ?? ask.Title);
            if (turn.Options.ReadOnly) { reply = "reject"; message = "Hover has OpenCode set to read only."; turn.Refused = true; }
            else if (trusted.ContainsKey("*") || trusted.ContainsKey(key)) reply = "once";
            else if (Asking is not { } asking) reply = "reject";
            else
            {
                var answer = asking(turn.Sid, ask, cts.Token);
                var gone = Task.Delay(Timeout.Infinite, cts.Token);
                if (await Task.WhenAny(answer, gone) != answer) { reply = "reject"; message = "Stopped."; }
                else
                {
                    switch (await answer)
                    {
                        case AskAnswer.Allow: reply = "once"; break;
                        case AskAnswer.Trust: trusted[key] = true; reply = "once"; break;
                        case AskAnswer.TrustAll: trusted["*"] = true; reply = "once"; break;
                        default: reply = "reject"; break;
                    }
                }
            }
            // Answered elsewhere meanwhile: nothing to send.
            if (turn.Resolved.ContainsKey(id)) return;
            turn.Resolved[id] = 0;
            var body = new JsonObject { ["reply"] = reply };
            if (message is not null) body["message"] = message;
            await Send(HttpMethod.Post, $"/permission/{Uri.EscapeDataString(id)}/reply", turn.Folder, body, CancellationToken.None, TimeSpan.FromSeconds(10));
        }
        catch (Exception e) when (e is OpenCodeError or HttpRequestException or OperationCanceledException)
        {
            Log.Line($"opencode: permission {id} - {e.Message}");
        }
        finally { turn.Open.TryRemove(id, out _); }
    }

    /// A question goes to the user as it is; the answer is theirs, never made up. A
    /// skipped or withdrawn one is rejected, which OpenCode tells the agent.
    private async Task Question(Turn turn, JsonObject req)
    {
        if (Str(req, "id") is not { } id || turn.Resolved.ContainsKey(id)) return;
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(turn.Token);
        if (!turn.Open.TryAdd(id, cts)) return;
        try
        {
            var questions = (req["questions"] as JsonArray ?? new JsonArray()).OfType<JsonObject>().Select(q => new AgentQuestion(
                Str(q, "header") ?? "Question", Str(q, "question") ?? "",
                (q["options"] as JsonArray ?? new JsonArray()).OfType<JsonObject>().Select(o => (Str(o, "label") ?? "", Str(o, "description") ?? "")).Where(o => o.Item1.Length > 0).ToList(),
                q["multiple"]?.GetValueKind() == JsonValueKind.True, q["custom"]?.GetValueKind() != JsonValueKind.False)).ToList();
            IReadOnlyList<IReadOnlyList<string>>? answers = null;
            if (questions.Count > 0 && Questioning is { } questioning)
            {
                var first = questions[0];
                var ask = new AgentAsk(id, "question", first.Header, null, null, null, 0, 0, first.Question, false, questions);
                var answer = questioning(turn.Sid, ask, cts.Token);
                var gone = Task.Delay(Timeout.Infinite, cts.Token);
                if (await Task.WhenAny(answer, gone) == answer) answers = await answer;
            }
            if (turn.Resolved.ContainsKey(id)) return;
            turn.Resolved[id] = 0;
            if (answers is { Count: > 0 })
                await Send(HttpMethod.Post, $"/question/{Uri.EscapeDataString(id)}/reply", turn.Folder,
                    new JsonObject { ["answers"] = new JsonArray(answers.Select(a => (JsonNode?)new JsonArray(a.Select(x => (JsonNode?)x).ToArray())).ToArray()) },
                    CancellationToken.None, TimeSpan.FromSeconds(10));
            else
                await Send(HttpMethod.Post, $"/question/{Uri.EscapeDataString(id)}/reject", turn.Folder, null, CancellationToken.None, TimeSpan.FromSeconds(10));
        }
        catch (Exception e) when (e is OpenCodeError or HttpRequestException or OperationCanceledException)
        {
            Log.Line($"opencode: question {id} - {e.Message}");
        }
        finally { turn.Open.TryRemove(id, out _); }
    }

    /// OpenCode's permission request as the notch and the office show it.
    internal static AgentAsk Describe(JsonObject req, string folder)
    {
        var permission = Str(req, "permission") ?? "tool";
        var meta = req["metadata"] as JsonObject;
        var pattern = (req["patterns"] as JsonArray)?.Select(x => x?.GetValueKind() == JsonValueKind.String ? x.GetValue<string>() : null).FirstOrDefault(x => x is { Length: > 0 });
        var id = Str(req, "id") ?? Guid.NewGuid().ToString("N");
        string kind = "other", title = Cap(permission);
        string? command = null, path = null, preview = null;
        int added = 0, removed = 0;
        switch (permission)
        {
            case "bash":
                kind = "execute";
                title = "Run a command";
                command = Str(meta, "command") ?? pattern;
                break;
            case "edit":
                kind = "edit";
                title = "Edit a file";
                path = Str(meta, "filepath") ?? Str(meta, "filePath") ?? pattern;
                if (Str(meta, "diff") is { } diff)
                {
                    var lines = diff.Replace("\r", "").Split('\n');
                    var changed = lines.Where(l => (l.StartsWith('+') && !l.StartsWith("+++")) || (l.StartsWith('-') && !l.StartsWith("---"))).ToList();
                    added = changed.Count(l => l[0] == '+');
                    removed = changed.Count(l => l[0] == '-');
                    preview = string.Join("\n", changed.Take(6).Select(l => l[0] + " " + Clip(l[1..].Trim(), 110)));
                }
                break;
            case "webfetch" or "websearch" or "codesearch":
                kind = "fetch";
                title = "Use the network";
                command = Str(meta, "url") ?? Str(meta, "query") ?? pattern;
                break;
            case "read":
                kind = "read";
                title = "Read a file";
                path = Str(meta, "filePath") ?? pattern;
                break;
            case "external_directory":
                title = "Work outside the folder";
                path = pattern;
                break;
            case "task":
                title = "Start a subagent";
                command = pattern;
                break;
            case "doom_loop":
                title = "Repeat the same tool call";
                break;
        }
        var outside = permission == "external_directory";
        if (path is { Length: > 0 })
        {
            try
            {
                var full = Path.IsPathFullyQualified(path) ? Path.GetFullPath(path) : Path.GetFullPath(Path.Combine(folder, path.TrimEnd('*')));
                var root = Path.GetFullPath(folder).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
                if (full.StartsWith(root, StringComparison.OrdinalIgnoreCase)) path = full[root.Length..].Replace('\\', '/');
                else outside = true;
            }
            catch (Exception e) when (e is ArgumentException or NotSupportedException or PathTooLongException) { }
        }
        var danger = permission == "doom_loop" || (command is not null && AcpHost.Destructive.IsMatch(command));
        var reason = kind switch
        {
            "execute" => danger ? "Can delete or overwrite things" : command is not null && AcpHost.Network.IsMatch(command) ? "Installs packages or uses the network" : "Runs a command",
            "edit" => outside ? "Edits a file outside the folder" : added + removed > 0 ? $"Changes {added + removed} line{(added + removed == 1 ? "" : "s")}" : "Edits a file",
            "fetch" => "Uses the network",
            "read" => "Reads a file your OpenCode rules protect",
            _ => permission switch
            {
                "external_directory" => "Reaches outside the folder",
                "doom_loop" => "OpenCode saw it call the same tool again and again",
                "task" => "Hands part of the task to a subagent",
                _ => "Uses a tool",
            },
        };
        if (outside && kind is not ("edit" or "other")) reason += " · outside the folder";
        return new AgentAsk(id, kind, title, command is { Length: > 0 } ? Clip(command, 400) : null, path, preview, added, removed, reason, danger);
    }

    private static string Clip(string s, int max) => s.Length <= max ? s : s[..(max - 1)] + "…";

    // MARK: The process

    private async Task<OpenCodeLink?> Launch(CancellationToken ct)
    {
        if (Agents.Exe(AgentTool.OpenCode) is not { } exe) return null;
        var password = Convert.ToHexString(RandomNumberGenerator.GetBytes(24));
        var psi = Quota.Hidden(exe, Agents.Arguments(AgentTool.OpenCode));
        psi.WorkingDirectory = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        psi.Environment["OPENCODE_SERVER_PASSWORD"] = password;
        // The question tool, which Hover answers in the notch and the office.
        psi.Environment["OPENCODE_ENABLE_QUESTION_TOOL"] = "1";
        var p = new Process { StartInfo = psi, EnableRaisingEvents = true };
        p.Start();
        // In the job that closes with Hover: the server, its MCP servers and whatever
        // its tools start all go with Hover, however it exits.
        ChildJob.Add(p);
        p.StandardInput.Close();
        Log.Line($"opencode: started (pid {p.Id})");
        var tail = new StringBuilder();
        var ready = new TaskCompletionSource<Uri>(TaskCreationOptions.RunContinuationsAsynchronously);
        var exited = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        void Keep(string? line)
        {
            if (line is null) return;
            lock (tail) { tail.AppendLine(line); if (tail.Length > 8192) tail.Remove(0, tail.Length - 8192); }
            var at = line.IndexOf("listening on ", StringComparison.OrdinalIgnoreCase);
            if (at >= 0 && Uri.TryCreate(line[(at + 13)..].Trim(), UriKind.Absolute, out var url)) ready.TrySetResult(url);
        }
        // Both pipes are read to the end, so the server never blocks on a full one.
        p.OutputDataReceived += (_, e) => Keep(e.Data);
        p.ErrorDataReceived += (_, e) => Keep(e.Data);
        p.Exited += (_, _) =>
        {
            ready.TrySetException(new OpenCodeError(null, "OpenCode stopped before its server started."));
            exited.TrySetResult();
        };
        p.BeginOutputReadLine();
        p.BeginErrorReadLine();
        void Kill()
        {
            try { if (!p.HasExited) p.Kill(entireProcessTree: true); }
            catch { /* already gone */ }
            p.Dispose();
        }
        try
        {
            var url = await ready.Task.WaitAsync(StartTimeout, ct);
            if (!url.IsLoopback) throw new OpenCodeError(null, $"OpenCode listened on {url.Host}, not on this PC only.");
            return new OpenCodeLink(url, password, Kill, () => { lock (tail) return tail.ToString(); }, exited.Task);
        }
        catch (Exception e) when (e is TimeoutException or OpenCodeError or OperationCanceledException)
        {
            string why;
            lock (tail) why = string.Join(" / ", tail.ToString().Split('\n').Select(l => l.Trim()).Where(l => l.Length > 0).TakeLast(2));
            Kill();
            if (e is OperationCanceledException) throw;
            throw new OpenCodeError(null, e is TimeoutException ? $"OpenCode’s server didn’t start within {StartTimeout.TotalSeconds:0} s. {why}" : $"{e.Message} {why}".Trim());
        }
    }

    private async Task Start(CancellationToken ct)
    {
        await _gate.WaitAsync(ct);
        try
        {
            if (_link is not null) return;
            OpenCodeLink? link;
            try { link = await _connect(ct); }
            catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException or InvalidOperationException)
            {
                throw new OpenCodeError(null, $"OpenCode couldn’t start: {e.Message}");
            }
            if (link is null) throw new OpenCodeError(null, $"OpenCode isn’t installed. {Agents.InstallHint(AgentTool.OpenCode)}");
            var http = new HttpClient { BaseAddress = link.Url, Timeout = Timeout.InfiniteTimeSpan };
            http.DefaultRequestHeaders.Authorization = new System.Net.Http.Headers.AuthenticationHeaderValue("Basic",
                Convert.ToBase64String(Encoding.UTF8.GetBytes("opencode:" + link.Password)));
            _link = link;
            _http = http;
            _inventory.Clear();
            _ = link.Exited.ContinueWith(_ => Gone(link), TaskScheduler.Default);
            try
            {
                // Its health and version before any task: an API Hover wasn't checked
                // against is a clear error, not a strange failure later.
                var health = await Get("/global/health", null, ct, TimeSpan.FromSeconds(5));
                var version = Str(health, "version") ?? "";
                if (health?["healthy"]?.GetValueKind() != JsonValueKind.True || !Version.TryParse(version.Split('-')[0], out var v))
                    throw new OpenCodeError(null, "OpenCode’s server didn’t say it was healthy.");
                if (v < Version.Parse(Agents.OpenCodeMinVersion))
                    throw new OpenCodeError(null, $"OpenCode {version} is too old for Hover. {Agents.InstallHint(AgentTool.OpenCode)}");
                Log.Line($"opencode: server {version} at {link.Url.Authority}");
            }
            catch
            {
                Shutdown("didn't start");
                throw;
            }
        }
        finally { _gate.Release(); }
    }

    public void Shutdown(string why = "shut down") => End(null, why, "OpenCode stopped.");

    /// The server exited on its own: the runs using it fail and say why.
    private void Gone(OpenCodeLink link)
    {
        var why = Quota.StripAnsi(link.Errors()).Split('\n').Select(l => l.Trim()).Where(l => l.Length > 0).TakeLast(2).ToList();
        End(link, "exited - " + string.Join(" / ", why), "OpenCode stopped unexpectedly." + (why.Count > 0 ? " " + string.Join("\n", why) : ""));
    }

    private void End(OpenCodeLink? only, string why, string failure)
    {
        var link = only is null ? Interlocked.Exchange(ref _link, null)
            : ReferenceEquals(Interlocked.CompareExchange(ref _link, null, only), only) ? only : null;
        if (link is null) return;
        _idle.Change(Timeout.Infinite, Timeout.Infinite);
        Log.Line($"opencode: {why}");
        var http = Interlocked.Exchange(ref _http, null);
        _inventory.Clear();
        foreach (var turn in _turns.Values)
        {
            turn.Done.TrySetResult(turn.Stopping ? Stopped(turn) : new(KiroState.Failed, failure));
            try { turn.Stream.Cancel(); } catch (ObjectDisposedException) { }
        }
        link.Kill();
        http?.Dispose();
    }

    // MARK: HTTP

    private string Url(string path, string? folder) =>
        folder is null ? path : path + (path.Contains('?') ? "&" : "?") + "directory=" + Uri.EscapeDataString(folder);

    private Task<JsonNode?> Get(string path, string? folder, CancellationToken ct, TimeSpan? timeout = null) =>
        Send(HttpMethod.Get, path, folder, null, ct, timeout ?? TimeSpan.FromSeconds(30));

    private async Task<JsonNode?> Send(HttpMethod method, string path, string? folder, JsonNode? body, CancellationToken ct, TimeSpan? timeout = null)
    {
        var http = _http ?? throw new OpenCodeError(null, "OpenCode stopped.");
        using var late = new CancellationTokenSource(timeout ?? TimeSpan.FromSeconds(30));
        using var both = CancellationTokenSource.CreateLinkedTokenSource(ct, late.Token);
        using var req = new HttpRequestMessage(method, Url(path, folder));
        if (body is not null) req.Content = new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json");
        HttpResponseMessage res;
        try { res = await http.SendAsync(req, both.Token); }
        catch (OperationCanceledException) when (late.IsCancellationRequested && !ct.IsCancellationRequested)
        {
            throw new OpenCodeError(null, $"OpenCode didn’t answer ({method} {path}).");
        }
        using (res)
        {
            var text = await res.Content.ReadAsStringAsync(both.Token);
            if (!res.IsSuccessStatusCode)
            {
                string? message = null;
                try { var n = JsonNode.Parse(text); message = Str(n?["data"], "message") ?? Str(n, "message") ?? Str(n?["error"], "message"); }
                catch (JsonException) { }
                throw new OpenCodeError(res.StatusCode, message ?? $"OpenCode answered {(int)res.StatusCode} to {method} {path}.");
            }
            if (text.Length == 0) return null;
            try { return JsonNode.Parse(text); }
            catch (JsonException) { return null; }
        }
    }

    // MARK: Helpers

    private static long _lastMs, _counter;
    private static readonly object IdLock = new();

    /// A message id in OpenCode's own form (msg_, 12 hex digits of time, 14 random
    /// characters), later than any before it, so the server orders it last. As T3 makes it.
    internal static string NewMessageId()
    {
        long ms, n;
        lock (IdLock)
        {
            ms = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
            if (ms <= _lastMs) ms = _lastMs; else { _lastMs = ms; _counter = 0; }
            n = ++_counter;
        }
        var time = ((ulong)ms * 0x1000UL + (ulong)n) & 0xFFFF_FFFF_FFFFUL;
        const string alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        var random = RandomNumberGenerator.GetBytes(14).Select(b => alphabet[b % alphabet.Length]);
        return "msg_" + time.ToString("x12") + string.Concat(random);
    }

    private static string? ErrorText(JsonObject? error) =>
        error is null ? null : Str(error["data"], "message") ?? Str(error, "message") ?? Str(error, "name");

    internal static string? Str(JsonNode? n, string name) =>
        n is JsonObject o && o[name] is JsonValue v && v.GetValueKind() == JsonValueKind.String ? v.GetValue<string>() : null;

    private static double? Num(JsonNode? n, string name) =>
        n is JsonObject o && o[name] is JsonValue v && v.GetValueKind() == JsonValueKind.Number ? v.GetValue<double>() : null;
}

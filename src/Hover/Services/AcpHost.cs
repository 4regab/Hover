using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using Hover.Core;

namespace Hover.Services;

/// One choice in a session config option (ACP session/set_config_option).
/// Levels are the efforts this choice takes, where they differ by choice (OpenCode's
/// variants belong to each model); null when the tool lists effort on its own.
public sealed record AcpChoice(string Value, string Name, IReadOnlyList<string>? Levels = null);

/// A setting an agent offers for a session: its model, effort, mode and the like.
/// Category is ACP's hint (model, thought_level, mode), when the agent gives one.
public sealed record AcpOption(string Id, string? Category, string? Current, IReadOnlyList<AcpChoice> Choices)
{
    public bool Has(string value) => Choices.Any(c => c.Value == value);
}

/// The pipes to a running agent: what Hover writes, what it reads, and how to end it.
public sealed class AcpLink
{
    public AcpLink(Stream toAgent, Stream fromAgent, Action kill, Func<string>? errors = null)
    {
        ToAgent = toAgent;
        FromAgent = fromAgent;
        Kill = kill;
        Errors = errors ?? (() => "");
    }

    public Stream ToAgent { get; }
    public Stream FromAgent { get; }
    public Action Kill { get; }
    /// The end of what the agent printed on stderr, to say why it gave up.
    public Func<string> Errors { get; }
}

/// One agent tool running as a long-lived ACP server (JSON-RPC over stdio), shared by
/// every session of that tool. It starts on the first run, keeps each conversation as
/// an ACP session, and is shut down once it has had nothing to do for the idle time in
/// its settings. A reply after that starts it again and loads the conversation back
/// (session/load). The prompt only ever goes over stdin. No WPF in here.
public sealed class AcpHost : IAgentRuntime
{
    private readonly Func<AgentOptions> _options;
    private readonly Func<AcpLink?> _connect;
    private readonly SemaphoreSlim _gate = new(1, 1), _write = new(1, 1);
    private readonly ConcurrentDictionary<long, TaskCompletionSource<JsonElement>> _pending = new();
    private readonly ConcurrentDictionary<string, Turn> _turns = new();
    private readonly ConcurrentDictionary<string, List<AcpOption>> _sessionOptions = new();
    // The MCP servers each live session was given (ComputerUse.Signature).
    private readonly ConcurrentDictionary<string, string> _sessionMcp = new();
    private readonly Func<IReadOnlyList<McpServer>> _mcp;
    private readonly Timer _idle;
    private AcpLink? _link;
    // The folders its process was sandboxed for; null when it wasn't (or isn't Hover's own).
    private IReadOnlyList<string>? _boxFolders;
    private bool _ownLaunch;
    private bool _canLoad;
    private long _ids;
    private int _busy;

    /// mcp names the MCP servers each new or loaded session gets (Cua Driver, when
    /// computer use is on); read at the start of every run.
    public AcpHost(AgentTool tool, Func<AgentOptions> options, Func<AcpLink?>? connect = null, Func<IReadOnlyList<McpServer>>? mcp = null)
    {
        Tool = tool;
        _options = options;
        _connect = connect ?? Launch;
        _mcp = mcp ?? ComputerUse.Servers;
        _idle = new Timer(_ => { if (Volatile.Read(ref _busy) == 0) Shutdown("idle"); });
    }

    public AgentTool Tool { get; }
    private string Name => Agents.Name(Tool);

    /// ACP has no questions of its own; effort is a session option where offered.
    public AgentCaps Caps => new(Questions: false, ReadOnly: Agents.ReadOnlyWorks(Tool), Resume: true, EffortLabel: "Effort");

    /// ACP agents don't ask questions; this is never called.
    public Func<string, AgentAsk, CancellationToken, Task<IReadOnlyList<IReadOnlyList<string>>?>>? Questioning { get; set; }

    /// The tool's process is up.
    public bool Alive => _link is not null;

    /// The settings the agent offered for a session, whenever they are read or change.
    /// Raised off the UI thread.
    public event Action<AgentTool, IReadOnlyList<AcpOption>>? OptionsSeen;

    /// Asks the user about a tool call the agent wants to make, for the ACP session
    /// named first; the token ends when the run is stopped. Called off the UI thread.
    /// Without it, whatever the settings say should be asked about is turned down.
    public Func<string, AgentAsk, CancellationToken, Task<AskAnswer>>? Asking { get; set; }

    /// What the user trusted for the rest of a session, by ACP session id: the keys of
    /// tool calls (Key()), or "*" for everything.
    private readonly ConcurrentDictionary<string, ConcurrentDictionary<string, bool>> _trusted = new();

    private sealed class Turn(KiroStream stream, IProgress<KiroPhase>? progress, IProgress<KiroEvent>? events, AgentOptions options,
        string folder, CancellationToken token)
    {
        public KiroStream Stream { get; } = stream;
        public IProgress<KiroPhase>? Progress { get; } = progress;
        public IProgress<KiroEvent>? Events { get; } = events;
        public AgentOptions Options { get; } = options;
        public string Folder { get; } = folder;
        /// Cancelled when the run is stopped, which also withdraws a question.
        public CancellationToken Token { get; } = token;
        /// While a conversation is loaded back, the agent replays it; that isn't news.
        public volatile bool Muted;
        public volatile bool Refused;
        public volatile string? McpFailed;
    }

    private sealed class AcpError(int code, string message) : Exception(message)
    {
        public int Code { get; } = code;
    }

    private sealed class AcpGone(string why) : Exception(why);

    // MARK: A run

    /// Runs one turn in a folder: a new conversation, or the one resume names. Never
    /// throws; every way it goes wrong comes back as a Failed result. Cancelling ct
    /// asks the agent to stop (session/cancel); one that doesn't within a few seconds
    /// is left, or shut down when nothing else of its runs.
    public async Task<KiroResult> Run(string folder, string prompt, IProgress<KiroPhase>? progress, CancellationToken ct,
        string? resume = null, IProgress<KiroEvent>? events = null, string? access = null, string? tag = null)
    {
        if (!KiroRunner.UsableFolder(folder)) return new(KiroState.Failed, "That folder isn’t there any more. Choose another one.");
        if (string.IsNullOrWhiteSpace(prompt)) return new(KiroState.Failed, $"Tell {Name} what to do first.");
        // A sandboxed tool reaches only the folders it started with, and the sandbox
        // switched on or off applies from its next start: one that no longer fits is
        // started again when nothing of it runs. Busy in other folders, it can't take
        // this one yet.
        Sandbox.Remember(folder);
        if (_link is not null && _ownLaunch)
        {
            var box = _boxFolders;
            var outside = box is not null && !Sandbox.Covers(box, folder);
            if (outside || (box is not null) != Sandbox.Wanted)
            {
                if (Volatile.Read(ref _busy) == 0) Shutdown("its sandbox changed");
                else if (outside)
                    return new(KiroState.Failed, $"{Name} is working on a task in another folder, and its sandbox reaches only the folders it started with. Start this one when that task is done.");
            }
        }
        // The agent's cua-driver can't start CuaDriver's daemon from inside the
        // sandbox (no Launch Services there), so Hover does, outside it.
        if (Sandbox.Wanted && Settings.ComputerUse) await ComputerUse.EnsureDaemon();
        var o = _options().WithAccess(access);
        var servers = _mcp().Concat(BrowserTool.Servers(Tool, tag)).ToList();
        var mcp = ComputerUse.Signature(servers);
        // A session's MCP servers are fixed when it is made or loaded. A reply to one
        // made with others (computer use switched since) loads it again in a fresh
        // process, when nothing else of this tool runs; otherwise it carries on as is.
        if (resume is { Length: > 0 } && _canLoad && _sessionMcp.TryGetValue(resume, out var had) && had != mcp && Volatile.Read(ref _busy) == 0)
            Shutdown("its MCP servers changed");
        Interlocked.Increment(ref _busy);
        _idle.Change(Timeout.Infinite, Timeout.Infinite);
        var turn = new Turn(new KiroStream { Name = Name }, progress, events, o, folder, ct);
        string? sid = null;
        try
        {
            progress?.Report(KiroPhase.Starting);
            await Start(ct);
            List<AcpOption>? offered = null;
            if (resume is { Length: > 0 } && _sessionOptions.TryGetValue(resume, out var known))
            {
                sid = resume;
                offered = known;
            }
            else if (resume is { Length: > 0 } && _canLoad)
            {
                turn.Muted = true;
                _turns[resume] = turn;
                try
                {
                    var r = await Call("session/load", new { sessionId = resume, cwd = folder, mcpServers = ComputerUse.Acp(servers) }, ct, TimeSpan.FromMinutes(2));
                    sid = resume;
                    offered = Options(r);
                    _sessionMcp[sid] = mcp;
                }
                catch (AcpError e)
                {
                    // Gone from the agent's own history: carry on in a new conversation.
                    Log.Line($"acp {Name}: couldn't load {resume} - {e.Message}");
                    _turns.TryRemove(resume, out _);
                }
                turn.Muted = false;
            }
            if (sid is null)
            {
                var r = await Call("session/new", new { cwd = folder, mcpServers = ComputerUse.Acp(servers) }, ct, TimeSpan.FromMinutes(2));
                sid = Str(r, "sessionId") ?? throw new AcpError(0, $"{Name} didn’t start a session.");
                offered = Options(r);
                _sessionMcp[sid] = mcp;
            }
            _turns[sid] = turn;
            events?.Report(new KiroEvent(SessionId: sid));
            _sessionOptions[sid] = await Configure(sid, offered ?? new(), o, ct);

            // Started: from here the model has the prompt, and the notch says Thinking.
            progress?.Report(KiroPhase.Thinking);
            var call = Call("session/prompt", new { sessionId = sid, prompt = new[] { new { type = "text", text = prompt.Trim() } } }, CancellationToken.None, null);
            var cancelled = Task.Delay(Timeout.Infinite, ct).ContinueWith(_ => { }, TaskScheduler.Default);
            using (ct.Register(() => _ = Notify("session/cancel", new { sessionId = sid })))
            {
                if (await Task.WhenAny(call, cancelled) != call &&
                    await Task.WhenAny(call, Task.Delay(TimeSpan.FromSeconds(8))) != call)
                {
                    // It didn't stop when asked. Only this run is using it: end it.
                    if (Volatile.Read(ref _busy) == 1) Shutdown("didn't stop when asked");
                    return Finish(turn, "cancelled", true);
                }
            }
            var result = await call;
            return Finish(turn, Str(result, "stopReason"), ct.IsCancellationRequested);
        }
        catch (OperationCanceledException) { return Finish(turn, "cancelled", true); }
        catch (AcpError e) { return new(KiroState.Failed, Explain(e.Message)); }
        catch (AcpGone e) { return ct.IsCancellationRequested ? Finish(turn, "cancelled", true) : new(KiroState.Failed, e.Message); }
        finally
        {
            if (sid is not null && _turns.TryGetValue(sid, out var t) && ReferenceEquals(t, turn)) _turns.TryRemove(sid, out _);
            if (Interlocked.Decrement(ref _busy) == 0 && _link is not null)
                _idle.Change(TimeSpan.FromMinutes(Math.Max(1, _options().IdleMinutes)), Timeout.InfiniteTimeSpan);
        }
    }

    private KiroResult Finish(Turn t, string? stopReason, bool cancelled)
    {
        if (t.McpFailed is { } server) return new(KiroState.Failed, $"An MCP server {Name} depends on ({server}) didn’t start.");
        var said = t.Stream.Said.Trim();
        if (stopReason == "cancelled" && !cancelled && t.Refused)
            return new(KiroState.Failed, $"{Name} wanted to change files or run a command, and it is set to read only (Settings → {Name}).");
        if (cancelled || stopReason == "cancelled")
            return new(KiroState.Cancelled, said.Length > 0 ? said : $"Stopped before {Name} finished.");
        if (stopReason == "refusal") return new(KiroState.Failed, $"{Name} declined this request.");
        return t.Stream.Outcome(0, false, "");
    }

    private string Explain(string message)
    {
        var lower = message.ToLowerInvariant();
        if (lower.Contains("sign in") || lower.Contains("signed in") || lower.Contains("log in") || lower.Contains("login") ||
            lower.Contains("unauthenticated") || lower.Contains("unauthorized") || lower.Contains("authentication"))
            return $"{Name} needs you to sign in. {Agents.SignInHint(Tool)}";
        return message.Length > 600 ? message[..599] + "…" : message;
    }

    /// Sets the model, effort and access the settings ask for, where the agent offers
    /// them and they differ. What the agent offers after that is passed on to Settings.
    private async Task<List<AcpOption>> Configure(string sid, List<AcpOption> offered, AgentOptions o, CancellationToken ct)
    {
        AcpOption? Find(string? category, params string[] ids) =>
            offered.FirstOrDefault(x => category is not null && x.Category == category) ?? offered.FirstOrDefault(x => ids.Contains(x.Id));
        async Task Set(AcpOption? option, string? value)
        {
            if (option is null || value is null || option.Current == value) return;
            if (!option.Has(value)) { Log.Line($"acp {Name}: {option.Id}={value} isn't offered"); return; }
            try
            {
                var r = await Call("session/set_config_option", new { sessionId = sid, configId = option.Id, value }, ct, TimeSpan.FromSeconds(30));
                if (Options(r) is { Count: > 0 } now) offered = now;
            }
            catch (AcpError e) { Log.Line($"acp {Name}: {option.Id}={value} refused - {e.Message}"); }
        }

        await Set(Find("model", "model"), o.Model);
        // An effort list can appear only once a model is picked (Kiro's does).
        await Set(Find("thought_level", "effortLevel", "reasoning_effort", "effort"), o.Effort);
        // Asking needs the agent to ask Hover: each tool is put where it sends every
        // call it would stop for as session/request_permission, and Hover's own rules
        // (NeedsAsking) decide which of those reach the user. What each offers
        // (checked against their sources, Sep 2026):
        // - Kiro (v3): the autopilot option; off, everything past its built-in
        //   defaults (workspace reads, read-only git) asks.
        // - Codex (codex-acp): the mode option. agent-full-access never asks; "agent"
        //   is Auto review, where Codex's own reviewer approves what it thinks safe and
        //   Hover would rarely hear of it; workspace-write asks for writes outside the
        //   folder and the network; read-only asks for every write and command. Ask
        //   always takes read-only (Hover then allows reads itself), Ask first
        //   workspace-write, as Codex's own "Auto" preset does.
        // - Cursor (agent acp): asks unless started with --force; its modes are agent,
        //   plan and ask. So it asks either way, and Full answers yes (Permission()).
        var asks = !o.ReadOnly && o.Approval != AgentApproval.Autopilot;
        switch (Tool)
        {
            case AgentTool.Kiro:
                await Set(Find(null, "autopilot"), o.ReadOnly || asks ? "off" : "on");
                await Set(Find("mode", "mode"), o.Agent ?? "vibe");
                break;
            case AgentTool.Codex:
                // codex-acp 1.13 dropped workspace-write, and its read-only became that
                // preset ("Ask for approval": asks for outside the folder and the
                // network). So Ask first takes whichever of the two is there.
                var mode = Find("mode", "mode");
                var askFirst = mode?.Has("workspace-write") == true ? "workspace-write" : "read-only";
                await Set(mode, o.ReadOnly ? "read-only" : !asks ? "agent-full-access"
                    : o.Approval == AgentApproval.Always ? "read-only" : askFirst);
                break;
            case AgentTool.Cursor:
                await Set(Find("mode", "mode"), o.ReadOnly ? "ask" : "agent");
                break;
        }
        if (offered.Count > 0) OptionsSeen?.Invoke(Tool, offered);
        return offered;
    }

    // MARK: The process

    private AcpLink? Launch()
    {
        if (Agents.Exe(Tool) is not { } exe) return null;
        var psi = Quota.Hidden(exe, Agents.Arguments(Tool));
        psi.WorkingDirectory = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        // In the sandbox, for the folders its sessions use (Services.Sandbox).
        var folders = Sandbox.Folders();
        psi = Sandbox.Wrap(psi, Tool, folders);
        _boxFolders = Sandbox.Wanted ? folders : null;
        _ownLaunch = true;
        var p = new Process { StartInfo = psi, EnableRaisingEvents = true };
        p.Start();
        // In a job that closes with Hover, so the tool and everything it started go
        // too, even when Hover is killed rather than quit (they were left running).
        ChildJob.Add(p);
        Log.Line($"acp {Name}: started (pid {p.Id}){(_boxFolders is null ? "" : " in the sandbox")}");
        var tail = new StringBuilder();
        _ = Task.Run(async () =>
        {
            var buf = new char[2048];
            int n;
            try
            {
                while ((n = await p.StandardError.ReadAsync(buf)) > 0)
                    lock (tail) { tail.Append(buf, 0, n); if (tail.Length > 8192) tail.Remove(0, tail.Length - 8192); }
            }
            catch (Exception) { /* it has gone */ }
        });
        return new AcpLink(p.StandardInput.BaseStream, p.StandardOutput.BaseStream, () =>
        {
            try { if (!p.HasExited) p.Kill(entireProcessTree: true); }
            catch { /* already gone */ }
            p.Dispose();
        }, () => { lock (tail) return tail.ToString(); });
    }

    private async Task Start(CancellationToken ct)
    {
        await _gate.WaitAsync(ct);
        try
        {
            if (_link is not null) return;
            AcpLink? link;
            try { link = _connect(); }
            catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException or InvalidOperationException)
            {
                throw new AcpError(0, $"{Name} couldn’t start: {e.Message}");
            }
            if (link is null) throw new AcpError(0, $"{Name} isn’t installed. {Agents.InstallHint(Tool)}");
            _link = link;
            _sessionOptions.Clear();
            _sessionMcp.Clear();
            _ = Task.Run(() => Read(link));
            try
            {
                var init = await Call("initialize", new
                {
                    protocolVersion = 1,
                    clientCapabilities = new { fs = new { readTextFile = false, writeTextFile = false }, terminal = false },
                    clientInfo = new { name = "hover", version = "1" },
                }, ct, TimeSpan.FromMinutes(1));
                _canLoad = init.TryGetProperty("agentCapabilities", out var caps) && caps.ValueKind == JsonValueKind.Object &&
                           caps.TryGetProperty("loadSession", out var load) && load.ValueKind == JsonValueKind.True;
            }
            catch
            {
                Shutdown("didn't start");
                throw;
            }
        }
        finally { _gate.Release(); }
    }

    /// End the tool's process now. Runs still going fail; the next one starts it again.
    public void Shutdown(string why = "shut down")
    {
        var link = Interlocked.Exchange(ref _link, null);
        if (link is null) return;
        _idle.Change(Timeout.Infinite, Timeout.Infinite);
        Log.Line($"acp {Name}: {why}");
        _sessionOptions.Clear();
        _sessionMcp.Clear();
        Fail(new AcpGone($"{Name} stopped."));
        link.Kill();
    }

    private void Gone(AcpLink link)
    {
        if (!ReferenceEquals(Interlocked.CompareExchange(ref _link, null, link), link)) return;
        _idle.Change(Timeout.Infinite, Timeout.Infinite);
        _sessionOptions.Clear();
        _sessionMcp.Clear();
        var why = Quota.StripAnsi(link.Errors()).Split('\n').Select(l => l.Trim()).Where(l => l.Length > 0).TakeLast(2).ToList();
        Log.Line($"acp {Name}: exited - {string.Join(" / ", why)}");
        Fail(new AcpGone($"{Name} stopped unexpectedly." + (why.Count > 0 ? " " + string.Join("\n", why) : "")));
        link.Kill();
    }

    private void Fail(Exception e)
    {
        foreach (var id in _pending.Keys.ToList())
            if (_pending.TryRemove(id, out var tcs)) tcs.TrySetException(e);
    }

    // MARK: JSON-RPC

    private async Task Send(object message)
    {
        var link = _link ?? throw new AcpGone($"{Name} stopped.");
        var bytes = JsonSerializer.SerializeToUtf8Bytes(message);
        await _write.WaitAsync();
        try
        {
            await link.ToAgent.WriteAsync(bytes);
            await link.ToAgent.WriteAsync("\n"u8.ToArray());
            await link.ToAgent.FlushAsync();
        }
        catch (Exception e) when (e is IOException or ObjectDisposedException) { throw new AcpGone($"{Name} stopped."); }
        finally { _write.Release(); }
    }

    private Task Notify(string method, object @params) =>
        Send(new { jsonrpc = "2.0", method, @params }).ContinueWith(_ => { }, TaskScheduler.Default);

    private async Task<JsonElement> Call(string method, object @params, CancellationToken ct, TimeSpan? timeout)
    {
        var id = Interlocked.Increment(ref _ids);
        var tcs = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        _pending[id] = tcs;
        using var stop = ct.Register(() => { if (_pending.TryRemove(id, out var t)) t.TrySetCanceled(ct); });
        using var late = timeout is { } span ? new CancellationTokenSource(span) : null;
        using var expire = late?.Token.Register(() =>
        {
            if (_pending.TryRemove(id, out var t)) t.TrySetException(new AcpError(0, $"{Name} didn’t answer ({method})."));
        });
        try { await Send(new { jsonrpc = "2.0", id, method, @params }); }
        catch { _pending.TryRemove(id, out _); throw; }
        return await tcs.Task;
    }

    private async Task Read(AcpLink link)
    {
        try
        {
            using var reader = new StreamReader(link.FromAgent, new UTF8Encoding(false));
            while (await reader.ReadLineAsync() is { } line)
            {
                if (line.Length == 0 || line[0] != '{') continue;
                try { await Handle(line); }
                catch (JsonException) { /* not a message */ }
                catch (Exception e) when (e is AcpGone or IOException) { }
            }
        }
        catch (Exception e) when (e is IOException or ObjectDisposedException) { }
        Gone(link);
    }

    private async Task Handle(string line)
    {
        using var doc = JsonDocument.Parse(line);
        var m = doc.RootElement;
        if (m.ValueKind != JsonValueKind.Object) return;
        var method = Str(m, "method");
        var hasId = m.TryGetProperty("id", out var id) && id.ValueKind is JsonValueKind.Number or JsonValueKind.String;
        if (method is null)
        {
            // An answer to one of ours.
            if (!hasId || id.ValueKind != JsonValueKind.Number || !_pending.TryRemove(id.GetInt64(), out var tcs)) return;
            if (m.TryGetProperty("error", out var err) && err.ValueKind == JsonValueKind.Object)
                tcs.TrySetException(new AcpError(err.TryGetProperty("code", out var c) && c.ValueKind == JsonValueKind.Number ? c.GetInt32() : 0,
                    Str(err, "message") ?? $"{Name} reported an error."));
            else tcs.TrySetResult(m.TryGetProperty("result", out var r) ? r.Clone() : default);
            return;
        }
        var p = m.TryGetProperty("params", out var pp) ? pp : default;
        var turn = Str(p, "sessionId") is { } sid && _turns.TryGetValue(sid, out var t) ? t : null;
        if (hasId)
        {
            var idValue = id.Clone();
            // Answered on its own: the user may take minutes, and every other session's
            // news comes down this same pipe meanwhile.
            if (method == "session/request_permission")
                _ = AnswerPermission(idValue, turn, Str(p, "sessionId"), p.Clone(), Reviewed(turn, p));
            else
                await Send(new { jsonrpc = "2.0", id = idValue, error = new { code = -32601, message = "Not supported by Hover." } });
            return;
        }
        if (turn is null || turn.Muted) return;
        switch (method)
        {
            case "session/update":
                if (turn.Stream.Feed(line) is { } phase) turn.Progress?.Report(phase);
                foreach (var e in turn.Stream.Drain()) turn.Events?.Report(e);
                if (p.TryGetProperty("update", out var u) && Str(u, "sessionUpdate") == "config_option_update" && Options(u) is { Count: > 0 } now)
                {
                    _sessionOptions[Str(p, "sessionId")!] = now;
                    OptionsSeen?.Invoke(Tool, now);
                }
                break;
            case "_kiro/mcp/status" when turn.Options.RequireMcp:
                if (p.TryGetProperty("servers", out var servers) && servers.ValueKind == JsonValueKind.Array)
                    foreach (var s in servers.EnumerateArray())
                        if (Str(s, "status") is "failed" or "error" && turn.McpFailed is null)
                        {
                            turn.McpFailed = Str(s, "name") ?? "one";
                            await Notify("session/cancel", new { sessionId = Str(p, "sessionId") });
                        }
                break;
        }
    }

    private async Task AnswerPermission(JsonElement id, Turn? turn, string? sid, JsonElement p, IReadOnlyList<(string Path, KiroStep? Step)>? reviewed)
    {
        object outcome;
        try { outcome = await Permission(turn, sid, p, reviewed); }
        catch (Exception e)
        {
            Log.Line($"acp {Name}: permission - {e.Message}");
            outcome = new { outcome = "cancelled" };
        }
        try { await Send(new { jsonrpc = "2.0", id, result = new { outcome } }); }
        catch (AcpGone) { /* the run went with it */ }
    }

    /// Read only allows reading and refuses the rest. Otherwise what the approval
    /// setting leaves alone is allowed, and the rest goes to the user, unless they
    /// trusted it earlier in the session. A stopped run withdraws the question.
    private async Task<object> Permission(Turn? turn, string? sid, JsonElement p, IReadOnlyList<(string Path, KiroStep? Step)>? reviewed)
    {
        object Cancelled() => new { outcome = "cancelled" };
        if (turn is null || !p.TryGetProperty("options", out var options) || options.ValueKind != JsonValueKind.Array)
            return Cancelled();
        string? Pick(params string[] kinds)
        {
            foreach (var k in kinds)
                foreach (var o in options.EnumerateArray())
                    if ((Str(o, "kind") ?? "").StartsWith(k, StringComparison.Ordinal) && Str(o, "optionId") is { } oid) return oid;
            return null;
        }
        object Selected(string? option) => option is null ? Cancelled() : new { outcome = "selected", optionId = option };
        object Allow() => Selected(Pick("allow_once", "allow"));
        object Reject() => Selected(Pick("reject_once", "reject"));

        var call = p.TryGetProperty("toolCall", out var c) ? c : default;
        var kind = reviewed is not null ? "edit" : Str(call, "kind") ?? "other";
        if (turn.Options.ReadOnly)
        {
            if (kind is "read" or "search" or "fetch" or "think") return Allow();
            turn.Refused = true;
            return Reject();
        }
        var outside = false;
        var ask = reviewed is not null ? Review(call, reviewed, turn.Folder, out outside) : Describe(call, kind, turn.Folder, out outside);
        if (!NeedsAsking(turn.Options.Approval, kind, outside)) return Allow();
        var trusted = sid is null ? null : _trusted.GetOrAdd(sid, _ => new());
        var key = Key(ask);
        if (trusted is not null && (trusted.ContainsKey("*") || trusted.ContainsKey(key))) return Allow();
        if (Asking is not { } asking || sid is null) return Reject();

        var answer = asking(sid, ask, turn.Token);
        var stopped = Task.Delay(Timeout.Infinite, turn.Token).ContinueWith(_ => { }, TaskScheduler.Default);
        if (await Task.WhenAny(answer, stopped) != answer || turn.Token.IsCancellationRequested) return Cancelled();
        switch (await answer)
        {
            case AskAnswer.Allow: return Allow();
            case AskAnswer.Trust:
                trusted![key] = true;
                return Selected(TrustOption());
            case AskAnswer.TrustAll:
                trusted!["*"] = true;
                return Selected(TrustOption());
            default: return Reject();
        }

        // Trust lasts the session and is Hover's: Hover answers the same call itself
        // from then on. The tool's own "always" is only picked where it too is for the
        // session. Cursor's allow-always writes a lasting rule into the user's own
        // ~/.cursor/cli-config.json, and Kiro's can change a Kiro setting
        // (setting_key); a click in the notch must never do that.
        string? TrustOption() => Tool == AgentTool.Codex ? Pick("allow_always", "allow") : Pick("allow_once", "allow");
    }

    /// Whether a tool call of this kind waits for the user under this setting.
    internal static bool NeedsAsking(AgentApproval approval, string kind, bool outside) => approval switch
    {
        AgentApproval.Autopilot => false,
        AgentApproval.Risky => kind switch
        {
            "read" or "search" or "think" or "switch_mode" => false,
            "edit" => outside,
            _ => true,
        },
        _ => kind is not ("read" or "search" or "think" or "switch_mode"),
    };

    /// What "the same again" means for Trust: the kind, and the command, the file or
    /// the title. A Kiro review of several files says "3 files"; its title names them.
    private static string Key(AgentAsk a) => a.Kind + ":" + (a.Command ?? a.Path ?? a.Title) + (a.Title.StartsWith("Review changes: ", StringComparison.Ordinal) ? ":" + a.Title : "");

    internal static readonly System.Text.RegularExpressions.Regex Destructive = new(
        @"(^|[\s;&|(])(rm|rmdir|del|erase|rd|remove-item|format|mkfs|shutdown|git\s+(push|reset|clean|checkout\s+--))\b",
        System.Text.RegularExpressions.RegexOptions.IgnoreCase);
    internal static readonly System.Text.RegularExpressions.Regex Network = new(
        @"\b((npm|pnpm|yarn|bun|pip|pip3|uv|cargo|dotnet|nuget|gem|go)\s+(i|install|add|restore|get|update|upgrade)|curl|wget|invoke-webrequest|iwr|git\s+(push|pull|fetch|clone))\b",
        System.Text.RegularExpressions.RegexOptions.IgnoreCase);

    /// The tool call as the user is asked about it. Outside is set when the file it
    /// names is not in the folder the session works in.
    internal static AgentAsk Describe(JsonElement call, string kind, string folder, out bool outside)
    {
        var title = Str(call, "title") is { Length: > 0 } t ? t : "Use a tool";
        var raw = call.ValueKind == JsonValueKind.Object && call.TryGetProperty("rawInput", out var ri) ? ri : default;
        string? command = null;
        if (raw.ValueKind == JsonValueKind.Object)
        {
            foreach (var name in new[] { "command", "cmd" })
                if (raw.TryGetProperty(name, out var cv))
                {
                    command = cv.ValueKind == JsonValueKind.String ? cv.GetString()
                        : cv.ValueKind == JsonValueKind.Array ? string.Join(" ", cv.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.String).Select(x => x.GetString())) : null;
                    if (command is { Length: > 0 }) break;
                }
            // Codex sends ["bash", "-lc", "the command"]; the command is what matters.
            if (command is not null && System.Text.RegularExpressions.Regex.Match(command, @"^(ba|z|)sh\s+-l?c\s+(.+)$", System.Text.RegularExpressions.RegexOptions.Singleline) is { Success: true } sh)
                command = sh.Groups[2].Value.Trim().Trim('\'', '"');
            // On Windows it wraps it in "…\pwsh.exe" [-NoProfile] -Command "the command".
            if (command is not null && System.Text.RegularExpressions.Regex.Match(command, @"^""?[^""]*?(pwsh|powershell)(\.exe)?""?\s+(-NoProfile\s+)?-(Command|c)\s+(.+)$",
                    System.Text.RegularExpressions.RegexOptions.Singleline | System.Text.RegularExpressions.RegexOptions.IgnoreCase) is { Success: true } ps)
                command = ps.Groups[5].Value.Trim().Trim('\'', '"');
        }
        // Cursor's question carries no input; its title is the command, in backticks.
        if (command is null && kind == "execute" && title.Length > 2 && title[0] == '`' && title[^1] == '`')
            command = title[1..^1];
        string? path = null;
        if (call.ValueKind == JsonValueKind.Object && call.TryGetProperty("locations", out var locs) && locs.ValueKind == JsonValueKind.Array)
            foreach (var l in locs.EnumerateArray()) { path = Str(l, "path"); if (path is not null) break; }
        path ??= Str(raw, "path") ?? Str(raw, "file_path") ?? Str(raw, "filePath");

        // A change comes with its old and new text (ACP diff content).
        var added = 0;
        var removed = 0;
        var preview = new List<string>();
        if (call.ValueKind == JsonValueKind.Object && call.TryGetProperty("content", out var content) && content.ValueKind == JsonValueKind.Array)
            foreach (var item in content.EnumerateArray())
            {
                if (Str(item, "type") != "diff") continue;
                path ??= Str(item, "path");
                var before = (Str(item, "oldText") ?? "").Replace("\r", "").Split('\n', StringSplitOptions.None).ToList();
                var after = (Str(item, "newText") ?? "").Replace("\r", "").Split('\n', StringSplitOptions.None).ToList();
                if (Str(item, "oldText") is null) before.Clear();
                var gone = before.ToList();
                foreach (var line in after) gone.Remove(line);
                var came = after.ToList();
                foreach (var line in before) came.Remove(line);
                removed += gone.Count;
                added += came.Count;
                preview.AddRange(gone.Where(x => x.Trim().Length > 0).Take(3).Select(x => "- " + Clip(x.Trim(), 110)));
                preview.AddRange(came.Where(x => x.Trim().Length > 0).Take(6 - Math.Min(3, preview.Count)).Select(x => "+ " + Clip(x.Trim(), 110)));
            }

        outside = false;
        if (path is { Length: > 0 }) path = InFolder(path, folder, ref outside);

        var danger = kind == "delete" || (command is not null && Destructive.IsMatch(command));
        var reason = kind switch
        {
            "execute" => danger ? "Can delete or overwrite things" : command is not null && Network.IsMatch(command) ? "Installs packages or uses the network" : "Runs a command",
            "delete" => "Deletes files",
            "move" => "Moves or renames files",
            "fetch" => "Uses the network",
            "edit" => outside ? "Edits a file outside the folder" : added + removed > 0 ? $"Changes {added + removed} line{(added + removed == 1 ? "" : "s")}" : "Edits a file",
            _ => "Uses a tool",
        };
        if (outside && kind != "edit") reason += " · outside the folder";
        var id = Str(call, "toolCallId") is { Length: > 0 } tid ? tid : Guid.NewGuid().ToString("N");
        return new AgentAsk(id, kind, title, command is { Length: > 0 } ? Clip(command, 400) : null, path, preview.Count > 0 ? string.Join("\n", preview) : null,
            added, removed, reason, danger);
    }

    private static string Clip(string s, int max) => s.Length <= max ? s : s[..(max - 1)] + "…";

    /// A path relative to the folder when it is inside it; outside is set when it isn't.
    private static string InFolder(string path, string folder, ref bool outside)
    {
        try
        {
            var full = Path.IsPathFullyQualified(path) ? Path.GetFullPath(path) : Path.GetFullPath(Path.Combine(folder, path));
            var root = Path.GetFullPath(folder).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
            if (full.StartsWith(root, StringComparison.OrdinalIgnoreCase)) return full[root.Length..].Replace('\\', '/');
            outside = true;
        }
        catch (Exception e) when (e is ArgumentException or NotSupportedException or PathTooLongException) { }
        return path;
    }

    /// Kiro's "Review changes" (checked against kiro-cli 2, Sep 2026): with autopilot
    /// off it asks once for the edits a turn made, as a toolCall with only that title,
    /// and names the files in _meta.kiro.files, each with the edit's toolCallId. That
    /// is an edit, and the steps already seen hold its change. Null for anything else.
    /// Read here, on the read loop that writes the steps.
    private IReadOnlyList<(string Path, KiroStep? Step)>? Reviewed(Turn? turn, JsonElement p)
    {
        if (Tool != AgentTool.Kiro || !p.TryGetProperty("_meta", out var meta) || meta.ValueKind != JsonValueKind.Object ||
            !meta.TryGetProperty("kiro", out var kiro) || kiro.ValueKind != JsonValueKind.Object || Str(kiro, "type") != "turn_approval" ||
            !kiro.TryGetProperty("files", out var files) || files.ValueKind != JsonValueKind.Array)
            return null;
        var list = new List<(string, KiroStep?)>();
        foreach (var f in files.EnumerateArray())
            if (Str(f, "path") is { Length: > 0 } path)
                list.Add((path, Str(f, "toolCallId") is { } tid ? turn?.Stream.StepOf(tid) : null));
        return list.Count > 0 ? list : null;
    }

    /// The edits of a Kiro "Review changes", told as one edit: the file (or how many),
    /// the lines it changes and a few of them.
    internal static AgentAsk Review(JsonElement call, IReadOnlyList<(string Path, KiroStep? Step)> files, string folder, out bool outside)
    {
        outside = false;
        var paths = files.Select(f => f.Path).Distinct(StringComparer.OrdinalIgnoreCase).ToList();
        var shown = new List<string>();
        foreach (var p in paths) shown.Add(InFolder(p, folder, ref outside));
        var added = files.Sum(f => f.Step?.Added ?? 0);
        var removed = files.Sum(f => f.Step?.Removed ?? 0);
        var preview = string.Join("\n", files.Select(f => f.Step?.Diff).OfType<string>()).Split('\n').Where(l => l.Length > 0).Take(8).ToList();
        var path = shown.Count == 1 ? shown[0] : $"{shown.Count} files";
        var reason = outside ? "Edits a file outside the folder" : added + removed > 0 ? $"Changes {added + removed} line{(added + removed == 1 ? "" : "s")}" : "Edits a file";
        if (shown.Count > 1) reason += $" in {shown.Count} files";
        var id = Str(call, "toolCallId") is { Length: > 0 } tid ? tid : Guid.NewGuid().ToString("N");
        // The title names every file, so trusting one review of three files doesn't
        // trust any other three (Key).
        return new AgentAsk(id, "edit", "Review changes: " + string.Join(", ", shown), null, path, preview.Count > 0 ? string.Join("\n", preview) : null,
            added, removed, reason, false);
    }

    /// The configOptions of a session/new, session/load or set_config_option answer.
    internal static List<AcpOption>? Options(JsonElement r)
    {
        if (r.ValueKind != JsonValueKind.Object || !r.TryGetProperty("configOptions", out var list) || list.ValueKind != JsonValueKind.Array) return null;
        var all = new List<AcpOption>();
        foreach (var o in list.EnumerateArray())
        {
            if (Str(o, "id") is not { } id) continue;
            var choices = new List<AcpChoice>();
            if (o.TryGetProperty("options", out var opts) && opts.ValueKind == JsonValueKind.Array)
                foreach (var c in opts.EnumerateArray())
                {
                    // Flat, or in named groups of their own.
                    if (Str(c, "value") is { } v) choices.Add(new(v, Str(c, "name") ?? v));
                    else if (c.TryGetProperty("options", out var inner) && inner.ValueKind == JsonValueKind.Array)
                        foreach (var g in inner.EnumerateArray())
                            if (Str(g, "value") is { } gv) choices.Add(new(gv, Str(g, "name") ?? gv));
                }
            all.Add(new AcpOption(id, Str(o, "category"), Str(o, "currentValue"), choices));
        }
        return all;
    }

    private static string? Str(JsonElement e, string name) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
}


/// One Windows job for the agent tools Hover starts, set to kill everything in it
/// when its last handle closes: when Hover exits, however it exits. Processes the
/// tools start join it too. Elsewhere it does nothing.
internal static class ChildJob
{
    private static readonly IntPtr Job = Make();

    public static void Add(Process p)
    {
        if (Job == IntPtr.Zero) return;
        try { if (!AssignProcessToJobObject(Job, p.Handle)) Log.Line($"child job: couldn't add pid {p.Id} ({System.Runtime.InteropServices.Marshal.GetLastWin32Error()})"); }
        catch (InvalidOperationException) { /* it has already exited */ }
    }

    private static IntPtr Make()
    {
        if (!OperatingSystem.IsWindows()) return IntPtr.Zero;
        var job = CreateJobObject(IntPtr.Zero, null);
        if (job == IntPtr.Zero) return IntPtr.Zero;
        var info = new ExtendedLimits { Basic = new BasicLimits { Flags = 0x2000 /* KILL_ON_JOB_CLOSE */ } };
        var size = System.Runtime.InteropServices.Marshal.SizeOf<ExtendedLimits>();
        var ptr = System.Runtime.InteropServices.Marshal.AllocHGlobal(size);
        try
        {
            System.Runtime.InteropServices.Marshal.StructureToPtr(info, ptr, false);
            if (!SetInformationJobObject(job, 9 /* JobObjectExtendedLimitInformation */, ptr, (uint)size)) return IntPtr.Zero;
        }
        finally { System.Runtime.InteropServices.Marshal.FreeHGlobal(ptr); }
        return job;
    }

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    private struct BasicLimits
    {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
        public uint Flags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    private struct ExtendedLimits
    {
        public BasicLimits Basic;
        public ulong ReadOps, WriteOps, OtherOps, ReadBytes, WriteBytes, OtherBytes;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }

    [System.Runtime.InteropServices.DllImport("kernel32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObject(IntPtr attributes, string? name);

    [System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint length);

    [System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
}

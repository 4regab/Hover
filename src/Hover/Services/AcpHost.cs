using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using Hover.Core;

namespace Hover.Services;

/// One choice in a session config option (ACP session/set_config_option).
public sealed record AcpChoice(string Value, string Name);

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
public sealed class AcpHost
{
    private readonly Func<AgentOptions> _options;
    private readonly Func<AcpLink?> _connect;
    private readonly SemaphoreSlim _gate = new(1, 1), _write = new(1, 1);
    private readonly ConcurrentDictionary<long, TaskCompletionSource<JsonElement>> _pending = new();
    private readonly ConcurrentDictionary<string, Turn> _turns = new();
    private readonly ConcurrentDictionary<string, List<AcpOption>> _sessionOptions = new();
    private readonly Timer _idle;
    private AcpLink? _link;
    private bool _canLoad;
    private long _ids;
    private int _busy;

    public AcpHost(AgentTool tool, Func<AgentOptions> options, Func<AcpLink?>? connect = null)
    {
        Tool = tool;
        _options = options;
        _connect = connect ?? Launch;
        _idle = new Timer(_ => { if (Volatile.Read(ref _busy) == 0) Shutdown("idle"); });
    }

    public AgentTool Tool { get; }
    private string Name => Agents.Name(Tool);

    /// The tool's process is up.
    public bool Alive => _link is not null;

    /// The settings the agent offered for a session, whenever they are read or change.
    /// Raised off the UI thread.
    public event Action<AgentTool, IReadOnlyList<AcpOption>>? OptionsSeen;

    private sealed class Turn(KiroStream stream, IProgress<KiroPhase>? progress, IProgress<KiroEvent>? events, AgentOptions options)
    {
        public KiroStream Stream { get; } = stream;
        public IProgress<KiroPhase>? Progress { get; } = progress;
        public IProgress<KiroEvent>? Events { get; } = events;
        public AgentOptions Options { get; } = options;
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
        string? resume = null, IProgress<KiroEvent>? events = null)
    {
        if (!KiroRunner.UsableFolder(folder)) return new(KiroState.Failed, "That folder isn’t there any more. Choose another one.");
        if (string.IsNullOrWhiteSpace(prompt)) return new(KiroState.Failed, $"Tell {Name} what to do first.");
        var o = _options();
        Interlocked.Increment(ref _busy);
        _idle.Change(Timeout.Infinite, Timeout.Infinite);
        var turn = new Turn(new KiroStream { Name = Name }, progress, events, o);
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
                    var r = await Call("session/load", new { sessionId = resume, cwd = folder, mcpServers = Array.Empty<object>() }, ct, TimeSpan.FromMinutes(2));
                    sid = resume;
                    offered = Options(r);
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
                var r = await Call("session/new", new { cwd = folder, mcpServers = Array.Empty<object>() }, ct, TimeSpan.FromMinutes(2));
                sid = Str(r, "sessionId") ?? throw new AcpError(0, $"{Name} didn’t start a session.");
                offered = Options(r);
            }
            _turns[sid] = turn;
            events?.Report(new KiroEvent(SessionId: sid));
            _sessionOptions[sid] = await Configure(sid, offered ?? new(), o, ct);

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
            if (option is null || value is null || option.Current == value || !option.Has(value)) return;
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
        switch (Tool)
        {
            case AgentTool.Kiro:
                // Writes then wait for an approval, which Refuse() turns down.
                await Set(Find(null, "autopilot"), o.ReadOnly ? "off" : "on");
                await Set(Find("mode", "mode"), o.Agent ?? "vibe");
                break;
            case AgentTool.Codex:
                await Set(Find("mode", "mode"), o.ReadOnly ? "read-only" : "agent-full-access");
                break;
            default:
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
        var p = new Process { StartInfo = psi, EnableRaisingEvents = true };
        p.Start();
        // In a job that closes with Hover, so the tool and everything it started go
        // too, even when Hover is killed rather than quit (they were left running).
        ChildJob.Add(p);
        Log.Line($"acp {Name}: started (pid {p.Id})");
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
        Fail(new AcpGone($"{Name} stopped."));
        link.Kill();
    }

    private void Gone(AcpLink link)
    {
        if (!ReferenceEquals(Interlocked.CompareExchange(ref _link, null, link), link)) return;
        _idle.Change(Timeout.Infinite, Timeout.Infinite);
        _sessionOptions.Clear();
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
            if (method == "session/request_permission")
                await Send(new { jsonrpc = "2.0", id = idValue, result = new { outcome = Permission(turn, p) } });
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

    /// Full access allows what is asked; read only allows reading and refuses the rest.
    private static object Permission(Turn? turn, JsonElement p)
    {
        if (turn is null || !p.TryGetProperty("options", out var options) || options.ValueKind != JsonValueKind.Array)
            return new { outcome = "cancelled" };
        var kind = p.TryGetProperty("toolCall", out var call) ? Str(call, "kind") : null;
        var allow = !turn.Options.ReadOnly || kind is "read" or "search" or "fetch" or "think";
        string? pick = null;
        foreach (var o in options.EnumerateArray())
            if ((Str(o, "kind") ?? "").StartsWith(allow ? "allow" : "reject", StringComparison.Ordinal)) { pick = Str(o, "optionId"); break; }
        if (!allow) turn.Refused = true;
        return pick is null ? new { outcome = "cancelled" } : new { outcome = "selected", optionId = pick };
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

using System.Buffers.Binary;
using System.Globalization;
using System.IO.Pipes;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Hover.Settings;

/// <summary>
/// Hover on the user's Discord status, "Playing Hover", the agents at work and for how long Hover has been open
/// (<c>internal/agents/discord.go</c>, same words and same rules). It talks to the Discord app on this computer over the
/// local connection Discord opens for this: a named pipe on Windows. No internet, no sign-in and no token. Off until the user
/// switches it on in Settings → Integrations. The status clears by itself when Hover quits (the pipe closes) or the switch goes off.
/// </summary>
public sealed class DiscordPresence : IDisposable
{
    /// <summary>The "Hover" app in Discord's Developer Portal. Its name there is what shows after "Playing".</summary>
    public const string AppId = "1556886510786314252";

    /// <summary>The picture beside the status (Discord fetches it through its own proxy).</summary>
    public const string Icon = "https://raw.githubusercontent.com/4regab/Hover/main/assets/hover.png";

    static readonly JsonSerializerOptions Compact = new() { Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

    /// <summary>The tools with a task at work, each once, in the order they started; null while the status is off. An empty list is Idle.</summary>
    readonly Func<IReadOnlyList<string>?> want;
    readonly Func<Stream?> connect;
    readonly Action<string> log;
    readonly SemaphoreSlim wake = new(0, 1);
    readonly CancellationTokenSource stop = new();
    readonly Thread thread;
    readonly TimeSpan gap, tick, answerWithin;

    /// <summary>Discord allows 5 status changes per 20 s. One per 5 s stays under it.</summary>
    public static readonly TimeSpan DefaultGap = TimeSpan.FromSeconds(5);

    /// <summary>Without news, the status is sent again this often: it finds Discord after it was started late and puts the status back after Discord was restarted.</summary>
    public static readonly TimeSpan DefaultTick = TimeSpan.FromSeconds(30);

    /// <param name="want">What to show now (see the field).</param>
    /// <param name="connect">Opens Discord's connection, or null when Discord is not running.</param>
    public DiscordPresence(Func<IReadOnlyList<string>?> want, Func<Stream?>? connect = null, Action<string>? log = null,
        TimeSpan? gap = null, TimeSpan? tick = null, TimeSpan? answerWithin = null, long? startedAt = null)
    {
        this.want = want;
        this.connect = connect ?? ConnectPipe;
        this.log = log ?? (_ => { });
        this.gap = gap ?? DefaultGap;
        this.tick = tick ?? DefaultTick;
        this.answerWithin = answerWithin ?? TimeSpan.FromSeconds(5);
        started = startedAt ?? DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        thread = new Thread(Run) { IsBackground = true, Name = "Discord status" };
        thread.Start();
    }

    readonly long started;

    /// <summary>Something changed (a switch, a task): look again now.</summary>
    public void Wake()
    {
        try { wake.Release(); }
        catch (SemaphoreFullException) { }
        catch (ObjectDisposedException) { }
    }

    /// <summary>The status as Discord is told it: how many agents are at work and which, or Idle.</summary>
    public static JsonObject Activity(IReadOnlyList<string> tools, long started)
    {
        var details = tools.Count switch { 0 => "Idle", 1 => "1 agent working", var n => $"{n} agents working" };
        var activity = new JsonObject { ["details"] = details };
        if (tools.Count > 0) activity["state"] = FirstRunes(string.Join(", ", tools), 128);
        activity["timestamps"] = new JsonObject { ["start"] = started };
        activity["assets"] = new JsonObject { ["large_image"] = Icon, ["large_text"] = "Hover" };
        return activity;
    }

    static string FirstRunes(string s, int count)
    {
        var sb = new StringBuilder();
        var n = 0;
        foreach (var r in s.EnumerateRunes())
        {
            if (n++ == count) break;
            sb.Append(r.ToString());
        }
        return sb.ToString();
    }

    // MARK: The loop

    void Run()
    {
        Link? link = null;
        // What Discord shows now (null: nothing), and when it was last told.
        string? shown = null;
        DateTime? told = null;
        bool complained = false, force = false;
        try
        {
            while (!stop.IsCancellationRequested)
            {
                IReadOnlyList<string>? tools;
                try { tools = want(); }
                catch (Exception e) { log($"discord status: {e.Message}"); tools = null; }
                var waitFor = tick;
                if (tools is null)
                {
                    if (link is not null)
                    {
                        link.TrySet(null);
                        link.Dispose();
                        link = null;
                    }
                    shown = null;
                    complained = false;
                }
                else
                {
                    var activity = Activity(tools, started);
                    var text = activity.ToJsonString(Compact);
                    if (force || shown is null || shown != text)
                    {
                        var since = told is { } t ? DateTime.UtcNow - t : TimeSpan.MaxValue;
                        if (told is not null && gap - since > TimeSpan.Zero) waitFor = gap - since;
                        else
                        {
                            told = DateTime.UtcNow;
                            var l = link;
                            link = null;
                            string? error = null;
                            if (l is null)
                            {
                                try
                                {
                                    var pipe = connect();
                                    if (pipe is null) error = "Discord isn't running";
                                    else
                                    {
                                        try { l = Link.Open(pipe, answerWithin, stop.Token); }
                                        catch (Exception e)
                                        {
                                            pipe.Dispose();
                                            error = e.Message;
                                        }
                                    }
                                }
                                catch (Exception e) { error = e.Message; }
                            }
                            if (error is null && l is not null)
                            {
                                try { l.Set(activity); }
                                catch (Exception e)
                                {
                                    error = e.Message;
                                    l.Dispose();
                                }
                            }
                            if (error is null)
                            {
                                link = l;
                                shown = text;
                                complained = false;
                            }
                            else if (!complained)
                            {
                                // Not running is the usual reason, and is tried again on the next tick.
                                log($"discord status not sent: {error}");
                                complained = true;
                            }
                        }
                    }
                }
                try { force = !wake.Wait(waitFor, stop.Token); }
                catch (OperationCanceledException) { break; }
            }
        }
        finally { link?.Dispose(); }
    }

    // MARK: Discord's connection

    /// <summary>One message: the opcode and the length (both u32, little endian), then JSON.</summary>
    public static byte[] Frame(uint op, string body)
    {
        var json = Encoding.UTF8.GetBytes(body);
        var frame = new byte[8 + json.Length];
        BinaryPrimitives.WriteUInt32LittleEndian(frame, op);
        BinaryPrimitives.WriteUInt32LittleEndian(frame.AsSpan(4), (uint)json.Length);
        json.CopyTo(frame, 8);
        return frame;
    }

    /// <summary>A connection to Discord that has shaken hands.</summary>
    sealed class Link : IDisposable
    {
        readonly Stream pipe;
        readonly TimeSpan within;
        readonly CancellationToken stop;
        long nonce;

        Link(Stream pipe, TimeSpan within, CancellationToken stop)
        {
            this.pipe = pipe;
            this.within = within;
            this.stop = stop;
        }

        public static Link Open(Stream pipe, TimeSpan within, CancellationToken stop)
        {
            var link = new Link(pipe, within, stop);
            var hello = new JsonObject { ["v"] = 1, ["client_id"] = AppId }.ToJsonString(Compact);
            link.Write(Frame(0, hello));
            var (_, reply) = link.Read();
            if (reply?["evt"]?.GetValue<string>() != "READY") throw new IOException($"Discord refused: {reply?.ToJsonString(Compact)}");
            return link;
        }

        void Write(byte[] frame)
        {
            using var cts = CancellationTokenSource.CreateLinkedTokenSource(stop);
            cts.CancelAfter(within);
            pipe.WriteAsync(frame, cts.Token).AsTask().GetAwaiter().GetResult();
            pipe.FlushAsync(cts.Token).GetAwaiter().GetResult();
        }

        void ReadExactly(byte[] buffer)
        {
            using var cts = CancellationTokenSource.CreateLinkedTokenSource(stop);
            cts.CancelAfter(within);
            var got = 0;
            while (got < buffer.Length)
            {
                var n = pipe.ReadAsync(buffer.AsMemory(got), cts.Token).AsTask().GetAwaiter().GetResult();
                if (n == 0) throw new EndOfStreamException("Discord closed the connection");
                got += n;
            }
        }

        (uint Op, JsonNode? Body) Read()
        {
            var head = new byte[8];
            ReadExactly(head);
            var op = BinaryPrimitives.ReadUInt32LittleEndian(head);
            var length = BinaryPrimitives.ReadUInt32LittleEndian(head.AsSpan(4));
            if (length > 64 * 1024) throw new IOException("Discord sent an oversized message");
            var body = new byte[length];
            ReadExactly(body);
            try { return (op, JsonNode.Parse(body)); }
            catch (JsonException) { return (op, null); }
        }

        /// <summary>Shows the status, or clears it (null). Waits for Discord's answer so an error is seen.</summary>
        public void Set(JsonObject? activity)
        {
            nonce++;
            var n = nonce.ToString(CultureInfo.InvariantCulture);
            var args = new JsonObject { ["pid"] = Environment.ProcessId };
            if (activity is not null) args["activity"] = activity.DeepClone();
            var message = new JsonObject { ["cmd"] = "SET_ACTIVITY", ["args"] = args, ["nonce"] = n };
            Write(Frame(1, message.ToJsonString(Compact)));
            for (var i = 0; i < 8; i++)
            {
                var (op, reply) = Read();
                var replyNonce = reply?["nonce"] is JsonValue v && v.TryGetValue<string>(out var s) ? s : null;
                if (op == 3)
                {
                    // Ping: answered with the same body.
                    Write(Frame(4, reply?.ToJsonString(Compact) ?? "{}"));
                }
                else if (op == 2) throw new IOException("Discord closed the connection");
                else if (replyNonce == n)
                {
                    if (reply?["evt"] is JsonValue e && e.TryGetValue<string>(out var evt) && evt == "ERROR")
                        throw new IOException($"Discord said: {reply["data"]?.ToJsonString(Compact)}");
                    return;
                }
            }
            throw new IOException("Discord did not answer");
        }

        public void TrySet(JsonObject? activity)
        {
            try { Set(activity); }
            catch (Exception) { }
        }

        public void Dispose() => pipe.Dispose();
    }

    /// <summary>Discord's pipe: the first of discord-ipc-0 to -9 that answers.</summary>
    static Stream? ConnectPipe()
    {
        for (var i = 0; i < 10; i++)
        {
            var pipe = new NamedPipeClientStream(".", $"discord-ipc-{i}", PipeDirection.InOut, PipeOptions.Asynchronous);
            try
            {
                pipe.Connect(50);
                return pipe;
            }
            catch (Exception e) when (e is TimeoutException or IOException or UnauthorizedAccessException)
            {
                pipe.Dispose();
            }
        }
        return null;
    }

    public void Dispose()
    {
        stop.Cancel();
        Wake();
        thread.Join(2000);
        wake.Dispose();
        stop.Dispose();
    }
}

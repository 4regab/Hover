using System.Buffers.Binary;
using System.IO.Pipes;
using System.Text;
using System.Text.Json.Nodes;
using Xunit;

namespace Hover.Settings.Tests;

/// <summary>
/// The Discord status client against a fake Discord on a named pipe: the handshake, the SET_ACTIVITY payloads, the pings, the errors,
/// a Discord that is not running, and the five-second rule. (Windows' pipe is \\.\pipe\discord-ipc-N; the same stream class
/// is a socket in the temp folder elsewhere, so this runs on any system.)
/// </summary>
public class DiscordTests
{
    /// <summary>A Discord that answers as the real one does: READY to the hello, then one reply for each SET_ACTIVITY.</summary>
    sealed class FakeDiscord : IDisposable
    {
        public readonly string Name = "hover-test-" + Guid.NewGuid().ToString("N");
        public readonly List<JsonNode> Hello = [];
        public readonly List<JsonNode> Activities = [];
        public readonly List<string> Closed = [];
        readonly CancellationTokenSource stop = new();
        readonly object gate = new();
        Task? serving;

        /// <summary>The first SET_ACTIVITY is answered with an ERROR, a ping goes before the answer to the next, and so on.</summary>
        public bool PingFirst, ErrorFirst, CloseAfterHello;
        public int Connections;

        public Func<Stream?> Connect => () =>
        {
            var pipe = new NamedPipeClientStream(".", Name, PipeDirection.InOut, PipeOptions.Asynchronous);
            try
            {
                pipe.Connect(300);
                return pipe;
            }
            catch (TimeoutException)
            {
                pipe.Dispose();
                return null;
            }
        };

        public void Start() => serving = Task.Run(Serve);

        async Task Serve()
        {
            while (!stop.IsCancellationRequested)
            {
                var server = new NamedPipeServerStream(Name, PipeDirection.InOut, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
                try
                {
                    await server.WaitForConnectionAsync(stop.Token);
                    lock (gate) Connections++;
                    await Talk(server);
                }
                catch (OperationCanceledException) { }
                catch (IOException) { }
                finally { server.Dispose(); }
            }
        }

        static async Task<(uint Op, JsonNode? Body)> Read(Stream s, CancellationToken ct)
        {
            var head = new byte[8];
            await s.ReadExactlyAsync(head, ct);
            var op = BinaryPrimitives.ReadUInt32LittleEndian(head);
            var body = new byte[BinaryPrimitives.ReadUInt32LittleEndian(head.AsSpan(4))];
            await s.ReadExactlyAsync(body, ct);
            return (op, JsonNode.Parse(body));
        }

        static async Task Write(Stream s, uint op, string body, CancellationToken ct)
        {
            await s.WriteAsync(DiscordPresence.Frame(op, body), ct);
            await s.FlushAsync(ct);
        }

        async Task Talk(Stream s)
        {
            var (op, hello) = await Read(s, stop.Token);
            Assert.Equal(0u, op);
            lock (gate) Hello.Add(hello!);
            await Write(s, 1, """{"cmd":"DISPATCH","evt":"READY","data":{"v":1}}""", stop.Token);
            if (CloseAfterHello) return;
            var first = true;
            while (true)
            {
                var (o, msg) = await Read(s, stop.Token);
                if (o != 1) continue;
                lock (gate) Activities.Add(msg!);
                var nonce = msg!["nonce"]!.GetValue<string>();
                if (first && PingFirst) await Write(s, 3, """{"ping":"a"}""", stop.Token);
                if (first && PingFirst)
                {
                    // The client answers a ping with the same body (opcode 4) before it reads on.
                    var (pongOp, pong) = await Read(s, stop.Token);
                    lock (gate) Closed.Add($"pong:{pongOp}:{pong?.ToJsonString()}");
                }
                if (first && ErrorFirst) await Write(s, 1, $$"""{"cmd":"SET_ACTIVITY","evt":"ERROR","data":{"code":4000,"message":"nope"},"nonce":"{{nonce}}"}""", stop.Token);
                else await Write(s, 1, $$"""{"cmd":"SET_ACTIVITY","data":{},"evt":null,"nonce":"{{nonce}}"}""", stop.Token);
                first = false;
            }
        }

        public JsonNode[] Seen() { lock (gate) return [.. Activities]; }

        public void Dispose()
        {
            stop.Cancel();
            try { serving?.Wait(2000); } catch (AggregateException) { }
            stop.Dispose();
        }
    }

    static async Task Until(Func<bool> condition, string what, int ms = 15000)
    {
        var end = DateTime.UtcNow.AddMilliseconds(ms);
        while (DateTime.UtcNow < end)
        {
            if (condition()) return;
            await Task.Delay(20);
        }
        throw new Xunit.Sdk.XunitException("Timed out waiting for " + what);
    }

    [Fact]
    public void The_status_says_how_many_agents_work_and_which_and_task_names_are_never_in_it()
    {
        var one = DiscordPresence.Activity(["Kiro"], 1000);
        Assert.Equal("1 agent working", one["details"]!.GetValue<string>());
        Assert.Equal("Kiro", one["state"]!.GetValue<string>());
        Assert.Equal(1000, one["timestamps"]!["start"]!.GetValue<long>());
        Assert.Equal("Hover", one["assets"]!["large_text"]!.GetValue<string>());
        Assert.Equal(DiscordPresence.Icon, one["assets"]!["large_image"]!.GetValue<string>());

        var two = DiscordPresence.Activity(["Kiro", "Claude Code"], 5);
        Assert.Equal("2 agents working", two["details"]!.GetValue<string>());
        Assert.Equal("Kiro, Claude Code", two["state"]!.GetValue<string>());

        var idle = DiscordPresence.Activity([], 5);
        Assert.Equal("Idle", idle["details"]!.GetValue<string>());
        Assert.Null(idle["state"]);
        // The order is details, state, timestamps, assets, as the Go app writes it.
        Assert.Equal(["details", "state", "timestamps", "assets"], two.Select(p => p.Key).ToArray());

        // A long list is cut at 128 characters (runes, not bytes).
        var many = DiscordPresence.Activity(Enumerable.Repeat("Antigravity", 30).ToList(), 1);
        Assert.Equal(128, many["state"]!.GetValue<string>().Length);
    }

    [Fact]
    public void A_frame_is_the_opcode_and_the_length_little_endian_then_the_json()
    {
        var f = DiscordPresence.Frame(1, "{\"a\":\"é\"}");
        Assert.Equal(1u, BinaryPrimitives.ReadUInt32LittleEndian(f));
        Assert.Equal(f.Length - 8, (int)BinaryPrimitives.ReadUInt32LittleEndian(f.AsSpan(4)));
        Assert.Equal("{\"a\":\"é\"}", Encoding.UTF8.GetString(f, 8, f.Length - 8));
    }

    [Fact]
    public async Task It_shakes_hands_then_sets_the_activity_and_changes_it_when_the_agents_change_and_clears_it_when_off()
    {
        using var discord = new FakeDiscord();
        discord.Start();
        IReadOnlyList<string>? tools = ["Kiro", "Codex"];
        var log = new List<string>();
        using var presence = new DiscordPresence(() => tools, discord.Connect, log.Add, gap: TimeSpan.FromMilliseconds(150), tick: TimeSpan.FromSeconds(30));

        await Until(() => discord.Seen().Length >= 1, "the first status");
        var hello = Assert.Single(discord.Hello);
        Assert.Equal(1, hello["v"]!.GetValue<int>());
        Assert.Equal(DiscordPresence.AppId, hello["client_id"]!.GetValue<string>());
        var first = discord.Seen()[0];
        Assert.Equal("SET_ACTIVITY", first["cmd"]!.GetValue<string>());
        Assert.Equal(Environment.ProcessId, first["args"]!["pid"]!.GetValue<int>());
        Assert.Equal("1", first["nonce"]!.GetValue<string>());
        var activity = first["args"]!["activity"]!;
        Assert.Equal("2 agents working", activity["details"]!.GetValue<string>());
        Assert.Equal("Kiro, Codex", activity["state"]!.GetValue<string>());
        Assert.True(activity["timestamps"]!["start"]!.GetValue<long>() > 1_700_000_000);

        // Something changed: the same connection is used, with the next nonce.
        tools = [];
        presence.Wake();
        await Until(() => discord.Seen().Length >= 2, "the idle status");
        var idle = discord.Seen()[1];
        Assert.Equal("2", idle["nonce"]!.GetValue<string>());
        Assert.Equal("Idle", idle["args"]!["activity"]!["details"]!.GetValue<string>());
        Assert.Null(idle["args"]!["activity"]!["state"]);
        Assert.Equal(1, discord.Connections);

        // Nothing changed: nothing is sent again before the tick.
        presence.Wake();
        await Task.Delay(400);
        Assert.Equal(2, discord.Seen().Length);

        // The switch goes off: the status is cleared (no activity in the arguments), and the connection is closed.
        tools = null;
        presence.Wake();
        await Until(() => discord.Seen().Length >= 3, "the clearing");
        var cleared = discord.Seen()[2];
        Assert.Null(cleared["args"]!["activity"]);
        Assert.Equal(Environment.ProcessId, cleared["args"]!["pid"]!.GetValue<int>());
        Assert.Empty(log);
    }

    [Fact]
    public async Task Status_changes_are_at_least_a_gap_apart()
    {
        using var discord = new FakeDiscord();
        discord.Start();
        IReadOnlyList<string>? tools = [];
        var stamps = new List<DateTime>();
        using var presence = new DiscordPresence(() => tools, () => { stamps.Add(DateTime.UtcNow); return discord.Connect(); }, gap: TimeSpan.FromMilliseconds(600), tick: TimeSpan.FromSeconds(30));
        await Until(() => discord.Seen().Length >= 1, "the first status");
        var t0 = DateTime.UtcNow;
        // Three changes in a row: only the last is sent, once the gap has passed.
        tools = ["Kiro"];
        presence.Wake();
        tools = ["Kiro", "Codex"];
        presence.Wake();
        tools = ["Cursor"];
        presence.Wake();
        await Until(() => discord.Seen().Length >= 2, "the next status");
        Assert.True(DateTime.UtcNow - t0 >= TimeSpan.FromMilliseconds(350), "the second status came too soon");
        await Task.Delay(300);
        Assert.Equal(2, discord.Seen().Length);
        Assert.Equal("Cursor", discord.Seen()[1]["args"]!["activity"]!["state"]!.GetValue<string>());
    }

    [Fact]
    public async Task A_ping_is_answered_and_a_refusal_is_said_once_in_the_log_and_tried_again_on_the_tick()
    {
        using var discord = new FakeDiscord { PingFirst = true };
        discord.Start();
        IReadOnlyList<string>? tools = ["Kiro"];
        var log = new List<string>();
        using (var presence = new DiscordPresence(() => tools, discord.Connect, log.Add, gap: TimeSpan.FromMilliseconds(100), tick: TimeSpan.FromMilliseconds(300)))
        {
            await Until(() => discord.Seen().Length >= 1, "the status");
            await Until(() => discord.Closed.Count >= 1, "the pong");
            Assert.Equal("pong:4:{\"ping\":\"a\"}", discord.Closed[0]);
            Assert.Empty(log);
        }

        using var strict = new FakeDiscord { ErrorFirst = true };
        strict.Start();
        var complaints = new List<string>();
        using var second = new DiscordPresence(() => tools, strict.Connect, complaints.Add, gap: TimeSpan.FromMilliseconds(100), tick: TimeSpan.FromMilliseconds(250));
        await Until(() => complaints.Count >= 1, "the complaint");
        Assert.Contains("Discord said", complaints[0]);
        // The next tick tries again on a new connection, and the answer is good: no second complaint.
        await Until(() => strict.Seen().Length >= 2, "the second try");
        await Task.Delay(300);
        Assert.Single(complaints);
    }

    [Fact]
    public async Task Discord_not_running_is_said_once_and_found_when_it_starts_later()
    {
        using var discord = new FakeDiscord();
        IReadOnlyList<string>? tools = ["Kiro"];
        var log = new List<string>();
        using var presence = new DiscordPresence(() => tools, discord.Connect, log.Add, gap: TimeSpan.FromMilliseconds(100), tick: TimeSpan.FromMilliseconds(250));
        await Until(() => log.Count >= 1, "the first complaint");
        Assert.Equal("discord status not sent: Discord isn't running", log[0]);
        await Task.Delay(800);
        Assert.Single(log);

        discord.Start();
        await Until(() => discord.Seen().Length >= 1, "the status once Discord is there");
        Assert.Equal("1 agent working", discord.Seen()[0]["args"]!["activity"]!["details"]!.GetValue<string>());
    }

    [Fact]
    public async Task A_discord_that_closes_after_hello_does_not_stop_the_app_and_is_tried_again()
    {
        using var discord = new FakeDiscord { CloseAfterHello = true };
        discord.Start();
        var log = new List<string>();
        using var presence = new DiscordPresence(() => ["Kiro"], discord.Connect, log.Add, gap: TimeSpan.FromMilliseconds(100), tick: TimeSpan.FromMilliseconds(250), answerWithin: TimeSpan.FromMilliseconds(800));
        await Until(() => log.Count >= 1, "the complaint");
        await Until(() => discord.Connections >= 2, "another try");
        Assert.Single(log);
    }

    [Fact]
    public async Task Disposing_while_it_waits_ends_at_once()
    {
        var presence = new DiscordPresence(() => null, () => null);
        var t = Task.Run(presence.Dispose);
        Assert.True(await Task.WhenAny(t, Task.Delay(5000)) == t, "Dispose did not return");
    }
}

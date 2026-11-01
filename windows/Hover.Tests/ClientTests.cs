using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Host;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>The commands and messages as lines, and how the host copes with input it should not trust.</summary>
public class ProtocolJsonTests
{
    static string Line(BackendCommand command) => Encoding.UTF8.GetString(ProtocolJson.Serialize(command));

    [Fact]
    public void Initialize_is_one_object_with_the_type_first()
    {
        Assert.Equal("""{"type":"initialize","key":"AAEC"}""", Line(new InitializeCommand("AAEC")));
        Assert.Equal("""{"type":"shutdown"}""", Line(new ShutdownCommand()));
    }

    [Fact]
    public void A_setting_left_out_stays_out_of_the_line()
    {
        // The backend reads a missing field as "no change", so null must never be sent.
        Assert.Equal("""{"type":"saveSettings","maxRunning":4,"noticeSeen":true}""",
            Line(new SaveSettingsCommand { MaxRunning = 4, NoticeSeen = true }));
        Assert.Equal("""{"type":"saveSettings","tools":[{"id":"codex","access":"always","idle":15,"hideSteps":true}]}""",
            Line(new SaveSettingsCommand { Tools = [new ToolSetting("codex", "always", 15, true)] }));
        Assert.Equal("""{"type":"new","tool":"codex","folder":"C:\\p","prompt":"hi"}""",
            Line(new NewTaskCommand("codex", @"C:\p", "hi")));
    }

    [Fact]
    public void A_line_is_never_split_by_the_text_in_it()
    {
        var line = Line(new NewTaskCommand("codex", "f", "one\ntwo\r\nthree \u2028 caf\u00e9 \u2713 \ud83d\ude00"));
        Assert.DoesNotContain('\n', line);
        Assert.DoesNotContain('\r', line);
        var back = JsonDocument.Parse(line).RootElement.GetProperty("prompt").GetString();
        Assert.Equal("one\ntwo\r\nthree \u2028 caf\u00e9 \u2713 \ud83d\ude00", back);
    }

    [Fact]
    public void A_raw_command_carries_its_own_fields_but_not_a_second_type()
    {
        var raw = new RawCommand("teleport", new JsonObject { ["type"] = "evil", ["path"] = "x" });
        Assert.Equal("""{"type":"teleport","path":"x"}""", Line(raw));
        Assert.Equal("""{"type":"gh"}""", Line(new RawCommand("gh")));
    }

    [Fact]
    public void A_state_message_is_read_with_the_fields_it_has()
    {
        const string json = """
            {"type":"state","canStart":true,"maxRunning":3,"folder":null,"tool":"codex",
             "tools":[{"id":"codex","name":"Codex","ready":true,"future":1}],
             "sessions":[{"id":7,"key":"k","stage":"waiting","ctx":null,"ask":{"id":"a1","kind":"edit"},
               "turns":[{"prompt":"p","answer":"a","t0":5,"woke":null,"steps":[{"k":"run","verb":"Ran","status":"completed","cmd":"npm"}]}]}],
             "history":[{"key":"h","tool":"codex","title":"T","folder":"f","at":9,"stage":"done","turns":2}]}
            """;
        var state = Assert.IsType<StateEvent>(ProtocolJson.Parse(Encoding.UTF8.GetBytes(json), out var problem));
        Assert.Null(problem);
        Assert.True(state.CanStart);
        Assert.Null(state.Folder);
        Assert.Equal(7, state.Sessions[0].Id);
        Assert.Null(state.Sessions[0].Ctx);
        Assert.Equal("a1", state.Sessions[0].Ask!.Id);
        Assert.Equal("run", state.Sessions[0].Turns[0].Steps[0].K);
        // What the host does not read yet is kept, not lost.
        Assert.Equal("npm", state.Sessions[0].Turns[0].Steps[0].Extra!["cmd"].GetString());
        Assert.Equal(1, state.Tools[0].Extra!["future"].GetInt32());
        Assert.Equal(2, state.History[0].Turns);
    }
}

public class LineReaderTests
{
    /// <summary>A stream that gives one byte at a time, so every split of a line is tried.</summary>
    sealed class Trickle(byte[] data) : Stream
    {
        int at;
        public override int Read(byte[] buffer, int offset, int count)
        {
            if (at >= data.Length) return 0;
            buffer[offset] = data[at++];
            return 1;
        }
        public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancel = default)
        {
            if (at >= data.Length) return ValueTask.FromResult(0);
            buffer.Span[0] = data[at++];
            return ValueTask.FromResult(1);
        }
        public override bool CanRead => true;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();
        public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
        public override void Flush() { }
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }

    static async Task<(List<string> Lines, List<long> TooLong)> Read(Stream stream, int limit)
    {
        var lines = new List<string>();
        var tooLong = new List<long>();
        await LineReader.ReadAsync(stream, limit, l => lines.Add(Encoding.UTF8.GetString(l)), tooLong.Add);
        return (lines, tooLong);
    }

    [Fact]
    public async Task Lines_end_at_LF_and_a_CR_before_it_is_dropped()
    {
        var (lines, tooLong) = await Read(new MemoryStream("one\r\ntwo\n\n\nthree\n"u8.ToArray()), 100);
        Assert.Equal(["one", "two", "three"], lines);
        Assert.Empty(tooLong);
    }

    [Fact]
    public async Task A_last_line_without_LF_is_not_a_message()
    {
        var (lines, _) = await Read(new MemoryStream("one\ntwo"u8.ToArray()), 100);
        Assert.Equal(["one"], lines);
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task A_line_at_the_limit_passes_and_one_byte_more_is_dropped(bool oneByteAtATime)
    {
        var data = Encoding.UTF8.GetBytes(new string('a', 50) + "\n" + new string('b', 51) + "\n" + "after\n");
        Stream Source() => oneByteAtATime ? new Trickle(data) : new MemoryStream(data);
        var (lines, tooLong) = await Read(Source(), 50);
        Assert.Equal([new string('a', 50), "after"], lines);
        Assert.Equal([51L], tooLong);
    }

    [Fact]
    public async Task A_line_longer_than_the_read_buffer_arrives_whole()
    {
        var big = new string('x', 300_000);
        var (lines, _) = await Read(new MemoryStream(Encoding.UTF8.GetBytes(big + "\nnext\n")), 1_000_000);
        Assert.Equal([big, "next"], lines);
    }

    [Fact]
    public async Task A_dropped_line_costs_no_memory_beyond_the_limit()
    {
        // 20 MB in one line against a 1 KB limit: read through without keeping it.
        var data = new byte[20 * 1024 * 1024 + 1];
        Array.Fill(data, (byte)'z');
        data[^1] = (byte)'\n';
        var (lines, tooLong) = await Read(new MemoryStream(data), 1024);
        Assert.Empty(lines);
        Assert.Equal([20L * 1024 * 1024], tooLong);
    }
}

/// <summary>
/// A backend that sends what it is told to (Hover.TestTools, fake-backend), to see the client
/// meet bad JSON, a line that is too long, a type it has never heard of, and a backend that
/// ends in the middle of a command.
/// </summary>
public class BackendClientRobustnessTests
{
    static BackendClient Start(int maxLineBytes = 64 * 1024 * 1024) => BackendClient.Start(new BackendOptions
    {
        ExecutablePath = FakeTools.Exe("Hover.TestTools"),
        Arguments = ["fake-backend"],
        DataDir = Directory.CreateTempSubdirectory("hover-fake-data.").FullName,
        MaxLineBytes = maxLineBytes,
    });

    static async Task<(BackendClient Client, EventQueue Events)> Open(int maxLineBytes = 64 * 1024 * 1024)
    {
        var client = Start(maxLineBytes);
        var events = client.Subscribe();
        await client.InitializeAsync(new byte[32]);
        return (client, events);
    }

    static RawCommand Emit(params string[] lines) =>
        new("emit", new JsonObject { ["lines"] = new JsonArray(lines.Select(l => (JsonNode?)JsonValue.Create(l)).ToArray()) });

    [Fact]
    public async Task Bad_json_is_dropped_and_the_next_line_is_read()
    {
        var (client, events) = await Open();
        await using var cleanup = client;
        client.Send(Emit(
            "this is not json", """{"type":""", "[1,2,3]", "\"just text\"", """{"no":"type"}""", """{"type":5}""", "",
            """{"type":"toast","text":"after the bad lines"}"""));
        Assert.Equal("after the bad lines", (await events.Until<ToastEvent>()).Text);
        Assert.Equal(6, client.DroppedLines);
    }

    [Fact]
    public async Task Bytes_that_are_not_utf8_are_dropped()
    {
        var (client, events) = await Open();
        await using var cleanup = client;
        client.Send(new RawCommand("emitBytes", new JsonObject { ["base64"] = Convert.ToBase64String([0xFF, 0xFE, 0x7B, 0x7D]) }));
        client.Send(Emit("""{"type":"toast","text":"still here"}"""));
        Assert.Equal("still here", (await events.Until<ToastEvent>()).Text);
        Assert.Equal(1, client.DroppedLines);
    }

    [Fact]
    public async Task A_line_over_the_limit_is_dropped_and_the_client_goes_on()
    {
        var (client, events) = await Open(maxLineBytes: 10_000);
        await using var cleanup = client;
        client.Send(new RawCommand("emitHuge", new JsonObject { ["bytes"] = 200_000 }));
        client.Send(Emit("""{"type":"toast","text":"after the huge line"}"""));
        Assert.Equal("after the huge line", (await events.Until<ToastEvent>()).Text);
        Assert.Equal(1, client.DroppedLines);
    }

    [Fact]
    public async Task A_line_under_the_limit_arrives_whole_even_when_it_is_large()
    {
        var (client, events) = await Open();
        await using var cleanup = client;
        client.Send(new RawCommand("emitHuge", new JsonObject { ["bytes"] = 300_000 }));
        Assert.Equal(300_000, (await events.Until<ToastEvent>()).Text.Length);
        Assert.Equal(0, client.DroppedLines);
    }

    [Fact]
    public async Task An_unknown_type_comes_back_as_an_unknown_event()
    {
        var (client, events) = await Open();
        await using var cleanup = client;
        client.Send(Emit("""{"type":"somethingNew","x":[1,2]}""", """{"type":"toast","text":"next"}"""));
        var unknown = await events.Until<UnknownEvent>();
        Assert.Equal("somethingNew", unknown.Type);
        Assert.Equal(2, unknown.Raw.GetProperty("x").GetArrayLength());
        Assert.Equal("next", (await events.Until<ToastEvent>()).Text);
        Assert.Equal(0, client.DroppedLines);
    }

    [Fact]
    public async Task A_known_type_with_fields_of_the_wrong_kind_is_dropped()
    {
        var (client, events) = await Open();
        await using var cleanup = client;
        client.Send(Emit(
            """{"type":"state","sessions":"nope"}""", """{"type":"state","tools":null}""", """{"type":"toast","text":null}""",
            """{"type":"state","maxRunning":"three"}""", """{"type":"toast","text":"after the wrong shapes"}"""));
        Assert.Equal("after the wrong shapes", (await events.Until<ToastEvent>()).Text);
        Assert.Equal(4, client.DroppedLines);
    }

    [Fact]
    public async Task CRLF_line_ends_are_accepted()
    {
        var (client, events) = await Open();
        await using var cleanup = client;
        client.Send(new RawCommand("emitBytes", new JsonObject { ["base64"] = Convert.ToBase64String("""{"type":"toast","text":"crlf"}"""u8.ToArray().Append((byte)'\r').ToArray()) }));
        Assert.Equal("crlf", (await events.Until<ToastEvent>()).Text);
    }

    [Fact]
    public async Task A_handler_that_throws_does_not_stop_the_messages()
    {
        var (client, events) = await Open();
        await using var cleanup = client;
        client.Event += _ => throw new InvalidOperationException("a handler with a bug");
        client.Send(Emit("""{"type":"toast","text":"one"}""", """{"type":"toast","text":"two"}"""));
        Assert.Equal("one", (await events.Until<ToastEvent>()).Text);
        Assert.Equal("two", (await events.Until<ToastEvent>()).Text);
    }

    [Fact]
    public async Task A_backend_that_exits_in_the_middle_of_a_command_is_reported_once_and_sending_goes_on_quietly()
    {
        var (client, events) = await Open();
        await using var cleanup = client;
        var exitCode = new TaskCompletionSource<int>();
        client.Exited += exitCode.SetResult;

        client.Send(Emit("""{"type":"toast","text":"last words"}"""));
        client.Send(new RawCommand("exit", new JsonObject { ["code"] = 3 }));

        Assert.Equal("last words", (await events.Until<ToastEvent>()).Text);
        var failure = await Assert.ThrowsAsync<BackendException>(() => events.Until<ToastEvent>());
        Assert.Contains("(3)", failure.Message);
        Assert.Equal(3, await exitCode.Task.WaitAsync(TimeSpan.FromSeconds(10)));

        // Nothing throws, nothing hangs, and a new wait ends at once with the reason.
        client.Send(new ReadyCommand());
        client.Send(new ShutdownCommand());
        var late = client.Subscribe();
        await Assert.ThrowsAsync<BackendException>(() => late.Until<StateEvent>());
        Assert.Equal(3, await client.StopAsync());
    }

    [Fact]
    public async Task The_failure_event_is_sent_when_the_backend_ends_by_itself()
    {
        var (client, _) = await Open();
        await using var cleanup = client;
        var failure = new TaskCompletionSource<BackendFailureEvent>();
        client.Event += e => { if (e is BackendFailureEvent f) failure.TrySetResult(f); };
        client.Send(new RawCommand("exit", new JsonObject { ["code"] = 7 }));
        Assert.Contains("(7)", (await failure.Task.WaitAsync(TimeSpan.FromSeconds(10))).Text);
    }
}

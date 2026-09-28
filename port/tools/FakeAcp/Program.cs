// FakeAcp: speaks enough ACP (newline JSON-RPC over stdio) for Hover to run a task.
//   FakeAcp acp ...        serve ACP (what `kiro-cli acp ...` does)
//   FakeAcp whoami         exit 0 (signed in)
// FAKEACP_SECONDS (default 30) and FAKEACP_RATE (updates per second, default 20) shape a
// run; FAKEACP_ANSWER names a Markdown file sent as the answer.
using System.Text.Json;
using System.Text.Json.Nodes;

if (args.Length == 0 || args[0] != "acp") return 0;   // whoami, status, login status: signed in

var seconds = double.TryParse(Environment.GetEnvironmentVariable("FAKEACP_SECONDS"), out var s) ? s : 30;
var rate = double.TryParse(Environment.GetEnvironmentVariable("FAKEACP_RATE"), out var r) ? r : 20;
var answerFile = Environment.GetEnvironmentVariable("FAKEACP_ANSWER");
var answer = answerFile is not null && File.Exists(answerFile) ? File.ReadAllText(answerFile) : "Done. Nothing needed changing.";
var stdout = Console.OpenStandardOutput();
var gate = new object();
var cancelled = new HashSet<string>();
var n = 0;

void Send(JsonNode msg)
{
    var bytes = JsonSerializer.SerializeToUtf8Bytes(msg);
    lock (gate) { stdout.Write(bytes); stdout.WriteByte((byte)'\n'); stdout.Flush(); }
}
void Reply(JsonNode? id, JsonNode result) => Send(new JsonObject { ["jsonrpc"] = "2.0", ["id"] = id?.DeepClone(), ["result"] = result });
void Update(string sid, JsonObject u) => Send(new JsonObject { ["jsonrpc"] = "2.0", ["method"] = "session/update", ["params"] = new JsonObject { ["sessionId"] = sid, ["update"] = u } });

async Task Prompt(JsonNode? id, string sid)
{
    var total = (int)Math.Max(1, seconds * rate);
    var delay = TimeSpan.FromSeconds(1 / rate);
    string[] kinds = ["read", "search", "edit", "execute"];
    for (var i = 0; i < total; i++)
    {
        lock (gate) if (cancelled.Remove(sid)) { Reply(id, new JsonObject { ["stopReason"] = "cancelled" }); return; }
        if (i % 10 == 0)
            Update(sid, new JsonObject { ["sessionUpdate"] = "tool_call", ["toolCallId"] = $"t{i}", ["kind"] = kinds[i / 10 % 4], ["title"] = $"Step {i / 10}", ["status"] = "in_progress", ["locations"] = new JsonArray(new JsonObject { ["path"] = $"src/file{i / 10}.cs" }) });
        else if (i % 10 == 5)
            Update(sid, new JsonObject { ["sessionUpdate"] = "tool_call_update", ["toolCallId"] = $"t{i - 5}", ["status"] = "completed" });
        else if (i % 10 == 7)
            Update(sid, new JsonObject { ["sessionUpdate"] = "usage_update", ["used"] = 1000 + i * 40, ["size"] = 200000 });
        else
            Update(sid, new JsonObject { ["sessionUpdate"] = "agent_thought_chunk", ["content"] = new JsonObject { ["type"] = "text", ["text"] = "thinking " } });
        await Task.Delay(delay);
    }
    Update(sid, new JsonObject { ["sessionUpdate"] = "agent_message_chunk", ["content"] = new JsonObject { ["type"] = "text", ["text"] = answer } });
    Reply(id, new JsonObject { ["stopReason"] = "end_turn" });
}

using var stdin = new StreamReader(Console.OpenStandardInput());
while (await stdin.ReadLineAsync() is { } line)
{
    if (!line.StartsWith('{')) continue;
    var m = JsonNode.Parse(line)!;
    var method = (string?)m["method"];
    var id = m["id"];
    var sid = (string?)m["params"]?["sessionId"];
    switch (method)
    {
        case "initialize": Reply(id, new JsonObject { ["protocolVersion"] = 1, ["agentCapabilities"] = new JsonObject { ["loadSession"] = true } }); break;
        case "session/new": Reply(id, new JsonObject { ["sessionId"] = $"fake-{Environment.ProcessId}-{++n}", ["configOptions"] = new JsonArray() }); break;
        case "session/load": Reply(id, new JsonObject()); break;
        case "session/set_config_option": Reply(id, new JsonObject()); break;
        case "session/prompt": _ = Prompt(id, sid ?? ""); break;
        case "session/cancel": lock (gate) cancelled.Add(sid ?? ""); break;
        default:
            if (id is not null) Send(new JsonObject { ["jsonrpc"] = "2.0", ["id"] = id.DeepClone(), ["error"] = new JsonObject { ["code"] = -32601, ["message"] = "fake" } });
            break;
    }
}
return 0;

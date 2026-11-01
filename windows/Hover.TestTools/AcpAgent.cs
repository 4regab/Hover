using System.Diagnostics;
using System.Text;
using System.Text.Json.Nodes;

namespace Hover.TestTools;

/// <summary>
/// The stand-in <c>codex-acp</c>: what tests/macos/backend-smoke.py writes as a Python
/// script, as a program that runs on Windows too. It speaks the agent protocol on stdin and
/// stdout, asks for one permission, and answers with a thought, a command, an edit, a
/// subagent, a page fetch and a reply. No network, no account, no real agent.
/// </summary>
static class AcpAgent
{
    const string Session = "fake-session";

    public static int Run()
    {
        var input = new StreamReader(Console.OpenStandardInput(), new UTF8Encoding(false));
        var output = new StreamWriter(Console.OpenStandardOutput(), new UTF8Encoding(false)) { AutoFlush = true, NewLine = "\n" };
        var root = Environment.GetEnvironmentVariable("HOVER_SANDBOX_ROOT") ?? ".";
        string? pending = null; // the id of the prompt being answered, as JSON

        void Print(JsonObject message) => output.WriteLine(message.ToJsonString());
        void Update(JsonObject update) => Print(new()
        {
            ["jsonrpc"] = "2.0", ["method"] = "session/update",
            ["params"] = new JsonObject { ["sessionId"] = Session, ["update"] = update },
        });
        void Result(string idJson, JsonNode result) => Print(new()
        {
            ["jsonrpc"] = "2.0", ["id"] = JsonNode.Parse(idJson), ["result"] = result,
        });

        string? line;
        while ((line = input.ReadLine()) != null)
        {
            if (JsonNode.Parse(line) is not JsonObject message) continue;
            var method = message["method"]?.GetValue<string>();
            var id = message["id"]?.ToJsonString();
            var parameters = message["params"] as JsonObject;

            // The answer to the permission question this agent asked (id 900).
            if (method is null && id == "900" && pending is not null)
            {
                if (message["result"]?["outcome"]?["optionId"]?.GetValue<string>() != "yes")
                {
                    Console.Error.WriteLine("the permission was not allowed: " + line);
                    return 1;
                }
                var cwd = Directory.GetCurrentDirectory();
                var app = Path.Combine(cwd, "app.js");
                Update(new() { ["sessionUpdate"] = "agent_thought_chunk", ["content"] = new JsonObject { ["type"] = "text", ["text"] = "Plan: run the build, then check it." } });
                foreach (var call in new JsonObject[]
                {
                    new() { ["toolCallId"] = "run-1", ["kind"] = "execute", ["title"] = "npm run dev", ["rawInput"] = new JsonObject { ["command"] = "npm run dev" }, ["status"] = "completed", ["rawOutput"] = new JsonObject { ["stdout"] = "ready on http://localhost:5173/", ["exit_code"] = 0 } },
                    new() { ["toolCallId"] = "edit-2", ["kind"] = "edit", ["title"] = "Edit app.js", ["locations"] = new JsonArray(new JsonObject { ["path"] = app }), ["status"] = "completed", ["content"] = new JsonArray(new JsonObject { ["type"] = "diff", ["path"] = app, ["oldText"] = "a\n", ["newText"] = "a\nb\n" }) },
                    new() { ["toolCallId"] = "agent-1", ["kind"] = "other", ["title"] = "Task", ["rawInput"] = new JsonObject { ["subagent_type"] = "explore", ["description"] = "Find the config", ["prompt"] = "Look for config files" }, ["status"] = "completed", ["rawOutput"] = "Found config.json" },
                    new() { ["toolCallId"] = "fetch-1", ["kind"] = "fetch", ["title"] = "Fetch docs", ["rawInput"] = new JsonObject { ["url"] = "https://example.com/docs" }, ["status"] = "completed" },
                    new() { ["toolCallId"] = "cua-1", ["kind"] = "other", ["title"] = "cua-driver: screenshot", ["rawInput"] = new JsonObject(), ["status"] = "completed" },
                })
                {
                    var update = new JsonObject { ["sessionUpdate"] = "tool_call" };
                    foreach (var (name, value) in call.ToList()) { call.Remove(name); update[name] = value; }
                    Update(update);
                }
                Update(new() { ["sessionUpdate"] = "agent_message_chunk", ["content"] = new JsonObject { ["type"] = "text", ["text"] = "Sandbox reply" } });
                Result(pending, new JsonObject { ["stopReason"] = "end_turn" });
                pending = null;
                continue;
            }

            JsonNode result = new JsonObject();
            switch (method)
            {
                case "initialize":
                    result = new JsonObject { ["protocolVersion"] = 1, ["agentCapabilities"] = new JsonObject { ["loadSession"] = true } };
                    break;
                case "session/new":
                    // What the backend handed this session to start (browser, computer use).
                    File.WriteAllText(Path.Combine(root, "mcp-servers.json"), (parameters?["mcpServers"] ?? new JsonArray()).ToJsonString());
                    result = new JsonObject { ["sessionId"] = Session };
                    break;
                case "session/cancel":
                    if (pending is not null) Result(pending, new JsonObject { ["stopReason"] = "cancelled" });
                    pending = null;
                    continue;
                case "session/prompt":
                    pending = id;
                    // A child that the backend must stop with the agent: the tests look for it afterwards.
                    var child = Process.Start(new ProcessStartInfo(Environment.ProcessPath!, ["sleep", "60"])
                    {
                        UseShellExecute = false, CreateNoWindow = true,
                        RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
                    })!;
                    File.WriteAllText(Path.Combine(root, "fixture-child.pid"), child.Id.ToString());
                    Print(new()
                    {
                        ["jsonrpc"] = "2.0", ["id"] = 900, ["method"] = "session/request_permission",
                        ["params"] = new JsonObject
                        {
                            ["sessionId"] = Session,
                            ["toolCall"] = new JsonObject { ["toolCallId"] = "edit-1", ["kind"] = "edit", ["title"] = "Sandbox edit", ["rawInput"] = new JsonObject { ["path"] = "fixture.txt" } },
                            ["options"] = new JsonArray(
                                new JsonObject { ["optionId"] = "yes", ["name"] = "Allow", ["kind"] = "allow_once" },
                                new JsonObject { ["optionId"] = "no", ["name"] = "Deny", ["kind"] = "reject_once" }),
                        },
                    });
                    continue;
            }
            if (id is not null) Result(id, result);
        }
        return 0;
    }
}

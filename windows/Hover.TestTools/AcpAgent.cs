using System.Diagnostics;
using System.Text;
using System.Text.Json.Nodes;

namespace Hover.TestTools;

/// <summary>
/// The stand-in <c>codex-acp</c>: what tests/macos/backend-smoke.py writes as a Python
/// script, as a program that runs on Windows too. It speaks the agent protocol on stdin and
/// stdout, asks for one permission, and answers with a thought, a command, an edit, a
/// subagent, a page fetch and a reply. No network, no account, no real agent.
///
/// A prompt may carry a word in square brackets that changes what the agent does, for the checks
/// of the chat: [slow] never finishes on its own (it ends when cancelled), [rich] answers with a
/// long Markdown page, [two] asks two permissions at once, [crash] exits in the middle of the turn,
/// [long] answers with a code block of ten thousand lines, [diff] edits a file with a very large
/// change. With none of them it is the agent the smoke tests know.
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
        var scenario = "";
        var asked = new HashSet<string>(); // permission ids still waiting for an answer
        // Each session/new gets a name of its own: the backend may keep one agent for the model list and for a task.
        var made = 0;
        var current = Session;
        Process? kid = null; // the child below

        // What this program was asked and what it said, for a test that has to explain a failure.
        var trace = Path.Combine(root, "agent-log.txt");
        void Log(string kind, string text)
        {
            try { File.AppendAllText(trace, $"{DateTime.UtcNow:HH:mm:ss.fff} {kind} {(text.Length > 300 ? text[..300] + "…" : text)}\n"); }
            catch (IOException) { }
        }
        void Print(JsonObject message)
        {
            var text = message.ToJsonString();
            Log("out", text);
            output.WriteLine(text);
        }
        void Update(JsonObject update) => Print(new()
        {
            ["jsonrpc"] = "2.0", ["method"] = "session/update",
            ["params"] = new JsonObject { ["sessionId"] = current, ["update"] = update },
        });
        void Result(string idJson, JsonNode result) => Print(new()
        {
            ["jsonrpc"] = "2.0", ["id"] = JsonNode.Parse(idJson), ["result"] = result,
        });

        string? line;
        while ((line = input.ReadLine()) != null)
        {
            Log("in ", line);
            if (JsonNode.Parse(line) is not JsonObject message) continue;
            var method = message["method"]?.GetValue<string>();
            var id = message["id"]?.ToJsonString();
            var parameters = message["params"] as JsonObject;

            // The answer to a permission question this agent asked (ids 900 and, for [two], 901).
            if (method is null && id is "900" or "901" && pending is not null)
            {
                if (message["result"]?["outcome"]?["optionId"]?.GetValue<string>() != "yes")
                {
                    Console.Error.WriteLine("the permission was not allowed: " + line);
                    return 1;
                }
                asked.Remove(id!);
                if (asked.Count > 0) continue;
                if (scenario == "crash")
                {
                    // On Windows the child would keep this program's pipes open and the backend would not see it end.
                    try { kid?.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
                    return 3;
                }
                var cwd = Directory.GetCurrentDirectory();
                var app = Path.Combine(cwd, "app.js");
                Update(new() { ["sessionUpdate"] = "agent_thought_chunk", ["content"] = new JsonObject { ["type"] = "text", ["text"] = "Plan: run the build, then check it." } });
                var edit = new JsonObject { ["type"] = "diff", ["path"] = app, ["oldText"] = "a\n", ["newText"] = "a\nb\n" };
                if (scenario == "diff")
                    edit = new JsonObject
                    {
                        ["type"] = "diff", ["path"] = app,
                        ["oldText"] = string.Join("\n", Enumerable.Range(1, 6000).Select(i => $"old line {i}")) + "\n",
                        ["newText"] = string.Join("\n", Enumerable.Range(1, 6000).Select(i => $"new line {i}")) + "\n",
                    };
                foreach (var call in new JsonObject[]
                {
                    new() { ["toolCallId"] = "run-1", ["kind"] = "execute", ["title"] = "npm run dev", ["rawInput"] = new JsonObject { ["command"] = "npm run dev" }, ["status"] = "completed", ["rawOutput"] = new JsonObject { ["stdout"] = "ready on http://localhost:5173/", ["exit_code"] = 0 } },
                    new() { ["toolCallId"] = "edit-2", ["kind"] = "edit", ["title"] = "Edit app.js", ["locations"] = new JsonArray(new JsonObject { ["path"] = app }), ["status"] = "completed", ["content"] = new JsonArray(edit) },
                    new() { ["toolCallId"] = "agent-1", ["kind"] = "other", ["title"] = "Task", ["rawInput"] = new JsonObject { ["subagent_type"] = "explore", ["description"] = "Find the config", ["prompt"] = "Look for config files" }, ["status"] = "completed", ["rawOutput"] = "Found config.json" },
                    new() { ["toolCallId"] = "fetch-1", ["kind"] = "fetch", ["title"] = "Fetch docs", ["rawInput"] = new JsonObject { ["url"] = "https://example.com/docs" }, ["status"] = "completed" },
                    new() { ["toolCallId"] = "cua-1", ["kind"] = "other", ["title"] = "cua-driver: screenshot", ["rawInput"] = new JsonObject(), ["status"] = "completed" },
                })
                {
                    var update = new JsonObject { ["sessionUpdate"] = "tool_call" };
                    foreach (var (name, value) in call.ToList()) { call.Remove(name); update[name] = value; }
                    Update(update);
                }
                Update(new() { ["sessionUpdate"] = "agent_message_chunk", ["content"] = new JsonObject { ["type"] = "text", ["text"] = Answer(scenario) } });
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
                    current = made++ == 0 ? Session : Session + "-" + made;
                    result = new JsonObject { ["sessionId"] = current };
                    break;
                case "session/cancel":
                    if (pending is not null) Result(pending, new JsonObject { ["stopReason"] = "cancelled" });
                    pending = null;
                    continue;
                case "session/prompt":
                    pending = id;
                    if (parameters?["sessionId"]?.GetValue<string>() is { Length: > 0 } named) current = named;
                    var said = string.Concat((parameters?["prompt"] as JsonArray ?? []).Select(b => b?["text"]?.GetValue<string>() ?? ""));
                    scenario = new[] { "slow", "rich", "two", "crash", "long", "diff" }.FirstOrDefault(w => said.Contains("[" + w + "]", StringComparison.Ordinal)) ?? "";
                    // A child that the backend must stop with the agent: the tests look for it afterwards.
                    var child = kid = Process.Start(new ProcessStartInfo(Environment.ProcessPath!, ["sleep", "60"])
                    {
                        UseShellExecute = false, CreateNoWindow = true,
                        RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
                    })!;
                    File.WriteAllText(Path.Combine(root, "fixture-child.pid"), child.Id.ToString());
                    if (scenario == "slow")
                    {
                        // A thought and a command that is still going; nothing more until it is cancelled.
                        Update(new() { ["sessionUpdate"] = "agent_thought_chunk", ["content"] = new JsonObject { ["type"] = "text", ["text"] = "Looking into it." } });
                        Update(new()
                        {
                            ["sessionUpdate"] = "tool_call", ["toolCallId"] = "slow-1", ["kind"] = "execute", ["title"] = "npm test",
                            ["rawInput"] = new JsonObject { ["command"] = "npm test" }, ["status"] = "in_progress",
                        });
                        continue;
                    }
                    foreach (var (askId, title, file) in scenario == "two"
                        ? new[] { (900, "Sandbox edit", "fixture.txt"), (901, "Second edit", "second.txt") }
                        : [(900, "Sandbox edit", "fixture.txt")])
                    {
                        asked.Add(askId.ToString());
                        Print(new()
                        {
                            ["jsonrpc"] = "2.0", ["id"] = askId, ["method"] = "session/request_permission",
                            ["params"] = new JsonObject
                            {
                                ["sessionId"] = current,
                                ["toolCall"] = new JsonObject { ["toolCallId"] = "edit-" + askId, ["kind"] = "edit", ["title"] = title, ["rawInput"] = new JsonObject { ["path"] = file } },
                                ["options"] = new JsonArray(
                                    new JsonObject { ["optionId"] = "yes", ["name"] = "Allow", ["kind"] = "allow_once" },
                                    new JsonObject { ["optionId"] = "no", ["name"] = "Deny", ["kind"] = "reject_once" }),
                            },
                        });
                    }
                    continue;
            }
            if (id is not null) Result(id, result);
        }
        return 0;
    }

    /// <summary>What the agent finally says: the smoke tests' one line, or a page for the chat's checks.</summary>
    static string Answer(string scenario) => scenario switch
    {
        "rich" => Rich,
        "long" => "Here is the whole file:\n\n```ts\n" + string.Join("\n", Enumerable.Range(1, 10_000).Select(i => $"export const value{i} = {i};")) + "\n```\n",
        _ => "Sandbox reply",
    };

    const string Rich = """
        ## Result

        The **build** passes, and `npm test` is *green*. See [the docs](https://example.com/docs).

        - first item
        - second item
          - nested item
        - [x] a task that is done
        - [ ] a task still to do

        1. one
        2. two

        > A quoted line.

        | Monitor | Scale | Blinks |
        |:--|--:|:-:|
        | Main | 100% | 0 |
        | Second | 150% | 1 |

        ```ts
        export const tidy = (f) => f.map(sortImports); // sorts them
        ```

        ```mermaid
        flowchart LR
          A[Start] --> B{Ok?}
          B -->|yes| C[Ship]
          B -->|no| D[Fix]
        ```

        ```mermaid
        flowchart LR
          A[Start
        ```

        ![a chart that is not there](missing/chart.png)

        ---

        Done.
        """;
}

using System.Text.Json;
using System.Text.Json.Nodes;

namespace Hover.Office.Tests;

/// <summary>
/// The Go office's own fixture (tests/golden/fixtures/office-state.json: five sessions, one in each
/// stage) read into the office's input, and the helpers that run the office the way the Go tests do.
/// </summary>
static class Fixture
{
    /// <summary>The page's clock, in milliseconds, advanced 16 per frame.</summary>
    public sealed class Clock
    {
        public double Now;

        public void Run(OfficeModel o, int frames)
        {
            for (var i = 0; i < frames; i++)
            {
                o.Frame(Now, 16);
                Now += 16;
            }
        }
    }

    public static string RepoRoot()
    {
        for (var d = new DirectoryInfo(AppContext.BaseDirectory); d is not null; d = d.Parent)
            if (File.Exists(Path.Combine(d.FullName, "tests", "golden", "fixtures", "office-state.json"))) return d.FullName;
        throw new FileNotFoundException("tests/golden/fixtures/office-state.json was not found above " + AppContext.BaseDirectory);
    }

    static JsonObject Root() => JsonNode.Parse(File.ReadAllText(Path.Combine(RepoRoot(), "tests", "golden", "fixtures", "office-state.json")))!.AsObject();

    public static double Now() => Root()["now"]!.GetValue<double>();

    /// <summary>The fixture's state message, as JSON.</summary>
    public static JsonObject State() => Root()["state"]!.AsObject();

    static string Str(JsonNode? n) => n is JsonValue v && v.TryGetValue<string>(out var s) ? s : "";

    public static List<SessionInput> Inputs(JsonObject state)
    {
        var list = new List<SessionInput>();
        foreach (var h in state["sessions"]!.AsArray())
        {
            var turns = new List<TurnInput>();
            foreach (var t in h!["turns"]!.AsArray())
            {
                var steps = (t!["steps"] as JsonArray ?? []).Select(x => new StepInput(Str(x!["k"]), Str(x["status"]))).ToList();
                turns.Add(new TurnInput
                {
                    Prompt = Str(t["prompt"]),
                    Stage = t["stage"] is null ? null : Str(t["stage"]),
                    Queued = t["queued"]?.GetValue<bool>() ?? false,
                    T0 = t["t0"]?.GetValue<double>(),
                    Took = t["took"]?.GetValue<double>(),
                    Steps = steps,
                });
            }
            list.Add(new SessionInput
            {
                Id = h["id"]!.GetValue<int>(), Key = Str(h["key"]), Tool = Str(h["tool"]), Bot = h["bot"]!.GetValue<int>(), Seat = h["seat"]!.GetValue<int>(),
                Title = Str(h["title"]), Folder = Str(h["folder"]), Ctx = h["ctx"]?.GetValue<double>(),
                Act = h["act"] is null ? null : Str(h["act"]), Pose = h["pose"] is null ? null : Str(h["pose"]), File = Str(h["file"]), Turns = turns,
            });
        }
        return list;
    }

    /// <summary>An office at the fixture's instant with nothing in it yet.</summary>
    public static (OfficeModel Office, Clock Clock) Fresh()
    {
        var now = Now();
        var o = new OfficeModel(1104, 424, false) { WallClock = () => (now, 0) };
        return (o, new Clock());
    }

    /// <summary>The office settled with the fixture's sessions at their desks (6.4 s, past the done bubble).</summary>
    public static (OfficeModel Office, Clock Clock, JsonObject State) Settled()
    {
        var (o, clock) = Fresh();
        var state = State();
        o.State(Inputs(state));
        clock.Run(o, 400);
        return (o, clock, state);
    }

    public static IReadOnlyList<SessionInput> WithNewSession(JsonObject state, int id)
    {
        var s = JsonNode.Parse(state.ToJsonString())!.AsObject();
        var list = s["sessions"]!.AsArray();
        var n = JsonNode.Parse(list[2]!.ToJsonString())!.AsObject();
        n["id"] = id;
        n["seat"] = 5;
        n["bot"] = 5;
        list.Add(n);
        return Inputs(s);
    }

    /// <summary>The state with the newest turn of one session given these steps.</summary>
    public static IReadOnlyList<SessionInput> WithAgents(JsonObject state, int session, (string K, string Status)[] steps)
    {
        var s = JsonNode.Parse(state.ToJsonString())!.AsObject();
        var turns = s["sessions"]![session]!["turns"]!.AsArray();
        var last = turns[^1]!.AsObject();
        var arr = new JsonArray();
        foreach (var (k, status) in steps) arr.Add(new JsonObject { ["k"] = k, ["status"] = status });
        last["steps"] = arr;
        return Inputs(s);
    }

    /// <summary>The settled office given the same inputs again with a changed state.</summary>
    public static void Feed(OfficeModel o, JsonObject state) => o.State(Inputs(state));
}

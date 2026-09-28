// HoverFixture <dataDir> <projectFolder> <turns> <answer.md>
// Prints the key of the long session it wrote.
using System.Text.Json;
using Hover.Owl;
using Hover.Services;

if (args.Length < 4) { Console.Error.WriteLine("HoverFixture <dataDir> <projectFolder> <turns> <answer.md>"); return 2; }
var (data, folder, turns, answer) = (Path.GetFullPath(args[0]), Path.GetFullPath(args[1]), int.Parse(args[2]), File.ReadAllText(args[3]));
Directory.CreateDirectory(data);
Directory.CreateDirectory(folder);
// Before any Hover type is touched: Paths reads it once.
Environment.SetEnvironmentVariable("HOVER_DATA_DIR", data);

File.WriteAllText(Path.Combine(data, "settings.json"), JsonSerializer.Serialize(new
{
    HoverOpensWorkspace = false,   // the pointer must not open the notch mid-benchmark
    NotchItems = Array.Empty<string>(),
    KiroFolder = folder,
    KiroNoticeSeen = true,
}, new JsonSerializerOptions { WriteIndented = true }));

var history = new AgentHistory(Path.Combine(Hover.Core.Paths.Support, "agents"));
var t0 = DateTime.Now.AddHours(-3);
var list = new List<SavedTurn>();
for (var i = 0; i < turns; i++)
{
    var at = t0.AddSeconds(i * 40);
    var steps = new List<KiroStep>
    {
        new($"r{i}", "read", "Read File", Path.Combine(folder, "src", $"file{i % 7}.ts"), "completed"),
        new($"e{i}", "edit", "Edit", Path.Combine(folder, "src", $"file{i % 7}.ts"), "completed"),
        new($"x{i}", "execute", "Run", "npm test", i % 9 == 0 ? "failed" : "completed"),
    };
    list.Add(new SavedTurn($"Step {i + 1}: tighten the refresh-token check and tell me what changed.", Array.Empty<string>(), steps,
        KiroState.Completed, answer, at, at.AddSeconds(2), at.AddSeconds(35)));
}
var key = Guid.NewGuid().ToString("N");
history.Save(new SavedSession(key, AgentTool.Kiro, folder, "A long rich conversation", "fake-acp-1", 42, list, DateTime.Now));
history.Flush();
Console.WriteLine(key);
return 0;

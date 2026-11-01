using System.Text;
using System.Text.Json.Nodes;

namespace Hover.TestTools;

/// <summary>
/// A backend that does what the tests tell it, to see how the host copes with a backend that
/// misbehaves: it answers <c>initialize</c>, then <c>emit</c> writes the lines it is given,
/// <c>emitBytes</c> writes raw bytes, <c>emitHuge</c> writes one very long message, and
/// <c>exit</c> ends the process at once with a code.
/// </summary>
static class FakeBackend
{
    public static int Run()
    {
        var input = new StreamReader(Console.OpenStandardInput(), new UTF8Encoding(false));
        using var output = Console.OpenStandardOutput();
        void Write(byte[] bytes)
        {
            output.Write(bytes);
            output.WriteByte((byte)'\n');
            output.Flush();
        }
        void WriteText(string text) => Write(Encoding.UTF8.GetBytes(text));

        string? line;
        while ((line = input.ReadLine()) != null)
        {
            if (JsonNode.Parse(line) is not JsonObject command) continue;
            switch (command["type"]?.GetValue<string>())
            {
                case "initialize":
                    WriteText("""{"type":"initialized","version":1}""");
                    break;
                case "emit":
                    foreach (var text in command["lines"]!.AsArray()) WriteText(text!.GetValue<string>());
                    break;
                case "emitBytes":
                    Write(Convert.FromBase64String(command["base64"]!.GetValue<string>()));
                    break;
                case "emitHuge":
                    var padding = new string('x', command["bytes"]!.GetValue<int>());
                    WriteText($$"""{"type":"toast","text":"{{padding}}"}""");
                    break;
                case "exit":
                    return command["code"]?.GetValue<int>() ?? 0;
            }
        }
        return 0;
    }
}

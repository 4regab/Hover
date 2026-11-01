using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace Hover.Host.Protocol;

/// <summary>
/// The wire format: JSON lines, UTF-8, camelCase. Everything the backend sends is untrusted
/// input, so <see cref="Parse"/> never throws: a line it cannot use is reported and dropped.
/// </summary>
public static class ProtocolJson
{
    static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        // Readable non-ASCII on the wire. The backend splits lines on LF only, and the writer
        // escapes every LF inside a string.
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        // A null where the backend never sends one makes the message invalid instead of
        // handing the app a null it did not expect.
        RespectNullableAnnotations = true,
    };

    /// <summary>One command as a JSON line's UTF-8 bytes, without the line end.</summary>
    public static byte[] Serialize(BackendCommand command)
    {
        if (command is RawCommand raw)
        {
            var fields = raw.Fields?.DeepClone() as JsonObject ?? [];
            fields.Remove("type");
            var line = new JsonObject { ["type"] = raw.CommandType };
            foreach (var (name, value) in fields.ToList())
            {
                fields.Remove(name);
                line[name] = value;
            }
            return JsonSerializer.SerializeToUtf8Bytes(line, Options);
        }
        return JsonSerializer.SerializeToUtf8Bytes(command, command.GetType(), Options);
    }

    /// <summary>
    /// The message on a line, or null with <paramref name="problem"/> saying why not: not JSON,
    /// not an object, no type, or a known type with fields of the wrong kind. An unknown type
    /// is not a problem; it comes back as an <see cref="UnknownEvent"/>.
    /// </summary>
    public static BackendEvent? Parse(ReadOnlyMemory<byte> line, out string? problem)
    {
        problem = null;
        try
        {
            using var doc = JsonDocument.Parse(line);
            var root = doc.RootElement;
            if (root.ValueKind != JsonValueKind.Object
                || !root.TryGetProperty("type", out var typeElement)
                || typeElement.ValueKind != JsonValueKind.String)
            {
                problem = "not a message with a type";
                return null;
            }
            var type = typeElement.GetString()!;
            return type switch
            {
                "initialized" => root.Deserialize<InitializedEvent>(Options),
                "toast" => root.Deserialize<ToastEvent>(Options),
                "backendFailure" => root.Deserialize<BackendFailureEvent>(Options),
                "ended" => root.Deserialize<EndedEvent>(Options),
                "settings" => root.Deserialize<SettingsEvent>(Options),
                "preferences" => root.Deserialize<PreferencesEvent>(Options),
                "state" => root.Deserialize<StateEvent>(Options),
                "transcript" => root.Deserialize<TranscriptEvent>(Options),
                "desk" => root.Deserialize<DeskEvent>(Options),
                "deskAction" => root.Deserialize<DeskActionEvent>(Options),
                "quotas" => root.Deserialize<QuotasEvent>(Options),
                "computerUse" => root.Deserialize<ComputerUseEvent>(Options),
                "markdown" => root.Deserialize<MarkdownEvent>(Options),
                "highlight" => root.Deserialize<HighlightEvent>(Options),
                "diffLines" => root.Deserialize<DiffLinesEvent>(Options),
                "flowchart" => root.Deserialize<FlowchartEvent>(Options),
                "queueRemove" => root.Deserialize<QueueRemoveEvent>(Options),
                "switchProvider" => root.Deserialize<SwitchProviderEvent>(Options),
                "fork" => root.Deserialize<ForkEvent>(Options),
                "bringBack" => root.Deserialize<BringBackEvent>(Options),
                "cloudRepos" => root.Deserialize<CloudReposEvent>(Options),
                "projects" => root.Deserialize<ProjectsEvent>(Options),
                "palette" => root.Deserialize<PaletteEvent>(Options),
                "inspectFolder" => root.Deserialize<InspectFolderEvent>(Options),
                "term" => root.Deserialize<TermEvent>(Options),
                "writeFile" => root.Deserialize<WriteFileEvent>(Options),
                "editors" => root.Deserialize<EditorsEvent>(Options),
                "openIn" => root.Deserialize<OpenInEvent>(Options),
                "gh" => root.Deserialize<GhEvent>(Options),
                "webSessions" => root.Deserialize<WebSessionsEvent>(Options),
                "cliSessions" => root.Deserialize<CliSessionsEvent>(Options),
                "adoptWeb" => root.Deserialize<AdoptWebEvent>(Options),
                "adoptCli" => root.Deserialize<AdoptCliEvent>(Options),
                "themes" => root.Deserialize<ThemesEvent>(Options),
                "themeRead" => root.Deserialize<ThemeReadEvent>(Options),
                "mcp" => root.Deserialize<McpEvent>(Options),
                "kiroAgents" => root.Deserialize<KiroAgentsEvent>(Options),
                "secret" => root.Deserialize<SecretEvent>(Options),
                "credits" => root.Deserialize<CreditsEvent>(Options),
                _ => new UnknownEvent(type, root.Clone()),
            };
        }
        catch (Exception e)
        {
            // This is the trust boundary: whatever the line was, it must not take the host down.
            problem = e is JsonException ? e.Message : e.GetType().Name + ": " + e.Message;
            return null;
        }
    }
}

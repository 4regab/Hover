using System.Text.Json;
using Hover.Host.Protocol;
using Hover.Office;

namespace Hover.Windowing;

/// <summary>
/// The backend's <c>state</c> message as the office reads it. Hover.Office has no app dependency, so it has its own
/// input types; this turns the protocol's records into them. The protocol is untrusted input: the office takes a
/// seat or a bot out of range as it comes (it wraps them), and a field that is not the expected kind reads as missing.
/// </summary>
static class OfficeStateMap
{
    /// <summary>A string field the protocol records do not type (<c>act</c>, <c>pose</c>, <c>file</c>), or null.</summary>
    static string? Word(SessionInfo s, string name) =>
        s.Extra is not null && s.Extra.TryGetValue(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    public static List<SessionInput> Read(StateEvent state) =>
    [
        .. (state.Sessions ?? []).Select(h => new SessionInput
        {
            Id = h.Id, Key = h.Key ?? "", Tool = h.Tool ?? "", Bot = h.Bot, Seat = h.Seat, Title = h.Title ?? "", Folder = h.Folder ?? "", Ctx = h.Ctx,
            Act = Word(h, "act"), Pose = Word(h, "pose"), File = Word(h, "file") ?? "",
            Turns =
            [
                .. (h.Turns ?? []).Select(t => new TurnInput
                {
                    Prompt = t.Prompt ?? "", Stage = t.Stage, Queued = t.Queued, T0 = t.T0, Took = t.Took,
                    Steps = [.. (t.Steps ?? []).Select(x => new StepInput(x.K ?? "", x.Status ?? ""))],
                }),
            ],
        }),
    ];
}

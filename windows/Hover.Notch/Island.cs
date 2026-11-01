using System.Globalization;
using System.Text.Json;
using Hover.Host.Protocol;

namespace Hover.Notch;

// What the resting notch shows (internal/app/rest.go): the island's one agent segment (a
// question waiting, the agents at work, or an end nobody has seen), a divider, then the quotas
// switched on; or the question's card. Pure data: the window draws it.

public enum IslandKind { None, Pill, Card }

public enum SegKind { None, Ask, Work, Done }

/// <summary>How a task ended.</summary>
public enum EndState { Completed, Failed, Stopped }

/// <summary>
/// One quota's segment: its ring (null: an empty track), its number ("38", or "—") and whether a %
/// follows, and whether it is the dim one of a failed read.
/// </summary>
public sealed record QuotaSeg(string Id, string Name, double? Ring, string Value, bool Pct, bool Dim);

/// <summary>An end nobody has seen yet: the latest one, and how many there are.</summary>
public sealed record Unseen(int Count, string Tool, EndState State, string Title, double Took);

/// <summary>
/// The agent segment. Ask: Session, Tool, Ask, Total (how many wait in all). Work: Tools in order,
/// Active (the one speaking), Verb, Obj, Secs, Name, More. Done: Tool, State, Title, Took, Count.
/// </summary>
public sealed record Seg
{
    public SegKind Kind { get; init; }
    public int Session { get; init; }
    public string Tool { get; init; } = "";
    public AskInfo? Ask { get; init; }
    public int Total { get; init; }
    public IReadOnlyList<string> Tools { get; init; } = [];
    public int Active { get; init; }
    public string Verb { get; init; } = "";
    public string Obj { get; init; } = "";
    public double Secs { get; init; }
    public string Name { get; init; } = "";
    public int More { get; init; }
    public EndState State { get; init; }
    public string Title { get; init; } = "";
    public double Took { get; init; }
    public int Count { get; init; }
}

public sealed record Island(IslandKind Kind, Seg Seg, bool Divider, IReadOnlyList<QuotaSeg> Quotas, string Key);

public static class Rest
{
    /// <summary>The quotas the notch can show, in the order it shows them.</summary>
    public static readonly string[] QuotaIds = ["claude", "kiro", "codex", "cursor"];

    public static string QuotaName(string id) => id switch
    {
        "claude" => "Claude",
        "kiro" => "Kiro",
        "codex" => "Codex",
        _ => "Cursor",
    };

    /// <summary>A quota's segment from its reading: "—" until one arrives, dim when it failed.</summary>
    public static QuotaSeg QuotaSegment(string id, QuotaValue? reading)
    {
        var used = reading?.Used;
        // .NET's "0" format rounds half away from zero, as the Go code does by hand.
        return new QuotaSeg(id, QuotaName(id), used, used is { } u ? u.ToString("0", CultureInfo.InvariantCulture) : "—",
            used is not null, reading is { Ok: false });
    }

    /// <summary>The verb and the object of a question: ("Wants to run", "npm install").</summary>
    public static (string Verb, string Obj) AskWords(AskInfo ask)
    {
        var verb = ask.Kind switch
        {
            "question" => "Asks you",
            "execute" => "Wants to run",
            "edit" => "Wants to edit",
            "delete" => "Wants to delete",
            "move" => "Wants to move",
            "fetch" => "Wants to go online",
            _ => "Wants to use",
        };
        // The backend sends the two joined, so the verb is taken off the front again.
        var line = ask.Line.Trim();
        return (verb, line.StartsWith(verb, StringComparison.Ordinal) ? line[verb.Length..].Trim() : line);
    }

    /// <summary>
    /// Why the question is asked, for the card. The backend leaves out the reasons every kind of question
    /// has ("Runs a command") and sends only what is particular, so the card puts the plain one back, as
    /// the Go card shows it: "Runs a command", or "Edits a file · +3 −1".
    /// </summary>
    public static string AskReason(AskInfo ask)
    {
        var plain = ask.Kind switch
        {
            "execute" => "Runs a command",
            "edit" => "Edits a file",
            "delete" => "Deletes files",
            "move" => "Moves or renames files",
            "fetch" => "Uses the network",
            "question" => "",
            _ => "Uses a tool",
        };
        var reason = ask.Reason.Trim();
        // Only the count of lines came: "+3 −1".
        if (reason.StartsWith('+')) return plain.Length == 0 ? reason : plain + " · " + reason;
        return reason.Length > 0 ? reason : plain;
    }

    /// <summary>
    /// What the island shows. A question waiting wins, else the agents at work, else an end nobody
    /// has seen; then a divider and the quotas. <paramref name="speaker"/> picks who speaks among
    /// those at work (it moves on every 3 s); <paramref name="nowMs"/> is the clock for their timers;
    /// <paramref name="card"/> says the question's card is open.
    /// </summary>
    public static Island Make(Func<string, bool> on, Func<string, QuotaValue?> reading, IReadOnlyList<SessionInfo> sessions,
        Unseen? unseen, int speaker, long nowMs, bool card, Func<string, string>? toolName = null)
    {
        var waiting = sessions.Where(s => s.Stage == "waiting" && s.Ask is not null).ToList();
        var working = sessions.Where(s => s.Stage is "working" or "waking").ToList();
        var quotas = QuotaIds.Where(on).Select(id => QuotaSegment(id, reading(id))).ToList();

        Seg seg = new();
        var name = "";
        if (waiting.Count > 0)
        {
            var s = waiting[0];
            // "More" counts the questions behind the one in front.
            seg = new Seg { Kind = SegKind.Ask, Session = s.Id, Tool = s.Tool, Ask = s.Ask, Total = waiting.Sum(w => w.Ask!.More + 1) };
            name = "ask";
        }
        else if (working.Count > 0)
        {
            var active = speaker % working.Count;
            var sp = working[active];
            var secs = 0.0;
            var turn = sp.Turns.LastOrDefault(t => !t.Queued);
            if (turn is not null) secs = turn.Took ?? Math.Max((nowMs - turn.T0) / 1000.0, 0);
            seg = new Seg
            {
                Kind = SegKind.Work, Tools = working.Select(w => w.Tool).ToList(), Active = active,
                Verb = sp.Activity.Verb, Obj = sp.Activity.Obj, Secs = secs,
                Name = toolName?.Invoke(sp.Tool) ?? sp.Tool, More = working.Count - 1,
            };
            name = "work";
        }
        else if (unseen is { Count: > 0 })
        {
            seg = new Seg { Kind = SegKind.Done, Tool = unseen.Tool, State = unseen.State, Title = unseen.Title, Took = unseen.Took, Count = unseen.Count };
            name = "done";
        }

        var items = new List<string>();
        if (name != "") items.Add(name);
        var divider = items.Count > 0 && quotas.Count > 0;
        if (divider) items.Add("|");
        items.AddRange(quotas.Select(q => q.Id));
        var kind = card && seg.Kind == SegKind.Ask ? IslandKind.Card : items.Count > 0 ? IslandKind.Pill : IslandKind.None;
        return new Island(kind, seg, divider, quotas, string.Join(',', items));
    }

    /// <summary>The timer in the island: m:ss, or h:mm:ss past an hour.</summary>
    public static string Clock(double secs)
    {
        var n = (long)Math.Floor(Math.Max(secs, 0));
        return n >= 3600
            ? string.Create(CultureInfo.InvariantCulture, $"{n / 3600}:{n / 60 % 60:00}:{n % 60:00}")
            : string.Create(CultureInfo.InvariantCulture, $"{n / 60}:{n % 60:00}");
    }

    /// <summary>How long a task took: "45 s", "3m 07s", "1h 05m".</summary>
    public static string Took(double secs)
    {
        if (secs < 60) return string.Create(CultureInfo.InvariantCulture, $"{Math.Max((long)Math.Round(secs, MidpointRounding.AwayFromZero), 1)} s");
        if (secs < 3600)
            return string.Create(CultureInfo.InvariantCulture, $"{(long)Math.Floor(secs / 60)}m {(long)Math.Round(secs, MidpointRounding.AwayFromZero) % 60:00}s");
        return string.Create(CultureInfo.InvariantCulture, $"{(long)Math.Floor(secs / 3600)}h {(long)Math.Floor(secs / 60) % 60:00}m");
    }
}

/// <summary>A tray menu item: its label, and its tick (null: not a checkbox). A null item is a separator.</summary>
public sealed record MenuItem(string Label, bool? Check = null);

public static class TrayMenu
{
    /// <summary>The tray icon's menu, top to bottom. A pick is its index in this list.</summary>
    public static IReadOnlyList<MenuItem?> Build(string shortcut, bool launchAtLogin) =>
    [
        new MenuItem("Open Agent Office  " + shortcut),
        new MenuItem("Open App Window"),
        null,
        new MenuItem("Launch at Login", launchAtLogin),
        null,
        new MenuItem("Settings…"),
        new MenuItem("Quit Hover"),
    ];
}

/// <summary>The parts of the settings model the notch uses.</summary>
public sealed record NotchSettings(bool HoverOpens, IReadOnlyList<string> Items, OfficeSize Size, string Appearance, Shortcut Shortcut)
{
    /// <summary>What an empty folder starts with.</summary>
    public static NotchSettings Default { get; } = new(true, [], OfficeSize.Default, "System", Shortcut.Default);

    /// <summary>The settings in a <c>settings</c> event's model. Anything missing or of the wrong kind keeps its default.</summary>
    public static NotchSettings From(JsonElement model)
    {
        var d = Default;
        if (model.ValueKind != JsonValueKind.Object) return d;
        var hover = d.HoverOpens;
        var items = d.Items;
        var size = d.Size;
        var appearance = d.Appearance;
        var shortcut = d.Shortcut;
        foreach (var p in model.EnumerateObject())
        {
            switch (p.Name)
            {
                case "HoverOpensWorkspace" when p.Value.ValueKind is JsonValueKind.True or JsonValueKind.False:
                    hover = p.Value.GetBoolean();
                    break;
                case "NotchItems" when p.Value.ValueKind == JsonValueKind.Array:
                    items = p.Value.EnumerateArray().Where(e => e.ValueKind == JsonValueKind.String).Select(e => e.GetString()!).ToList();
                    break;
                case "WorkspaceSize" when p.Value.ValueKind == JsonValueKind.String:
                    size = Enum.TryParse<OfficeSize>(p.Value.GetString(), true, out var s) ? s : size;
                    break;
                case "Appearance" when p.Value.ValueKind == JsonValueKind.String:
                    appearance = p.Value.GetString()!;
                    break;
                case "ScWorkspace":
                    shortcut = Shortcut.FromJson(p.Value) ?? shortcut;
                    break;
            }
        }
        return new NotchSettings(hover, items, size, appearance, shortcut);
    }
}

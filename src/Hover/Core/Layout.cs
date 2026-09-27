namespace Hover.Core;

/// One workspace card's place: shown or not, and its share of the width (a star
/// weight, so the row fills whatever panel it is in).
public sealed record CardSlot(string Id, bool Visible = true, double Width = 1);

/// The workspace cards and the rules that keep a saved layout sane. No WPF here, so
/// the rules can be checked on their own.
public static class CardLayout
{
    public const string Tasks = "tasks", Timer = "timer", Notepad = "notepad", Events = "events", Shots = "shots";

    public const double MinWidth = 0.5, MaxWidth = 4;

    /// Every card, in its default order and width.
    public static readonly IReadOnlyList<CardSlot> Default = new[]
    {
        new CardSlot(Tasks, true, 1.6), new CardSlot(Timer, true, 1.1), new CardSlot(Notepad),
        new CardSlot(Events, true, 1.05), new CardSlot(Shots, true, 1.25),
    };

    public static string Title(string id) => id switch
    {
        Tasks => "Today’s tasks",
        Timer => "Focus timer",
        Notepad => "Notepad",
        Events => "Events",
        Shots => "Screenshots",
        _ => id,
    };

    /// A saved layout, repaired: unknown and repeated cards dropped, cards added by a
    /// newer build appended in their default place, widths clamped, and never every
    /// card hidden — an empty workspace would leave nothing to click to get one back.
    public static List<CardSlot> Normalize(IEnumerable<CardSlot>? saved)
    {
        var known = Default.ToDictionary(c => c.Id);
        var seen = new HashSet<string>();
        var list = new List<CardSlot>();
        foreach (var c in saved ?? Enumerable.Empty<CardSlot>())
        {
            if (c?.Id is null || !known.ContainsKey(c.Id) || !seen.Add(c.Id)) continue;
            var w = double.IsFinite(c.Width) ? Math.Clamp(c.Width, MinWidth, MaxWidth) : known[c.Id].Width;
            list.Add(c with { Width = w });
        }
        foreach (var d in Default)
            if (seen.Add(d.Id)) list.Insert(Math.Min(Default.ToList().IndexOf(d), list.Count), d);
        var tasks = list.FindIndex(c => c.Id == Tasks);
        if (!list.Any(c => c.Visible)) list[tasks] = list[tasks] with { Visible = true };
        return list;
    }

    /// Move a card one step left (-1) or right (+1) among all cards.
    public static List<CardSlot> Move(IReadOnlyList<CardSlot> cards, string id, int by)
    {
        var list = cards.ToList();
        var i = list.FindIndex(c => c.Id == id);
        var j = i + by;
        if (i < 0 || j < 0 || j >= list.Count) return list;
        (list[i], list[j]) = (list[j], list[i]);
        return list;
    }

    public static List<CardSlot> Show(IReadOnlyList<CardSlot> cards, string id, bool visible) =>
        Normalize(cards.Select(c => c.Id == id ? c with { Visible = visible } : c));
}

/// What the resting notch can show.
public static class NotchItem
{
    public const string Timer = "timer", Kiro = "kiro", Codex = "codex", Cursor = "cursor", Claude = "claude";

    public static readonly IReadOnlyList<string> All = new[] { Timer, Claude, Kiro, Codex, Cursor };
    public static readonly IReadOnlyList<string> Quotas = new[] { Claude, Kiro, Codex, Cursor };

    public static string Title(string id) => id switch
    {
        Timer => "Focus timer",
        Claude => "Claude Code quota",
        Kiro => "Kiro CLI quota",
        Codex => "Codex quota",
        Cursor => "Cursor quota",
        _ => id,
    };
}

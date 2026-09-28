namespace Hover.Core;

/// What the resting notch can show: the AI quotas. No WPF here, so the ids can be
/// checked on their own.
public static class NotchItem
{
    public const string Kiro = "kiro", Codex = "codex", Cursor = "cursor", Claude = "claude";

    public static readonly IReadOnlyList<string> All = new[] { Claude, Kiro, Codex, Cursor };
    public static readonly IReadOnlyList<string> Quotas = All;

    public static string Title(string id) => id switch
    {
        Claude => "Claude Code quota",
        Kiro => "Kiro CLI quota",
        Codex => "Codex quota",
        Cursor => "Cursor quota",
        _ => id,
    };

    /// The name beside a quota on the notch.
    public static string Short(string id) => id switch
    {
        Claude => "Claude", Kiro => "Kiro", Codex => "Codex", _ => "Cursor",
    };
}

using System.Globalization;
using Avalonia.Media;

namespace Hover.Core;

/// One entry per face offered for note bodies.
public sealed record NoteFace(string Name, string Family, double Bump);

public static class Ink
{
    /// Faces that suit a note, filtered to what is actually installed so the menu
    /// never offers something that would silently fall back. These stay the
    /// favourites, listed first; every other installed family follows them.
    private static readonly NoteFace[] AllFaces =
    {
        new("System",        "",                  0),
        new("Segoe Script",  "Segoe Script",      1.5),
        new("Ink Free",      "Ink Free",          2.0),
        new("Comic Sans MS", "Comic Sans MS",     0.5),
        new("Gabriola",      "Gabriola",          3.0),
        new("Segoe UI",      "Segoe UI",          0),
        new("Georgia",       "Georgia",           0),
        new("Cambria",       "Cambria",           0),
        new("Consolas",      "Consolas",         -1),
    };

    /// The nine favourites that are installed. The tray menu keeps to these so it
    /// does not become a list of two hundred fonts.
    public static IReadOnlyList<NoteFace> Favourites => _favourites ??= AllFaces
        .Where(f => f.Family.Length == 0 || Installed(f.Family))
        .ToList();

    private static IReadOnlyList<NoteFace>? _favourites;
    private static IReadOnlyList<NoteFace>? _faces;

    /// The favourites first, then every other installed family after them, sorted.
    /// So the picker offers everything without burying the nine that suit a note.
    public static IReadOnlyList<NoteFace> Faces => _faces ??= BuildFaces();

    private static IReadOnlyList<NoteFace> BuildFaces()
    {
        var known = Favourites.Select(f => f.Family)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var rest = InstalledFamilies()
            .Where(name => !known.Contains(name))
            .OrderBy(name => name, StringComparer.CurrentCultureIgnoreCase)
            .Select(name => new NoteFace(name, name, 0));

        return Favourites.Concat(rest).ToList();
    }

    /// The size bump for any family — a favourite keeps its tuned value, anything
    /// else sits at zero.
    public static double BumpFor(string family) =>
        AllFaces.FirstOrDefault(f => f.Family == family)?.Bump ?? 0;

    public static NoteFace Face
    {
        get
        {
            var want = Settings.NoteFontName;
            return Faces.FirstOrDefault(f => f.Family == want) ?? Faces[0];
        }
    }

    public static FontFamily SystemFace { get; } = new("Segoe UI");

    public static FontFamily BodyFamily
    {
        get
        {
            var f = Face;
            return f.Family.Length == 0 ? SystemFace : new FontFamily(f.Family);
        }
    }

    public static double BodySize(double size) => size + Face.Bump;

    // Tab labels use the same face a shade bolder, so they hold up turned on their
    // side at this size.
    private const double BaseTabSize = 9.5;
    /// The label type grows with the deck, so a bigger deck is actually readable.
    public static double TabSize => BaseTabSize * Settings.DeckScale;
    public const double TabTracking = 0.1;

    /// The tab label face. Empty TabFontName follows the note face; otherwise the
    /// chosen family, but only if it is installed.
    public static FontFamily TabFamily
    {
        get
        {
            var want = Settings.TabFontName;
            return want.Length > 0 && Installed(want) ? new FontFamily(want) : BodyFamily;
        }
    }

    /// The bump that matches the tab face, so the strip is measured at the size it
    /// is drawn — a chosen tab face uses its own bump, not the note face's.
    private static double TabBump =>
        Settings.TabFontName.Length > 0 && Installed(Settings.TabFontName)
            ? BumpFor(Settings.TabFontName)
            : Face.Bump;

    public static double TabFontSize => TabSize + TabBump;

    /// The face inline `code` is set in. Defaults to Consolas; falls back to it when
    /// the chosen family is not installed.
    public static FontFamily CodeFamily
    {
        get
        {
            var want = Settings.CodeFontName;
            return want.Length > 0 && Installed(want)
                ? new FontFamily(want)
                : new FontFamily("Consolas");
        }
    }

    /// Rendered width of a tab label, used to size the strip that shows it.
    /// Must measure with the same face the tab draws with or the strip will not fit.
    public static double MeasureTabLabel(string title)
    {
        var text = (title ?? "").ToUpperInvariant();
        if (text.Length == 0) return 0;
        var ft = new FormattedText(text, CultureInfo.CurrentUICulture, FlowDirection.LeftToRight,
            new Typeface(TabFamily, FontStyle.Normal, FontWeight.SemiBold, FontStretch.Normal),
            TabFontSize, Brushes.Black);
        return ft.Width + TabTracking * text.Length;
    }

    private static HashSet<string>? _installed;

    private static bool Installed(string family)
    {
        _installed ??= InstalledFamilies();
        return _installed.Contains(family);
    }

    private static HashSet<string> InstalledFamilies() =>
        _installed ??= FontManager.Current.SystemFonts
            .Select(f => f.Name)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
}

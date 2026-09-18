using System.Globalization;
using Avalonia.Media.Imaging;

namespace Hover.Images;

/// One picture as the tray shows it: the file, plus the two facts that tell it apart
/// from the one above it.
public sealed class ShotItem
{
    public Shot Shot { get; }
    public string Name => Shot.Name;

    /// "18:44" for something taken today, "Thu 21:30" for anything older, because a
    /// bare time means nothing once the day has changed.
    public string When { get; }

    /// "214 KB". Empty when the file has gone.
    public string Size { get; }

    /// Decoded at twice the tray's width so it stays crisp on a high-DPI display.
    public Bitmap? Thumb => Shot.Thumbnail(ThumbPixels);

    private const int ThumbPixels = 560;

    /// A very tall or very wide picture is clamped, so one phone screenshot cannot
    /// take over the whole tray and a wide banner still shows something.
    public const double MinRatio = 0.3, MaxRatio = 1.2;

    /// How tall this picture's row should be at the given width.
    ///
    /// Rows follow the shape of their picture, so a wide terminal capture is short and
    /// a tall page is tall. Nothing is letterboxed: the picture fills the row and the
    /// clamps decide how much of a very extreme shape is shown.
    public double HeightFor(double width)
    {
        var size = Thumb?.PixelSize;
        var ratio = size is { Width: > 0, Height: > 0 }
            ? (double)size.Value.Height / size.Value.Width
            : 0.62;                                    // unreadable file: a plain box
        return width * Math.Clamp(ratio, MinRatio, MaxRatio);
    }

    /// `today` may carry a time; only its date is used.
    public ShotItem(Shot shot, DateTime today)
    {
        Shot = shot;
        var taken = shot.Taken;
        When = taken.Date == today.Date
            ? taken.ToString("HH:mm", CultureInfo.CurrentCulture)
            : taken.ToString("ddd HH:mm", CultureInfo.CurrentCulture);
        Size = ReadSize(shot.Path);
    }

    private static string ReadSize(string path)
    {
        try
        {
            var bytes = new FileInfo(path).Length;
            if (bytes < 1024) return $"{bytes} B";
            if (bytes < 1024 * 1024) return $"{bytes / 1024.0:0.#} KB";
            return $"{bytes / (1024.0 * 1024):0.#} MB";
        }
        catch
        {
            return "";
        }
    }
}

/// Pictures taken on one day, under one heading.
public sealed class ShotGroup
{
    public string Label { get; }
    public IReadOnlyList<ShotItem> Items { get; }

    public ShotGroup(string label, IReadOnlyList<ShotItem> items)
    {
        Label = label;
        Items = items;
    }
}

/// Splits the tray into days.
///
/// A flat list of nine thumbnails gives no sense of when anything was taken, which is
/// the main way people tell one screenshot from another. Days are cheap to read and
/// need no extra chrome.
public static class ShotGroups
{
    /// Groups newest day first, keeping the order within each day. `today` is passed
    /// in rather than read from the clock so the result is predictable.
    public static List<ShotGroup> Build(IReadOnlyList<Shot> shots, DateTime today)
    {
        var day = today.Date;
        var groups = new List<ShotGroup>();

        foreach (var run in shots.GroupBy(s => s.Taken.Date).OrderByDescending(g => g.Key))
        {
            var items = run.Select(s => new ShotItem(s, day)).ToList();
            groups.Add(new ShotGroup(LabelFor(run.Key, day), items));
        }
        return groups;
    }

    /// The heading for one day. Named days for the last week, then a date, because
    /// "Tuesday" nine days ago is ambiguous.
    public static string LabelFor(DateTime date, DateTime today)
    {
        var days = (today.Date - date.Date).Days;
        return days switch
        {
            0 => "Today",
            1 => "Yesterday",
            < 0 => date.ToString("d MMMM", CultureInfo.CurrentCulture),   // clock skew
            < 7 => date.ToString("dddd", CultureInfo.CurrentCulture),
            _ => date.ToString("d MMMM", CultureInfo.CurrentCulture),
        };
    }
}

using Avalonia.Media.Imaging;
using Hover.Core;

namespace Hover.Images;

/// One picture in the tray: the plain file on disk, plus a small thumbnail decoded
/// once and kept so the strip can redraw without touching the disk again.
public sealed class Shot
{
    public string Path { get; }
    public string Name => System.IO.Path.GetFileName(Path);
    public DateTime Taken { get; }

    public Shot(string path, DateTime taken)
    {
        Path = path;
        Taken = taken;
    }

    private Bitmap? _thumb;

    /// A small thumbnail, decoded at a reduced width so a wall of phone screenshots
    /// does not load full-size bitmaps into memory. Null if the file cannot be read,
    /// which happens while a screenshot tool is still writing it.
    ///
    /// Its PixelSize is also how a row learns the shape of its picture, so nothing
    /// decodes the file twice.
    public Bitmap? Thumbnail(int pixelWidth) => _thumb ??= Decode(pixelWidth);

    /// A larger copy for the hover preview. Not cached: moving the pointer down a long
    /// tray would otherwise hold one preview bitmap per row passed.
    public Bitmap? Preview(int pixelWidth) => Decode(pixelWidth);

    /// The full-size picture, for the preview window.
    public Bitmap? FullSize()
    {
        try
        {
            using var file = File.OpenRead(Path);
            return new Bitmap(file);
        }
        catch (Exception e)
        {
            Log.Line($"full-size load failed for {Name} — {e.Message}");
            return null;
        }
    }

    private Bitmap? Decode(int pixelWidth)
    {
        try
        {
            using var file = File.OpenRead(Path);
            return Bitmap.DecodeToWidth(file, Math.Max(1, pixelWidth),
                BitmapInterpolationMode.HighQuality);
        }
        catch (Exception e)
        {
            Log.Line($"decode failed for {Name} — {e.Message}");
            return null;
        }
    }
}

using System.IO;
using System.Windows.Media.Imaging;
using Hover.Core;

namespace Hover.Images;

/// One picture in the tray: the plain file on disk, plus a small thumbnail decoded
/// once and kept frozen so the strip can redraw without touching the disk again.
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

    private BitmapSource? _thumb;

    /// A small, frozen thumbnail. Decoded at a reduced pixel width so a wall of
    /// phone screenshots does not load full-size bitmaps into memory. Null if the
    /// file cannot be read (deleted or mid-write).
    public BitmapSource? Thumbnail(int pixelWidth) => _thumb ??= Decode(pixelWidth);

    /// A larger copy for the tray's hover card. Not cached, because moving the pointer
    /// down a long tray would otherwise hold one preview bitmap per row passed.
    public BitmapSource? Preview(int pixelWidth) => Decode(pixelWidth);

    private BitmapSource? Decode(int pixelWidth)
    {
        try
        {
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;   // read now, then release the file
            bmp.CreateOptions = BitmapCreateOptions.IgnoreColorProfile;
            bmp.DecodePixelWidth = Math.Max(1, pixelWidth);
            bmp.UriSource = new Uri(Path);
            bmp.EndInit();
            bmp.Freeze();
            return bmp;
        }
        catch (Exception e)
        {
            Log.Line($"decode failed for {Name} — {e.Message}");
            return null;
        }
    }

    /// The full-size picture, decoded fresh (not the thumbnail), for the preview
    /// window. Null on failure.
    public BitmapSource? FullSize()
    {
        try
        {
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.UriSource = new Uri(Path);
            bmp.EndInit();
            bmp.Freeze();
            return bmp;
        }
        catch (Exception e)
        {
            Log.Line($"full-size load failed for {Name} — {e.Message}");
            return null;
        }
    }
}

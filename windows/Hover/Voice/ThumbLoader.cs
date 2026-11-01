using Hover.Host;
using Hover.Voice;
using Windows.Graphics.Imaging;
using Windows.Storage;

namespace Hover.Speaking;

/// <summary>
/// The screenshots on the preview, as small pictures. The files are the ones the backend kept in
/// the data folder's <c>kiro-images</c>; any other path is not read. Each is decoded once, off the
/// UI thread, to about twice the size it is shown at.
/// </summary>
sealed class ThumbLoader
{
    const int Wide = 144, High = 104;

    readonly string root;
    readonly Dictionary<string, CardImage?> done = [];
    readonly HashSet<string> loading = [];
    readonly object gate = new();
    readonly Action loaded;

    public ThumbLoader(string dataDir, Action loaded)
    {
        root = Path.GetFullPath(Path.Combine(dataDir, "kiro-images")) + Path.DirectorySeparatorChar;
        this.loaded = loaded;
    }

    /// <summary>The picture of a file, or null while it loads or when it cannot be read.</summary>
    public CardImage? Get(string path)
    {
        lock (gate)
        {
            if (done.TryGetValue(path, out var image)) return image;
            if (!loading.Add(path)) return null;
        }
        _ = Task.Run(() => LoadAsync(path));
        return null;
    }

    async Task LoadAsync(string path)
    {
        CardImage? image = null;
        try
        {
            var full = Path.GetFullPath(path);
            if (!full.StartsWith(root, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("not one of the backend's pictures");
            var file = await StorageFile.GetFileFromPathAsync(full);
            using var stream = await file.OpenReadAsync();
            var decoder = await BitmapDecoder.CreateAsync(stream);
            double w = decoder.OrientedPixelWidth, h = decoder.OrientedPixelHeight;
            if (w <= 0 || h <= 0 || w * h > 100_000_000) throw new InvalidOperationException("a picture of that size is not shown");
            // To cover the box, no bigger than it needs.
            var k = Math.Min(Math.Max(Wide / w, High / h), 1);
            var transform = new BitmapTransform
            {
                ScaledWidth = (uint)Math.Max(Math.Round(w * k), 1),
                ScaledHeight = (uint)Math.Max(Math.Round(h * k), 1),
            };
            var data = await decoder.GetPixelDataAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied, transform, ExifOrientationMode.RespectExifOrientation, ColorManagementMode.DoNotColorManage);
            image = new CardImage((int)transform.ScaledWidth, (int)transform.ScaledHeight, data.DetachPixelData());
        }
        catch (Exception e)
        {
            HostLog.Line($"voice: a screenshot did not load ({e.Message})");
        }
        lock (gate)
        {
            done[path] = image;
            loading.Remove(path);
        }
        loaded();
    }
}

using Hover.Host;
using Hover.Settings;
using Windows.Foundation;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Hover.Screening;

/// <summary>A picture for the Screen panel: JPEG bytes and their size.</summary>
sealed record ScreenPicture(byte[] Jpeg, int Width, int Height)
{
    /// <summary>The picture as the chat's pictures are named: a data address.</summary>
    public string DataUrl => "data:image/jpeg;base64," + Convert.ToBase64String(Jpeg);
}

/// <summary>
/// The app's side of the Screen panel: the picture of the main display with only the windows of an agent's apps over the desktop
/// (<see cref="ScreenCapture"/>), over the wallpaper, scaled to fit 1280 x 800 and made a JPEG. One picture at a time.
/// </summary>
sealed class ScreenSource
{
    const int MaxHeight = 800;
    const long MaxFileBytes = 64L << 20;

    readonly SemaphoreSlim one = new(1, 1);
    ScreenFrame? wallpaper;
    (string Path, DateTime Stamp, int W, int H)? wallpaperOf;
    bool saidNoWallpaper;

    /// <summary>Whether this system can show the panel at all.</summary>
    public static bool Supported => ScreenCapture.Supported;

    /// <summary>The desktop with the windows of <paramref name="pids"/> over it, or the desktop alone when <paramref name="live"/> is false, scaled to fit 1280 x 800. Off the UI thread.</summary>
    public async Task<ScreenFrame> CaptureAsync(IReadOnlyCollection<uint> pids, bool live)
    {
        await one.WaitAsync();
        try
        {
            var wall = await WallpaperAsync();
            var frame = await Task.Run(() => ScreenCapture.Capture(live ? pids : [], wall));
            return ScreenCapture.Scaled(frame, ScreenCapture.Width, MaxHeight);
        }
        finally { one.Release(); }
    }

    /// <summary>The same as a JPEG.</summary>
    public async Task<ScreenPicture> TakeAsync(IReadOnlyCollection<uint> pids, bool live)
    {
        var small = await CaptureAsync(pids, live);
        return new ScreenPicture(await JpegAsync(small), small.Width, small.Height);
    }

    /// <summary>The wallpaper covering the screen, from the file Windows names (or its own copy of it); null when there is none to read.</summary>
    async Task<ScreenFrame?> WallpaperAsync()
    {
        var (sw, sh) = ScreenCapture.Size();
        var files = new List<string>();
        if (ScreenCapture.WallpaperPath() is { } path) files.Add(path);
        if (Environment.GetEnvironmentVariable("APPDATA") is { Length: > 0 } roaming)
            files.Add(Path.Combine(roaming, "Microsoft", "Windows", "Themes", "TranscodedWallpaper"));
        foreach (var file in files)
        {
            FileInfo info;
            try
            {
                info = new FileInfo(file);
                if (!info.Exists || info.Length is 0 or > MaxFileBytes) continue;
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException) { continue; }
            var key = (file, info.LastWriteTimeUtc, sw, sh);
            if (wallpaper is not null && wallpaperOf == key) return wallpaper;
            try
            {
                wallpaper = await DecodeAsync(await File.ReadAllBytesAsync(file), sw, sh);
                wallpaperOf = key;
                saidNoWallpaper = false;
                return wallpaper;
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException or System.Runtime.InteropServices.COMException or InvalidOperationException or ArgumentException)
            {
                HostLog.Line($"screen: couldn’t read the desktop picture {file} — {e.Message}");
            }
        }
        if (!saidNoWallpaper) HostLog.Line("screen: no desktop picture to read; the desktop's colour is used");
        saidNoWallpaper = true;
        wallpaper = null;
        wallpaperOf = null;
        return null;
    }

    static async Task<ScreenFrame> DecodeAsync(byte[] file, int sw, int sh)
    {
        using var stream = new InMemoryRandomAccessStream();
        using (var writer = new DataWriter(stream))
        {
            writer.WriteBytes(file);
            await writer.StoreAsync();
            writer.DetachStream();
        }
        stream.Seek(0);
        var decoder = await BitmapDecoder.CreateAsync(stream);
        int w = (int)decoder.PixelWidth, h = (int)decoder.PixelHeight;
        if (w <= 0 || h <= 0 || (long)w * h > 400_000_000) throw new InvalidOperationException("the picture is " + w + " x " + h);
        // A picture far larger than the screen is made smaller as it is read.
        var transform = new BitmapTransform();
        var k = Math.Sqrt(2.0 * sw * sh / ((double)w * h));
        if (k < 1)
        {
            transform.ScaledWidth = (uint)Math.Max(1, Math.Round(w * k));
            transform.ScaledHeight = (uint)Math.Max(1, Math.Round(h * k));
            w = (int)transform.ScaledWidth;
            h = (int)transform.ScaledHeight;
        }
        var data = await decoder.GetPixelDataAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, transform, ExifOrientationMode.IgnoreExifOrientation, ColorManagementMode.DoNotColorManage);
        var px = data.DetachPixelData();
        for (var i = 3; i < px.Length; i += 4) px[i] = 255;
        return ScreenCapture.Cover(new ScreenFrame(w, h, px), sw, sh);
    }

    static async Task<byte[]> JpegAsync(ScreenFrame f)
    {
        using var stream = new InMemoryRandomAccessStream();
        var options = new BitmapPropertySet { { "ImageQuality", new BitmapTypedValue(0.8f, PropertyType.Single) } };
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.JpegEncoderId, stream, options);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, (uint)f.Width, (uint)f.Height, 96, 96, f.Bgra);
        await encoder.FlushAsync();
        stream.Seek(0);
        var jpeg = new byte[stream.Size];
        using var reader = new DataReader(stream);
        await reader.LoadAsync((uint)stream.Size);
        reader.ReadBytes(jpeg);
        return jpeg;
    }
}

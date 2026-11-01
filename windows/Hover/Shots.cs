using Hover.Host;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Hover;

/// <summary>
/// <c>--shots DIR</c>: the app draws its own views to JPEG files, as the Go app does to PNG.
/// The pictures come from the window's element tree, so they need no screen capture, which
/// some machines block. Later phases add their views here.
/// </summary>
public static class Shots
{
    /// <summary>No picture is wider than this; a wider view is drawn smaller.</summary>
    public const int MaxWidth = 1400;

    public static async Task TakeAsync(string directory, DashboardWindow dashboard)
    {
        Directory.CreateDirectory(directory);
        await Save(dashboard.Root, Path.Combine(directory, "dashboard.jpg"));
        await Hover.Windowing.NotchShots.TakeAsync(directory);
    }

    static async Task Save(FrameworkElement view, string file)
    {
        // Wait until the view has been laid out and drawn once.
        if (!view.IsLoaded)
        {
            var loaded = new TaskCompletionSource();
            view.Loaded += (_, _) => loaded.TrySetResult();
            await loaded.Task;
        }
        await Task.Delay(400);

        var scale = Math.Min(1.0, MaxWidth / Math.Max(1.0, view.ActualWidth));
        var width = Math.Max(1, (int)Math.Round(view.ActualWidth * scale));
        var height = Math.Max(1, (int)Math.Round(view.ActualHeight * scale));

        var bitmap = new RenderTargetBitmap();
        await bitmap.RenderAsync(view, width, height);
        var pixels = await bitmap.GetPixelsAsync();
        var bytes = new byte[pixels.Length];
        using (var reader = DataReader.FromBuffer(pixels)) reader.ReadBytes(bytes);

        await WriteJpegAsync(bytes, bitmap.PixelWidth, bitmap.PixelHeight, file);
        HostLog.Line($"picture {file} ({bitmap.PixelWidth} x {bitmap.PixelHeight})");
    }

    /// <summary>Writes opaque BGRA pixels as a JPEG file.</summary>
    public static async Task WriteJpegAsync(byte[] bgra, int width, int height, string file)
    {
        using var stream = new InMemoryRandomAccessStream();
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.JpegEncoderId, stream);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, (uint)width, (uint)height, 96, 96, bgra);
        await encoder.FlushAsync();

        stream.Seek(0);
        var jpeg = new byte[stream.Size];
        using (var reader = new DataReader(stream)) { await reader.LoadAsync((uint)stream.Size); reader.ReadBytes(jpeg); }
        await File.WriteAllBytesAsync(file, jpeg);
    }
}

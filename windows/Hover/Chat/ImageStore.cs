using Hover.Chat;
using Hover.Host;
using Vortice.Direct2D1;
using Vortice.DXGI;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;
using AlphaMode = Vortice.DCommon.AlphaMode;
using PixelFormat = Vortice.DCommon.PixelFormat;

namespace Hover.Chatting;

/// <summary>
/// The pictures the chat shows. A picture is asked for the first time its size is wanted; it is read
/// in the background (a file the app serves, or the web), decoded, and the chat is told. A picture
/// that cannot be read or decoded is broken, which the chat shows as the broken-image mark.
/// </summary>
sealed class ImageStore : IDisposable
{
    const int MaxBytes = 32 << 20;
    const int MaxSide = 4096;
    const long MaxPixels = 100_000_000;

    sealed class Slot
    {
        public bool Failed;
        public byte[]? Pixels;
        public int W, H;
        public ID2D1Bitmap1? Bitmap;
    }

    readonly Dictionary<string, Slot> slots = [];
    readonly object gate = new();
    readonly string imagesFolder;
    readonly Func<string, string?> folderOfFiles;
    readonly HttpClient http = new() { Timeout = TimeSpan.FromSeconds(30) };
    readonly SemaphoreSlim workers = new(2);

    /// <summary>A picture finished loading (or failed). Raised on a worker thread with the picture's address.</summary>
    public event Action<string>? Arrived;

    /// <param name="dataDir">The data folder: pictures pasted into a prompt are kept in its kiro-images folder.</param>
    /// <param name="folderOfFiles">The folder the session with this files host is in (null for a host nobody serves).</param>
    public ImageStore(string dataDir, Func<string, string?> folderOfFiles)
    {
        imagesFolder = Path.Combine(dataDir, "kiro-images");
        this.folderOfFiles = folderOfFiles;
    }

    public ImageState State(string src)
    {
        Slot? slot;
        lock (gate)
        {
            if (!slots.TryGetValue(src, out slot))
            {
                if (slots.Count > 400) Trim();
                slots[src] = null!;
                _ = Task.Run(() => LoadAsync(src));
                return new ImageState(ImageKind.Pending);
            }
        }
        if (slot is null) return new ImageState(ImageKind.Pending);
        if (slot.Failed) return new ImageState(ImageKind.Broken);
        return new ImageState(ImageKind.Ready, slot.W, slot.H);
    }

    void Trim()
    {
        foreach (var key in slots.Keys.ToList())
        {
            if (slots[key] is { } s) s.Bitmap?.Dispose();
            slots.Remove(key);
        }
    }

    /// <summary>A picture made in memory (opaque B8G8R8A8), ready at once under an address of its own: the Screen tab's.</summary>
    public void Put(string src, int width, int height, byte[] bgra)
    {
        if (width < 1 || height < 1 || bgra.Length < width * height * 4) return;
        Slot? old;
        lock (gate)
        {
            slots.TryGetValue(src, out old);
            slots[src] = new Slot { Pixels = bgra, W = width, H = height };
        }
        old?.Bitmap?.Dispose();
    }

    /// <summary>Lets a picture go: its pixels and its bitmap.</summary>
    public void Forget(string src)
    {
        Slot? slot;
        lock (gate)
        {
            if (!slots.Remove(src, out slot)) return;
        }
        slot?.Bitmap?.Dispose();
    }

    /// <summary>The picture as a bitmap for this context, or null when it is not ready.</summary>
    public ID2D1Bitmap1? BitmapFor(ID2D1DeviceContext dc, string src)
    {
        Slot? slot;
        lock (gate)
        {
            if (!slots.TryGetValue(src, out slot) || slot is null || slot.Failed || slot.Pixels is null) return null;
        }
        if (slot.Bitmap is not null) return slot.Bitmap;
        var handle = System.Runtime.InteropServices.GCHandle.Alloc(slot.Pixels, System.Runtime.InteropServices.GCHandleType.Pinned);
        try
        {
            slot.Bitmap = dc.CreateBitmap(new Vortice.Mathematics.SizeI(slot.W, slot.H), handle.AddrOfPinnedObject(), (uint)(slot.W * 4),
                new BitmapProperties1(new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied), 96, 96, BitmapOptions.None));
        }
        finally { handle.Free(); }
        return slot.Bitmap;
    }

    async Task LoadAsync(string src)
    {
        await workers.WaitAsync();
        var slot = new Slot();
        try
        {
            var bytes = await ReadAsync(src);
            if (bytes is null) slot.Failed = true;
            else await DecodeAsync(bytes, slot);
        }
        catch (Exception e)
        {
            slot.Failed = true;
            HostLog.Line($"picture {Short(src)}: {e.Message}");
        }
        finally { workers.Release(); }
        lock (gate) slots[src] = slot;
        Arrived?.Invoke(src);
    }

    static string Short(string s) => s.Length > 80 ? s[..80] + "…" : s;

    async Task<byte[]?> ReadAsync(string src)
    {
        if (src.StartsWith("data:image/", StringComparison.OrdinalIgnoreCase))
        {
            var comma = src.IndexOf(',');
            if (comma < 0 || !src[..comma].Contains(";base64", StringComparison.OrdinalIgnoreCase)) return null;
            var b64 = src[(comma + 1)..];
            return b64.Length > MaxBytes * 4 / 3 ? null : Convert.FromBase64String(b64);
        }
        if (!Uri.TryCreate(src, UriKind.Absolute, out var uri)) return null;
        // hover://images/<name>: a picture pasted into a prompt, kept in the data folder.
        if (uri.Scheme == "hover")
        {
            string? root = null;
            string rel;
            if (uri.Host == "images")
            {
                root = imagesFolder;
                rel = Uri.UnescapeDataString(uri.AbsolutePath).TrimStart('/');
                if (rel.Contains('/') || rel.Contains('\\')) return null;
            }
            else return null;
            if (root is null || rel.Length == 0) return null;
            // A file under the folder served, and nowhere else.
            var full = Path.GetFullPath(Path.Combine(root, rel));
            var rootFull = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            if (!full.StartsWith(rootFull, StringComparison.OrdinalIgnoreCase)) return null;
            var info = new FileInfo(full);
            if (!info.Exists || info.Length > MaxBytes) return null;
            return await File.ReadAllBytesAsync(full);
        }
        if (uri.Scheme is not ("http" or "https")) return null;
        // https://f<key>.hover/<path>: a file in the folder of the session that host belongs to.
        if (uri.Host.EndsWith(".hover", StringComparison.OrdinalIgnoreCase))
        {
            var root = folderOfFiles(uri.Host);
            var rel = Uri.UnescapeDataString(uri.AbsolutePath).TrimStart('/').Replace('/', Path.DirectorySeparatorChar);
            if (root is null || rel.Length == 0) return null;
            var full = Path.GetFullPath(Path.Combine(root, rel));
            var rootFull = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            if (!full.StartsWith(rootFull, StringComparison.OrdinalIgnoreCase)) return null;
            var info = new FileInfo(full);
            if (!info.Exists || info.Length > MaxBytes) return null;
            return await File.ReadAllBytesAsync(full);
        }
        using var response = await http.GetAsync(uri, HttpCompletionOption.ResponseHeadersRead);
        if (!response.IsSuccessStatusCode) return null;
        if (response.Content.Headers.ContentLength > MaxBytes) return null;
        await using var stream = await response.Content.ReadAsStreamAsync();
        using var memory = new MemoryStream();
        var buffer = new byte[81920];
        int n;
        while ((n = await stream.ReadAsync(buffer)) > 0)
        {
            memory.Write(buffer, 0, n);
            if (memory.Length > MaxBytes) return null;
        }
        return memory.ToArray();
    }

    static async Task DecodeAsync(byte[] bytes, Slot slot)
    {
        using var stream = new InMemoryRandomAccessStream();
        using (var writer = new DataWriter(stream))
        {
            writer.WriteBytes(bytes);
            await writer.StoreAsync();
            writer.DetachStream();
        }
        stream.Seek(0);
        var decoder = await BitmapDecoder.CreateAsync(stream);
        long w = decoder.OrientedPixelWidth, h = decoder.OrientedPixelHeight;
        if (w <= 0 || h <= 0 || w * h > MaxPixels)
        {
            slot.Failed = true;
            return;
        }
        var transform = new BitmapTransform();
        var longest = Math.Max(w, h);
        if (longest > MaxSide)
        {
            var k = (double)MaxSide / longest;
            transform.ScaledWidth = (uint)Math.Max(Math.Round(w * k), 1);
            transform.ScaledHeight = (uint)Math.Max(Math.Round(h * k), 1);
            w = transform.ScaledWidth;
            h = transform.ScaledHeight;
        }
        var data = await decoder.GetPixelDataAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied, transform, ExifOrientationMode.RespectExifOrientation, ColorManagementMode.DoNotColorManage);
        slot.Pixels = data.DetachPixelData();
        slot.W = (int)w;
        slot.H = (int)h;
    }

    public void Dispose()
    {
        lock (gate)
        {
            foreach (var s in slots.Values) s?.Bitmap?.Dispose();
            slots.Clear();
        }
        http.Dispose();
    }
}

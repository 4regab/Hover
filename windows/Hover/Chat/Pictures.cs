using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using Hover.Chat.Ui;
using Hover.Host;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Hover.Chatting;

/// <summary>
/// Pictures on their way into a prompt (internal/shell/newtask.go attachPixels and attachFile): the clipboard's bitmap
/// shrunk to 2000 px on its long side and kept as PNG (JPEG when that is still over 8 MB), and a file the user picked,
/// kept as it is when it is a PNG, JPEG, GIF or WebP of at most 8 MB. Each becomes a data: address the backend keeps.
/// </summary>
static class Pictures
{
    const int MaxSide = 2000;
    const long MaxBytes = 8 * 1024 * 1024;
    const long MaxPixels = 100_000_000;

    /// <summary>A file the user picked, as a data: address or the reason it cannot be used.</summary>
    public static PickedImage FromFile(string file)
    {
        var kind = Path.GetExtension(file).TrimStart('.').ToLowerInvariant() switch
        {
            "png" => "png", "jpg" or "jpeg" => "jpeg", "gif" => "gif", "webp" => "webp", _ => "",
        };
        if (kind.Length == 0) return new PickedImage(null, "Only PNG, JPEG, GIF and WebP images.");
        try
        {
            var info = new FileInfo(file);
            if (!info.Exists) return new PickedImage(null, "Couldn’t read that image: it is not there.");
            if (info.Length > MaxBytes) return new PickedImage(null, "That image is over 8 MB.");
            return new PickedImage($"data:image/{kind};base64,{Convert.ToBase64String(File.ReadAllBytes(file))}", null);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return new PickedImage(null, "Couldn’t read that image: " + e.Message);
        }
    }

    /// <summary>The clipboard's bitmap (a DIB) as a data: address, or null when it cannot be read.</summary>
    public static string? FromDib(byte[] dib)
    {
        if (dib.Length < 40) return null;
        try { return Task.Run(() => ConvertAsync(WithFileHeader(dib))).GetAwaiter().GetResult(); }
        catch (Exception e)
        {
            HostLog.Line($"clipboard picture: {e.Message}");
            return null;
        }
    }

    /// <summary>A DIB with the 14-byte header a BMP file has, so the system's decoder reads it.</summary>
    static byte[] WithFileHeader(byte[] dib)
    {
        var size = BitConverter.ToInt32(dib, 0);
        var bits = BitConverter.ToUInt16(dib, 14);
        var compression = BitConverter.ToUInt32(dib, 16);
        var used = BitConverter.ToUInt32(dib, 32);
        var colours = bits <= 8 ? (used != 0 ? (int)used : 1 << bits) : (int)used;
        var masks = compression == 3 && size == 40 ? 12 : 0;
        var offset = 14 + size + masks + colours * 4;
        var file = new byte[14 + dib.Length];
        file[0] = (byte)'B';
        file[1] = (byte)'M';
        BitConverter.GetBytes(file.Length).CopyTo(file, 2);
        BitConverter.GetBytes(offset).CopyTo(file, 10);
        dib.CopyTo(file, 14);
        return file;
    }

    static async Task<string?> ConvertAsync(byte[] bmp)
    {
        using var input = new InMemoryRandomAccessStream();
        using (var writer = new DataWriter(input))
        {
            writer.WriteBytes(bmp);
            await writer.StoreAsync();
            writer.DetachStream();
        }
        input.Seek(0);
        var decoder = await BitmapDecoder.CreateAsync(input);
        long w = decoder.OrientedPixelWidth, h = decoder.OrientedPixelHeight;
        if (w <= 0 || h <= 0 || w * h > MaxPixels) return null;
        var transform = new BitmapTransform();
        if (Math.Max(w, h) > MaxSide)
        {
            var k = (double)MaxSide / Math.Max(w, h);
            transform.ScaledWidth = (uint)Math.Max(Math.Round(w * k), 1);
            transform.ScaledHeight = (uint)Math.Max(Math.Round(h * k), 1);
            transform.InterpolationMode = BitmapInterpolationMode.Fant;
            w = transform.ScaledWidth;
            h = transform.ScaledHeight;
        }
        // A screenshot's bitmap has no alpha of its own (the fourth byte is 0): it is opaque.
        var data = await decoder.GetPixelDataAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, transform, ExifOrientationMode.IgnoreExifOrientation, ColorManagementMode.DoNotColorManage);
        var pixels = data.DetachPixelData();
        for (var i = 3; i < pixels.Length; i += 4) pixels[i] = 255;
        using var bitmap = SoftwareBitmap.CreateCopyFromBuffer(pixels.AsBuffer(), BitmapPixelFormat.Bgra8, (int)w, (int)h, BitmapAlphaMode.Premultiplied);
        foreach (var (encoderId, kind) in new[] { (BitmapEncoder.PngEncoderId, "png"), (BitmapEncoder.JpegEncoderId, "jpeg") })
        {
            using var output = new InMemoryRandomAccessStream();
            var encoder = await BitmapEncoder.CreateAsync(encoderId, output);
            encoder.SetSoftwareBitmap(bitmap);
            await encoder.FlushAsync();
            if (output.Size > MaxBytes && kind == "png") continue;
            if (output.Size > MaxBytes) return null;
            output.Seek(0);
            var bytes = new byte[output.Size];
            using var reader = new DataReader(output);
            await reader.LoadAsync((uint)output.Size);
            reader.ReadBytes(bytes);
            return $"data:image/{kind};base64,{Convert.ToBase64String(bytes)}";
        }
        return null;
    }
}

/// <summary>The system's Open dialog for a picture (comdlg32 GetOpenFileName): a file of a kind the backend keeps.</summary>
static class ImageDialog
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct OpenFileName
    {
        public int StructSize;
        public nint Owner, Instance;
        public string Filter;
        public string? CustomFilter;
        public int MaxCustFilter, FilterIndex;
        public nint File;
        public int MaxFile;
        public string? FileTitle;
        public int MaxFileTitle;
        public string? InitialDir, Title;
        public int Flags;
        public short FileOffset, FileExtension;
        public string? DefExt;
        public nint CustData, Hook;
        public string? TemplateName;
        public nint Reserved;
        public int Reserved2, FlagsEx;
    }

    [DllImport("comdlg32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool GetOpenFileNameW(ref OpenFileName ofn);

    [DllImport("user32.dll")]
    static extern nint GetActiveWindow();

    const int Explorer = 0x80000, FileMustExist = 0x1000, PathMustExist = 0x800, NoChangeDir = 0x8;

    /// <summary>The picture picked, or null when the dialog was cancelled.</summary>
    public static PickedImage? Pick()
    {
        const int chars = 32768;
        var buffer = Marshal.AllocHGlobal(chars * 2);
        try
        {
            Marshal.WriteInt16(buffer, 0, 0);
            var ofn = new OpenFileName
            {
                Filter = "Pictures (PNG, JPEG, GIF, WebP)\0*.png;*.jpg;*.jpeg;*.gif;*.webp\0\0",
                FilterIndex = 1, File = buffer, MaxFile = chars, Title = "Attach a picture",
                Flags = Explorer | FileMustExist | PathMustExist | NoChangeDir, Owner = GetActiveWindow(),
            };
            ofn.StructSize = Marshal.SizeOf(ofn);
            if (!GetOpenFileNameW(ref ofn)) return null;
            var path = Marshal.PtrToStringUni(buffer);
            return string.IsNullOrEmpty(path) ? null : Pictures.FromFile(path);
        }
        finally { Marshal.FreeHGlobal(buffer); }
    }
}

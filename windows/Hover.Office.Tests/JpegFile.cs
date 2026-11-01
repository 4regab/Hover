using Vortice.WIC;

namespace Hover.Office.Tests;

/// <summary>Writes opaque BGRA pixels as a JPEG file with Windows' imaging component, no wider than 1400 pixels.</summary>
static class JpegFile
{
    public const int MaxWidth = 1400;

    public static (int Width, int Height) Write(string file, byte[] bgra, int width, int height)
    {
        if (width > MaxWidth)
        {
            // Box filter down to the limit: the office is a picture of small hard-edged voxels, and a box keeps them clean.
            var w = MaxWidth;
            var h = Math.Max((int)Math.Round(height * (double)w / width), 1);
            bgra = Shrink(bgra, width, height, w, h);
            width = w;
            height = h;
        }
        var bgr = new byte[width * height * 3];
        for (var i = 0; i < width * height; i++)
        {
            bgr[i * 3] = bgra[i * 4];
            bgr[i * 3 + 1] = bgra[i * 4 + 1];
            bgr[i * 3 + 2] = bgra[i * 4 + 2];
        }
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(file))!);
        using var factory = new IWICImagingFactory();
        using var stream = File.Create(file);
        using var encoder = factory.CreateEncoder(ContainerFormat.Jpeg);
        encoder.Initialize(stream, BitmapEncoderCacheOption.NoCache);
        using var frame = encoder.CreateNewFrame(out var bag);
        frame.Initialize(bag);
        frame.SetSize((uint)width, (uint)height);
        var format = PixelFormat.Format24bppBGR;
        frame.SetPixelFormat(ref format);
        unsafe
        {
            fixed (byte* p = bgr) frame.WritePixels((uint)height, (uint)(width * 3), (uint)bgr.Length, p);
        }
        frame.Commit();
        encoder.Commit();
        return (width, height);
    }

    static byte[] Shrink(byte[] src, int sw, int sh, int dw, int dh)
    {
        var o = new byte[dw * dh * 4];
        for (var y = 0; y < dh; y++)
        {
            int y0 = y * sh / dh, y1 = Math.Max((y + 1) * sh / dh, y0 + 1);
            for (var x = 0; x < dw; x++)
            {
                int x0 = x * sw / dw, x1 = Math.Max((x + 1) * sw / dw, x0 + 1);
                long b = 0, g = 0, r = 0, n = 0;
                for (var yy = y0; yy < y1; yy++)
                    for (var xx = x0; xx < x1; xx++)
                    {
                        var i = (yy * sw + xx) * 4;
                        b += src[i];
                        g += src[i + 1];
                        r += src[i + 2];
                        n++;
                    }
                var d = (y * dw + x) * 4;
                o[d] = (byte)(b / n);
                o[d + 1] = (byte)(g / n);
                o[d + 2] = (byte)(r / n);
                o[d + 3] = 255;
            }
        }
        return o;
    }

    /// <summary>The size a JPEG says it has (its start-of-frame marker).</summary>
    public static (int Width, int Height) Size(string file)
    {
        var j = File.ReadAllBytes(file);
        if (j.Length < 4 || j[0] != 0xFF || j[1] != 0xD8) throw new InvalidDataException(file + " is not a JPEG");
        for (var i = 2; i + 9 < j.Length;)
        {
            if (j[i] != 0xFF) { i++; continue; }
            var m = j[i + 1];
            if (m is >= 0xC0 and <= 0xCF and not 0xC4 and not 0xC8 and not 0xCC) return (j[i + 7] << 8 | j[i + 8], j[i + 5] << 8 | j[i + 6]);
            i += 2 + (j[i + 2] << 8 | j[i + 3]);
        }
        throw new InvalidDataException("no frame in " + file);
    }
}

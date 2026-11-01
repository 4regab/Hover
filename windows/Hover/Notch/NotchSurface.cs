using Hover.Host;
using Vortice.Direct2D1;
using Vortice.DirectComposition;
using Vortice.DXGI;
using AlphaMode = Vortice.DCommon.AlphaMode;
using PixelFormat = Vortice.DCommon.PixelFormat;

namespace Hover.Windowing;

/// <summary>
/// The pixels of the notch window. A swap chain made for composition with premultiplied alpha is
/// shown by a DirectComposition visual: the one path that keeps per-pixel see-through (a swap
/// chain on the window itself shows black where nothing is drawn). Direct2D draws into the
/// swap chain's buffer.
/// </summary>
sealed class NotchSurface : IDisposable
{
    readonly Gfx g;
    readonly IDXGISwapChain1 swap;
    readonly IDCompositionDevice composition;
    readonly IDCompositionTarget target;
    readonly IDCompositionVisual visual;
    ID2D1Bitmap1? buffer;
    int width, height;

    public NotchSurface(Gfx gfx, nint window, int w, int h)
    {
        g = gfx;
        width = Math.Max(w, 1);
        height = Math.Max(h, 1);
        var description = new SwapChainDescription1
        {
            Width = (uint)width,
            Height = (uint)height,
            Format = Format.B8G8R8A8_UNorm,
            Stereo = false,
            SampleDescription = new SampleDescription(1, 0),
            BufferUsage = Usage.RenderTargetOutput,
            BufferCount = 2,
            Scaling = Scaling.Stretch,
            SwapEffect = SwapEffect.FlipSequential,
            AlphaMode = Vortice.DXGI.AlphaMode.Premultiplied,
            Flags = SwapChainFlags.None,
        };
        swap = g.Factory.CreateSwapChainForComposition(g.Device, description, null);
        composition = DComp.DCompositionCreateDevice<IDCompositionDevice>(g.DxgiDevice);
        composition.CreateTargetForHwnd(window, true, out target).CheckError();
        composition.CreateVisual(out visual).CheckError();
        visual.SetContent(swap);
        target.SetRoot(visual);
        composition.Commit();
    }

    /// <summary>The composition device and the root visual: where a later view (the office) adds its own visual.</summary>
    public (IDCompositionDevice Device, IDCompositionVisual Root) Composition => (composition, visual);

    public (int W, int H) Size => (width, height);

    public void Resize(int w, int h)
    {
        w = Math.Max(w, 1);
        h = Math.Max(h, 1);
        if (w == width && h == height) return;
        buffer?.Dispose();
        buffer = null;
        swap.ResizeBuffers(0, (uint)w, (uint)h, Format.Unknown, SwapChainFlags.None).CheckError();
        width = w;
        height = h;
        HostLog.Line($"notch surface: {w} x {h}");
    }

    ID2D1Bitmap1 Buffer()
    {
        if (buffer is not null) return buffer;
        using var surface = swap.GetBuffer<IDXGISurface>(0);
        buffer = g.Context.CreateBitmapFromDxgiSurface(surface,
            new BitmapProperties1(new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied), 96, 96, BitmapOptions.Target | BitmapOptions.CannotDraw));
        return buffer;
    }

    /// <summary>Draws one frame and shows it. <paramref name="scale"/> is device pixels per DIP.</summary>
    public void Draw(Action<ID2D1DeviceContext> draw, double scale)
    {
        var dc = g.Context;
        dc.Target = Buffer();
        dc.SetDpi((float)(96 * scale), (float)(96 * scale));
        dc.BeginDraw();
        try { draw(dc); }
        finally
        {
            var result = dc.EndDraw();
            dc.Target = null;
            if (result.Failure)
            {
                // The device went away (a driver reset, a remote session): the frame is lost.
                HostLog.Line($"notch draw failed: {result}");
                buffer?.Dispose();
                buffer = null;
            }
        }
        swap.Present(1, PresentFlags.None);
    }

    public void Dispose()
    {
        buffer?.Dispose();
        visual.Dispose();
        target.Dispose();
        composition.Dispose();
        swap.Dispose();
    }
}

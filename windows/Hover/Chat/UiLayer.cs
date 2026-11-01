using Hover.Host;
using Hover.Windowing;
using Vortice.Direct2D1;
using Vortice.DirectComposition;
using Vortice.DXGI;
using AlphaMode = Vortice.DCommon.AlphaMode;
using PixelFormat = Vortice.DCommon.PixelFormat;

namespace Hover.Chatting;

/// <summary>
/// The panels in the open notch: a see-through swap chain shown by its own DirectComposition visual,
/// kept above the other visuals of the notch (the 3D office is one), so the panels draw over the office.
/// </summary>
sealed class UiLayer : IDisposable
{
    readonly Gfx g;
    readonly IDCompositionDevice composition;
    readonly IDCompositionVisual root;
    IDXGISwapChain1? swap;
    IDCompositionVisual? visual;
    ID2D1Bitmap1? buffer;
    int width, height;
    float offsetX, offsetY;
    bool shown;

    public UiLayer(Gfx gfx, IDCompositionDevice composition, IDCompositionVisual root)
    {
        g = gfx;
        this.composition = composition;
        this.root = root;
    }

    /// <summary>The layer's place and size in the window, in device pixels.</summary>
    public void Place(float x, float y, int w, int h)
    {
        w = Math.Max(w, 1);
        h = Math.Max(h, 1);
        if (swap is null)
        {
            var description = new SwapChainDescription1
            {
                Width = (uint)w, Height = (uint)h, Format = Format.B8G8R8A8_UNorm, Stereo = false,
                SampleDescription = new SampleDescription(1, 0), BufferUsage = Usage.RenderTargetOutput, BufferCount = 2,
                Scaling = Scaling.Stretch, SwapEffect = SwapEffect.FlipSequential, AlphaMode = Vortice.DXGI.AlphaMode.Premultiplied,
            };
            swap = g.Factory.CreateSwapChainForComposition(g.Device, description, null);
            composition.CreateVisual(out visual).CheckError();
            visual!.SetContent(swap);
            width = w;
            height = h;
        }
        else if (w != width || h != height)
        {
            buffer?.Dispose();
            buffer = null;
            swap.ResizeBuffers(0, (uint)w, (uint)h, Format.Unknown, SwapChainFlags.None).CheckError();
            width = w;
            height = h;
        }
        if (offsetX != x || offsetY != y || !shown)
        {
            offsetX = x;
            offsetY = y;
            visual!.SetOffsetX(x);
            visual.SetOffsetY(y);
        }
    }

    /// <summary>Puts the layer above every other visual of the window, or takes it away.</summary>
    public void Show(bool on)
    {
        if (visual is null) return;
        // Adding again moves it to the top, which keeps it above a visual added after it.
        if (shown) root.RemoveVisual(visual);
        if (on) root.AddVisual(visual, true, null!);
        shown = on;
        composition.Commit();
    }

    ID2D1Bitmap1 Buffer()
    {
        if (buffer is not null) return buffer;
        using var surface = swap!.GetBuffer<IDXGISurface>(0);
        buffer = g.Context.CreateBitmapFromDxgiSurface(surface,
            new BitmapProperties1(new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied), 96, 96, BitmapOptions.Target | BitmapOptions.CannotDraw));
        return buffer;
    }

    /// <summary>Draws a frame. <paramref name="scale"/> is device pixels per DIP.</summary>
    public void Draw(Action<ID2D1DeviceContext> draw, double scale)
    {
        if (swap is null) return;
        var dc = g.Context;
        dc.Target = Buffer();
        dc.SetDpi((float)(96 * scale), (float)(96 * scale));
        dc.BeginDraw();
        try
        {
            dc.Clear(null);
            draw(dc);
        }
        finally
        {
            var result = dc.EndDraw();
            dc.Target = null;
            if (result.Failure)
            {
                HostLog.Line($"panels draw failed: {result}");
                buffer?.Dispose();
                buffer = null;
            }
        }
        swap.Present(1, PresentFlags.None);
    }

    public void Dispose()
    {
        if (visual is not null && shown) root.RemoveVisual(visual);
        buffer?.Dispose();
        visual?.Dispose();
        swap?.Dispose();
    }
}

using Hover.Host;
using Vortice.Direct2D1;
using Vortice.Direct3D;
using Vortice.Direct3D11;
using Vortice.DirectWrite;
using Vortice.DXGI;
using FeatureLevel = Vortice.Direct3D.FeatureLevel;

namespace Hover.Windowing;

/// <summary>
/// The graphics the notch draws with: one Direct3D 11 device (hardware, or Windows' own software
/// renderer where there is no GPU, as on a build machine), Direct2D on it for shapes and
/// DirectWrite for text. One of these serves the notch window and the pictures.
/// </summary>
sealed class Gfx : IDisposable
{
    public required ID3D11Device Device { get; init; }
    public required IDXGIDevice DxgiDevice { get; init; }
    public required IDXGIFactory2 Factory { get; init; }
    public required ID2D1Factory1 D2D { get; init; }
    public required ID2D1Device D2DDevice { get; init; }
    public required ID2D1DeviceContext Context { get; init; }
    public required IDWriteFactory Write { get; init; }
    /// <summary>The Inter fonts that ship beside the app, or null when they could not be loaded (the system's UI font is used).</summary>
    public IDWriteFontCollection? Fonts { get; init; }
    public required string Driver { get; init; }

    public static Gfx Create()
    {
        var levels = new[] { FeatureLevel.Level_11_1, FeatureLevel.Level_11_0, FeatureLevel.Level_10_1, FeatureLevel.Level_10_0 };
        var driver = "hardware";
        var hr = D3D11.D3D11CreateDevice((IDXGIAdapter?)null, DriverType.Hardware, DeviceCreationFlags.BgraSupport, levels, out var device);
        if (hr.Failure || device is null)
        {
            // No GPU (a build machine, a virtual machine): Windows' own software renderer.
            driver = "WARP";
            hr = D3D11.D3D11CreateDevice((IDXGIAdapter?)null, DriverType.Warp, DeviceCreationFlags.BgraSupport, levels, out device);
            hr.CheckError();
        }
        var dxgi = device!.QueryInterface<IDXGIDevice>();
        using var adapter = dxgi.GetAdapter();
        var factory = adapter.GetParent<IDXGIFactory2>();
        var d2d = D2D1.D2D1CreateFactory<ID2D1Factory1>();
        var d2dDevice = d2d.CreateDevice(dxgi);
        var context = d2dDevice.CreateDeviceContext(DeviceContextOptions.None);
        var write = DWrite.DWriteCreateFactory<IDWriteFactory>();
        HostLog.Line($"notch graphics: Direct3D 11 ({driver})");
        return new Gfx
        {
            Device = device, DxgiDevice = dxgi, Factory = factory, D2D = d2d, D2DDevice = d2dDevice, Context = context,
            Write = write, Fonts = LoadFonts(write), Driver = driver,
        };
    }

    // The four Inter weights are in Assets\Fonts beside the app; a font set of just those files
    // gives one "Inter" family with the weights 400 to 700.
    static IDWriteFontCollection? LoadFonts(IDWriteFactory write)
    {
        try
        {
            var folder = Path.Combine(AppContext.BaseDirectory, "Assets", "Fonts");
            var files = new[] { "Inter-Regular.ttf", "Inter-Medium.ttf", "Inter-SemiBold.ttf", "Inter-Bold.ttf" }
                .Select(f => Path.Combine(folder, f)).ToList();
            if (files.Any(f => !File.Exists(f)))
            {
                HostLog.Line("notch fonts: the Inter files are missing; using the system font");
                return null;
            }
            using var factory3 = write.QueryInterface<IDWriteFactory3>();
            using var builder = factory3.CreateFontSetBuilder();
            foreach (var path in files)
            {
                using var file = write.CreateFontFileReference(path);
                using var builder1 = builder.QueryInterface<IDWriteFontSetBuilder1>();
                builder1.AddFontFile(file);
            }
            using var set = builder.CreateFontSet();
            return factory3.CreateFontCollectionFromFontSet(set);
        }
        catch (Exception e)
        {
            HostLog.Line($"notch fonts: could not load Inter ({e.Message}); using the system font");
            return null;
        }
    }

    public void Dispose()
    {
        Context.Dispose();
        D2DDevice.Dispose();
        D2D.Dispose();
        Fonts?.Dispose();
        Write.Dispose();
        Factory.Dispose();
        DxgiDevice.Dispose();
        Device.Dispose();
    }
}

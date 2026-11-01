using System.Diagnostics;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host;
using Hover.Host.Protocol;
using Hover.Windowing;
using Microsoft.UI.Dispatching;
using Vortice.Direct2D1;

namespace Hover.Chatting;

/// <summary>
/// The office's panels as the app has them: one <see cref="OfficeUi"/> with the DirectWrite shaper, the
/// Direct2D canvas and the picture store behind it, following the backend. The open notch and the app
/// window each draw it (only one shows at a time), so a chat opened in one is the chat in the other.
/// Everything runs on the UI thread.
/// </summary>
sealed class UiCore : IDisposable
{
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly DispatcherQueue queue;
    readonly BackendSession backend;
    readonly Action<BackendEvent> onEvent;
    bool disposed;

    public Gfx Gfx { get; }
    public DwShaper Shaper { get; }
    public ImageStore Images { get; }
    public D2DCanvas Canvas { get; }
    public OfficeUi Ui { get; }

    /// <summary>Something wants a new frame (a message arrived, a picture came in). Raised on the UI thread.</summary>
    public event Action? Changed;

    public double NowMs => clock.Elapsed.TotalMilliseconds;

    /// <summary>The pictures set the state themselves, so the backend's own state messages are not taken.</summary>
    public bool FreezeState { get; set; }

    public UiCore(Gfx gfx, DispatcherQueue queue, BackendSession backend, string dataDir, OfficeHost? office, bool inNotch)
    {
        Gfx = gfx;
        this.queue = queue;
        this.backend = backend;
        Shaper = new DwShaper(gfx);
        Images = new ImageStore(dataDir, host => Ui?.State?.Sessions.FirstOrDefault(s => OfficeUi.FilesHost(s.Key) == host)?.Folder);
        Canvas = new D2DCanvas(gfx, Shaper, Images);
        Ui = new OfficeUi(Shaper, new UiEnv
        {
            Send = backend.Send,
            Clipboard = new Win32Clipboard(),
            Images = Images.State,
            BotRect = id => office?.BotScreenRect(id) is { } r ? new Rect4((float)r.X, (float)r.Y, (float)r.W, (float)r.H) : null,
            OpenUrl = OpenUrl,
            Log = HostLog.Line,
            InNotch = inNotch,
        });
        onEvent = e => queue.TryEnqueue(() =>
        {
            if (disposed || (FreezeState && e is StateEvent)) return;
            try { Ui.Accept(e); }
            catch (Exception ex) { HostLog.Line($"panels: a message failed — {ex}"); }
            Changed?.Invoke();
        });
        backend.Event += onEvent;
        Images.Arrived += src => queue.TryEnqueue(() =>
        {
            if (disposed) return;
            Ui.ImageArrived(src);
            Changed?.Invoke();
        });
    }

    static void OpenUrl(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri) || uri.Scheme is not ("http" or "https")) return;
        try { Process.Start(new ProcessStartInfo(uri.AbsoluteUri) { UseShellExecute = true }); }
        catch (Exception e) { HostLog.Line($"could not open {url}: {e.Message}"); }
    }

    /// <summary>Draws a frame on the context, which must be between BeginDraw and EndDraw. Sizes are in device-independent pixels.</summary>
    public void Paint(ID2D1DeviceContext dc, float width, float height)
    {
        Canvas.Begin(dc);
        Ui.Paint(Canvas, width, height, NowMs);
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        backend.Event -= onEvent;
        Canvas.Dispose();
        Images.Dispose();
        Shaper.Dispose();
    }
}

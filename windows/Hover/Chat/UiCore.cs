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

    /// <summary>The top menu's Settings row was picked. Raised on the UI thread.</summary>
    public event Action? SettingsRequested;

    /// <summary>Something wants a new frame (a message arrived, a picture came in). Raised on the UI thread.</summary>
    public event Action? Changed;

    public double NowMs => clock.Elapsed.TotalMilliseconds;

    readonly OfficeHost? office;

    /// <summary>Where the office says a session's bot is, in the panels' own pixels (or where a picture says it is).</summary>
    public Rect4? BotRectOf(int id)
        => BotRectFixture?.Invoke(id) ?? (office?.BotScreenRect(id) is { } r ? new Rect4((float)r.X, (float)r.Y, (float)r.W, (float)r.H) : null);

    /// <summary>The colours of the helpers a bot has out at its desk, from the office's tag (or from what a picture says).</summary>
    IReadOnlyList<(byte R, byte G, byte B)> HelpersOf(int id)
        => HelpersFixture?.Invoke(id) ?? office?.HelpersOf(id) ?? [];

    (int Kind, int Id, string Hint) TipOf()
    {
        if (TipFixture is { } fixture) return fixture();
        var t = office?.Tip() ?? default;
        return (t.Kind, t.Id, t.Hint ?? "");
    }

    /// <summary>What a picture says the pointer is on, when the office is not live to say it.</summary>
    public Func<(int Kind, int Id, string Hint)>? TipFixture { get; set; }

    /// <summary>The helpers a picture says a bot has out, when the office is not live to say it.</summary>
    public Func<int, IReadOnlyList<(byte R, byte G, byte B)>>? HelpersFixture { get; set; }

    /// <summary>Where the pictures say a bot is, when the office is not live to say it.</summary>
    public Func<int, Rect4?>? BotRectFixture { get; set; }

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
        this.office = office;
        Ui = new OfficeUi(Shaper, new UiEnv
        {
            Send = backend.Send,
            Clipboard = new Win32Clipboard(),
            PickImage = ImageDialog.Pick,
            Images = Images.State,
            BotRect = BotRectOf,
            Helpers = HelpersOf,
            Tip = TipOf,
            OpenUrl = OpenUrl,
            Log = HostLog.Line,
            InNotch = inNotch,
            OpenSettings = () => SettingsRequested?.Invoke(),
        });
        onEvent = e => queue.TryEnqueue(() =>
        {
            if (disposed || (FreezeState && e is StateEvent)) return;
            try { Ui.Accept(e); }
            catch (Exception ex) { HostLog.Line($"panels: a message failed — {ex}"); }
            Changed?.Invoke();
        });
        backend.Event += onEvent;
        // The office and the panels: a click on a bot or its desk opens its chat, the door opens the new-task box,
        // and the office moves its camera to the bot whose chat is open.
        if (office is not null)
        {
            office.BotPicked += id => Picked($"bot {id}", () => Ui.OpenChat(id));
            // A desk with a session at it opens its card; the board, the TV and the shelf open their panels.
            office.DeskPicked += (id, x, y) => Picked($"desk {id}", () => Ui.OpenDeskCard(id, (float)x, (float)y));
            office.PropPicked += prop => Picked($"prop {prop}", () =>
            {
                switch (prop)
                {
                    case OfficeProp.Door: Ui.OpenNewTask(); break;
                    case OfficeProp.Tv: Ui.OpenOfficePanel("tv"); break;
                    case OfficeProp.Board: Ui.OpenOfficePanel("board"); break;
                    case OfficeProp.History: Ui.OpenOfficePanel("history"); break;
                }
            });
            office.Covered = (x, y) => Ui.Covers((float)x, (float)y);
            office.TipChanged += () => queue.TryEnqueue(() => { if (!disposed) Ui.Invalidate(); });
            Ui.OfficePanelChanged += name => office.ShowPanel(name);
            Ui.DeskChanged += id => office.MarkDeskOpen(id);
            Ui.DrawerChanged += id =>
            {
                if (id is { } open) office.OpenDrawer(open); else office.CloseDrawer();
            };
        }
        Images.Arrived += src => queue.TryEnqueue(() =>
        {
            if (disposed) return;
            Ui.ImageArrived(src);
            Changed?.Invoke();
        });
    }

    void Picked(string what, Action act)
    {
        if (disposed) return;
        if (UiDump.On) HostLog.Line($"panels: the office picked {what}");
        try { act(); }
        catch (Exception e) { HostLog.Line($"panels: a pick failed — {e}"); }
        Changed?.Invoke();
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

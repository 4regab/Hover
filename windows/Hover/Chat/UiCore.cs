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

    /// <summary>The Screen tab's pictures (the pictures of <c>--shots</c> give it a made-up screen).</summary>
    public Hover.Screening.ScreenFeed ScreenFeed { get; }
    public D2DCanvas Canvas { get; }
    public OfficeUi Ui { get; }

    /// <summary>The top menu's Settings row was picked. Raised on the UI thread.</summary>
    public event Action? SettingsRequested;

    /// <summary>The panels asked for a page of Settings by name ("kiro").</summary>
    public event Action<string>? SettingsPageRequested;

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

    internal static BotTag BotTagOf(Hover.Office.Tag t)
        => new(t.Id, (float)t.X, (float)t.Y, t.Name, Rgba.Of((uint)(t.Color.R << 16 | t.Color.G << 8 | t.Color.B)), t.Tool ?? "", t.Text ?? "", (int)t.Stage, t.Hot);

    IReadOnlyList<BotTag> TagsOf()
        => TagsFixture?.Invoke() ?? (office?.Tags() is { Count: > 0 } list ? [.. list.Select(BotTagOf)] : []);

    /// <summary>The name tags a picture says the office has, when it is not live to say it.</summary>
    public Func<IReadOnlyList<BotTag>>? TagsFixture { get; set; }

    /// <summary>What a picture says the pointer is on, when the office is not live to say it.</summary>
    public Func<(int Kind, int Id, string Hint)>? TipFixture { get; set; }

    /// <summary>The helpers a picture says a bot has out, when the office is not live to say it.</summary>
    public Func<int, IReadOnlyList<(byte R, byte G, byte B)>>? HelpersFixture { get; set; }

    /// <summary>Where the pictures say a bot is, when the office is not live to say it.</summary>
    public Func<int, Rect4?>? BotRectFixture { get; set; }

    /// <summary>The pictures set the state themselves, so the backend's own state messages are not taken.</summary>
    public bool FreezeState { get; set; }

    /// <summary>
    /// What the backend says that a picture does not take while the state is frozen: its state, and its answers about the made-up
    /// sessions the pictures show (it does not know them: "That chat isn't here."). The pictures feed those themselves.
    /// </summary>
    static bool Frozen(BackendEvent e) => e is StateEvent or ToastEvent or TermEvent or WriteFileEvent or EditorsEvent or OpenInEvent or DeskEvent
        or DeskActionEvent or GhEvent or ForkEvent or BringBackEvent or SwitchProviderEvent or QueueRemoveEvent or WebSessionsEvent or CliSessionsEvent
        or AdoptWebEvent or AdoptCliEvent or PreferencesEvent or SettingsEvent;

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
            Tags = TagsOf,
            OpenUrl = OpenUrl,
            Log = HostLog.Line,
            InNotch = inNotch,
            PickFolder = () => Hover.Configuring.Dialogs.Folder("Choose a folder"),
            OpenSettings = () => SettingsRequested?.Invoke(),
            TimeMode = () => office?.TimeMode ?? 0,
            SetTime = mode => office?.SetTimeMode(mode),
            Beats = () => office?.Beats ?? false,
            SetBeats = on => { if (office is not null) office.Beats = on; },
            ShowSettingsPage = page => SettingsPageRequested?.Invoke(page),
            Screen = ScreenFeed = new Hover.Screening.ScreenFeed(new Hover.Screening.ScreenSource(), Images, queue),
        });
        onEvent = e => queue.TryEnqueue(() =>
        {
            if (disposed || (FreezeState && Frozen(e))) return;
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
            // The beats go off again when there is no sound output: the menu's row shows it.
            office.BeatsChanged += _ => queue.TryEnqueue(() => { if (!disposed) Ui.Invalidate(); });
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

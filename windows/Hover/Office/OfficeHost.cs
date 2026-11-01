using Hover.Host;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml.Controls;

namespace Hover.Windowing;

/// <summary>Where the office is being shown.</summary>
public enum OfficeTarget
{
    /// <summary>Nowhere: the notch is folded and the app window is not showing.</summary>
    None,
    /// <summary>The open notch, in the rectangle <c>NotchScene.OfficeArea</c> gives.</summary>
    Notch,
    /// <summary>The app window, in <c>DashboardWindow.OfficeArea</c>.</summary>
    Dashboard,
}

/// <summary>A rectangle in device-independent pixels.</summary>
public readonly record struct OfficeRect(double X, double Y, double W, double H);

/// <summary>
/// The 3D office as the app uses it: one office, shown in the open notch or, when the notch is
/// folded, in the app window. It follows the backend's <c>state</c> messages by itself.
///
/// What other parts of the app use (the chat, the desk card and the permission prompts are drawn
/// over the scene by them, not here):
/// <list type="bullet">
/// <item><see cref="DeskPicked"/> and <see cref="BotPicked"/>: the user clicked a desk or a bot.
/// The value is the session's <c>id</c> from the backend's <c>state</c> message.</item>
/// <item><see cref="BotScreenRect"/>: where a session's bot is on screen, to draw a prompt over it.</item>
/// <item><see cref="Target"/>: which surface shows the office now.</item>
/// </list>
/// Everything runs on the UI thread.
/// </summary>
public sealed class OfficeHost : IDisposable
{
    readonly DispatcherQueue queue;
    readonly BackendSession backend;
    bool disposed;

    public OfficeHost(DispatcherQueue queue, BackendSession backend)
    {
        this.queue = queue;
        this.backend = backend;
    }

    /// <summary>A click landed on a desk that has a session at it. The value is that session's id.</summary>
    public event Action<int>? DeskPicked;

    /// <summary>A click landed on a bot. The value is its session's id.</summary>
    public event Action<int>? BotPicked;

    /// <summary>Which surface shows the office now.</summary>
    public OfficeTarget Target { get; private set; }

    /// <summary>True once the office draws into the notch, so the notch stops drawing its placeholder.</summary>
    internal bool DrawsInNotch { get; private set; }

    /// <summary>
    /// Where a session's bot is on screen, in DIPs from the top left of the surface that shows the
    /// office (the rectangle <c>NotchScene.OfficeArea</c> gives in the notch, the top left of
    /// <c>DashboardWindow.OfficeArea</c> in the app window). Null when the office is not shown or
    /// the session has no bot in it.
    /// </summary>
    public OfficeRect? BotScreenRect(int sessionId) => null;

    /// <summary>The open notch shows the office here. Called after every notch frame; cheap when nothing changed.</summary>
    internal void UpdateNotch(NotchSlot slot)
    {
    }

    /// <summary>The primary button went down in the notch window (device pixels, window coordinates).</summary>
    internal void NotchPressed(int x, int y)
    {
    }

    /// <summary>The primary button went up in the notch window.</summary>
    internal void NotchReleased(int x, int y)
    {
    }

    /// <summary>The wheel turned over the notch window (120 is one notch of the wheel, positive away from the user).</summary>
    internal void NotchWheel(int delta)
    {
    }

    /// <summary>A key went down in the notch window (virtual-key code).</summary>
    internal void NotchKey(int virtualKey)
    {
    }

    /// <summary>The app window has its office area; the office is shown there while the notch is folded.</summary>
    internal void AttachDashboard(Border area, nint window)
    {
    }

    /// <summary>The notch window is going away.</summary>
    internal void DetachNotch()
    {
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        HostLog.Line("office: stopped");
    }
}

/// <summary>
/// Everything the office needs to know of the open notch for one frame: the window, the composition
/// device and root visual to add its swap chain to, and where the office goes.
/// </summary>
internal sealed record NotchSlot(
    nint Window,
    Vortice.Direct3D11.ID3D11Device Device,
    Vortice.DirectComposition.IDCompositionDevice Composition,
    Vortice.DirectComposition.IDCompositionVisual Root,
    OfficeRect Area,
    double Scale,
    OfficeRect Shape,
    double ShapeRadius,
    double Opacity,
    bool Visible);

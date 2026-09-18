using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Threading;
using Hover.Core;

namespace Hover.Images;

/// A plain window holding the new screenshot tray, so it can be used and judged before
/// the rest of the app is moved over.
///
/// This is scaffolding for the rewrite, not a feature. It goes when the tray gets its
/// real edge window. Snipping works from here: the Snip button photographs the screen,
/// you drag a box, the mark-up window opens, and saving puts the picture in the tray.
public sealed class TrayPreviewWindow : Window
{
    private readonly ShotTray _tray = new();
    private readonly ShotStore _store = ShotStore.Shared;

    public TrayPreviewWindow()
    {
        Title = "Hover — new tray preview";
        Width = 340;
        Height = 760;
        Background = new SolidColorBrush(Color.Parse("#141414"));
        Content = new ScrollViewer
        {
            HorizontalScrollBarVisibility = Avalonia.Controls.Primitives.ScrollBarVisibility.Disabled,
            Content = new Border
            {
                Padding = new Thickness(16),
                Child = _tray,
            },
        };

        _tray.DeleteRequested += (_, shot) => _store.Delete(shot);
        _tray.SnipRequested += (_, _) => Snip.Begin();
        _tray.MarkUpRequested += (_, shot) => OpenMarkUp(shot);
        _store.Changed += OnStoreChanged;

        _store.Start();
        Reload();
        Log.Line("tray preview open");
    }

    private void OnStoreChanged(object? sender, EventArgs e) => Dispatcher.UIThread.Post(Reload);

    /// Opens a picture already in the tray for drawing on. Saving adds an edited copy
    /// rather than writing over the original.
    private static void OpenMarkUp(Shot shot)
    {
        var picture = shot.FullSize();
        if (picture is null)
        {
            Log.Line($"could not open {shot.Name} for drawing on");
            return;
        }
        Snip.MarkUp(picture);
    }

    private void Reload() => _tray.Show(_store.Shots);

    protected override void OnClosed(EventArgs e)
    {
        _store.Changed -= OnStoreChanged;
        base.OnClosed(e);
    }
}

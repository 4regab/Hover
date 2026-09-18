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
/// real edge window. What works here is what is finished: your real pictures, grouped
/// by day, hover states, and delete. Snipping is not moved over yet, so its button is
/// hidden rather than left there doing nothing.
public sealed class TrayPreviewWindow : Window
{
    private readonly ShotTray _tray = new() { CanSnip = false };
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
        _store.Changed += OnStoreChanged;

        _store.Start();
        Reload();
        Log.Line("tray preview open");
    }

    private void OnStoreChanged(object? sender, EventArgs e) => Dispatcher.UIThread.Post(Reload);

    private void Reload() => _tray.Show(_store.Shots);

    protected override void OnClosed(EventArgs e)
    {
        _store.Changed -= OnStoreChanged;
        base.OnClosed(e);
    }
}

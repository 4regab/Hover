using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;

namespace Hover.Images;

/// One picture in the tray.
///
/// The row takes the shape of its picture rather than a fixed height, so a wide
/// terminal capture is short and a tall page is tall. Nothing is letterboxed.
public partial class ShotRowView : UserControl
{
    /// Raised when the row is asked to delete its picture.
    public static readonly RoutedEvent<RoutedEventArgs> DeleteRequestedEvent =
        RoutedEvent.Register<ShotRowView, RoutedEventArgs>(
            nameof(DeleteRequested), RoutingStrategies.Bubble);

    /// Raised when a drag out of the tray should start.
    public static readonly RoutedEvent<RoutedEventArgs> DragRequestedEvent =
        RoutedEvent.Register<ShotRowView, RoutedEventArgs>(
            nameof(DragRequested), RoutingStrategies.Bubble);

    /// Raised when the picture should be opened for drawing on.
    public static readonly RoutedEvent<RoutedEventArgs> MarkUpRequestedEvent =
        RoutedEvent.Register<ShotRowView, RoutedEventArgs>(
            nameof(MarkUpRequested), RoutingStrategies.Bubble);

    public event EventHandler<RoutedEventArgs>? MarkUpRequested
    {
        add => AddHandler(MarkUpRequestedEvent, value);
        remove => RemoveHandler(MarkUpRequestedEvent, value);
    }

    public event EventHandler<RoutedEventArgs>? DeleteRequested
    {
        add => AddHandler(DeleteRequestedEvent, value);
        remove => RemoveHandler(DeleteRequestedEvent, value);
    }

    public event EventHandler<RoutedEventArgs>? DragRequested
    {
        add => AddHandler(DragRequestedEvent, value);
        remove => RemoveHandler(DragRequestedEvent, value);
    }

    private Border? _frame;

    public ShotRowView()
    {
        InitializeComponent();
        _frame = this.FindControl<Border>("Frame");
        // The height depends on the width, which is not known until layout runs.
        SizeChanged += (_, _) => Resize();
    }

    public ShotItem? Item => DataContext as ShotItem;

    protected override void OnDataContextChanged(EventArgs e)
    {
        base.OnDataContextChanged(e);
        Resize();
    }

    private void Resize()
    {
        if (_frame is null || Item is null) return;
        var width = _frame.Bounds.Width;
        if (width <= 0) return;
        var height = Item.HeightFor(width);
        // Only write when it actually changes, or setting Height would re-run layout
        // and call straight back in here.
        if (Math.Abs(_frame.Height - height) > 0.5) _frame.Height = height;
    }

    private void OnDelete(object? sender, RoutedEventArgs e)
    {
        // The click would otherwise also reach the row and start a drag.
        e.Handled = true;
        RaiseEvent(new RoutedEventArgs(DeleteRequestedEvent));
    }

    private void OnMarkUp(object? sender, RoutedEventArgs e)
    {
        e.Handled = true;
        RaiseEvent(new RoutedEventArgs(MarkUpRequestedEvent));
    }

    private void OnPressed(object? sender, PointerPressedEventArgs e)
    {
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed) return;
        RaiseEvent(new RoutedEventArgs(DragRequestedEvent));
    }
}

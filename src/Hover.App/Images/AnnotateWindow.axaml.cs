using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Hover.Core;

namespace Hover.Images;

/// Draw on a picture before it is saved.
///
/// Opened straight after a snip. Enter saves, Escape throws the picture away, so a
/// quick snip is still two keys and nobody is trapped in an editor they did not want.
public partial class AnnotateWindow : Window
{
    private readonly Bitmap _picture;
    private readonly Markup _markup = new();
    private readonly MarkupCanvas _canvas;

    private Button? _arrow, _box, _draw, _text, _highlight, _crop, _undo;
    private Button? _red, _yellow, _blue;
    private TextBlock? _status;
    private readonly Canvas _typingLayer = new();
    private TextBox? _typing;
    private Point _typingAt;

    /// Raised with the finished PNG when the user saves. Not raised if they discard.
    public event EventHandler<byte[]>? Saved;

    public AnnotateWindow(Bitmap picture)
    {
        _picture = picture;
        InitializeComponent();

        _canvas = new MarkupCanvas(_picture, _markup);
        _canvas.Drawn += (_, _) => Sync();
        _canvas.TextRequested += (_, at) => StartTyping(at);

        _arrow = this.FindControl<Button>("ArrowTool");
        _box = this.FindControl<Button>("BoxTool");
        _draw = this.FindControl<Button>("DrawTool");
        _text = this.FindControl<Button>("TextTool");
        _highlight = this.FindControl<Button>("HighlightTool");
        _crop = this.FindControl<Button>("CropTool");
        _undo = this.FindControl<Button>("UndoButton");
        _red = this.FindControl<Button>("RedSwatch");
        _yellow = this.FindControl<Button>("YellowSwatch");
        _blue = this.FindControl<Button>("BlueSwatch");
        _status = this.FindControl<TextBlock>("Status");

        var stage = this.FindControl<Border>("Stage");
        // The typing box sits above the picture rather than inside it: the picture is
        // painted, not built from controls, so it cannot hold one.
        if (stage is not null)
        {
            var layers = new Grid();
            layers.Children.Add(_canvas);
            layers.Children.Add(_typingLayer);
            stage.Child = layers;
        }

        // Open at the picture's size where that fits, so nothing is scaled down unless
        // the screenshot is bigger than the screen.
        var size = _picture.PixelSize;
        var limit = Screens.Primary?.WorkingArea;
        var maxWidth = (limit?.Width ?? 1600) * 0.9;
        var maxHeight = (limit?.Height ?? 900) * 0.85;
        Width = Math.Clamp(size.Width + 28, 520, maxWidth);
        Height = Math.Clamp(size.Height + 128, 380, maxHeight);

        Sync();
    }

    /// Only used by the XAML previewer, which needs a constructor with no arguments.
    public AnnotateWindow() : this(BlankPicture()) { }

    private static Bitmap BlankPicture()
    {
        var target = new RenderTargetBitmap(new Avalonia.PixelSize(640, 400));
        using (var ctx = target.CreateDrawingContext())
            ctx.FillRectangle(Brushes.DimGray, new Avalonia.Rect(0, 0, 640, 400));
        return target;
    }

    private void Sync()
    {
        Mark(_arrow, Tool.Arrow);
        Mark(_box, Tool.Box);
        Mark(_draw, Tool.Draw);
        Mark(_text, Tool.Text);
        Mark(_highlight, Tool.Highlight);
        Mark(_crop, Tool.Crop);

        Ring(_red, Markup.Red);
        Ring(_yellow, Markup.Yellow);
        Ring(_blue, Markup.Blue);

        if (_undo is not null) _undo.IsEnabled = _markup.CanUndo;

        if (_status is not null)
        {
            var size = _markup.SizeFor(_picture.PixelSize);
            var marks = _markup.Marks.Count;
            _status.Text = marks == 0
                ? $"{size.Width} x {size.Height}"
                : $"{size.Width} x {size.Height}  ·  {marks} mark{(marks == 1 ? "" : "s")}";
        }
    }

    private void Mark(Button? button, Tool tool)
    {
        if (button is null) return;
        if (_canvas.Tool == tool) button.Classes.Add("on");
        else button.Classes.Remove("on");
    }

    private void Ring(Button? button, Color colour)
    {
        if (button is null) return;
        if (_canvas.Colour == colour) button.Classes.Add("on");
        else button.Classes.Remove("on");
    }

    private void Choose(Tool tool)
    {
        // Changing tool finishes whatever was being typed, so the words are not lost.
        CommitTyping();
        _canvas.Tool = tool;
        Sync();
    }

    private void Choose(Color colour)
    {
        _canvas.Colour = colour;
        if (_typing is not null) _typing.Foreground = new SolidColorBrush(colour);
        Sync();
    }

    // MARK: Typing

    /// Puts a typing box on the picture where it was clicked, at the size the words will
    /// be saved, so what is typed is what comes out.
    private void StartTyping(Point at)
    {
        CommitTyping();

        var spot = _canvas.ToLocal(at);
        var box = new TextBox
        {
            FontFamily = Ink.SystemFace,
            FontWeight = FontWeight.SemiBold,
            FontSize = Math.Max(8, _canvas.TextSize * _canvas.Shown),
            Foreground = new SolidColorBrush(_canvas.Colour),
            CaretBrush = Brushes.White,
            Background = new SolidColorBrush(Colors.Black, 0.55),
            BorderThickness = new Thickness(0),
            Padding = new Thickness(0),
            MinWidth = 60,
            AcceptsReturn = false,
        };
        Canvas.SetLeft(box, spot.X);
        Canvas.SetTop(box, spot.Y);

        box.LostFocus += (_, _) => CommitTyping();
        box.KeyDown += (_, e) =>
        {
            if (e.Key == Key.Enter) { e.Handled = true; CommitTyping(); }
            else if (e.Key == Key.Escape) { e.Handled = true; CancelTyping(); }
        };

        _typing = box;
        _typingAt = at;
        _typingLayer.Children.Add(box);
        box.Focus();
    }

    /// Turns whatever was typed into a mark and takes the box away.
    private void CommitTyping()
    {
        if (_typing is not { } box) return;
        _typing = null;
        _typingLayer.Children.Remove(box);
        _canvas.AddText(_typingAt, box.Text ?? string.Empty);
        Sync();
    }

    private void CancelTyping()
    {
        if (_typing is not { } box) return;
        _typing = null;
        _typingLayer.Children.Remove(box);
    }

    private void OnArrow(object? s, RoutedEventArgs e) => Choose(Tool.Arrow);
    private void OnBox(object? s, RoutedEventArgs e) => Choose(Tool.Box);
    private void OnDraw(object? s, RoutedEventArgs e) => Choose(Tool.Draw);
    private void OnText(object? s, RoutedEventArgs e) => Choose(Tool.Text);
    private void OnHighlight(object? s, RoutedEventArgs e) => Choose(Tool.Highlight);
    private void OnCrop(object? s, RoutedEventArgs e) => Choose(Tool.Crop);

    private void OnRed(object? s, RoutedEventArgs e) => Choose(Markup.Red);
    private void OnYellow(object? s, RoutedEventArgs e) => Choose(Markup.Yellow);
    private void OnBlue(object? s, RoutedEventArgs e) => Choose(Markup.Blue);

    private void OnUndo(object? s, RoutedEventArgs e)
    {
        _markup.Undo();
        Sync();
    }

    private void OnDiscard(object? s, RoutedEventArgs e) => Discard();

    private void OnSave(object? s, RoutedEventArgs e) => Save();

    /// Saves the picture with everything on it. Exposed so a test can do what the
    /// button does.
    public void Save()
    {
        CommitTyping();
        var png = _markup.Export(_picture);
        if (png is null)
        {
            Log.Line("nothing to save: the crop has no area");
            return;
        }
        Saved?.Invoke(this, png);
        Close();
    }

    public void Discard()
    {
        CancelTyping();
        Log.Line("snip thrown away from the mark-up window");
        Close();
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        // While the typing box is up the keyboard belongs to it: numbers are numbers,
        // and Enter finishes the words rather than saving the picture.
        if (_typing is not null)
        {
            base.OnKeyDown(e);
            return;
        }

        switch (e.Key)
        {
            case Key.Enter:
                Save();
                e.Handled = true;
                return;
            case Key.Escape:
                Discard();
                e.Handled = true;
                return;
            case Key.Z when e.KeyModifiers.HasFlag(KeyModifiers.Control):
                _markup.Undo();
                Sync();
                e.Handled = true;
                return;
            case Key.D1: Choose(Tool.Arrow); e.Handled = true; return;
            case Key.D2: Choose(Tool.Box); e.Handled = true; return;
            case Key.D3: Choose(Tool.Draw); e.Handled = true; return;
            case Key.D4: Choose(Tool.Text); e.Handled = true; return;
            case Key.D5: Choose(Tool.Highlight); e.Handled = true; return;
            case Key.D6: Choose(Tool.Crop); e.Handled = true; return;
        }
        base.OnKeyDown(e);
    }

    protected override void OnClosed(EventArgs e)
    {
        _picture.Dispose();
        base.OnClosed(e);
    }
}

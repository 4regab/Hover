using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Hover.Core;

namespace Hover.Notes;

/// One tab on the deck: a sheet of coloured paper docked to the screen edge, with its
/// title turned on its side.
///
/// Tabs overlap, so the label is pinned to the top of the tab — the part that stays
/// uncovered by the tab below it. Hovering deepens the shadow so the tab reads as live.
public sealed class NoteTab : Panel
{
    private readonly Avalonia.Controls.Shapes.Path _sheet;
    private readonly bool _onRight;
    private bool _hovering;

    public Note Note { get; }
    public bool IsOpen { get; }

    public NoteTab(Note note, bool isOpen, double height, double strip, bool onRight)
    {
        Note = note;
        IsOpen = isOpen;
        _onRight = onRight;

        Width = DeckGeom.TabWidth + DeckGeom.Bleed;
        Height = height;
        Cursor = new Cursor(StandardCursorType.Hand);

        _sheet = new Avalonia.Controls.Shapes.Path
        {
            Data = TabShapes.EdgeTab(Width, height, onRight),
            Fill = note.Palette.PaperBrush,
        };
        Children.Add(_sheet);

        Children.Add(Label(note.DisplayTitle, strip, onRight, note.Palette.InkAt(0.85)));

        if (note.Pinned)
        {
            Children.Add(new Ellipse
            {
                Width = 5,
                Height = 5,
                Fill = note.Palette.DashBrush,
                HorizontalAlignment = onRight
                    ? Avalonia.Layout.HorizontalAlignment.Right
                    : Avalonia.Layout.HorizontalAlignment.Left,
                VerticalAlignment = Avalonia.Layout.VerticalAlignment.Top,
                Margin = onRight
                    ? new Thickness(0, 7, DeckGeom.Bleed + 9, 0)
                    : new Thickness(9, 7, 0, 0),
            });
        }

        // Everything leans the same way, anchored to the edge it is stuck to.
        RenderTransformOrigin = new RelativePoint(onRight ? 1 : 0, 0.5, RelativeUnit.Relative);
        RenderTransform = new RotateTransform(DeckGeom.Lean(onRight));

        Shade();
        PointerEntered += (_, _) => { _hovering = true; Shade(); };
        PointerExited += (_, _) => { _hovering = false; Shade(); };
    }

    /// A label turned on its side.
    ///
    /// `LayoutTransformControl`, not a render transform: the rotated text has to
    /// *measure* rotated too, or it claims its unrotated width and bleeds across the
    /// whole note.
    private static Control Label(string title, double strip, bool onRight, IBrush ink)
    {
        var text = new TextBlock
        {
            Text = title.ToUpperInvariant(),
            FontFamily = Ink.TabFamily,
            FontSize = Ink.TabFontSize,
            FontWeight = FontWeight.SemiBold,
            Foreground = ink,
            TextTrimming = TextTrimming.CharacterEllipsis,
            TextAlignment = TextAlignment.Center,
            Width = Math.Max(20, strip - DeckGeom.LabelInset),
            Height = DeckGeom.TabWidth,
            LineHeight = DeckGeom.TabWidth,
        };

        var turned = new LayoutTransformControl
        {
            LayoutTransform = new RotateTransform(onRight ? 90 : -90),
            Child = text,
        };

        return new Panel
        {
            Width = DeckGeom.TabWidth,
            Height = strip,
            ClipToBounds = true,
            VerticalAlignment = Avalonia.Layout.VerticalAlignment.Top,
            HorizontalAlignment = onRight
                ? Avalonia.Layout.HorizontalAlignment.Left
                : Avalonia.Layout.HorizontalAlignment.Right,
            Margin = onRight
                ? new Thickness(0, 0, DeckGeom.Bleed, 0)
                : new Thickness(DeckGeom.Bleed, 0, 0, 0),
            Children = { turned },
            IsHitTestVisible = false,
        };
    }

    private void Shade() => _sheet.Effect = TabShapes.Shadow(
        IsOpen || _hovering ? 0.32 : 0.22,
        IsOpen || _hovering ? 9 : 6,
        _onRight ? -3 : 3, 2);
}

using System.Globalization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Effects;

namespace Hover.Owl;

/// The small light sheets that drop from a row or the timer: focus duration and a
/// custom reminder time.
internal static class Popover
{
    /// Count of popovers and menus open right now; the notch stays open while > 0.
    public static int Open { get; private set; }

    private static readonly Brush Sheet = Ui.Frozen(Color.FromArgb(0xF5, 0xDD, 0xE4, 0xE6));

    /// A sheet centred under its anchor, with an arrow pointing up at it — as wide as
    /// a task row when it drops from one.
    private static Popup Show(FrameworkElement anchor, FrameworkElement body)
    {
        const double Shadow = 14;
        var width = Math.Clamp(anchor.ActualWidth + 16, 330, 470);
        var card = new Border
        {
            Background = Sheet,
            CornerRadius = new CornerRadius(12),
            Padding = new Thickness(18, 14, 18, 14),
            Width = width,
            Child = body,
        };
        var arrow = new System.Windows.Shapes.Path
        {
            Data = Geometry.Parse("M0,9 L9,0.6 Q10,-0.2 11,0.6 L20,9 Z"),
            Fill = Sheet,
            HorizontalAlignment = HorizontalAlignment.Center,
            Margin = new Thickness(0, 0, 0, -0.5),
        };
        var sheet = new StackPanel
        {
            Margin = new Thickness(Shadow),
            Effect = new DropShadowEffect { BlurRadius = 24, ShadowDepth = 4, Direction = 270, Opacity = 0.35 },
        };
        sheet.Children.Add(arrow);
        sheet.Children.Add(card);
        var pop = new Popup
        {
            Child = sheet,
            PlacementTarget = anchor,
            Placement = PlacementMode.Bottom,
            HorizontalOffset = (anchor.ActualWidth - width) / 2 - Shadow,
            VerticalOffset = -Shadow + 2,
            AllowsTransparency = true,
            StaysOpen = false,
            Focusable = true,
        };
        pop.Opened += (_, _) => Open++;
        pop.Closed += (_, _) => Open--;
        sheet.PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Key.Escape) { e.Handled = true; pop.IsOpen = false; }
        };
        pop.IsOpen = true;
        return pop;
    }

    public static void Duration(FrameworkElement anchor, string subtitle, int minutes,
        Action<int> save, Action<int> start)
    {
        var s = new StackPanel();
        s.Children.Add(Ui.Text("Focus duration", 15, Ui.Ink, FontWeights.SemiBold));
        s.Children.Add(Ui.Text(subtitle, 12.5, Ui.InkDim).Margin(0, 3, 0, 12));

        var custom = new TextBox
        {
            Style = Ui.Style("OwlField"),
            Text = minutes.ToString(CultureInfo.CurrentCulture),
            Width = 44,
            TextAlignment = TextAlignment.Center,
        };
        AutomationProperties.SetAutomationId(custom, "CustomMinutes");
        AutomationProperties.SetName(custom, "Custom minutes");

        var presets = new UniformGrid { Rows = 1 };
        var group = "dur" + Guid.NewGuid().ToString("N");
        var radios = new List<(int, RadioButton)>();
        foreach (var m in new[] { 15, 25, 45, 60 })
        {
            var rb = new RadioButton { Style = Ui.Style("OwlSegmentInk"), Content = $"{m}m", GroupName = group, IsChecked = m == minutes };
            AutomationProperties.SetAutomationId(rb, $"Preset{m}");
            AutomationProperties.SetName(rb, $"{m} minutes");
            var captured = m;
            rb.Checked += (_, _) => { if (custom.Text != captured.ToString(CultureInfo.CurrentCulture)) custom.Text = captured.ToString(CultureInfo.CurrentCulture); };
            radios.Add((m, rb));
            presets.Children.Add(rb);
        }
        custom.TextChanged += (_, _) =>
        {
            var v = Parse(custom.Text);
            foreach (var (m, rb) in radios) rb.IsChecked = v == m;
        };
        s.Children.Add(new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(6), Padding = new Thickness(2), Child = presets });

        void Nudge(int by) => custom.Text = Math.Clamp((Parse(custom.Text) ?? minutes) + by, 1, 600).ToString(CultureInfo.CurrentCulture);
        var stepper = new StackPanel { Margin = new Thickness(6, 0, 0, 0) };
        var up = Ui.IconButton("\uE70E", "MinutesUp", "More minutes", () => Nudge(1), 8);
        var down = Ui.IconButton(Ui.IcChevronDown, "MinutesDown", "Fewer minutes", () => Nudge(-1), 8);
        up.Padding = down.Padding = new Thickness(3, 0, 3, 0);
        stepper.Children.Add(up);
        stepper.Children.Add(down);
        var box = new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(5), Padding = new Thickness(4, 3, 4, 3), Child = custom };
        var right = Ui.Row(box, Ui.Text("min", 12.5, Ui.InkDim).Margin(6, 0), stepper);
        var row = new DockPanel { Margin = new Thickness(0, 12, 0, 0) };
        DockPanel.SetDock(right, Dock.Right);
        row.Children.Add(right);
        row.Children.Add(Ui.Text("Custom", 13.5, Ui.Ink));
        s.Children.Add(row);

        s.Children.Add(Ui.Text("Your timer starts when you begin focusing.", 12, Ui.InkDim).Margin(0, 12));
        s.Children.Add(Ui.Rule().Margin(0, 12, 0, 12));

        Popup? pop = null;
        void Finish(Action<int> act)
        {
            if (Parse(custom.Text) is not { } v) { custom.Focus(); custom.SelectAll(); return; }
            pop!.IsOpen = false;
            act(v);
        }
        var bar = new DockPanel();
        var cancel = Ui.Button("OwlLightButton", "Cancel", "Cancel", "Cancel", () => pop!.IsOpen = false);
        var saveB = Ui.Button("OwlBlueButton", "Save", "Save", "Save", () => Finish(save));
        var startB = Ui.Button("OwlLightButton", "Start", "StartFocus", "Start", () => Finish(start));
        var rightButtons = Ui.Row(saveB, startB.Margin(8, 0));
        DockPanel.SetDock(rightButtons, Dock.Right);
        bar.Children.Add(rightButtons);
        bar.Children.Add(cancel);
        cancel.HorizontalAlignment = HorizontalAlignment.Left;
        s.Children.Add(bar);

        pop = Show(anchor, s);
        custom.KeyDown += (_, e) => { if (e.Key == Key.Enter) { e.Handled = true; Finish(save); } };
        custom.Focus();
        custom.SelectAll();
    }

    private static int? Parse(string text) =>
        int.TryParse(text.Trim(), NumberStyles.Integer, CultureInfo.CurrentCulture, out var v) && v is >= 1 and <= 600 ? v : null;

    public static void Reminder(FrameworkElement anchor, string subtitle, DateTime? current, Action<DateTime> save)
    {
        var now = DateTime.Now;
        var initial = current ?? new DateTime(now.Year, now.Month, now.Day, now.Hour, 0, 0).AddHours(1);
        var s = new StackPanel();
        s.Children.Add(Ui.Text("Remind me", 15, Ui.Ink, FontWeights.SemiBold));
        s.Children.Add(Ui.Text(subtitle, 12.5, Ui.InkDim).Margin(0, 3, 0, 12));

        TextBox Field(string text, string id)
        {
            var f = new TextBox { Style = Ui.Style("OwlField"), Text = text };
            AutomationProperties.SetAutomationId(f, id);
            AutomationProperties.SetName(f, id);
            return f;
        }
        var date = Field(initial.ToString("d MMM yyyy", CultureInfo.CurrentCulture), "ReminderDate");
        var time = Field(initial.ToString("t", CultureInfo.CurrentCulture), "ReminderTime");
        FrameworkElement Line(string label, TextBox box)
        {
            var d = new DockPanel { Margin = new Thickness(0, 0, 0, 8) };
            var b = new Border { Background = Ui.Wash, CornerRadius = new CornerRadius(5), Padding = new Thickness(8, 4, 8, 4), Width = 170, Child = box };
            DockPanel.SetDock(b, Dock.Right);
            d.Children.Add(b);
            d.Children.Add(Ui.Text(label, 13.5));
            return d;
        }
        s.Children.Add(Line("Date", date));
        s.Children.Add(Line("Time", time));
        var error = Ui.Text("", 12, Ui.Frozen(Color.FromRgb(0x9A, 0x16, 0x16)));
        s.Children.Add(error);
        s.Children.Add(Ui.Rule().Margin(0, 10, 0, 12));

        Popup? pop = null;
        void Commit()
        {
            if (!DateTime.TryParse($"{date.Text.Trim()} {time.Text.Trim()}", CultureInfo.CurrentCulture, DateTimeStyles.AllowWhiteSpaces, out var at))
            {
                error.Text = "That date and time could not be read.";
                return;
            }
            pop!.IsOpen = false;
            save(at);
        }
        var bar = new DockPanel();
        var saveB = Ui.Button("OwlBlueButton", "Save", "Save", "Save", Commit);
        DockPanel.SetDock(saveB, Dock.Right);
        bar.Children.Add(saveB);
        var cancel = Ui.Button("OwlLightButton", "Cancel", "Cancel", "Cancel", () => pop!.IsOpen = false);
        cancel.HorizontalAlignment = HorizontalAlignment.Left;
        bar.Children.Add(cancel);
        s.Children.Add(bar);

        pop = Show(anchor, s);
        foreach (var f in new[] { date, time })
            f.KeyDown += (_, e) => { if (e.Key == Key.Enter) { e.Handled = true; Commit(); } };
        time.Focus();
        time.SelectAll();
    }

    /// Menus count as open popovers too, so the notch does not fold away under them.
    public static void Track(ContextMenu menu)
    {
        menu.Opened += (_, _) => Open++;
        menu.Closed += (_, _) => Open--;
    }
}

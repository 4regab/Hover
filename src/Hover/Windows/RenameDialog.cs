using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace Hover.Windows;

/// A one-field prompt for naming a screenshot. WPF ships no input box,
/// and the rest of the app is dark and chromeless, so this is a small themed window
/// rather than a system dialog.
public sealed class RenameDialog : Window
{
    private readonly TextBox _field = new();

    private RenameDialog(Window? owner, string current, string prompt, string? hint)
    {
        Title = "Rename";
        Owner = owner;
        Width = 340;
        SizeToContent = SizeToContent.Height;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = owner is null
            ? WindowStartupLocation.CenterScreen
            : WindowStartupLocation.CenterOwner;
        Background = new SolidColorBrush(Color.FromRgb(0x1C, 0x1C, 0x1E));
        Foreground = Brushes.White;
        FontFamily = Hover.Owl.Ui.Font;

        var panel = new StackPanel { Margin = new Thickness(16) };
        panel.Children.Add(new TextBlock
        {
            Text = prompt,
            FontSize = 12,
            Foreground = Hover.Owl.Ui.InkDim,
            Margin = new Thickness(0, 0, 0, 6),
        });

        _field.Text = current;
        _field.FontSize = 13;
        _field.Padding = new Thickness(6, 4, 6, 4);
        _field.Background = new SolidColorBrush(Color.FromRgb(0x2C, 0x2C, 0x2E));
        _field.Foreground = Brushes.White;
        _field.CaretBrush = Brushes.White;
        _field.BorderThickness = new Thickness(0);
        panel.Children.Add(_field);

        if (hint is not null)
        {
            panel.Children.Add(new TextBlock
            {
                Text = hint,
                FontSize = 11,
                Foreground = Hover.Owl.Ui.InkFaint,
                TextWrapping = TextWrapping.Wrap,
                Margin = new Thickness(0, 6, 0, 0),
            });
        }

        var buttons = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            HorizontalAlignment = HorizontalAlignment.Right,
            Margin = new Thickness(0, 14, 0, 0),
        };
        var cancel = Button("Cancel", () => { DialogResult = false; Close(); });
        var ok = Button("Rename", () => { DialogResult = true; Close(); });
        buttons.Children.Add(cancel);
        buttons.Children.Add(ok);
        panel.Children.Add(buttons);

        Content = panel;

        PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Key.Escape) { DialogResult = false; Close(); }
            else if (e.Key == Key.Enter) { DialogResult = true; Close(); }
        };
        Loaded += (_, _) => { _field.Focus(); _field.SelectAll(); };
    }

    /// Show the prompt and return the entered name, or null if cancelled.
    public static string? Ask(Window? owner, string current,
                              string prompt = "New name", string? hint = null)
    {
        var dialog = new RenameDialog(owner, current, prompt, hint);
        return dialog.ShowDialog() == true ? dialog._field.Text : null;
    }

    private Button Button(string text, Action action)
    {
        var b = new Button
        {
            Content = text,
            MinWidth = 76,
            Margin = new Thickness(8, 0, 0, 0),
            Padding = new Thickness(10, 5, 10, 5),
            Background = new SolidColorBrush(Color.FromRgb(0x2C, 0x2C, 0x2E)),
            Foreground = Brushes.White,
            BorderThickness = new Thickness(0),
            Cursor = Cursors.Hand,
        };
        b.Click += (_, _) => action();
        return b;
    }
}

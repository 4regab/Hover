using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using Hover.Core;

namespace Hover.Windows;

/// Shown once, the first time Hover runs.
///
/// Hover has no main window and draws nothing until the pointer reaches a screen
/// edge, so a new install looks like nothing happened. This is the one screen that
/// says where the app went and how to reach it.
public sealed class WelcomeWindow : Window
{
    /// Shows the welcome, unless it has been seen. Returns without doing anything on
    /// every later run.
    public static void ShowOnce()
    {
        if (Settings.SeenWelcome) return;
        new WelcomeWindow().Show();
    }

    private WelcomeWindow()
    {
        Title = "Hover";
        Width = 460;
        SizeToContent = SizeToContent.Height;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        ShowInTaskbar = true;
        Background = new SolidColorBrush(Color.FromRgb(0x1E, 0x1E, 0x22));
        Foreground = Brushes.White;
        FontFamily = Ink.SystemFace;
        Theme.Apply(this);

        var body = new StackPanel { Margin = new Thickness(24, 20, 24, 20) };

        body.Children.Add(new TextBlock
        {
            Text = "Hover is running",
            FontSize = 19,
            FontWeight = FontWeights.SemiBold,
            Foreground = Brushes.White,
        });
        body.Children.Add(new TextBlock
        {
            Text = "There is no window to keep. Hover lives on two edges of your screen "
                 + "and stays hidden until you reach for it.",
            FontSize = 12.5,
            Foreground = NoteColor.Tint(Colors.White, 0.6),
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 6, 0, 0),
        });

        body.Children.Add(Step("Your notes — the right edge",
            "Push the pointer all the way into the right edge of the screen and hold it "
            + "there for a moment. Your notes fan out as tabs. Click one to open it."));
        body.Children.Add(Step("Your screenshots — the left edge",
            "The same push on the left edge opens the picture tray. Every snip you take "
            + "and every image you copy lands there. Drag one straight into a chat, a "
            + "folder or a website."));
        body.Children.Add(Step("Everything else — the tray icon",
            "Right-click Hover by the clock for a new note, All Notes, and Settings.",
            $"{Settings.ScNewNote} makes a note from anywhere."));

        body.Children.Add(new TextBlock
        {
            Text = "Holding at the very edge is deliberate: it keeps the panels out of the "
                 + "way of scrollbars. You can change the wait, or widen the edge, in Settings.",
            FontSize = 11,
            Foreground = NoteColor.Tint(Colors.White, 0.45),
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 14, 0, 0),
        });

        var got = new Button
        {
            Content = "Got it",
            Padding = new Thickness(18, 6, 18, 6),
            HorizontalAlignment = HorizontalAlignment.Right,
            Margin = new Thickness(0, 18, 0, 0),
            IsDefault = true,
            Cursor = Cursors.Hand,
        };
        got.Click += (_, _) => Close();
        body.Children.Add(got);

        Content = body;

        PreviewKeyDown += (_, e) => { if (e.Key == Key.Escape) Close(); };

        // Written on the way out, so a crash before this is dismissed shows it again
        // rather than losing the only explanation of how the app works.
        Closed += (_, _) =>
        {
            Settings.SeenWelcome = true;
            Settings.Flush();
        };
    }

    private static SizeToContent SizeToContentHeight() => SizeToContent.Height;

    /// `extra` is a second line rather than more of the first: WPF will break a line
    /// inside "Ctrl+Alt+N", and a shortcut split across two lines reads as gibberish.
    private static FrameworkElement Step(string title, string detail, string? extra = null)
    {
        var stack = new StackPanel { Margin = new Thickness(0, 16, 0, 0) };
        stack.Children.Add(new TextBlock
        {
            Text = title,
            FontSize = 13,
            FontWeight = FontWeights.SemiBold,
            Foreground = Brushes.White,
        });
        stack.Children.Add(new TextBlock
        {
            Text = detail,
            FontSize = 12,
            Foreground = NoteColor.Tint(Colors.White, 0.6),
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 3, 0, 0),
        });
        if (extra is not null)
        {
            stack.Children.Add(new TextBlock
            {
                Text = extra,
                FontSize = 12,
                Foreground = NoteColor.Tint(Colors.White, 0.6),
                Margin = new Thickness(0, 2, 0, 0),
            });
        }
        return stack;
    }
}

using Avalonia.Controls;
using Avalonia.Media;
using HoverAvalonia.Core;
using HoverAvalonia.Office;

namespace HoverAvalonia.Ui;

/// <summary>The chat drawer, driven by the simulated session state (no agent behind it). Layout/typography per the baseline screenshots.</summary>
public partial class ChatPanel : UserControl
{
    public ChatPanel() { InitializeComponent(); }

    public void SetSession(Session s)
    {
        var c = s.B.Color;
        var col = Color.FromRgb((byte)(Rgb.ToSrgb(c.R) * 255 + .5), (byte)(Rgb.ToSrgb(c.G) * 255 + .5), (byte)(Rgb.ToSrgb(c.B) * 255 + .5));
        ((Border)Avatar.Parent!).Background = new SolidColorBrush(col);
        ((Border)Mini.Parent!).Background = new SolidColorBrush(col);
        Who.Text = s.B.Name;
        Avatar.Text = Mini.Text = s.Tool[..1].ToUpperInvariant();
        bool done = s.Sim == SimState.Completed;
        Code.IsVisible = Changed.IsVisible = done;
        switch (s.Sim)
        {
            case SimState.Completed:
                Heading.Text = "Imports tidied"; Body.Text = "All 14 files now sort their imports."; Steps.Text = "2 read  ·  1 file edited  ·  1 run"; Meta.Text = "1 s · 0.09 credits"; break;
            case SimState.Working:
                Heading.Text = "Working…"; Body.Text = (s.Act ?? "Thinking") + " npm test…"; Steps.Text = "1 read  ·  1 run"; Meta.Text = ""; break;
            default:
                Heading.Text = "Ready"; Body.Text = "No task yet — this agent is idle."; Steps.Text = ""; Meta.Text = ""; break;
        }
    }
}

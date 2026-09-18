using System.Runtime.InteropServices;
using Avalonia.Input;
using Hover.Core;
using Hover.Interop;
using NUnit.Framework;

namespace Hover.Tests;

/// The Win32 layer WPF used to provide: an invisible message window, and the
/// key-to-Windows-code mapping global shortcuts are registered with.
public sealed class InteropTests
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr SendMessage(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool IsWindow(IntPtr hwnd);

    [TestCase(Key.A, 0x41u)]
    [TestCase(Key.Z, 0x5Au)]
    [TestCase(Key.N, 0x4Eu)]
    [TestCase(Key.S, 0x53u)]
    [TestCase(Key.D0, 0x30u)]
    [TestCase(Key.D9, 0x39u)]
    [TestCase(Key.F1, 0x70u)]
    [TestCase(Key.F12, 0x7Bu)]
    [TestCase(Key.NumPad5, 0x65u)]
    [TestCase(Key.Escape, 0x1Bu)]
    [TestCase(Key.Back, 0x08u)]
    [TestCase(Key.Space, 0x20u)]
    [TestCase(Key.Delete, 0x2Eu)]
    [TestCase(Key.Left, 0x25u)]
    [TestCase(Key.OemPeriod, 0xBEu)]
    [TestCase(Key.OemPlus, 0xBBu)]
    [TestCase(Key.OemMinus, 0xBDu)]
    [TestCase(Key.Add, 0x6Bu)]
    [TestCase(Key.Subtract, 0x6Du)]
    [TestCase(Key.None, 0u)]
    public void Keys_map_to_the_codes_Windows_expects(Key key, uint expected) =>
        Assert.That(Keys.VirtualKey(key), Is.EqualTo(expected));

    [Test]
    public void Every_key_a_default_shortcut_uses_has_a_code()
    {
        Shortcut[] defaults =
        {
            Settings.ScNewNote, Settings.ScAllNotes, Settings.ScArchive, Settings.ScSnip,
            Settings.ScArchiveNote, Settings.ScClose, Settings.ScFind, Settings.ScTask,
            Settings.ScPin, Settings.ScColour, Settings.ScDelete, Settings.ScBigger,
            Settings.ScSmaller,
        };
        Assert.Multiple(() =>
        {
            foreach (var s in defaults)
                Assert.That(Keys.VirtualKey(s.Key), Is.Not.Zero, $"{s} has no Windows key code");
        });
    }

    [Test]
    public void Message_window_is_real_and_delivers_messages()
    {
        var seen = new List<(int Msg, IntPtr WParam)>();
        using var window = new MessageWindow("HoverTest",
            (msg, wParam, _) => seen.Add((msg, wParam)), Win32.WM_HOTKEY);

        Assert.That(IsWindow(window.Handle), Is.True, "the window handle is not a window");

        // Sent, not posted: on the creating thread this calls the window procedure
        // straight away, so no message pump is needed to see the result.
        SendMessage(window.Handle, Win32.WM_HOTKEY, new IntPtr(7), IntPtr.Zero);

        Assert.That(seen, Is.EqualTo(new[] { (Win32.WM_HOTKEY, new IntPtr(7)) }));
    }

    [Test]
    public void Message_window_ignores_messages_it_was_not_asked_for()
    {
        var seen = 0;
        using var window = new MessageWindow("HoverTestFilter",
            (_, _, _) => seen++, Win32.WM_HOTKEY);

        SendMessage(window.Handle, Win32.WM_CLIPBOARDUPDATE, IntPtr.Zero, IntPtr.Zero);
        Assert.That(seen, Is.Zero);

        SendMessage(window.Handle, Win32.WM_HOTKEY, IntPtr.Zero, IntPtr.Zero);
        Assert.That(seen, Is.EqualTo(1));
    }

    [Test]
    public void Message_window_survives_a_handler_that_throws()
    {
        using var window = new MessageWindow("HoverTestThrow",
            (_, _, _) => throw new InvalidOperationException("boom"), Win32.WM_HOTKEY);

        Assert.DoesNotThrow(() =>
            SendMessage(window.Handle, Win32.WM_HOTKEY, IntPtr.Zero, IntPtr.Zero));
        Assert.That(IsWindow(window.Handle), Is.True, "the window died with the handler");
    }

    [Test]
    public void Registered_hotkey_runs_its_action_when_Windows_reports_it()
    {
        using var keys = new HotKeys();
        var fired = 0;
        var registered = keys.Register(new Shortcut(KeyModifiers.Control | KeyModifiers.Alt, Key.F13),
            () => fired++);

        Assert.That(registered, Is.True, "Windows refused the binding");

        // Ids are handed out from 1, so the first registration is 1.
        SendMessage(HandleOf(keys), Win32.WM_HOTKEY, new IntPtr(1), IntPtr.Zero);
        Assert.That(fired, Is.EqualTo(1));
    }

    [Test]
    public void Clearing_hotkeys_releases_them_so_they_can_be_registered_again()
    {
        using var keys = new HotKeys();
        var shortcut = new Shortcut(KeyModifiers.Control | KeyModifiers.Alt, Key.F14);

        Assert.That(keys.Register(shortcut, () => { }), Is.True, "first registration failed");
        keys.Clear();
        Assert.That(keys.Register(shortcut, () => { }), Is.True,
            "the key was still held after Clear");
    }

    /// HotKeys keeps its window private; the tests reach it the same way a debugger
    /// would rather than widening the class's surface for their benefit.
    private static IntPtr HandleOf(HotKeys keys)
    {
        var field = typeof(HotKeys).GetField("_window",
            System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic);
        var window = (MessageWindow)field!.GetValue(keys)!;
        return window.Handle;
    }
}


/// Screen geometry, which every panel's placement is computed from.
public sealed class ScreenTests
{
    [Test]
    public void Displays_are_found_and_measured()
    {
        var all = Screens.All();
        Assert.That(all, Is.Not.Empty, "no displays found");

        var first = all[0];
        Assert.Multiple(() =>
        {
            Assert.That(first.Bounds.Width, Is.GreaterThan(0));
            Assert.That(first.Bounds.Height, Is.GreaterThan(0));
            Assert.That(first.Scale, Is.GreaterThan(0));
            Assert.That(first.Device, Is.Not.Empty);
            // The work area is the screen minus the taskbar, so it never sticks out.
            Assert.That(first.Work.Width, Is.LessThanOrEqualTo(first.Bounds.Width));
            Assert.That(first.Work.Height, Is.LessThanOrEqualTo(first.Bounds.Height));
        });
    }

    [Test]
    public void Measurements_convert_to_the_units_layout_uses()
    {
        var s = Screens.All()[0];
        Assert.Multiple(() =>
        {
            Assert.That(s.BoundsDips.Width, Is.EqualTo(s.Bounds.Width / s.Scale).Within(0.001));
            Assert.That(s.WorkDips.Height, Is.EqualTo(s.Work.Height / s.Scale).Within(0.001));
        });
    }

    [Test]
    public void A_single_display_has_no_neighbours_to_either_side()
    {
        var all = Screens.All();
        if (all.Count > 1) Assert.Ignore("needs a single-display machine");
        Assert.Multiple(() =>
        {
            Assert.That(all[0].NeighbourLeft, Is.False);
            Assert.That(all[0].NeighbourRight, Is.False);
        });
    }
}

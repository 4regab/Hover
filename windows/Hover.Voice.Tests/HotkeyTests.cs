using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Hover.Host.Protocol;
using Hover.Notch;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Voice.Tests;

/// <summary>What the host sent to the backend, as lines, in order.</summary>
sealed class Wire
{
    readonly List<string> lines = [];

    public void Send(BackendCommand command)
    {
        lock (lines) lines.Add(Encoding.UTF8.GetString(ProtocolJson.Serialize(command)));
    }

    public List<string> Types
    {
        get
        {
            lock (lines) return lines.Select(l => JsonDocument.Parse(l).RootElement.GetProperty("type").GetString()!).ToList();
        }
    }

    public async Task<bool> WaitFor(string type, int count = 1, int milliseconds = 4000)
    {
        for (var waited = 0; waited < milliseconds; waited += 10)
        {
            if (Types.Count(t => t == type) >= count) return true;
            await Task.Delay(10);
        }
        return false;
    }
}

/// <summary>
/// The hold-to-talk shortcut, driven as a person drives it: the keys are pressed with keybd_event, the
/// chord is taken by RegisterHotKey and its release is read by GetAsyncKeyState. Windows only; a
/// desktop session (the runner has one) is needed.
/// </summary>
public class HotkeyTests(ITestOutputHelper output)
{
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, nuint extra);

    const byte Ctrl = 0x11, Alt = 0x12, Space = 0x20;

    static void Down(byte vk) => keybd_event(vk, 0, 0, 0);

    static void Up(byte vk) => keybd_event(vk, 0, 2, 0);

    /// <summary>Lets go of every key the test may have left down, whatever happened.</summary>
    static void LetGo()
    {
        Up(Space);
        Up(Alt);
        Up(Ctrl);
    }

    static SettingsEvent Settings(bool hold) => new()
    {
        Model = JsonDocument.Parse("""{"Voice":{"Enabled":true,"HoldToTalk":""" + (hold ? "true" : "false") + "}}").RootElement,
    };

    static async Task<bool> Until(Func<bool> test, int milliseconds = 4000)
    {
        for (var waited = 0; waited < milliseconds; waited += 10)
        {
            if (test()) return true;
            await Task.Delay(10);
        }
        return test();
    }

    [WindowsFact]
    public async Task Hold_mode_the_chord_presses_and_letting_go_of_the_key_releases()
    {
        using var key = VoiceHotkey.Register(VoiceSettings.DefaultShortcut, out var error);
        Assert.True(key is not null, error);
        var wire = new Wire();
        var mic = new FakeMic();
        using var service = new VoiceService(wire.Send, mic.Open) { PollEvery = TimeSpan.FromMilliseconds(15) };
        service.OnBackend(Settings(hold: true));
        var pressed = 0;
        key!.Pressed += () =>
        {
            Interlocked.Increment(ref pressed);
            service.PressFromKeys(key.IsDown);
        };
        try
        {
            Assert.False(key.IsDown());
            Down(Ctrl);
            Down(Alt);
            Down(Space);
            Assert.True(await Until(() => pressed == 1), "the chord did not arrive as a hotkey");
            Assert.True(await wire.WaitFor("voicePress"));
            Assert.True(key.IsDown(), "the chord is held");
            await service.FlushAsync();
            Assert.True(mic.IsOpen, "the microphone opens with the press");
            foreach (var block in FakeMic.Tone(0.3)) mic.Hear(block);
            // Held: no release yet.
            await Task.Delay(200);
            Assert.DoesNotContain("voiceRelease", wire.Types);
            Up(Space);
            Assert.True(await wire.WaitFor("voiceRelease"), "letting go of the key did not release");
            await service.FlushAsync();
            Assert.False(mic.IsOpen);
            var types = wire.Types;
            output.WriteLine(string.Join(" ", types.Distinct()));
            Assert.Equal("voicePress", types[0]);
            Assert.Equal("voiceRelease", types[^1]);
            Assert.Contains("voiceAudio", types);
            Assert.Equal(1, types.Count(t => t == "voiceRelease"));
            Assert.Equal(1, pressed);
        }
        finally { LetGo(); }
    }

    [WindowsFact]
    public async Task Letting_go_of_a_modifier_first_also_releases()
    {
        using var key = VoiceHotkey.Register(VoiceSettings.DefaultShortcut, out var error);
        Assert.True(key is not null, error);
        var wire = new Wire();
        using var service = new VoiceService(wire.Send, new FakeMic().Open) { PollEvery = TimeSpan.FromMilliseconds(15) };
        service.OnBackend(Settings(hold: true));
        key!.Pressed += () => service.PressFromKeys(key.IsDown);
        try
        {
            Down(Ctrl);
            Down(Alt);
            Down(Space);
            Assert.True(await wire.WaitFor("voicePress"));
            Up(Alt);
            Assert.True(await wire.WaitFor("voiceRelease"), "letting go of Alt did not release");
        }
        finally { LetGo(); }
    }

    [WindowsFact]
    public async Task Toggle_mode_the_second_press_ends_it_and_letting_go_does_nothing()
    {
        using var key = VoiceHotkey.Register(VoiceSettings.DefaultShortcut, out var error);
        Assert.True(key is not null, error);
        var wire = new Wire();
        var mic = new FakeMic();
        using var service = new VoiceService(wire.Send, mic.Open) { PollEvery = TimeSpan.FromMilliseconds(15) };
        service.OnBackend(Settings(hold: false));
        var results = new List<PressResult>();
        key!.Pressed += () =>
        {
            var r = service.PressFromKeys(key.IsDown);
            lock (results) results.Add(r);
        };
        try
        {
            Down(Ctrl); Down(Alt); Down(Space);
            Assert.True(await wire.WaitFor("voicePress"));
            Up(Space); Up(Alt); Up(Ctrl);
            await Task.Delay(300);
            Assert.DoesNotContain("voiceRelease", wire.Types);
            await service.FlushAsync();
            Assert.True(mic.IsOpen, "the microphone keeps recording after the keys come up");

            Down(Ctrl); Down(Alt); Down(Space);
            Assert.True(await wire.WaitFor("voiceRelease"), "the second press did not end the recording");
            Up(Space); Up(Alt); Up(Ctrl);
            await service.FlushAsync();
            Assert.False(mic.IsOpen);
            lock (results) Assert.Equal([PressResult.Started, PressResult.Ended], results);
        }
        finally { LetGo(); }
    }

    [WindowsFact]
    public void A_chord_that_is_already_taken_says_so_in_the_go_apps_words()
    {
        using var first = VoiceHotkey.Register(VoiceSettings.DefaultShortcut, out var error);
        Assert.True(first is not null, error);
        using var second = VoiceHotkey.Register(VoiceSettings.DefaultShortcut, out var why);
        Assert.Null(second);
        Assert.Equal("Ctrl+Alt+Space is already in use by another app, by Windows or by Hover’s own shortcut.", why);
    }

    [WindowsFact]
    public void A_key_windows_cannot_register_is_said_before_anything_is_tried()
    {
        using var key = VoiceHotkey.Register(new Shortcut(1, Shortcut.Control), out var why);
        Assert.Null(key);
        Assert.Equal("Ctrl+Cancel has no key Windows can register.", why);
    }

    [WindowsFact]
    public void No_shortcut_is_no_hotkey_and_no_error()
    {
        using var key = VoiceHotkey.Register(new Shortcut(0, 0), out var why);
        Assert.Null(key);
        Assert.Null(why);
    }

    [WindowsFact]
    public void The_chord_is_free_again_once_it_is_let_go()
    {
        var first = VoiceHotkey.Register(VoiceSettings.DefaultShortcut, out var error);
        Assert.True(first is not null, error);
        first!.Dispose();
        using var again = VoiceHotkey.Register(VoiceSettings.DefaultShortcut, out var why);
        Assert.True(again is not null, why);
    }

    [Fact]
    public async Task Without_windows_the_service_still_presses_and_releases_through_the_wire()
    {
        // The key itself is Windows; the service's side of it is the same everywhere.
        var wire = new Wire();
        var mic = new FakeMic();
        using var service = new VoiceService(wire.Send, mic.Open) { PollEvery = TimeSpan.FromMilliseconds(10) };
        service.OnBackend(Settings(hold: true));
        var down = true;
        Assert.Equal(PressResult.Started, service.PressFromKeys(() => down));
        Assert.True(await wire.WaitFor("voicePress"));
        await service.FlushAsync();
        Assert.True(mic.IsOpen);
        foreach (var block in FakeMic.Tone(0.2)) mic.Hear(block);
        // A second press of the same key hold is not a new press.
        Assert.Equal(PressResult.Busy, service.PressFromKeys(() => down));
        down = false;
        Assert.True(await wire.WaitFor("voiceRelease"));
        await service.FlushAsync();
        Assert.False(mic.IsOpen);
        var types = wire.Types;
        Assert.Equal("voicePress", types[0]);
        Assert.Equal("voiceRelease", types[^1]);
        Assert.Equal(1, types.Count(t => t == "voicePress"));
    }
}

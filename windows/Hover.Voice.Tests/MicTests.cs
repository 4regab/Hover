using Xunit;
using Xunit.Abstractions;

namespace Hover.Voice.Tests;

/// <summary>
/// Choosing the microphone by the name in settings.json, and the words for a microphone that is not
/// there (the Go app's own, internal/voice/audio_windows.go).
/// </summary>
public class MicTests(ITestOutputHelper output)
{
    // winmm cuts a device's name at 31 characters.
    const string Long = "Microphone (Realtek High Definition Audio)";
    static readonly string Cut = Long[..MicDevices.NameLimit];

    [Fact]
    public void A_name_is_cut_at_thirty_one_characters_as_winmm_cuts_it()
    {
        Assert.Equal(31, Cut.Length);
        Assert.Equal("Microphone (Realtek High Defini", Cut);
    }

    [Fact]
    public void The_same_name_picks_its_device()
    {
        string[] names = ["Headset Microphone (USB)", "Line In", Cut];
        Assert.Equal(1, MicDevices.Pick(names, "Line In"));
        Assert.Equal(0, MicDevices.Pick(names, "Headset Microphone (USB)"));
        Assert.Equal(2, MicDevices.Pick(names, Cut));
    }

    [Fact]
    public void A_name_over_thirty_one_characters_is_matched_by_its_start()
    {
        string[] names = ["Line In", Cut];
        Assert.Equal(1, MicDevices.Pick(names, Long));
        // The whole stored name, any longer, still starts with the device's cut name.
        Assert.Equal(1, MicDevices.Pick(names, Long + " (2)"));
        // A name that differs inside the first thirty-one characters is another device.
        Assert.Equal(-1, MicDevices.Pick(names, "Microphone (Realtek High Definu" + "tion Audio)"));
    }

    [Fact]
    public void A_short_name_is_never_matched_by_its_start()
    {
        // Only a name winmm has cut can be a start of something longer.
        string[] names = ["Mic", "Headset"];
        Assert.Equal(-1, MicDevices.Pick(names, "Mic (USB)"));
        Assert.Equal(-1, MicDevices.Pick(names, "Headset Microphone"));
        Assert.Equal(-1, MicDevices.Pick(names, ""));
    }

    [Fact]
    public void The_first_device_wins_and_devices_that_did_not_answer_never_match()
    {
        string[] names = ["", Cut, Cut];
        Assert.Equal(1, MicDevices.Pick(names, Long));
        Assert.Equal(-1, MicDevices.Pick([], "anything"));
        Assert.Equal(-1, MicDevices.Pick([""], ""));
    }

    [Fact]
    public void The_words_for_a_microphone_that_is_not_there_are_the_go_apps()
    {
        Assert.Equal("No microphone is connected.", MicDevices.NoneConnected);
        Assert.Equal("The microphone “Studio Mic” isn’t connected. Pick another in Settings → Voice.", MicDevices.NotConnected("Studio Mic"));
        Assert.Equal("Another app is using the microphone.", MicDevices.OpenError(4));
        foreach (var code in new uint[] { 2, 6, 7 }) Assert.Equal("The microphone isn’t available any more.", MicDevices.OpenError(code));
        Assert.Equal("The microphone failed: error 32", MicDevices.OpenError(32));
    }

    [Fact]
    public void With_no_device_at_all_the_default_microphone_says_none_is_connected()
    {
        var names = MicDevices.RawNames();
        output.WriteLine($"{names.Count} input devices on this machine: {string.Join(" | ", names)}");
        var blocks = 0;
        var open = MicDevices.Open(null, _ => blocks++, _ => { });
        try
        {
            if (names.Count == 0)
            {
                Assert.Null(open.Mic);
                Assert.Equal("No microphone is connected.", open.Error);
            }
            else Assert.True(open.Mic is not null || open.Error is not null);
        }
        finally { open.Mic?.Dispose(); }
    }

    [WindowsFact]
    public void A_name_that_is_not_connected_is_an_error_even_when_it_is_over_thirty_one_characters()
    {
        var wanted = "Hover Test Microphone That Does Not Exist, Over Thirty-One Characters Long";
        Assert.True(wanted.Length > 31);
        var open = MicDevices.Open(wanted, _ => { }, _ => { });
        try
        {
            Assert.Null(open.Mic);
            Assert.Equal(MicDevices.NotConnected(wanted), open.Error);
        }
        finally { open.Mic?.Dispose(); }
    }

    [WindowsFact]
    public void The_real_device_list_is_read_without_a_crash_and_matches_itself()
    {
        var names = MicDevices.RawNames();
        foreach (var name in names.Where(n => n.Length > 0))
        {
            Assert.True(name.Length <= MicDevices.NameLimit, name);
            Assert.True(MicDevices.Pick(names, name) >= 0, name);
        }
        Assert.Equal(names.Where(n => n.Length > 0).Distinct().ToList(), MicDevices.Names());
    }
}

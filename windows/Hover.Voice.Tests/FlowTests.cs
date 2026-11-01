using System.Text.Json.Nodes;
using Hover.Host.Protocol;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Voice.Tests;

/// <summary>
/// The voice flow through the real hover-backend: the host's microphone is a fake that hands over
/// blocks, Groq is a fake on the loopback, and the agent is the stand-in. What the host sends and
/// what the backend answers are the protocol of docs/development/architecture.md.
/// </summary>
public class FlowTests(ITestOutputHelper output)
{
    static async Task Speak(Rig rig, double seconds)
    {
        // The microphone is opened on the service's own thread after the press.
        await rig.Service.FlushAsync();
        Assert.True(rig.Mic.IsOpen, "the microphone was not opened by the press");
        foreach (var block in FakeMic.Tone(seconds)) rig.Mic.Hear(block);
    }

    [Fact]
    public async Task Press_audio_release_heard_preview_countdown_and_the_task_starts_on_the_agent()
    {
        await using var rig = await Rig.Start(new JsonObject { ["HoldToTalk"] = true });
        rig.Service.StartedFor = TimeSpan.FromMilliseconds(100);
        var before = rig.Mark;

        Assert.Equal(PressResult.Started, rig.Service.Press());
        await rig.Stage(VoiceStage.Recording, before);
        await Speak(rig, 1);
        // The level the notch shows is the backend's own, from the audio sent.
        var loud = await rig.Wait(s => s.Stage == VoiceStage.Recording && s.Secs >= 0.9 && s.Level > 0, before);
        output.WriteLine($"recording: {loud.Secs:0.0} s, level {loud.Level:0.00}");
        rig.Service.Release();

        // Heard, then the preview with its countdown (1 s in these settings), then the task starts.
        var preview = await rig.Wait(s => s.Stage is VoiceStage.Preview or VoiceStage.Editing && s.Preview is not null, before);
        Assert.Equal("fix the build", preview.Preview!.Heard);
        Assert.Equal("fix the build", preview.Preview.Task);
        Assert.Equal("codex", preview.Preview.Tool);
        Assert.Equal("workspace", Path.GetFileName(preview.Preview.Folder.TrimEnd('\\', '/')));
        Assert.True(preview.Preview.Counting, "the preview should count down");
        Assert.Equal(1, preview.CountdownTotal);
        Assert.Contains("codex", preview.ReadyTools);

        var started = await rig.Stage(VoiceStage.Started, before);
        Assert.True(started.Session > 0);
        var session = await rig.Session(started.Session);
        Assert.Equal("codex", session.Tool);
        Assert.Equal("fix the build", session.Turns[0].Prompt);

        // The microphone was let go at the release, and Groq was sent a WAV of the whole recording.
        Assert.False(rig.Mic.IsOpen);
        Assert.Equal("Bearer " + Rig.FakeKey, rig.Groq.Auth);
        Assert.True(rig.Groq.Riff, "Groq should be sent a RIFF/WAVE file");
        Assert.Equal(44 + 2 * 16_000, rig.Groq.WavBytes);

        // The started card closes by itself.
        await rig.Wait(s => s.Stage == VoiceStage.Idle, before, 20);
        // The key is in no message the backend sent.
        Assert.DoesNotContain(rig.Others.OfType<UnknownEvent>(), u => u.Raw.ToString().Contains("gsk_fake", StringComparison.Ordinal));
    }

    [Fact]
    public async Task Toggle_mode_a_second_press_ends_the_recording()
    {
        await using var rig = await Rig.Start();
        var before = rig.Mark;
        Assert.False(rig.Service.Settings.Hold);
        Assert.Equal(PressResult.Started, rig.Service.Press());
        await rig.Stage(VoiceStage.Recording, before);
        await Speak(rig, 0.6);
        Assert.Equal(PressResult.Ended, rig.Service.Press());
        await rig.Wait(s => s.Stage is VoiceStage.Preview or VoiceStage.Editing, before);
        await rig.Service.FlushAsync();
        Assert.False(rig.Mic.IsOpen);
        // Letting go of the key does nothing in toggle mode.
        rig.Service.Released();
        rig.Service.Cancel();
        await rig.Stage(VoiceStage.Cancelled, before);
    }

    [Fact]
    public async Task Esc_during_the_countdown_cancels_and_nothing_starts()
    {
        await using var rig = await Rig.Start(new JsonObject { ["Countdown"] = 10, ["HoldToTalk"] = true });
        rig.Service.CancelledFor = TimeSpan.FromMilliseconds(100);
        var before = rig.Mark;
        rig.Service.Press();
        await Speak(rig, 0.8);
        rig.Service.Release();
        var preview = await rig.Wait(s => s.Stage == VoiceStage.Preview && s.Preview!.Counting, before);
        Assert.True(preview.Preview!.Countdown > 5, "the countdown should have time left");

        rig.Service.Cancel();
        await rig.Stage(VoiceStage.Cancelled, before);
        await rig.Wait(s => s.Stage == VoiceStage.Idle, before, 20);
        await Task.Delay(300);
        Assert.DoesNotContain(rig.Tasks, t => t.Sessions.Count > 0);
        Assert.DoesNotContain(rig.States, s => s.Stage == VoiceStage.Started);
    }

    [Fact]
    public async Task Release_before_any_audio_is_nothing_heard_and_the_device_goes()
    {
        await using var rig = await Rig.Start(new JsonObject { ["HoldToTalk"] = true });
        var before = rig.Mark;
        rig.Service.Press();
        rig.Service.Release();
        var error = await rig.Stage(VoiceStage.Error, before);
        Assert.Contains("Nothing was heard", error.Message);
        Assert.True(error.Retry);
        await rig.Service.FlushAsync();
        Assert.False(rig.Mic.IsOpen);
        Assert.Equal(0, rig.Groq.Transcriptions);
    }

    [Fact]
    public async Task Groq_refusing_the_key_says_so_and_the_key_is_not_shown()
    {
        await using var rig = await Rig.Start(new JsonObject { ["HoldToTalk"] = true });
        rig.Groq.Status = 401;
        var before = rig.Mark;
        rig.Service.Press();
        await Speak(rig, 0.5);
        rig.Service.Release();
        var error = await rig.Stage(VoiceStage.Error, before);
        Assert.Equal("Groq didn’t accept the key. Check it in Settings → Voice.", error.Message);
        Assert.False(error.Retry, "a refused key is not retried");
        Assert.DoesNotContain("gsk_fake", error.Message);
        Assert.Equal(1, rig.Groq.Transcriptions);
    }

    [Fact]
    public async Task No_key_at_all_asks_for_one()
    {
        await using var rig = await Rig.Start(new JsonObject { ["HoldToTalk"] = true }, key: false);
        var before = rig.Mark;
        rig.Service.Press();
        await Speak(rig, 0.5);
        rig.Service.Release();
        var error = await rig.Stage(VoiceStage.Error, before);
        output.WriteLine(error.Message);
        Assert.Contains("Groq", error.Message);
        Assert.Equal(0, rig.Groq.Transcriptions);
    }

    [Fact]
    public async Task A_missing_microphone_is_sent_as_the_error_and_the_card_says_it()
    {
        await using var rig = await Rig.Start(new JsonObject { ["HoldToTalk"] = true });
        rig.Mic.Error = MicDevices.NoneConnected;
        var before = rig.Mark;
        rig.Service.Press();
        var error = await rig.Stage(VoiceStage.Error, before);
        Assert.Equal("No microphone is connected.", error.Message);
        Assert.True(error.Retry);
        Assert.False(rig.Mic.IsOpen);
        // And pressing again once it is plugged in works.
        rig.Mic.Error = null;
        rig.Service.Send(Hover.Voice.VoiceCommands.Dismiss);
        await rig.Wait(s => s.Stage == VoiceStage.Idle, before);
        var again = rig.Mark;
        Assert.Equal(PressResult.Started, rig.Service.Press());
        await rig.Stage(VoiceStage.Recording, again);
        await Speak(rig, 0.3);
        rig.Service.Release();
        await rig.Wait(s => s.Stage is VoiceStage.Preview or VoiceStage.Editing, again);
    }

    [Fact]
    public async Task A_microphone_that_fails_while_recording_ends_the_recording_with_its_words()
    {
        await using var rig = await Rig.Start();
        var before = rig.Mark;
        rig.Service.Press();
        await Speak(rig, 0.3);
        rig.Mic.Break("The microphone isn’t available any more.");
        var error = await rig.Stage(VoiceStage.Error, before);
        Assert.Equal("The microphone isn’t available any more.", error.Message);
        // The backend ended the recording, so the device is let go without a release.
        await rig.Service.FlushAsync();
        Assert.False(rig.Mic.IsOpen);
    }

    [Fact]
    public async Task A_press_while_one_is_in_progress_is_turned_away_and_asks_the_card_to_glow()
    {
        await using var rig = await Rig.Start(new JsonObject { ["Countdown"] = 10, ["HoldToTalk"] = true });
        var busy = 0;
        rig.Service.Busy += () => Interlocked.Increment(ref busy);
        var before = rig.Mark;
        rig.Service.Press();
        await Speak(rig, 0.5);
        rig.Service.Release();
        await rig.Wait(s => s.Stage == VoiceStage.Preview, before);
        Assert.Equal(PressResult.Busy, rig.Service.Press());
        Assert.Equal(1, busy);
        // The countdown was not started again, and no second microphone was opened.
        Assert.Single(rig.Mic.Asked);
    }

    [Fact]
    public async Task Dictation_hands_the_words_over_and_dismisses_the_interaction()
    {
        await using var rig = await Rig.Start(new JsonObject { ["HoldToTalk"] = true });
        rig.Groq.Text = "and check monitor 3 too";
        string? heard = null;
        rig.Service.Dictated += v => heard = v.Text;
        var before = rig.Mark;
        Assert.Equal(PressResult.Started, rig.Service.Press(dictate: true));
        Assert.True(rig.Service.Dictating);
        await Speak(rig, 0.5);
        rig.Service.Release();
        await rig.Stage(VoiceStage.Dictated, before);
        Assert.Equal("and check monitor 3 too", heard);
        await rig.Wait(s => s.Stage == VoiceStage.Idle, before);
        Assert.False(rig.Service.Dictating);
        // Nothing was routed or started.
        Assert.DoesNotContain(rig.Tasks, t => t.Sessions.Count > 0);
    }

    [Fact]
    public async Task Phonon_that_is_not_installed_says_so_and_a_made_up_groq_key_is_not_accepted()
    {
        await using var rig = await Rig.Start();
        var phonon = new TaskCompletionSource<PhononState>(TaskCreationOptions.RunContinuationsAsynchronously);
        rig.Service.PhononChanged += p => phonon.TrySetResult(p);
        rig.Service.Send(Hover.Voice.VoiceCommands.Phonon());
        var state = await phonon.Task.WaitAsync(TimeSpan.FromSeconds(20));
        output.WriteLine($"phonon: {state.Kind} ready {state.Ready} {state.Message}");
        Assert.True(state.Ok);
        Assert.False(state.Ready);
        Assert.Contains(state.Kind, new[] { "notInstalled", "unsupported" });

        var checkedKey = new TaskCompletionSource<GroqCheck>(TaskCreationOptions.RunContinuationsAsynchronously);
        rig.Service.GroqChecked += g => checkedKey.TrySetResult(g);
        rig.Service.Send(Hover.Voice.VoiceCommands.GroqCheck);
        var said = await checkedKey.Task.WaitAsync(TimeSpan.FromSeconds(40));
        // The check goes to Groq itself (the loopback address only stands in for transcriptions), so a
        // made-up key is refused, or Groq cannot be reached from where the test runs. Either way the
        // words are the Go app's, and the key is not in them.
        output.WriteLine(said.Said);
        Assert.False(said.Ok);
        Assert.StartsWith("Groq", said.Said);
        Assert.DoesNotContain("gsk_fake", said.Said);
    }

    [Fact]
    public async Task The_groq_check_without_a_key_asks_for_one()
    {
        await using var rig = await Rig.Start(key: false);
        var done = new TaskCompletionSource<GroqCheck>(TaskCreationOptions.RunContinuationsAsynchronously);
        rig.Service.GroqChecked += g => done.TrySetResult(g);
        rig.Service.Send(Hover.Voice.VoiceCommands.GroqCheck);
        var said = await done.Task.WaitAsync(TimeSpan.FromSeconds(20));
        Assert.False(said.Ok);
        Assert.Equal("Add your Groq key first.", said.Said);
    }

    [Fact]
    public async Task The_card_can_edit_the_task_pick_the_folder_and_start_it_now()
    {
        await using var rig = await Rig.Start(new JsonObject { ["Countdown"] = 0, ["HoldToTalk"] = true });
        var before = rig.Mark;
        rig.Service.Press();
        await Speak(rig, 0.5);
        rig.Service.Release();
        // With no countdown the preview waits for Start.
        var waiting = await rig.Wait(s => s.Stage is VoiceStage.Preview or VoiceStage.Editing && s.CanStart, before);
        Assert.False(waiting.Preview!.Counting);
        rig.Service.Send(Hover.Voice.VoiceCommands.Edit("fix the build and run the tests"));
        await rig.Wait(s => s.Stage == VoiceStage.Editing && s.Preview!.Task == "fix the build and run the tests" && s.CanStart, before);
        rig.Service.Send(Hover.Voice.VoiceCommands.ChangeFolder(null));
        rig.Service.Send(Hover.Voice.VoiceCommands.StartNow);
        var started = await rig.Stage(VoiceStage.Started, before);
        var session = await rig.Session(started.Session);
        Assert.Equal("fix the build and run the tests", session.Turns[0].Prompt);
    }
}

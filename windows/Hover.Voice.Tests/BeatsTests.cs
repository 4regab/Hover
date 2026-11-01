using NVorbis;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Voice.Tests;

/// <summary>A sound output that is not there: it keeps the fill the music gives it, so a test can play it.</summary>
sealed class FakeSpeaker
{
    public Action<float[]>? Fill { get; private set; }
    public int Opened { get; private set; }
    public int Closed { get; private set; }
    public string? Error { get; set; }
    public bool IsOpen => Opened > Closed;

    public AudioOpen Open(Action<float[]> fill)
    {
        if (Error is not null) return new AudioOpen(null, Error);
        Opened++;
        Fill = fill;
        return new AudioOpen(new Stream(this), null);
    }

    /// <summary>Plays this many frames and returns the loudest sample.</summary>
    public float Play(int frames)
    {
        var peak = 0f;
        var buffer = new float[4410 * AudioOutput.Channels];
        for (var done = 0; done < frames; done += 4410)
        {
            Array.Clear(buffer);
            Fill!(buffer);
            foreach (var v in buffer)
            {
                Assert.True(float.IsFinite(v));
                peak = Math.Max(peak, Math.Abs(v));
            }
        }
        return peak;
    }

    sealed class Stream(FakeSpeaker owner) : ISoundSink
    {
        bool closed;

        public void Dispose()
        {
            if (closed) return;
            closed = true;
            owner.Closed++;
        }
    }
}

/// <summary>The office beats: the ramp the Go app uses, the loop decoded to sound, and when the output is open.</summary>
public class BeatsTests(ITestOutputHelper output)
{
    static Beats Make(FakeSpeaker speaker, bool on = false) => new(on, speaker.Open, Beats.LoopFile);

    static int Arrive(Beats beats)
    {
        var frames = 0;
        while (beats.Frame()) frames++;
        return frames + 1;
    }

    [Fact]
    public void The_ramp_goes_up_by_a_fiftieth_and_down_by_three_hundredths_as_the_go_ramp_does()
    {
        // The numbers internal/music's RampStep prints for the same calls.
        double[] up = [0.02, 0.040000000000000001, 0.059999999999999998, 0.080000000000000002, 0.10000000000000001, 0.12000000000000001, 0.14000000000000001, 0.16,
            0.17999999999999999, 0.19999999999999998, 0.21999999999999997, 0.23999999999999996, 0.25999999999999995, 0.27999999999999997, 0.29999999999999999, 0.32000000000000001];
        var v = 0.0;
        for (var i = 0; i < up.Length; i++)
        {
            bool arrived;
            (v, arrived) = Beats.RampStep(v, Beats.Full);
            Assert.Equal(up[i], v);
            Assert.Equal(i == up.Length - 1, arrived);
        }
        double[] down = [0.29000000000000004, 0.26000000000000001, 0.23000000000000001, 0.20000000000000001, 0.17000000000000001, 0.14000000000000001,
            0.11000000000000001, 0.080000000000000016, 0.050000000000000017, 0.020000000000000018, 0];
        for (var i = 0; i < down.Length; i++)
        {
            bool arrived;
            (v, arrived) = Beats.RampStep(v, 0);
            Assert.Equal(down[i], v);
            Assert.Equal(i == down.Length - 1, arrived);
        }
        Assert.Equal((0.07, false), Beats.RampStep(0.1, 0.1));
        Assert.Equal((0.17000000000000001, false), Beats.RampStep(0.2, 0));
    }

    [Fact]
    public void The_loop_is_the_go_apps_own_ogg_file_linked_into_the_assembly()
    {
        using var file = Beats.LoopFile();
        var head = new byte[4];
        file.ReadExactly(head);
        Assert.Equal("OggS", System.Text.Encoding.ASCII.GetString(head));
        Assert.True(file.Length > 1_000_000, $"the loop is {file.Length} bytes");
        file.Position = 0;
        using var reader = new VorbisReader(file, false);
        output.WriteLine($"the loop: {reader.TotalTime.TotalSeconds:0.0} s, {reader.SampleRate} Hz, {reader.Channels} channels");
        Assert.True(reader.TotalTime > TimeSpan.FromSeconds(5));
    }

    [Fact]
    public void Switched_on_it_opens_the_output_fades_in_to_a_third_and_is_music()
    {
        var speaker = new FakeSpeaker();
        using var beats = Make(speaker);
        Assert.False(beats.Playing);
        Assert.True(beats.Toggle(true));
        Assert.True(speaker.IsOpen);
        Assert.Equal(1, speaker.Opened);
        // Before the fade has started the stream is silent.
        Assert.Equal(0f, speaker.Play(4410));
        Assert.Equal(16, Arrive(beats));
        Assert.Equal(Beats.Full, beats.Volume);
        var peak = speaker.Play(AudioOutput.Rate * 3);
        output.WriteLine($"peak after the fade: {peak:0.000}");
        Assert.InRange(peak, 0.01f, (float)Beats.Full + 0.001f);
        Assert.True(speaker.IsOpen);
    }

    [Fact]
    public void The_loop_starts_again_at_its_end_and_never_goes_silent()
    {
        var speaker = new FakeSpeaker();
        using var beats = Make(speaker, on: true);
        // Started with the button on, the output opens when an office comes into view.
        beats.Follow(true);
        Assert.True(speaker.IsOpen);
        Arrive(beats);
        double seconds;
        using (var f = Beats.LoopFile())
        using (var r = new VorbisReader(f, false)) seconds = r.TotalTime.TotalSeconds;
        // The whole loop, and three seconds into its second time round.
        var whole = (int)((seconds + 3) * AudioOutput.Rate);
        var loudest = 0f;
        var buffer = new float[4410 * AudioOutput.Channels];
        var lastSecond = 0f;
        for (var done = 0; done < whole; done += 4410)
        {
            Array.Clear(buffer);
            speaker.Fill!(buffer);
            var peak = buffer.Max(Math.Abs);
            loudest = Math.Max(loudest, peak);
            if (done > whole - AudioOutput.Rate) lastSecond = Math.Max(lastSecond, peak);
        }
        output.WriteLine($"{seconds:0.0} s loop played and wrapped; loudest {loudest:0.000}, last second {lastSecond:0.000}");
        Assert.InRange(loudest, 0.05f, (float)Beats.Full + 0.001f);
        Assert.True(lastSecond > 0.001f, "the loop should be playing again after its end");
    }

    [Fact]
    public void Switched_off_it_fades_out_and_lets_the_device_go()
    {
        var speaker = new FakeSpeaker();
        using var beats = Make(speaker);
        beats.Toggle(true);
        Arrive(beats);
        Assert.True(beats.Toggle(false));
        // Down by 0.03 a frame: 0.32 takes 11 frames, and the output is open until it arrives.
        Assert.True(beats.Frame());
        Assert.True(speaker.IsOpen);
        Assert.Equal(10, Arrive(beats));
        Assert.Equal(0, beats.Volume);
        Assert.False(speaker.IsOpen);
        Assert.Equal(1, speaker.Closed);
        Assert.False(beats.Playing);
        Assert.False(beats.Frame());
    }

    [Fact]
    public void When_no_office_is_in_view_it_goes_quiet_and_comes_back_with_the_office()
    {
        var speaker = new FakeSpeaker();
        using var beats = Make(speaker);
        beats.Toggle(true);
        Arrive(beats);
        beats.Follow(false);
        Assert.True(beats.Want, "the button stays on while the office is hidden");
        Assert.Equal(0, beats.Target);
        Arrive(beats);
        Assert.False(speaker.IsOpen);
        Assert.Equal(0, beats.Volume);
        beats.Follow(true);
        Assert.True(speaker.IsOpen);
        Assert.Equal(2, speaker.Opened);
        Arrive(beats);
        Assert.Equal(Beats.Full, beats.Volume);
        Assert.True(speaker.Play(AudioOutput.Rate) > 0.001f);
    }

    [Fact]
    public void Switched_on_while_the_office_is_hidden_waits_without_opening_anything()
    {
        var speaker = new FakeSpeaker();
        using var beats = Make(speaker);
        beats.Follow(false);
        Assert.True(beats.Toggle(true));
        Assert.True(beats.Want);
        Assert.Equal(0, speaker.Opened);
        Assert.False(beats.Frame());
        beats.Follow(true);
        Assert.Equal(1, speaker.Opened);
        Assert.Equal(16, Arrive(beats));
    }

    [Fact]
    public void With_no_sound_output_the_button_goes_back_off_and_nothing_crashes()
    {
        var speaker = new FakeSpeaker { Error = "There is no sound output." };
        using var beats = Make(speaker);
        Assert.False(beats.Toggle(true));
        Assert.False(beats.Want);
        Assert.False(beats.Playing);
        Assert.False(beats.Frame());
        // And a later try, with the device back, works.
        speaker.Error = null;
        Assert.True(beats.Toggle(true));
        Assert.True(speaker.IsOpen);
    }

    [Fact]
    public void A_loop_that_cannot_be_read_does_not_crash_the_app_and_turns_the_button_off()
    {
        var speaker = new FakeSpeaker();
        using var beats = new Beats(false, speaker.Open, () => new MemoryStream([1, 2, 3, 4, 5, 6, 7, 8]));
        Assert.False(beats.Toggle(true));
        Assert.False(beats.Want);
        Assert.Equal(0, speaker.Opened);
    }

    [Fact]
    public void Disposing_while_it_plays_lets_the_device_go()
    {
        var speaker = new FakeSpeaker();
        var beats = Make(speaker);
        beats.Toggle(true);
        Arrive(beats);
        beats.Dispose();
        Assert.False(speaker.IsOpen);
    }

    [Fact]
    public void The_chime_is_two_quiet_notes_and_then_silence()
    {
        var fill = Chime.Fill();
        // 0.3 s in 100 ms buffers, as the stream asks for them.
        var buffer = new float[4410 * AudioOutput.Channels];
        var all = new List<float>();
        for (var i = 0; i < 3; i++)
        {
            Array.Clear(buffer);
            fill(buffer);
            all.AddRange(buffer);
        }
        var first = all.Take(AudioOutput.Rate * 2 * 18 / 100).ToArray();
        var rest = all.Skip(AudioOutput.Rate * 2 * 18 / 100 + 4).ToArray();
        Assert.InRange(first.Max(Math.Abs), 0.05f, 0.121f);
        Assert.All(rest, v => Assert.Equal(0f, v));
        // Mono in both channels.
        for (var i = 0; i < 1000; i += 2) Assert.Equal(all[i], all[i + 1]);
    }

    [Fact]
    public async Task The_chime_opens_the_output_and_lets_it_go()
    {
        var speaker = new FakeSpeaker();
        Chime.Play(speaker.Open);
        for (var i = 0; i < 100 && speaker.Closed == 0; i++) await Task.Delay(20);
        Assert.Equal(1, speaker.Opened);
        Assert.Equal(1, speaker.Closed);
    }

    [Fact]
    public async Task The_chime_with_no_output_does_nothing()
    {
        var speaker = new FakeSpeaker { Error = "There is no sound output." };
        Chime.Play(speaker.Open);
        await Task.Delay(100);
        Assert.Equal(0, speaker.Opened);
    }

    [WindowsFact]
    public void The_real_output_opens_or_says_why_not_and_never_crashes()
    {
        var done = new ManualResetEventSlim();
        var opened = AudioOutput.Open(_ => done.Set());
        try
        {
            output.WriteLine(opened.Output is null ? "no output here: " + opened.Error : "an output opened");
            Assert.True(opened.Output is not null || !string.IsNullOrEmpty(opened.Error));
            if (opened.Output is not null) Assert.True(done.Wait(3000), "the device never asked for sound");
        }
        finally { opened.Output?.Dispose(); }
    }
}

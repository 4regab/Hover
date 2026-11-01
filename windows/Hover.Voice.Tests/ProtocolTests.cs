using System.Text.Json;
using Hover.Host.Protocol;
using Hover.Notch;
using Xunit;

namespace Hover.Voice.Tests;

/// <summary>The voice messages: what the host sends, and that nothing the backend sends can hurt the host.</summary>
public class ProtocolTests
{
    static VoiceState? Parse(string json) => VoiceState.Parse(JsonDocument.Parse(json).RootElement);

    [Fact]
    public void A_voice_event_is_read_with_its_preview_and_lists()
    {
        var v = Parse("""
            {"type":"voice","stage":"preview","level":0.5,"secs":2.5,"pending":null,
             "preview":{"id":3,"heard":"fix it","cleanupNote":"","task":"Fix it","folder":"C:\\work\\Hover","targetName":"Hover","note":"","tool":"codex","model":"gpt","access":"risky",
               "countdown":2.1,"counting":true,"trial":false,"cloud":true,"repo":{"kind":"named","name":"4regab/Hover"}},
             "session":0,"folder":"","text":"","message":"","retry":false,"transcript":null,"countdownTotal":5,"canStart":true,"tool":"codex",
             "folderChoices":[{"id":"p1","name":"Hover","folder":"C:\\work\\Hover"}],"readyTools":["kiro","codex"],"shots":["a.png"],"shotCount":2,"shotMessage":"Screenshot attached"}
            """)!;
        Assert.Equal(VoiceStage.Preview, v.Stage);
        Assert.Equal(0.5, v.Level);
        Assert.Equal("Fix it", v.Preview!.Task);
        Assert.True(v.Preview.Counting);
        Assert.True(v.Preview.Cloud);
        Assert.Equal("named", v.Preview.RepoKind);
        Assert.Equal("4regab/Hover", v.Preview.RepoName);
        Assert.Equal(5, v.CountdownTotal);
        Assert.True(v.CanStart);
        Assert.Equal(["kiro", "codex"], v.ReadyTools);
        Assert.Equal("Hover", Assert.Single(v.FolderChoices).Name);
        Assert.Equal(2, v.ShotCount);
        Assert.False(v.Fresh);
    }

    [Fact]
    public void Every_stage_name_the_backend_sends_is_known()
    {
        string[] names = ["idle", "recording", "loading", "transcribing", "cleaning", "resolving", "chooseAgent", "preview", "editing", "starting", "started", "dictated", "cancelled", "error"];
        for (var i = 0; i < names.Length; i++) Assert.Equal((VoiceStage)i, Parse($$"""{"type":"voice","stage":"{{names[i]}}"}""")!.Stage);
    }

    [Fact]
    public void A_voice_event_with_wrong_kinds_is_read_as_defaults_and_never_throws()
    {
        // The stage is the only thing that must be right.
        Assert.Null(Parse("""{"stage":"dancing"}"""));
        Assert.Null(Parse("""{"stage":5}"""));
        Assert.Null(Parse("""{}"""));
        Assert.Null(VoiceState.Parse(JsonDocument.Parse("[1]").RootElement));
        var v = Parse("""
            {"stage":"error","level":"loud","secs":-4,"pending":5,"preview":"x","session":"1","folder":3,"message":["a"],"retry":"yes",
             "folderChoices":[1,{"id":4}],"readyTools":[1,"codex",null],"shots":{"a":1},"countdownTotal":1e99,"canStart":1,"tool":9}
            """)!;
        Assert.Equal(VoiceStage.Error, v.Stage);
        Assert.Equal(0, v.Level);
        Assert.Equal(0, v.Secs);
        Assert.Null(v.Pending);
        Assert.Null(v.Preview);
        Assert.Equal(0, v.Session);
        Assert.Equal("", v.Folder);
        Assert.Equal("", v.Message);
        Assert.False(v.Retry);
        Assert.Equal(["codex"], v.ReadyTools);
        Assert.Empty(v.Shots);
        Assert.Equal(3600, v.CountdownTotal);
        Assert.False(v.CanStart);
        Assert.Null(v.Tool);
        Assert.Equal("", Assert.Single(v.FolderChoices).Name);
    }

    [Fact]
    public void Long_text_and_long_lists_from_the_backend_are_cut()
    {
        var tools = string.Join(",", Enumerable.Range(0, 100).Select(i => $"\"t{i}\""));
        var v = Parse($$"""{"stage":"error","message":"{{new string('m', 50_000)}}","readyTools":[{{tools}}],"transcript":"{{new string('t', 50_000)}}"}""")!;
        Assert.Equal(2000, v.Message.Length);
        Assert.Equal(16, v.ReadyTools.Count);
        Assert.Equal(20_000, v.Transcript!.Length);
    }

    [Fact]
    public void A_message_of_another_type_is_not_a_voice_event()
    {
        Assert.Null(VoiceState.From(new ToastEvent { Text = "x" }));
        Assert.Null(VoiceState.From(new UnknownEvent("phonon", JsonDocument.Parse("""{"stage":"idle"}""").RootElement)));
        Assert.NotNull(VoiceState.From(new UnknownEvent("voice", JsonDocument.Parse("""{"stage":"idle"}""").RootElement)));
    }

    [Fact]
    public void Phonon_and_the_groq_check_are_read()
    {
        var p = PhononState.From(new UnknownEvent("phonon", JsonDocument.Parse("""{"ok":true,"ready":false,"state":{"kind":"downloading","done":10,"total":100,"message":""}}""").RootElement))!;
        Assert.Equal("downloading", p.Kind);
        Assert.Equal(10, p.Done);
        Assert.Equal(100, p.Total);
        Assert.False(p.Ready);
        var g = GroqCheck.From(new UnknownEvent("groqCheck", JsonDocument.Parse("""{"ok":false,"said":"Add your Groq key first."}""").RootElement))!;
        Assert.False(g.Ok);
        Assert.Equal("Add your Groq key first.", g.Said);
        Assert.Null(PhononState.From(new UnknownEvent("phonon", JsonDocument.Parse("5").RootElement)));
    }

    static string Line(BackendCommand c) => System.Text.Encoding.UTF8.GetString(ProtocolJson.Serialize(c));

    [Fact]
    public void The_commands_are_the_lines_the_backend_reads()
    {
        Assert.Equal("""{"type":"voicePress"}""", Line(VoiceCommands.Press()));
        Assert.Equal("""{"type":"voicePress","trial":true,"dictate":true,"activeProject":"p1"}""", Line(VoiceCommands.Press(true, true, "p1")));
        Assert.Equal("""{"type":"voiceRelease"}""", Line(VoiceCommands.Release));
        Assert.Equal("""{"type":"voiceEdit","task":"a \"b\""}""", Line(VoiceCommands.Edit("a \"b\"")));
        Assert.Equal("""{"type":"voiceChangeFolder","project":null}""", Line(VoiceCommands.ChangeFolder(null)));
        Assert.Equal("""{"type":"voiceChangeRepo","kind":"named","name":"4regab/Hover"}""", Line(VoiceCommands.ChangeRepo("named", "4regab/Hover")));
        Assert.Equal("""{"type":"voiceUnattach","index":1}""", Line(VoiceCommands.Unattach(1)));
        Assert.Equal("""{"type":"phonon","op":"download"}""", Line(VoiceCommands.Phonon("download")));
    }

    [Fact]
    public void Audio_is_sixteen_bit_little_endian_base64_and_a_big_block_is_cut_to_fit()
    {
        short[] samples = [0, 1, -1, 32767, -32768];
        var line = JsonDocument.Parse(Line(VoiceCommands.Audio(samples).Single())).RootElement;
        Assert.Equal("voiceAudio", line.GetProperty("type").GetString());
        Assert.Equal(new byte[] { 0, 0, 1, 0, 0xff, 0xff, 0xff, 0x7f, 0x00, 0x80 }, Convert.FromBase64String(line.GetProperty("pcm").GetString()!));

        // 100,000 samples are 200,000 bytes: two blocks, none over 128 KiB, in order, with nothing lost.
        var big = Enumerable.Range(0, 100_000).Select(i => (short)(i % 30_000)).ToArray();
        var parts = VoiceCommands.Audio(big).Select(c => Convert.FromBase64String(JsonDocument.Parse(Line(c)).RootElement.GetProperty("pcm").GetString()!)).ToList();
        Assert.Equal(2, parts.Count);
        Assert.All(parts, p => Assert.True(p.Length <= VoiceCommands.BlockMax && p.Length % 2 == 0));
        var joined = parts.SelectMany(p => p).ToArray();
        Assert.Equal(200_000, joined.Length);
        Assert.Equal(big[99_999], BitConverter.ToInt16(joined, 2 * 99_999));
        Assert.Empty(VoiceCommands.Audio([]));
    }

    [Fact]
    public void An_error_is_cut_to_what_the_backend_takes()
    {
        var line = JsonDocument.Parse(Line(VoiceCommands.AudioError(new string('e', 1000)))).RootElement;
        Assert.Equal(300, line.GetProperty("error").GetString()!.Length);
    }

    [Fact]
    public void The_voice_settings_are_read_from_the_model_as_the_go_app_reads_them()
    {
        var model = JsonDocument.Parse("""
            {"Voice":{"Enabled":true,"Shortcut":{"Key":"Space","Modifiers":"Control, Alt"},"Microphone":"Studio Mic","Speech":"Local","Countdown":3,"AuraColor":"#abc","HoldToTalk":true}}
            """).RootElement;
        var v = VoiceSettings.From(model);
        Assert.True(v.Enabled);
        Assert.Equal(new Shortcut(18, Shortcut.Control | Shortcut.Alt), v.Shortcut);
        Assert.Equal("Ctrl+Alt+Space", v.Shortcut.Label);
        Assert.Equal("Studio Mic", v.Microphone);
        Assert.True(v.Hold);
        Assert.Equal(3, v.Countdown);
        Assert.Equal("Local", v.Speech);
        Assert.Equal("#AABBCC", v.AuraColor);
        Assert.Equal((0xAA, 0xBB, 0xCC), v.AuraRgb());
    }

    [Fact]
    public void Missing_or_wrong_voice_settings_are_the_defaults_and_voice_is_off()
    {
        foreach (var json in new[] { "{}", """{"Voice":5}""", """{"Voice":{"Enabled":"yes","Shortcut":7,"Microphone":3,"Countdown":99,"AuraColor":"red","HoldToTalk":1,"Speech":4}}""", "[]" })
        {
            var v = VoiceSettings.From(JsonDocument.Parse(json).RootElement);
            Assert.False(v.Enabled);
            Assert.Equal(VoiceSettings.DefaultShortcut, v.Shortcut);
            Assert.Equal("Ctrl+Alt+Space", v.Shortcut.Label);
            Assert.Null(v.Microphone);
            Assert.False(v.Hold);
            Assert.Equal(5, v.Countdown);
            Assert.Equal("#1FD5F9", v.AuraColor);
            Assert.Equal("Cloud", v.Speech);
        }
    }
}

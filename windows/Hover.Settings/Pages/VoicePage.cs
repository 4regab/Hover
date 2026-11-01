using System.Globalization;

namespace Hover.Settings;

public static partial class PageBuilder
{
    public static readonly string[] SpeechModes = ["Local", "Cloud"];
    public static readonly string[] SpeechLabels = ["Local (Phonon)", "Cloud (Groq)"];
    public static readonly string[] CleanupProviders = ["Gemini", "OpenAI", "Custom"];
    public static readonly int[] VoiceCountdowns = [0, 3, 5, 10];

    /// <summary>Groq's transcription models: id and name.</summary>
    public static readonly (string Id, string Name)[] TranscribeModels = [("whisper-large-v3-turbo", "Whisper Large V3 Turbo"), ("whisper-large-v3", "Whisper Large V3")];

    /// <summary>The aura colours Settings offers; any other is typed in.</summary>
    public static readonly (string Name, string Hex)[] AuraColors =
        [("Cyan", "#1FD5F9"), ("Violet", "#C4A2FF"), ("Green", "#4ADE80"), ("Amber", "#FFB340"), ("Pink", "#FF6BD5"), ("White", "#F6F2FF")];

    public const string GroqSecret = "voice.groq";

    public static string CleanupSecret(string provider) => provider switch { "OpenAI" => "cleanup.openai", "Custom" => "cleanup.custom", _ => "cleanup.gemini" };

    /// <summary>1.5 MB, 164 MB, 2.1 GB: sizes as the cards show them.</summary>
    public static string Size(ulong bytes)
    {
        var mb = bytes / 1_000_000.0;
        if (mb >= 1000) return (mb / 1000).ToString("0.0", CultureInfo.InvariantCulture) + " GB";
        if (mb >= 10) return Math.Round(mb, MidpointRounding.ToEven).ToString("0", CultureInfo.InvariantCulture) + " MB";
        return mb.ToString("0.0", CultureInfo.InvariantCulture) + " MB";
    }

    static string SecsWords(int n) => $"{n} second{(n == 1 ? "" : "s")}";

    static Control Secret(string id, string name, string nameInStore, PageInput i)
    {
        var has = i.HasSecret(nameInStore);
        var placeholder = !has ? "Not set" : i.SecretsKept ? "Saved" : "Kept this run only";
        return new Control { Kind = ControlKind.Field, Id = id, Name = name, Placeholder = placeholder, Secret = true, On = has };
    }

    static string KeyNote(PageInput i, string text)
        => i.SecretsKept ? text : text + " Hover can’t keep keys on this computer right now, so one set now lasts until Hover quits.";

    static void Voice(List<Block> b, PageInput i)
    {
        var s = i.Settings;
        var v = s.Voice;
        var local = v.Local;
        b.Add(new Block
        {
            Kind = BlockKind.Lead,
            Text = v.Hold ? "Talk to Hover from anywhere. The notch listens while you hold the shortcut."
                : "Talk to Hover from anywhere. Press the shortcut to start listening, and press it again to finish.",
        });
        var mics = new List<Opt> { new("System default", v.Microphone is null) };
        foreach (var m in i.Live.Mics) mics.Add(new Opt(m, v.Microphone is not null && v.Microphone == m));
        // A saved device that isn't plugged in now still shows as the one picked.
        if (v.Microphone is not null && !i.Live.Mics.Contains(v.Microphone)) mics.Add(new Opt(v.Microphone, true));
        var shortcutSub = v.Hold ? "Hold it to talk, let go to finish. Up to ten minutes." : "Press it to talk, press it again to finish. Up to ten minutes.";
        if (i.Live.ShortcutError is not null) shortcutSub = i.Live.ShortcutError;
        Group(b,
            R("Voice control", "", Switch("VoiceEnabled", "Voice control", v.Enabled)),
            R("Shortcut", shortcutSub, new Control { Kind = ControlKind.Shortcut, Id = "VoiceShortcut", Name = "Voice shortcut", Text = i.VoiceShortcut }),
            R("Voice Recording Mode", "", Segs("VoiceMode", ["Toggle (Click on/off)", "Hold to speak"], v.Hold ? 1 : 0)),
            R("Microphone", "", Picker("VoiceMicrophone", "Microphone", v.Microphone ?? "System default", mics)));
        // The aura on the listening card: one of the offered colours, or any typed in.
        var aura = v.Aura;
        var shown = "Custom";
        foreach (var c in AuraColors) if (c.Hex == aura && shown == "Custom") shown = c.Name;
        var auraOpts = AuraColors.Select(c => new Opt(c.Name, c.Name == shown)).ToList();
        Group(b,
            R("Aura colour", "The light that swirls on the notch while it listens and works on what you said.", Picker("VoiceAuraColor", "Aura colour", shown, auraOpts)),
            R("Custom colour", "Any colour, as hex: #1FD5F9.", Field("VoiceAuraHex", "Aura colour, hex", aura, "#1FD5F9")));

        Heading(b, "Speech recognition");
        var language = R("Language", "", TextCtl(local ? "English only" : "Detected automatically")) with { Enabled = !local };
        var speechSub = local ? "Speech recognition stays on this computer. English only. For other languages, choose Cloud (Groq)."
            : "Audio is sent to Groq. Language is detected automatically.";
        var rows = new List<Row> { R("Speech recognition", speechSub, Segs("VoiceSpeech", SpeechLabels, IndexOf(SpeechModes, v.Speech))), language };
        if (!local)
        {
            rows.Add(R("Groq API key", KeyNote(i, "Your own key, from console.groq.com."), Secret("VoiceGroqKey", "Groq API key", GroqSecret, i)));
            var check = i.Live.GroqCheck ?? "Asks Groq whether the key works.";
            rows.Add(R("Check the key", check, Button("groq.check", "Check the Groq key", "Check key", i.HasSecret(GroqSecret))));
            var model = v.Model;
            var opts = new List<Opt>();
            foreach (var m in TranscribeModels)
            {
                if (m.Id == v.Model) model = m.Name;
                opts.Add(new Opt(m.Name, m.Id == v.Model));
            }
            rows.Add(R("Model", "", Picker("VoiceModel", "Model", model, opts)));
        }
        Group(b, rows);
        if (local)
        {
            IReadOnlyList<Opt> english = [new Opt("English only", false)];
            var prows = new List<Row>();
            if (i.Live.Phonon is not { } c)
                prows.Add(R("Phonon-2", "Checking…", Chips(english, [], ""), Tile("cpu", Tint.Teal)));
            else
            {
                var sub = c.State;
                if (c.Progress is { } p)
                    sub += p.Total is { } total ? $" · {Size(p.Done)} of {Size(total)}" : " · " + Size(p.Done);
                if (c.Error is not null) sub += "\n" + c.Error;
                var buttons = c.Actions.Select(a => new Btn(a.Id(), a.Label(), a == PhononAction.Remove)).ToList();
                var row = R(c.Model, sub, Chips(english, buttons, ""), Tile("cpu", Tint.Teal));
                if (c.Progress is { Total: > 0 } t) row = row with { Progress = (float)Math.Min((double)t.Done / t.Total!.Value, 1) };
                prows.Add(row);
                // A long value (the folder) wraps under its label; a short one sits on the right.
                foreach (var (label, value) in c.Facts)
                    prows.Add(value.EnumerateRunes().Count() > 40 ? R(label, value, new Control()) : R(label, "", TextCtl(value)));
            }
            Group(b, prows);
        }

        Heading(b, "Cleanup");
        var provider = v.CleanupProvider;
        var custom = provider == "Custom";
        rows =
        [
            R("Clean up the text", "Fixes punctuation, grammar and filler words, in the language you spoke. If it fails, the original text is used.", Switch("VoiceCleanup", "Clean up the text", v.Cleanup)),
            R("Service", "", Segs("VoiceCleanupProvider", CleanupProviders, IndexOf(CleanupProviders, provider))),
            R("Model", "", Field("VoiceCleanupModel", "Cleanup model", v.CleanupModel ?? "", "The service’s model id")),
            R("API key", KeyNote(i, $"Your own {(custom ? "service’s" : provider)} key."), Secret("VoiceCleanupKey", "Cleanup API key", CleanupSecret(provider), i)),
        ];
        if (custom) rows.Add(R("Base URL", "An OpenAI-compatible API, ending in /v1.", Field("VoiceCleanupBase", "Base URL", v.CleanupBase ?? "", "https://…/v1")));
        Group(b, rows);
        Footnote(b, $"With cleanup on, the transcript (never the audio) is sent to {(custom ? "the address above" : provider)} with your key.");

        Heading(b, "Starting tasks");
        var tool = VoiceTool(s);
        var o = s.OptionsOf(tool);
        var models = Models(tool, s.OffersOf(tool));
        var current = o.Model ?? models[0].Value;
        var modelName = current;
        foreach (var m in models) if (m.Value == current) { modelName = m.Name; break; }
        var agentSub = v.Agent is not null ? "Voice tasks start with this agent. The card lets you pick another for one task."
            : "Voice tasks start with the agent the office picked for its last new task. Pick one to keep it.";
        if (i.Ready(tool) is { Checked: true, Ok: false } r) agentSub += $" {tool.Name} isn’t ready: {r.Hint} Voice will ask you to pick another.";
        var w = s.Workspace;
        var place = i.WorkspacePath == "" ? "No home folder" : i.WorkspacePath;
        var labels = VoiceCountdowns.Select(n => n == 0 ? "Off" : $"{n} s").ToList();
        var at = IndexOf(VoiceCountdowns, v.Countdown);
        var wait = v.Countdown == 0 ? "The card waits for Start (or Enter)."
            : $"The card starts the task {SecsWords(v.Countdown)} after it shows it, unless you edit it first.";
        var toolOpts = Tools.All.Select(t => new Opt(t.Name, t.Id == tool.Id)).ToList();
        Group(b,
            R("Agent", agentSub, Picker("VoiceAgentTool", "Voice agent", tool.Name, toolOpts)),
            R("Model", $"The model, effort and access are {tool.Name}’s own settings.", Button("VoiceAgent", $"Open {tool.Name} settings", $"{tool.Name} · {modelName}", true)),
            R("Start on its own", wait, Segs("VoiceCountdown", labels, at)),
            R("Default workspace", $"{place} · {Access.Label(w.Access)}", Button("VoiceWorkspace", "Open Projects", "Projects…", true)));

        Heading(b, "Try it");
        var t2 = i.Live.VoiceTry;
        var trySub = v.Hold ? "Hold the button and speak. It shows what voice would start; nothing starts and no files are touched."
            : "Click the button, speak, then click it again. It shows what voice would start; nothing starts and no files are touched.";
        if (t2 is not null)
        {
            if (t2.Error is not null) trySub = t2.Error;
            else if (t2.Status != "") trySub = t2.Status;
        }
        rows = [R("Try it", trySub, new Control { Kind = ControlKind.Hold, Id = "voice.try", Name = v.Hold ? "Hold to try voice" : "Click to try voice", Text = v.Hold ? "Hold to talk" : "Click to talk" })];
        if (t2 is not null) foreach (var (label, value) in t2.Lines) rows.Add(R(label, value, new Control()));
        Group(b, rows);
        var start = v.Countdown != 0 ? "and starts it after " + SecsWords(v.Countdown) : "and waits for Start";
        Footnote(b, $"Use the shortcut, say what to do (“in Hover, fix the notch blink”) and finish. A card shows the folder, agent, access and task, " +
            $"{start}. Enter starts it now, editing the task stops the countdown, and Esc cancels. With a chat open in the office and its reply box open, " +
            "use the shortcut with the pointer over the chat to write into the reply instead.");
    }
}

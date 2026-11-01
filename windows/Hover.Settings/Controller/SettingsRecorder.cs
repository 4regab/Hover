using System.Text.Json.Nodes;
using Hover.Host.Protocol;
using Hover.Notch;

namespace Hover.Settings;

public sealed partial class SettingsController
{
    // Keys between the worlds a shortcut passes through (internal/app/keys.go): what the window reports when a chord
    // is pressed in Settings (a Windows virtual-key code), and the WPF Key settings.json keeps.
    static readonly Dictionary<int, int> KeyOfVirtual = BuildKeyOfVirtual();

    static Dictionary<int, int> BuildKeyOfVirtual()
    {
        var map = new Dictionary<int, int>();
        for (var k = 1; k <= 160; k++)
            if (new Shortcut(k, 0).VirtualKey is { } vk && !map.ContainsKey(vk)) map[vk] = k;
        return map;
    }

    /// <summary>The WPF Key of a virtual-key code, or 0 when a shortcut cannot hold it.</summary>
    public static int WpfKey(int virtualKey) => KeyOfVirtual.TryGetValue(virtualKey, out var k) ? k : 0;

    /// <summary>Shift, Ctrl, Alt, the Windows keys and Caps Lock: pressed alone they start a chord, not end one.</summary>
    public static bool IsModifier(int vk) => vk is 0x10 or 0x11 or 0x12 or 0x5B or 0x5C or 0xA0 or 0xA1 or 0xA2 or 0xA3 or 0xA4 or 0xA5 or 0x14;

    public const string NeedModifier = "Add Ctrl, Alt, Shift or Win";

    /// <summary>The shortcut field's click: record, then a chord.</summary>
    public void RecordShortcut()
    {
        RecordAs(false);
        Dirty();
    }

    void RecordAs(bool voice)
    {
        if (Recording)
        {
            Recording = false;
            RecorderField = null;
        }
        else
        {
            Recording = true;
            RecordingVoice = voice;
            RecorderField = "Press keys…";
        }
        Dirty();
    }

    /// <summary>
    /// A key pressed while a shortcut records: true when it was taken. <paramref name="mods"/> is the modifiers held
    /// (<see cref="Shortcut.Alt"/> and the like). Esc stops, a modifier alone waits, a key without a modifier asks for one,
    /// anything else is the chord.
    /// </summary>
    public bool Chord(int virtualKey, int mods)
    {
        if (!Recording) return false;
        if (virtualKey == 0x1B)
        {
            Recording = false;
            RecorderField = null;
            Dirty();
            return true;
        }
        if (IsModifier(virtualKey)) return true;
        var key = WpfKey(virtualKey);
        if (key == 0) return true;
        if (mods == 0)
        {
            RecorderField = NeedModifier;
            Dirty();
            return true;
        }
        var chord = new Shortcut(key, mods);
        // One chord can't be both: the system gives it to whichever takes it first.
        var other = RecordingVoice ? data.Notch : data.Voice.Shortcut;
        if (chord == other)
        {
            RecorderField = RecordingVoice ? "Used by the notch" : "Used by Voice";
            Dirty();
            return true;
        }
        if (RecordingVoice)
        {
            var changed = chord != data.Voice.Shortcut;
            Recording = false;
            RecorderField = null;
            if (changed) Voice(v => v with { Shortcut = chord });
            else Dirty();
        }
        else
        {
            var changed = chord != data.Notch;
            Recording = false;
            RecorderField = null;
            if (changed)
            {
                Hold("notch-shortcut", d => d with { Notch = chord }, d => d.Notch == chord);
                env.Send(new SetSettingCommand("ScWorkspace", new JsonObject { ["Key"] = chord.Key, ["Modifiers"] = VoiceData.ModifierText(chord.Modifiers) }));
            }
            Dirty();
        }
        return true;
    }

    /// <summary>The recorder lost the keyboard: it stops.</summary>
    public void StopRecording()
    {
        if (!Recording) return;
        Recording = false;
        RecorderField = null;
        Dirty();
    }
}

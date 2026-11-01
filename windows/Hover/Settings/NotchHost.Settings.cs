using Hover.Chatting;
using Hover.Configuring;
using Hover.Host;
using Hover.Notch;
using Hover.Settings;
using Hover.Speaking;
using static Hover.Windowing.Native;

namespace Hover.Windowing;

/// <summary>
/// The notch's part of Settings: the page over the open notch (in a layer of its own, above the office and the panels), the pointer
/// and the keyboard for it, and the shortcuts let go while one records. The page itself is <see cref="SettingsPane"/>; a port of the
/// notch parts of <c>internal/shell/windows.go</c> (<c>ShowSettingsIn</c>) and <c>internal/ui/overlay.go</c>.
/// </summary>
sealed partial class NotchHost
{
    SettingsPane? cfg;
    UiLayer? cfgLayer;
    bool cfgDrawn, cfgKeys, cfgRecording;
    double cfgScale;

    /// <summary>The voice flow, for Settings' Try it and its shortcut's error.</summary>
    internal VoiceHost? VoiceFlow => voice;

    /// <summary>Ties Settings to the notch. Called once, after the notch is made.</summary>
    public void AttachSettings(SettingsPane host)
    {
        cfg = host;
        host.Changed += () => Guard("settings", () =>
        {
            CfgRecordingChanged();
            QueueRender();
            EnsureAnimating();
        });
        host.NotchOpenChanged += () => Guard("settings", () =>
        {
            // Settings asked for in a closed notch opens it; closing Settings leaves the notch as it is.
            if (host.NotchOpen && ctl.State == NotchState.Rest)
            {
                ctl.Expand(peek: false, focus: true);
                AfterChange(peek: false, focus: true);
            }
            QueueRender();
            EnsureAnimating();
        });
        host.FoldRequested += () => Guard("settings", () =>
        {
            if (ctl.State == NotchState.Rest) return;
            ctl.Collapse();
            AfterChange(false, false);
        });
        host.LoginChanged += () => Guard("settings", () => window.SetTrayMenu(TrayMenu.Build(settings.Shortcut.Label, LaunchAtLogin.Enabled)));
    }

    /// <summary>Settings shows in the open notch (the page is wanted and the office view has come up).</summary>
    bool CfgOpen => cfg is { NotchOpen: true } && ctl.State != NotchState.Rest && scene.ViewVisible && scene.ViewOpacity >= 0.95;

    /// <summary>Settings is wanted in the notch, so the office's panels step aside.</summary>
    bool CfgWanted => cfg is { NotchOpen: true };

    bool CfgMoving => CfgOpen && cfg!.NotchUi.Animating;

    // MARK: Recording a shortcut

    /// <summary>
    /// While a shortcut records, the notch's and voice's own are let go, or pressing them would be taken as the chord's
    /// own press instead of reaching the recorder.
    /// </summary>
    void CfgRecordingChanged()
    {
        if (cfg is null) return;
        var recording = cfg.Controller.Recording;
        if (recording == cfgRecording) return;
        cfgRecording = recording;
        if (recording)
        {
            window.UnregisterHotkey(ShortcutId);
            voice?.SuspendKey(true);
        }
        else
        {
            RegisterShortcut(settings.Shortcut);
            voice?.SuspendKey(false);
        }
    }

    // MARK: The pointer, the wheel, the keys

    (float X, float Y)? CfgPoint(int px, int py, bool clamp = false) => PanelPoint(px, py, clamp);

    bool CfgPressed(int px, int py)
    {
        if (!CfgOpen || CfgPoint(px, py) is not { } p) return false;
        var tick = Environment.TickCount64;
        if (tick - lastClickAt <= 500 && Math.Abs(px - lastClickAtPos.X) <= 4 && Math.Abs(py - lastClickAtPos.Y) <= 4) clickCount++;
        else clickCount = 1;
        lastClickAt = tick;
        lastClickAtPos = (px, py);
        var ui = cfg!.NotchUi;
        ui.PointerMove(p.X, p.Y);
        ui.PointerDown(p.X, p.Y, clickCount, (GetKeyState(0x10) & 0x8000) != 0);
        CfgSyncKeys();
        QueueRender();
        EnsureAnimating();
        return true;
    }

    bool CfgReleased(int px, int py)
    {
        if (!CfgOpen || CfgPoint(px, py, clamp: true) is not { } p) return false;
        cfg!.NotchUi.PointerUp(p.X, p.Y);
        CfgSyncKeys();
        QueueRender();
        return true;
    }

    bool CfgWheel(int delta)
    {
        if (!CfgOpen) return false;
        var (cx, cy) = window.Cursor();
        if (CfgPoint((int)(cx - ctl.Window.Left), (int)(cy - ctl.Window.Top)) is not { } p) return false;
        cfg!.NotchUi.Wheel(p.X, p.Y, delta / 120f);
        QueueRender();
        return true;
    }

    bool CfgKey(int vk)
    {
        if (!CfgOpen || cardOpen) return false;
        var ui = cfg!.NotchUi;
        // While a shortcut records, every chord is the recorder's; otherwise the page reads the key (Esc folds through the page itself).
        if (!ui.RawKey(vk, SettingsOverlay.Modifiers())) ui.Key(KeyMap.Of(vk), KeyMap.Mods(GetKeyState));
        CfgSyncKeys();
        QueueRender();
        EnsureAnimating();
        // Settings has the notch while it shows: no other part reads the key.
        return true;
    }

    bool CfgChar(string text)
    {
        if (!CfgOpen) return false;
        var used = cfg!.NotchUi.Char(text);
        CfgSyncKeys();
        if (used) QueueRender();
        return true;
    }

    void CfgHover()
    {
        if (!CfgOpen || !ctl.Over) return;
        var ui = cfg!.NotchUi;
        var (cx, cy) = window.Cursor();
        if (CfgPoint((int)(cx - ctl.Window.Left), (int)(cy - ctl.Window.Top)) is { } pt) ui.PointerMove(pt.X, pt.Y);
        else ui.PointerLeave();
        if (ui.Dirty) QueueRender();
    }

    /// <summary>The page needs the keyboard while a box of it has the focus or a shortcut records; the notch takes it for that time and gives it back.</summary>
    bool CfgWantsKeys => CfgOpen && (cfg!.NotchUi.TakesKeys || cfg.Controller.Recording);

    void CfgSyncKeys()
    {
        var wants = CfgWantsKeys;
        if (wants && !cfgKeys && !cardOpen)
        {
            cfgKeys = true;
            window.RememberForeground();
            window.SetAcceptsKeys(true);
            window.Focus();
        }
        else if (!wants && cfgKeys)
        {
            cfgKeys = false;
            if (!cardOpen && ctl.State == NotchState.Rest)
            {
                window.SetAcceptsKeys(false);
                window.RestoreForeground();
            }
        }
    }

    // MARK: Drawing

    /// <summary>Draws the page into its layer when it shows, and takes the layer away when it does not.</summary>
    void RenderSettings()
    {
        if (cfg is null || surface is null) return;
        if (!CfgOpen)
        {
            if (cfgDrawn)
            {
                cfgLayer?.Show(false);
                cfgDrawn = false;
            }
            CfgSyncKeys();
            return;
        }
        var (composition, root) = surface.Composition;
        cfgLayer ??= new UiLayer(gfx, composition, root);
        var (ax, ay, aw, ah) = scene.OfficeArea(ctl.WindowWidth);
        var sc = ctl.Scale;
        cfgLayer.Place((float)(ax * sc), (float)(ay * sc), (int)Math.Round(aw * sc), (int)Math.Round(ah * sc));
        var ui = cfg.NotchUi;
        var first = !cfgDrawn;
        if (first)
        {
            cfgLayer.Show(true);
            cfgDrawn = true;
            ui.Invalidate();
        }
        if (first || ui.Dirty || ui.Animating || cfgScale != sc)
        {
            cfgScale = sc;
            cfgLayer.Draw(dc => cfg.Paint(ui, dc, (float)aw, (float)ah), sc);
            if (UiDump.On) UiDump.Set("settings-notch", SettingsDump.Describe(cfg, ui, ctl.Window.Left + ax * sc, ctl.Window.Top + ay * sc, sc));
        }
        CfgSyncKeys();
    }
}

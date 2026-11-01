using System.Text.Json;

namespace Hover.Office;

/// <summary>
/// The office's life around its model, as internal/office/live.go runs it: what the app tells it (the
/// backend's state, a resize, the pointer, the wheel, a key, the drawer or a panel opening, being shown
/// or hidden), the drag that starts after 6 px, the click on release, the time of day that follows the
/// clock, and the frame pacing. It has no thread of its own and no window: the host calls
/// <see cref="Tick"/> on its timer and draws when that says a frame was made. Everything runs on one thread.
/// </summary>
public sealed class OfficeController(OfficeModel model, Func<int>? hourNow = null)
{
    readonly Func<int> hour = hourNow ?? (() => DateTime.Now.Hour);
    public OfficeModel Model { get; } = model;

    bool visible = true, isDown;
    (double X, double Y) down, prev;
    double last;
    double timeChecked;

    /// <summary>A click without a drag, as the model reads it (a bot, a desk, a prop). A click on the window already changed the time of day by the time this is raised.</summary>
    public event Action<Click>? Clicked;

    /// <summary>The user changed the camera or the time of day: persist it (office.json).</summary>
    public event Action? Changed;

    /// <summary>The primary button went down over the office and has not come up.</summary>
    public bool IsDown => isDown;

    public bool Visible
    {
        get => visible;
        set => visible = value;
    }

    // ---- what the app tells the office -----------------------------------------------------

    public void State(IReadOnlyList<SessionInput> sessions)
    {
        Model.Poke();
        Model.State(sessions);
    }

    public void Resize(double w, double h)
    {
        Model.Poke();
        if (w > 0 && h > 0) Model.Resize(w, h);
    }

    public void Pointer(double x, double y, bool on)
    {
        Model.Poke();
        var o = Model;
        if (isDown && on)
        {
            if (!o.Dragging && Math.Sqrt((x - down.X) * (x - down.X) + (y - down.Y) * (y - down.Y)) > 6 && !o.DrawerOpen && o.Panel == "") o.Dragging = true;
            if (o.Dragging) o.Drag(x - prev.X, y - prev.Y);
        }
        if (on) prev = (x, y);
        o.Pointer = (x, y);
        o.HasPointer = on;
    }

    public void Down(double x, double y)
    {
        Model.Poke();
        down = prev = (x, y);
        isDown = true;
        Model.Dragging = false;
    }

    public void Up()
    {
        Model.Poke();
        var o = Model;
        var was = o.Dragging;
        o.Dragging = false;
        isDown = false;
        if (was)
        {
            Changed?.Invoke();
            return;
        }
        o.Pick();
        var c = o.Click();
        if (c.Kind == ClickKind.Time)
        {
            o.ManualTime = c.Time;
            o.ApplyTime(c.Time);
            Changed?.Invoke();
        }
        Clicked?.Invoke(c);
    }

    /// <summary>The press was taken away from the office (focus lost, capture lost): no click, no drag.</summary>
    public void Cancel()
    {
        Model.Poke();
        isDown = false;
        if (Model.Dragging)
        {
            Model.Dragging = false;
            Changed?.Invoke();
        }
    }

    /// <summary>The wheel turned over the office at (x, y): dy is a browser's deltaY, positive away from the screen's top (zoom out).</summary>
    public void Wheel(double dy, double x, double y)
    {
        Model.Poke();
        var o = Model;
        if (o.DrawerOpen || o.Panel != "") return;
        o.ZoomBy(Math.Exp(-dy * 0.0015), x - o.W / 2, -(y - o.H / 2));
        Changed?.Invoke();
    }

    public void Key(char c)
    {
        Model.Poke();
        switch (c)
        {
            case '+' or '=':
                Model.ZoomBy(1.25, 0, 0);
                break;
            case '-':
                Model.ZoomBy(0.8, 0, 0);
                break;
            case '0':
                Model.ResetView();
                break;
            default:
                return;
        }
        Changed?.Invoke();
    }

    public void DoubleClick()
    {
        Model.Poke();
        var o = Model;
        if (o.Hovered.Kind == HoverKind.None && !o.DrawerOpen && o.Panel == "")
        {
            o.ResetView();
            Changed?.Invoke();
        }
    }

    /// <summary>The drawer opened on a session (or closed): the camera aims there.</summary>
    public void Drawer(int id, bool open)
    {
        Model.Poke();
        Model.DrawerOpen = open;
        Model.Sel = id;
        Model.HasSel = open;
        Model.DrawTV(Model.ClockT);
    }

    /// <summary>The panel open over the office ("board", "tv", "history"), "" for none.</summary>
    public void Panel(string panel)
    {
        Model.Poke();
        Model.Panel = panel;
    }

    /// <summary>The session whose desk card or panel is open (its bot shows as hot).</summary>
    public void DeskOpen(int id, bool set)
    {
        Model.Poke();
        Model.DeskOpen = id;
        Model.HasDeskOpen = set;
    }

    /// <summary>The time of day picked; null follows the clock.</summary>
    public void SetTime(TimeOfDay? t)
    {
        Model.Poke();
        Model.ManualTime = t;
        Model.ApplyTime(t ?? OfficeModel.AutoTime(hour()));
    }

    /// <summary>The user's camera, when the office is made again (office.view).</summary>
    public void SetView(double x, double z, double zoom)
    {
        Model.Poke();
        Model.User = [x, z, zoom];
        Model.ClampView();
        Model.Cam = [Model.User[0], 1.7, Model.User[1], Model.User[2]];
    }

    // ---- the frame ---------------------------------------------------------------------------

    /// <summary>One turn of the host's timer. True when a new frame was made and has to be drawn.</summary>
    public bool Tick(double nowMs)
    {
        // The time of day follows the clock unless picked, checked every minute.
        if (nowMs - timeChecked > 60_000)
        {
            timeChecked = nowMs;
            if (Model.ManualTime is null)
            {
                var t = OfficeModel.AutoTime(hour());
                if (t != Model.Time) Model.ApplyTime(t);
            }
        }
        if (!visible)
        {
            last = nowMs;
            return false;
        }
        var dt = nowMs - last;
        last = nowMs;
        return Model.Frame(nowMs, dt);
    }
}

/// <summary>
/// office.json: the page's localStorage, in the data folder. A flat object of strings: "view" ("x,y,zoom"),
/// "time" ("day", "night", or "" to follow the clock) and "beats" ("on" or "off"). The Go and Mac builds read it too.
/// </summary>
public sealed class OfficeStore(string file)
{
    public string? Get(string key)
    {
        try
        {
            if (!File.Exists(file)) return null;
            var m = JsonSerializer.Deserialize<Dictionary<string, string>>(File.ReadAllText(file));
            return m is not null && m.TryGetValue(key, out var v) ? v : null;
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException or NotSupportedException)
        {
            return null;
        }
    }

    /// <summary>Keeps a value; other keys stay as they are, and a file that cannot be read is replaced only by one that holds the new value and what could be kept.</summary>
    public void Set(string key, string value)
    {
        var m = new SortedDictionary<string, string>(StringComparer.Ordinal);
        try
        {
            if (File.Exists(file) && JsonSerializer.Deserialize<Dictionary<string, string>>(File.ReadAllText(file)) is { } old)
                foreach (var (k, v) in old) m[k] = v;
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException or NotSupportedException)
        {
        }
        m[key] = value;
        try
        {
            // Written beside and moved over, so a crash in the middle never leaves half a file.
            var tmp = file + ".tmp";
            File.WriteAllText(tmp, JsonSerializer.Serialize(m));
            File.Move(tmp, file, true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    /// <summary>The saved camera, if there is one that reads as three numbers.</summary>
    public (double X, double Z, double Zoom)? View()
    {
        if (Get("view") is not { } s) return null;
        var p = s.Split(',');
        if (p.Length < 3) return null;
        return double.TryParse(p[0].Trim(), System.Globalization.CultureInfo.InvariantCulture, out var a) &&
               double.TryParse(p[1].Trim(), System.Globalization.CultureInfo.InvariantCulture, out var b) &&
               double.TryParse(p[2].Trim(), System.Globalization.CultureInfo.InvariantCulture, out var c) ? (a, b, c) : null;
    }

    public void SetView(double x, double z, double zoom) =>
        Set("view", string.Create(System.Globalization.CultureInfo.InvariantCulture, $"{x},{z},{zoom}"));

    /// <summary>The saved time of day: null follows the clock.</summary>
    public TimeOfDay? Time() => Get("time") switch { "night" => TimeOfDay.Night, "day" => TimeOfDay.Day, _ => null };

    public void SetTime(TimeOfDay? t) => Set("time", t switch { TimeOfDay.Night => "night", TimeOfDay.Day => "day", _ => "" });
}

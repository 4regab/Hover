namespace Hover.Voice;

/// <summary>A picture for the card: opaque or premultiplied BGRA pixels.</summary>
public sealed record CardImage(int Width, int Height, byte[] Bgra);

/// <summary>A place on the card that answers a click.</summary>
public sealed record CardTarget(CardAct Act, string Id, double X, double Y, double W, double H, string Kind = "button")
{
    public bool Contains(double x, double y) => x >= X && x < X + W && y >= Y && y < Y + H;
}

/// <summary>
/// What the card keeps between frames and the painter reads: the two text boxes with their carets,
/// the scroll of the menu and of the task, which button the pointer is on, the screenshots'
/// pictures and the aura's frame.
/// </summary>
public sealed class CardUi
{
    /// <summary>The task, edited in place before it starts.</summary>
    public VoiceEditor Task { get; } = new(multiline: true);

    /// <summary>The Kiro Web repository menu's search box.</summary>
    public VoiceEditor Search { get; } = new(multiline: false, maxLength: 200);

    public bool TaskFocused { get; set; }
    public bool SearchFocused { get; set; }

    public double MenuScroll { get; set; }
    public double TaskScroll { get; set; }

    /// <summary>The height of the menu's rows and of the task's text, and the room they have; set by the painter.</summary>
    public double MenuContent { get; set; }
    public double MenuRoom { get; set; }
    public double TaskContent { get; set; }
    public double TaskRoom { get; set; }

    /// <summary>The id of the target under the pointer.</summary>
    public string? Hover { get; set; }

    /// <summary>Seconds on the notch's clock: the caret blinks by it.</summary>
    public double Now { get; set; }

    /// <summary>A screenshot's picture, by its file; null while it loads or when it cannot be read.</summary>
    public Func<string, CardImage?> Thumbs { get; set; } = _ => null;

    /// <summary>The aura's latest frame (premultiplied BGRA, <see cref="AuraPx"/> square), for the listening and working cards.</summary>
    public byte[]? AuraPixels { get; set; }
    public int AuraPx { get; set; }

    /// <summary>The card's white flash after a screenshot, 0 to 1, fading out.</summary>
    public double Flash { get; private set; }

    double flashFrom = double.NegativeInfinity;

    /// <summary>The flash is full while the card says so and fades over 450 ms after.</summary>
    public void UpdateFlash(bool on, double now)
    {
        if (on)
        {
            Flash = 0.85;
            flashFrom = now;
            return;
        }
        if (Flash <= 0) return;
        var k = Math.Clamp((now - flashFrom - 0.0) / 0.45, 0, 1);
        Flash = 0.85 * (1 - k * (2 - k));
    }

    /// <summary>The place keys go: false when neither text box has the keyboard.</summary>
    public bool TakesKeys => TaskFocused || SearchFocused;

    public VoiceEditor? Focused => SearchFocused ? Search : TaskFocused ? Task : null;

    public void Blur() => (TaskFocused, SearchFocused) = (false, false);
}

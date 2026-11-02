namespace Hover.Notch;

/// <summary>
/// When the notch may stop drawing frames. A frame takes its picture of the clock at one moment and is
/// finished at a later one, and the first frame that shows the office makes a Direct3D device, which on a
/// slow machine can take longer than the notch's whole opening. If the motion ended while such a frame was
/// being drawn, that frame is not the last: the picture it took is of the middle of the opening, and the
/// panels, which wait for the office to be fully in, are not up in it. So the timer stops only after a frame
/// that began when nothing moved.
/// </summary>
public sealed class FramePump
{
    bool movedAtStart;

    /// <summary>A frame is about to take its picture of the clock; <paramref name="moving"/> is whether anything moves right now.</summary>
    public void Begin(bool moving) => movedAtStart = moving;

    /// <summary>The frame is drawn. True when the timer may stop: nothing moves now, and nothing moved when the frame began.</summary>
    public bool CanStop(bool movingNow) => !movingNow && !movedAtStart;
}

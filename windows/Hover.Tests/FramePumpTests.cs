using Hover.Notch;
using Xunit;

namespace Hover.Tests;

public sealed class FramePumpTests
{
    /// <summary>The notch opening with the frame timer of <c>NotchHost.Animate</c>: each tick asks the pump, takes the clock's picture, and spends the frame's cost.</summary>
    sealed class Rig
    {
        public double Time;
        public readonly NotchController Notch;
        readonly FramePump pump = new();
        public double DrawnOpacity = -1;
        public int Frames;

        public Rig()
        {
            Notch = new NotchController(new FakeDesktop(), () => Time);
            Notch.Layout();
            Notch.Toggle();
        }

        /// <summary>One tick. True when the pump says the timer may stop.</summary>
        public bool Tick(double cost)
        {
            pump.Begin(Notch.Animating);
            DrawnOpacity = Notch.CurrentFrame.ViewOpacity;
            Time += cost;
            Frames++;
            return pump.CanStop(Notch.Animating);
        }
    }

    // The panels come up when the office view is at 0.95 or more (NotchHost.PanelsOpen).
    const double PanelsNeed = 0.95;

    [Fact]
    public void A_frame_that_outlasts_the_opening_is_followed_by_one_that_shows_the_open_notch()
    {
        var rig = new Rig();
        // The first frame with the office makes the graphics device. On the runner that took ~390 ms, in the 560 ms opening, after 110 ms.
        rig.Time = 110;
        var stopped = rig.Tick(cost: 450);
        Assert.False(rig.Notch.Animating);
        Assert.True(rig.DrawnOpacity < PanelsNeed, "that frame was drawn in the middle of the opening");
        while (!stopped && rig.Frames < 10)
        {
            rig.Time += 16;
            stopped = rig.Tick(cost: 2);
        }
        Assert.True(stopped);
        Assert.True(rig.DrawnOpacity >= PanelsNeed, $"the last frame before the timer stopped had the office at {rig.DrawnOpacity:0.00}");
    }

    [Fact]
    public void A_notch_that_does_not_move_stops_the_timer_at_the_first_frame()
    {
        var rig = new Rig();
        rig.Time = 2000;
        Assert.True(rig.Tick(cost: 5));
        Assert.Equal(1, rig.Frames);
    }

    [Fact]
    public void Quick_frames_stop_the_timer_one_frame_after_the_opening_ends()
    {
        var rig = new Rig();
        var stopped = false;
        while (!stopped && rig.Frames < 200)
        {
            rig.Time += 16;
            stopped = rig.Tick(cost: 2);
        }
        Assert.True(stopped);
        Assert.False(rig.Notch.Animating);
        Assert.True(rig.DrawnOpacity >= PanelsNeed);
        // 560 ms at 18 ms a tick is 32 ticks, and one more for the picture taken after the end.
        Assert.InRange(rig.Frames, 30, 36);
    }
}

using Hover.Notch;
using Xunit;

namespace Hover.Tests;

public sealed class PathDataTests
{
    [Fact]
    public void The_notch_outline_reads_as_a_move_four_arcs_three_lines_and_a_close()
    {
        var path = PathData.Parse(Geometry.Outline(128, 24, 12, 5, 40))!;
        Assert.Equal(
            new[] { PathOp.Move, PathOp.Arc, PathOp.Line, PathOp.Arc, PathOp.Line, PathOp.Arc, PathOp.Line, PathOp.Arc, PathOp.Close },
            path.Select(c => c.Op));
        Assert.Equal((35.0, 0.0), (path[0].X, path[0].Y));
        // The left ear curves from the top edge into the shape's left side.
        Assert.Equal((40.0, 5.0, true), (path[1].X, path[1].Y, path[1].Sweep));
        Assert.Equal((5.0, 5.0), (path[1].Rx, path[1].Ry));
        // The bottom corners curve the other way.
        Assert.False(path[3].Sweep);
        Assert.Equal((168.0 + 5, 0.0), (path[7].X, path[7].Y));
    }

    [Fact]
    public void Relative_commands_start_from_the_current_point()
    {
        var p = PathData.Parse("M10 10c1 1 2 2 3 3l1 1h2v-1z")!;
        Assert.Equal((13.0, 13.0), (p[1].X, p[1].Y));
        Assert.Equal((14.0, 14.0), (p[2].X, p[2].Y));
        Assert.Equal((16.0, 14.0), (p[3].X, p[3].Y));
        Assert.Equal((16.0, 13.0), (p[4].X, p[4].Y));
        Assert.Equal(PathOp.Close, p[5].Op);
    }

    [Fact]
    public void A_smooth_curve_mirrors_the_last_control_point()
    {
        var p = PathData.Parse("M0 0C1 0 2 1 2 2S3 4 4 4")!;
        Assert.Equal((2.0, 3.0), (p[2].X1, p[2].Y1));
        Assert.Equal((3.0, 4.0), (p[2].X2, p[2].Y2));
    }

    [Fact]
    public void Numbers_that_run_together_and_arc_flags_with_no_space_are_read()
    {
        var p = PathData.Parse("M12.964 6.687a.92.92 0 00-.81.428")!;
        Assert.Equal(2, p.Count);
        Assert.Equal((0.92, 0.92, false, false), (p[1].Rx, p[1].Ry, p[1].Large, p[1].Sweep));
        Assert.Equal(12.964 - 0.81, p[1].X, 9);
        Assert.Equal(6.687 + 0.428, p[1].Y, 9);
        var q = PathData.Parse("M0 0a1 1 0 11.5.5")!;
        Assert.True(q[1].Large);
        Assert.True(q[1].Sweep);
        Assert.Equal((.5, .5), (q[1].X, q[1].Y));
    }

    [Theory]
    [InlineData("")]
    [InlineData("L 1 2")]
    [InlineData("M 1")]
    [InlineData("M 1 2 X 3 4")]
    [InlineData("M 1 2 L 3")]
    [InlineData("M 0 0 a 1 1 0 2 0 1 1")]
    public void Anything_that_is_not_path_data_is_refused(string text) => Assert.Null(PathData.Parse(text));

    [Fact]
    public void Every_tool_mark_reads_and_stays_inside_its_bounds()
    {
        foreach (var (tool, spec) in Marks.All)
        {
            Assert.NotEmpty(spec.Layers);
            foreach (var layer in spec.Layers)
            {
                var p = PathData.Parse(layer.Path);
                Assert.True(p is { Count: > 2 }, $"{tool} did not read");
                var xs = p!.Where(c => c.Op != PathOp.Close).SelectMany(c => c.Op == PathOp.Cubic ? new[] { c.X1, c.X2, c.X } : [c.X]).ToList();
                // The bounds on the glyph's grid are what the Go app fits the glyph by; the outline
                // must lie inside them (a small allowance for curve control points).
                Assert.InRange(xs.Min(), spec.X - 1.5, spec.X + spec.W + 1.5);
                Assert.InRange(xs.Max(), spec.X - 1.5, spec.X + spec.W + 1.5);
            }
        }
    }

    [Fact]
    public void Tools_not_named_get_the_Kiro_mark()
    {
        Assert.Equal("kiro", Marks.IdOf("somethingelse"));
        Assert.Same(Marks.All["kiro"], Marks.Of(""));
        Assert.Equal(0x32d74bu, Marks.QuotaColor(69.9));
        Assert.Equal(0xffb340u, Marks.QuotaColor(70));
        Assert.Equal(0xff453au, Marks.QuotaColor(90));
    }
}

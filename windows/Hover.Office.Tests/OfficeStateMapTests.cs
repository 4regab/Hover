using System.Text;
using System.Text.RegularExpressions;
using Hover.Host.Protocol;
using Hover.Windowing;
using Xunit;

namespace Hover.Office.Tests;

/// <summary>The backend's <c>state</c> message, parsed by the app's own protocol reader, as the office gets it.</summary>
public sealed class OfficeStateMapTests
{
    [Fact]
    public void The_state_message_the_Go_fixture_holds_becomes_the_input_the_golden_numbers_were_printed_from()
    {
        var state = Fixture.State();
        // The fixture was written with some whole numbers as floats (1789999916000.0); the backend writes them whole.
        var json = Regex.Replace(state.ToJsonString(), @"(\d)\.0(?=[,}\]])", "$1");
        var ev = ProtocolJson.Parse(Encoding.UTF8.GetBytes(json), out var problem);
        Assert.True(problem is null, problem);
        var read = OfficeStateMap.Read(Assert.IsType<StateEvent>(ev));
        var want = Fixture.Inputs(state);
        Assert.Equal(want.Count, read.Count);
        for (var i = 0; i < want.Count; i++)
        {
            SessionInput w = want[i], r = read[i];
            Assert.Equal((w.Id, w.Key, w.Tool, w.Bot, w.Seat, w.Title, w.Folder), (r.Id, r.Key, r.Tool, r.Bot, r.Seat, r.Title, r.Folder));
            Assert.Equal((w.Ctx, w.Act, w.Pose, w.File), (r.Ctx, r.Act, r.Pose, r.File));
            Assert.Equal(w.Turns.Count, r.Turns.Count);
            for (var k = 0; k < w.Turns.Count; k++)
            {
                TurnInput wt = w.Turns[k], rt = r.Turns[k];
                Assert.Equal((wt.Prompt, wt.Stage, wt.Queued, wt.T0, wt.Took), (rt.Prompt, rt.Stage, rt.Queued, rt.T0, rt.Took));
                Assert.Equal(wt.Steps, rt.Steps);
            }
        }
    }

    [Fact]
    public void Odd_sessions_are_read_and_drawn_without_breaking_the_office()
    {
        // A seat and a bot out of range, text fields missing, act not a string, a session with no turns. (The protocol
        // reader refuses a null in a text field and skips that whole message.)
        const string json = """
            {"type":"state","sessions":[
              {"id":7,"bot":-3,"seat":99,"act":5,"turns":[
                {"stage":"working","t0":0}]},
              {"id":8,"bot":2,"seat":1,"turns":[]},
              {"id":9,"bot":1,"seat":2,"act":"Reading","pose":"Reading","file":"a.ts","turns":[
                {"prompt":"x","stage":"nonsense","queued":true,"t0":1790000000000,"steps":[{"k":"agent","status":"running"}]}]}
            ]}
            """;
        var ev = ProtocolJson.Parse(Encoding.UTF8.GetBytes(json), out var problem);
        Assert.True(problem is null, problem);
        var read = OfficeStateMap.Read(Assert.IsType<StateEvent>(ev));
        Assert.Equal(3, read.Count);
        Assert.Null(read[0].Act);
        Assert.Equal("", read[0].Title);
        Assert.Equal("Reading", read[2].Act);

        var (o, clock) = Fixture.Fresh();
        o.State(read);
        clock.Run(o, 200);
        // The session with no turns has no bot; the other two sit at desks inside the room.
        Assert.Equal(2, o.Sessions.Count);
        Assert.All(o.Sessions, s => Assert.InRange(s.Desk, 0, Layout.Desks.Length - 1));
        Assert.Equal(1, o.Sessions.First(s => s.Id == 9).Last().Agents);
    }
}

namespace HoverAvalonia.Office;

/// <summary>Deterministic simulated agents: no processes, no credentials. Every state is a pure function of the scene's clock, so two runs show the same thing.
/// "shot" freezes the four-bot state the baseline screenshot shows (Pip done, Juno done, Nova + Moss reading) so images can be compared side by side.
/// "demo" walks every bot through idle -> working (Thinking, Reading, Editing, Running) -> completed on a fixed timeline, then loops.</summary>
public static class Sim
{
    static readonly (int bot, int desk)[] Cast = [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)]; // Pip, Juno, Moss, Nova, Ada, Rue

    public static void Populate(OfficeScene o, int bots, string preset)
    {
        for (int i = 0; i < bots; i++) { var s = o.AddSession(Cast[i].bot, Cast[i].desk, SimState.Idle, walkIn: false); s.Tool = i == 3 ? "codex" : "kiro"; }
        Apply(o, preset);
    }

    public static void Apply(OfficeScene o, string preset)
    {
        double t = o.ClockT;
        string[] acts = ["Thinking", "Reading", "Editing", "Running"];
        for (int i = 0; i < o.Sessions.Count; i++)
        {
            var s = o.Sessions[i]; SimState sim = SimState.Idle; string? act = null;
            if (preset == "shot")
            {
                // Baseline office-drawer.png: Pip done, Juno done, Moss reading, Nova reading.
                (sim, act) = i switch { 0 or 1 => (SimState.Completed, null), _ => (SimState.Working, "Reading") };
            }
            else if (preset == "busy")
            {
                // Perf runs: every agent working for the whole run, cycling Thinking/Reading/Editing/Running (a steady 30 fps office, like three live tasks).
                sim = SimState.Working; act = acts[(int)((t + i * 3.0) / 3) % 4];
            }
            else
            {
                double loop = 36, u = (t + i * 3.5) % loop;     // staggered so the room is never in one state
                if (u < 3) sim = SimState.Idle;
                else if (u < 24) { sim = SimState.Working; act = acts[(int)((u - 3) / 3) % 4]; }
                else sim = SimState.Completed;
            }
            if (s.Sim != sim || s.Act != act) { s.Sim = sim; s.Act = act; }
        }
    }
}

using System.Text.Json;
using Hover.Chat.Desk;
using Hover.Host.Protocol;

namespace Hover.Chat.Ui;

public sealed partial class OfficeUi
{
    // Pull request, 1: the GitHub CLI in one click (install it, then sign in with GitHub's code). Pull request, 2: no
    // pull request yet; open one from the session's title and answer.

    /// <summary>The setup card's text for the last frame (for the tests): its title and the buttons it offers.</summary>
    public string GhTitleShown { get; private set; } = "";

    public GhEvent? GhState => gh;

    void PaintGhSetup(SessionInfo s, Rect4 r)
    {
        var pr = Got(s.Id, "pr");
        var installed = pr["setup"].Str() != "install";
        if (gh is { Checked: true } known) installed = known.Installed;
        var (title, text, button) = installed
            ? ("Sign in to GitHub", "gh is installed. Sign in once with your browser, and Hover can show this branch’s pull request and open new ones.", "Sign in with GitHub")
            : ("Set up the GitHub CLI", "Hover reads and opens pull requests through GitHub’s own command-line tool (gh). One click installs it and signs you in.", "Install and sign in");
        GhTitleShown = title;
        pen.Box(r, 12, Rgba.Of(0xffffff, 0.03));
        pen.Border(r.X, r.Y, r.W, r.H, 12, 1, Rgba.Of(0xffffff, 0.06));
        var ix = r.X + 12;
        var iw = r.W - 24;
        var cy = r.Y + 12;
        pen.Icon(DeskIcons.Github, ix, cy + 2, 26, Rgba.Of(0xffffff));
        var tw = iw - 26 - 12;
        var h1 = pen.Wrapped(title, ix + 26 + 12, cy, tw, Pen.F(14, 600, Pen.Ink));
        var h2 = pen.Wrapped(text, ix + 26 + 12, cy + h1 + 3, tw, Pen.F(12, 400, Pen.Dim));
        cy += Math.Max(26 + 2, h1 + 3 + h2) + 10;
        var busy = gh?.Busy ?? false;
        if (gh?.Code is { Length: > 0 } code)
        {
            var cf = Pen.Mono(22, Rgba.Of(0xffffff), 700);
            var bh = 10 + pen.LineHeight(Pen.F(11, 400, Pen.Ink)) + 4 + pen.LineHeight(cf) + 4 + 4 + 26 + 10;
            pen.Box(ix, cy, iw, bh, 10, Rgba.Of(0x7cc0ff, 0.08));
            pen.Border(ix, cy, iw, bh, 10, 1, Rgba.Of(0x7cc0ff, 0.2));
            pen.Text("Your one-time code", ix + 12, cy + 10, Pen.F(11, 400, Rgba.Of(0x9fd0ff)));
            pen.Text(code, ix + 12, cy + 10 + pen.LineHeight(Pen.F(11, 400, Pen.Ink)) + 4, cf);
            var by = cy + 10 + pen.LineHeight(Pen.F(11, 400, Pen.Ink)) + 4 + pen.LineHeight(cf) + 4 + 4;
            var cw = DeskButton("ghCopy", "Copy code", DeskIcons.Copy, ix + 12, by, true, false, 0, "");
            DeskButton("ghOpen", "Open github.com/login/device", DeskIcons.External, ix + 12 + cw + 6, by, true, true, 0, gh.Url);
            cy += bh + 10;
        }
        if (busy && gh is not null)
        {
            var line = gh.Line.Length > 0 ? gh.Line : gh.Step == "installing" ? "Installing…" : "Signing in…";
            cy += pen.Wrapped(line, ix, cy, iw, Pen.F(12, 400, Pen.Dim)) + 10;
        }
        if (!busy && gh?.Error is { Length: > 0 } error)
        {
            var th = pen.WrappedHeight(error, iw - 20, Pen.F(12, 400, Pen.Ink));
            pen.Box(ix, cy, iw, th + 14, 8, Rgba.Of(0xff453a, 0.1));
            pen.Wrapped(error, ix + 10, cy + 7, iw - 20, Pen.F(12, 400, Rgba.Of(0xffb0aa)));
            cy += th + 14 + 10;
        }
        var bx = ix;
        if (busy) DeskButton("ghCancel", "Cancel", null, bx, cy, false, false, 0, "");
        else
        {
            var bw = DeskButton("ghStart", button, DeskIcons.Github, bx, cy, false, true, 0, "");
            bx += bw + 10;
        }
        if (gh?.User is { Length: > 0 } user) Tv("Signed in as " + user, bx, cy, 30, Pen.F(11.5, 400, Pen.Faint));
    }

    void GhStart() => env.Send(new GhCommand("setup"));

    void GhCancel() => env.Send(new GhCommand("cancel"));

    // MARK: The form

    /// <summary>The form's words are the boxes'; they start from the session's title and answer, once, and after that they are the user's.</summary>
    void SyncForm(int id, DeskPrefs p, Jv create)
    {
        if (formOwner == id && p.Form is not null) return;
        if (formOwner >= 0 && deskPrefs.TryGetValue(formOwner, out var old) && old.Form is { } of) ReadForm(of);
        formOwner = id;
        p.Form ??= new PrForm
        {
            Title = create["title"].Str(), Body = create["body"].Str(), Base = create["base"].Str(),
            Commit = create["changed"].Long() > 0,
            Branch = create["onDefault"].Bool() ? create["suggest"].Str() : "",
        };
        Fld("prTitle", false).Box.SetText(p.Form.Title);
        Fld("prBody", true).Box.SetText(p.Form.Body);
        Fld("prBranch", false).Box.SetText(p.Form.Branch);
        Fld("prBase", false).Box.SetText(p.Form.Base);
    }

    void ReadForm(PrForm f)
    {
        f.Title = Fld("prTitle", false).Box.Text;
        f.Body = Fld("prBody", true).Box.Text;
        f.Branch = Fld("prBranch", false).Box.Text;
        f.Base = Fld("prBase", false).Box.Text;
    }

    void PaintPrForm(SessionInfo s, Rect4 r)
    {
        var p = Prefs(s.Id);
        var pr = Got(s.Id, "pr");
        var create = pr["create"];
        SyncForm(s.Id, p, create);
        var form = p.Form!;
        prView = r.H;
        deskScroll = Math.Clamp(deskScroll, 0, Math.Max(0, prContent - prView));
        deskListRect = r;
        pen.Cv.PushClip(r);
        var ix = r.X + 12 + 5;
        var iw = r.W - 10 - 24;
        var cy = r.Y + 12 - deskScroll;
        var changed = create["changed"].Long();
        var onDefault = create["onDefault"].Bool();
        var blocked = create["busy"].Bool() || Busy(s);
        var look = Pen.F(13, 400, Rgba.Of(0xf6f2ff));
        void Field(string key, string label, float fx, float fw, string placeholder)
        {
            Tv(label, fx, cy, pen.LineHeight(Pen.F(11, 400, Pen.Ink)), Pen.F(11, 400, Pen.Faint));
            var by = cy + pen.LineHeight(Pen.F(11, 400, Pen.Ink)) + 4;
            pen.Box(fx, by, fw, 32, 9, Rgba.Of(0x1b181e));
            var foc = focus == Focus.Desk && deskField?.Key == key;
            pen.Border(fx, by, fw, 32, 9, 1, foc ? Rgba.Of(0xc4a2ff, 0.55) : Rgba.Of(0xffffff, 0.08));
            PaintField(key, false, new Rect4(fx + 10, by + (32 - pen.LineHeight(look)) / 2, fw - 20, pen.LineHeight(look)), look, placeholder);
        }
        var rowH = pen.LineHeight(Pen.F(11, 400, Pen.Ink)) + 4 + 32;
        if (p.HasResult && new Jv(p.Result)["ok"].Bool())
        {
            pen.Box(ix, cy, iw, 38, 9, Rgba.Of(0x30d158, 0.1));
            pen.Icon(DeskIcons.Check, ix + 10, cy + (38 - 15) / 2f, 15, Rgba.Of(0x7ee59a));
            Tv("Pull request opened.", ix + 10 + 15 + 8, cy, 38, Pen.F(12.5, 400, Rgba.Of(0x7ee59a)));
            if (new Jv(p.Result)["url"].StrOrNull() is { } url)
            {
                var bw = DeskButton("prOpened", "Open on GitHub", DeskIcons.External, ix + iw - 10, cy + 6, true, false, 0, url, false, true);
                _ = bw;
            }
            cy += 38 + 8;
        }
        Pill("No pull request yet", Rgba.Of(0xf6f2ff, 0.62), Rgba.Of(0xffffff, 0.08), ix, cy);
        cy += 22 + 8;
        var parts = new List<string>();
        var branch = create["branch"].StrOrNull() ?? "";
        if (onDefault)
        {
            var from = branch.Length > 0 ? branch : create["base"].Str();
            parts.Add("A new branch from " + from);
        }
        else
        {
            var t = "Branch " + branch;
            var ahead = create["ahead"].Long();
            if (ahead > 0) t += $", {DeskRows.Num(ahead)} commit{(ahead == 1 ? "" : "s")} ahead of {create["base"].Str()}";
            parts.Add(t);
        }
        if (changed > 0) parts.Add($"{DeskRows.Num(changed)} file{(changed == 1 ? "" : "s")} not committed yet");
        cy += pen.Wrapped(string.Join(" · ", parts), ix, cy, iw, Pen.F(12, 400, Pen.Dim)) + 8;
        if (blocked)
        {
            const string note = "The agent is still working in this folder; open it when the run ends.";
            var bh = pen.WrappedHeight(note, iw - 20, Pen.F(11.5, 400, Pen.Ink));
            pen.Box(ix, cy, iw, bh + 14, 8, Amber with { A = 20 });
            pen.Wrapped(note, ix + 10, cy + 7, iw - 20, Pen.F(11.5, 400, Rgba.Of(0xffd59a)));
            cy += bh + 14 + 8;
        }
        Field("prTitle", "Title", ix, iw, "");
        cy += rowH + 8;
        Tv("Description", ix, cy, pen.LineHeight(Pen.F(11, 400, Pen.Ink)), Pen.F(11, 400, Pen.Faint));
        var dy = cy + pen.LineHeight(Pen.F(11, 400, Pen.Ink)) + 4;
        pen.Box(ix, dy, iw, 84, 9, Rgba.Of(0x1b181e));
        var bfoc = focus == Focus.Desk && deskField?.Key == "prBody";
        pen.Border(ix, dy, iw, 84, 9, 1, bfoc ? Rgba.Of(0xc4a2ff, 0.55) : Rgba.Of(0xffffff, 0.08));
        PaintField("prBody", true, new Rect4(ix + 10, dy + 7, iw - 20, 84 - 14), Pen.F(12.5, 400, Rgba.Of(0xf6f2ff)), "");
        cy = dy + 84 + 8;
        if (onDefault)
        {
            var half = (iw - 8) / 2;
            Field("prBranch", "New branch", ix, half, "hover/my-change");
            var save = cy;
            Field("prBase", "Into", ix + half + 8, half, "");
            cy = save;
        }
        else Field("prBase", "Into", ix, iw, "");
        cy += rowH + 8;
        void Check(string id, bool on, string label)
        {
            var row = new Rect4(ix, cy, iw, 24);
            pen.Box(ix, cy + 4, 16, 16, 5, on ? Rgba.Of(0x9b6bff) : Rgba.Of(0xffffff, 0.07));
            pen.Border(ix, cy + 4, 16, 16, 5, 1, on ? Rgba.Of(0x9b6bff) : Rgba.Of(0xffffff, 0.18));
            if (on) pen.Icon(DeskIcons.Check, ix + 2, cy + 6, 12, Rgba.Of(0xffffff));
            Tv(label, ix + 16 + 8, cy, 24, Pen.F(12.5, 400, Rgba.Of(0xececf0)), iw - 24);
            Add(row, "prCheck", 0, id);
            cy += 24 + 8;
        }
        if (changed > 0) Check("commit", form.Commit, $"Commit the {DeskRows.Num(changed)} changed file{(changed == 1 ? "" : "s")} first");
        Check("draft", form.Draft, "Open as a draft");
        if (p.HasResult && new Jv(p.Result)["error"].StrOrNull() is { } error)
        {
            var steps = string.Join(", ", new Jv(p.Result)["steps"].Strings());
            var eh = pen.WrappedHeight(error, iw - 20, Pen.F(12, 400, Pen.Ink));
            var sh = 0f;
            var said = steps.Length > 0 ? "Done before it: " + steps + "." : "";
            if (said.Length > 0) sh = pen.WrappedHeight(said, iw - 20, Pen.F(12, 400, Pen.Ink)) + 2;
            pen.Box(ix, cy, iw, 7 + eh + sh + 7, 8, Rgba.Of(0xff453a, 0.1));
            pen.Wrapped(error, ix + 10, cy + 7, iw - 20, Pen.F(12, 400, Rgba.Of(0xffb0aa)));
            if (said.Length > 0) pen.Wrapped(said, ix + 10, cy + 7 + eh + 2, iw - 20, Pen.F(12, 400, Rgba.Of(0xffb0aa, 0.7)));
            cy += 7 + eh + sh + 7 + 8;
        }
        var titleEmpty = Fld("prTitle", false).Box.Text.Trim().Length == 0;
        var cw = DeskButton("prCreate", p.Creating ? "Opening…" : "Create pull request", DeskIcons.PullRequest, ix, cy, false, true, 0, "", p.Creating || blocked || titleEmpty);
        Tv("Pushes the branch, then opens it with gh.", ix + cw + 10, cy, 30, Pen.F(11.5, 400, Pen.Faint));
        cy += 30 + 12;
        prContent = cy + deskScroll - r.Y;
        pen.Cv.PopClip();
    }

    /// <summary>The words and switches of the form go to the backend.</summary>
    void DeskPrCreateFromForm()
    {
        if (deskPanel is not { } id) return;
        var p = Prefs(id);
        if (p.Form is { } f) ReadForm(f);
        DeskPrCreate();
    }
}

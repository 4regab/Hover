using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// Faults the sweep of the notch, office, new-task bar, chat and desk found against the Go app. Each test here failed before its fix
/// (see reports/sweep1.md); the sweep's own invariants are in SweepTests.
/// </summary>
public class SweepFixTests
{
    static bool Dot(PanelHarness rig)
    {
        var fab = rig.Ui.Find("fab")!.Value.R;
        return rig.Canvas.Boxes.Any(b => b.C == Pen.Lilac && b.R.W == 6 && b.R.H == 6 && fab.Contains(b.R.X + 3, b.R.Y + 3));
    }

    [Fact]
    public void The_new_task_button_shows_its_lilac_dot_only_while_words_wait_in_the_box()
    {
        var rig = MenuFitTests.Rig(1200, 720);
        rig.Ui.ChatView = false;
        rig.Frame();
        Assert.False(Dot(rig), "nothing waits in the box, so the button has no dot");
        Assert.True(rig.Click("fab"));
        rig.Type("fix it");
        rig.Key(UiKey.Escape);
        Assert.NotNull(rig.Ui.Find("fab"));
        Assert.True(Dot(rig), "words wait in the box: the dot shows");
        rig.Ui.ClearTask();
        rig.Frame();
        Assert.False(Dot(rig), "the box is empty again: the dot is gone");
    }

    [Fact]
    public void The_repository_menu_has_its_heading_and_a_search_box_that_filters_and_Enter_picks_the_first_match()
    {
        var rig = MenuFitTests.Rig(1200, 720);
        MenuFitTests.OfficeBox(rig);
        Assert.True(rig.Click("cloud"));
        Assert.True(rig.Click("pillRepo"));
        Assert.Equal("repo", rig.Ui.OpenMenuKind);
        Assert.Contains("KIRO WEB CLONES", rig.Canvas.Texts);
        Assert.Contains("Search repositories", rig.Canvas.Texts);
        Assert.NotNull(rig.Ui.Find("menuSearch"));
        Assert.Equal("", rig.Ui.MenuSearchText);
        Assert.Equal(46, rig.Ui.OpenMenuPickable.Count);

        // Typing keeps the repositories that match (in any case); the heading, the box and "Empty workspace" stay.
        rig.Type("REPO-4");
        Assert.Equal("REPO-4", rig.Ui.MenuSearchText);
        var shown = rig.Ui.Targets.Where(t => t.Id == "menu").Select(t => t.S).ToList();
        Assert.Equal(["Empty workspace", "me/repo-4", "me/repo-40", "me/repo-41", "me/repo-42", "me/repo-43", "me/repo-44", "me/repo-45"], shown);
        rig.Key(UiKey.Backspace);
        Assert.Equal("REPO-", rig.Ui.MenuSearchText);

        // Nothing matches: the Go words, and Enter picks nothing.
        rig.Type("zzz");
        Assert.Equal(["Empty workspace"], rig.Ui.Targets.Where(t => t.Id == "menu").Select(t => t.S));
        Assert.Contains("No repository matches “REPO-zzz”.", rig.Canvas.Texts);
        rig.Key(UiKey.Enter);
        Assert.Equal("repo", rig.Ui.OpenMenuKind);
        rig.Key(UiKey.A, UiMods.Control);
        rig.Type("repo-4");

        // Enter picks the first match and closes the menu; the task is for that repository.
        rig.Key(UiKey.Enter);
        Assert.Null(rig.Ui.OpenMenuKind);
        Assert.Contains("repo-4", rig.Canvas.Texts);
        Assert.True(rig.Click("taskText"));
        rig.Type("web task");
        Assert.True(rig.Click("startTask"));
        Assert.Equal(["me/repo-4"], rig.Last<NewTaskCommand>().Cloud);
    }

    [Fact]
    public void Enter_in_the_repository_search_does_nothing_while_it_is_empty_and_Escape_closes_the_menu()
    {
        var rig = MenuFitTests.Rig(1200, 720);
        MenuFitTests.OfficeBox(rig);
        Assert.True(rig.Click("cloud"));
        Assert.True(rig.Click("pillRepo"));
        rig.Key(UiKey.Enter);
        Assert.Equal("repo", rig.Ui.OpenMenuKind);
        // The Up and Down keys light a row; Enter then picks that row, as in every other menu.
        rig.Key(UiKey.Down);
        rig.Key(UiKey.Down);
        rig.Key(UiKey.Enter);
        Assert.Null(rig.Ui.OpenMenuKind);
        Assert.True(rig.Click("pillRepo"));
        rig.Key(UiKey.Escape);
        Assert.Null(rig.Ui.OpenMenuKind);
    }

    /// <summary>The box's folder chip says "Choose a folder", or the start of it with an ellipsis where the bar is short.</summary>
    static bool Chip(PanelHarness rig) => rig.Canvas.Texts.Any(t => t.StartsWith("Choose", StringComparison.Ordinal) && "Choose a folder".StartsWith(t.TrimEnd('…'), StringComparison.Ordinal));

    [Fact]
    public void The_boxs_folder_chip_says_Choose_a_folder_and_opens_the_folder_dialog_at_once_while_it_has_no_folder()
    {
        var rig = MenuFitTests.Rig(1200, 720, pickedFolder: "");
        MenuFitTests.OfficeBox(rig);
        Assert.True(Chip(rig), "the chip says Choose a folder (cut with an ellipsis in a box this narrow)");
        Assert.DoesNotContain("Folder", rig.Canvas.Texts);
        rig.FolderAnswer = null;
        Assert.True(rig.Click("pillFolder"));
        Assert.Equal(1, rig.FolderAsked);
        Assert.Null(rig.Ui.OpenMenuKind);
        Assert.True(Chip(rig));

        // A folder picked is named by its last part, and the chip then opens the list of recent folders, which ends in "Open folder…".
        rig.FolderAnswer = @"C:\work\picked";
        Assert.True(rig.Click("pillFolder"));
        Assert.Equal(2, rig.FolderAsked);
        Assert.Contains("picked", rig.Canvas.Texts);
        Assert.False(Chip(rig));
        Assert.True(rig.Click("pillFolder"));
        Assert.Equal("project", rig.Ui.OpenMenuKind);
        Assert.Equal(2, rig.FolderAsked);
        Assert.Contains("Open folder…", rig.Canvas.Texts);
        Assert.True(rig.Click("menu", s: "Open folder…"));
        Assert.Equal(3, rig.FolderAsked);
    }

    [Fact]
    public void The_start_screens_folder_chip_always_opens_the_list()
    {
        var rig = MenuFitTests.Rig(1200, 720, pickedFolder: "");
        MenuFitTests.StartScreen(rig);
        Assert.Contains("Choose a folder", rig.Canvas.Texts);
        Assert.True(rig.Click("pillFolder"));
        Assert.Equal("project", rig.Ui.OpenMenuKind);
        Assert.Equal(0, rig.FolderAsked);
    }
}

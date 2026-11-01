using Hover.Voice;
using Xunit;

namespace Hover.Settings.Tests;

/// <summary>The screenshots said aloud during dictation are read from the backend's folder and nowhere else.</summary>
public class DictationShotsTests
{
    static (TempDir Temp, string Root) Folder()
    {
        var temp = new TempDir();
        var root = Directory.CreateDirectory(Path.Combine(temp.Path, "kiro-images")).FullName;
        return (temp, root);
    }

    [Fact]
    public void A_picture_in_the_folder_is_read_as_a_data_address_of_its_kind()
    {
        var (temp, root) = Folder();
        using var _ = temp;
        byte[] bytes = [1, 2, 3, 4, 5];
        foreach (var (name, kind) in new[] { ("a.png", "png"), ("b.JPG", "jpeg"), ("c.jpeg", "jpeg"), ("d.gif", "gif"), ("e.webp", "webp") })
        {
            File.WriteAllBytes(Path.Combine(root, name), bytes);
            Assert.Equal($"data:image/{kind};base64,AQIDBAU=", DictationShots.UrlOf(Path.Combine(root, name), root));
        }
    }

    [Fact]
    public void Any_other_path_a_file_that_is_not_a_picture_an_empty_one_and_a_big_one_are_not_read()
    {
        var (temp, root) = Folder();
        using var _ = temp;
        File.WriteAllBytes(Path.Combine(temp.Path, "outside.png"), [1]);
        Directory.CreateDirectory(root + "-evil");
        File.WriteAllBytes(Path.Combine(root + "-evil", "x.png"), [1]);
        File.WriteAllBytes(Path.Combine(root, "note.txt"), [1]);
        File.WriteAllBytes(Path.Combine(root, "empty.png"), []);
        File.WriteAllBytes(Path.Combine(root, "big.png"), new byte[DictationShots.MaxBytes + 1]);
        File.WriteAllBytes(Path.Combine(root, "ok.png"), [1]);

        Assert.Null(DictationShots.UrlOf(Path.Combine(temp.Path, "outside.png"), root));
        // A folder whose name starts with the same letters is not the folder.
        Assert.Null(DictationShots.UrlOf(Path.Combine(root + "-evil", "x.png"), root));
        // A path that climbs out of it.
        Assert.Null(DictationShots.UrlOf(Path.Combine(root, "..", "outside.png"), root));
        Assert.Null(DictationShots.UrlOf(Path.Combine(root, "note.txt"), root));
        Assert.Null(DictationShots.UrlOf(Path.Combine(root, "empty.png"), root));
        Assert.Null(DictationShots.UrlOf(Path.Combine(root, "big.png"), root));
        Assert.Null(DictationShots.UrlOf(Path.Combine(root, "missing.png"), root));
        // Nothing, odd characters and a folder named as a file.
        Assert.Null(DictationShots.UrlOf("", root));
        Assert.Null(DictationShots.UrlOf("bad\0name.png", root));
        Directory.CreateDirectory(Path.Combine(root, "dir.png"));
        Assert.Null(DictationShots.UrlOf(Path.Combine(root, "dir.png"), root));
        Assert.NotNull(DictationShots.UrlOf(Path.Combine(root, "ok.png"), root));
    }
}

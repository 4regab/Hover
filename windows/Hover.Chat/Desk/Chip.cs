using System.Text.Json;
using System.Text.Json.Nodes;

namespace Hover.Chat.Desk;

/// <summary>
/// Something attached to a reply: a file by reference, a hunk of a change, a quote. The same
/// as the backend's core.Chip (internal/core/ext.go), which checks every chip against the chat's
/// folder before the reply is sent.
/// </summary>
public sealed record Chip(string Kind, string Label, string Source, string? Text = null, bool Live = false, string? Session = null)
{
    /// <summary>What a chip says on the reply box: a reference is marked as one.</summary>
    public string Shown => Live ? Label + " (reference)" : Label;

    /// <summary>The chip as the backend reads it (its field names start with a capital letter).</summary>
    public JsonElement ToJson()
    {
        var o = new JsonObject { ["Kind"] = Kind, ["Label"] = Label, ["Source"] = Source };
        if (Text is not null) o["Text"] = Text;
        if (Live) o["Live"] = true;
        if (Session is not null) o["Session"] = Session;
        return JsonSerializer.SerializeToElement(o);
    }

    /// <summary>A file of the chat's folder, read when the message is sent.</summary>
    public static Chip FileLive(string rel) => new("file", rel, rel, Live: true);
}

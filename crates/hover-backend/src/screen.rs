//! DeskInfo.ScreenAction and DeskInfo.Number (Arz's 760d71e): a computer-use step in words,
//! for the Screen panel's activity ("Clicked", "Typed" and what it was on). The rest of
//! what the Screen panel needs from the backend (`testing`, `apps`, and which steps are
//! computer use) is hover-agents' desk.rs.

use fancy_regex::Regex;
use hover_agents::desk::{field, DeskStep};
use hover_core::json::Json;
use std::sync::LazyLock;

/// The verbs a computer-use title can name, in the order DeskInfo.ScreenVerb tries them at
/// each place in the title (so "double_click" wins over "click" where both start).
const VERBS: [&str; 21] = [
    "double_click", "right_click", "left_click", "click", "type_text", "press_key", "hotkey", "scroll", "drag", "move_mouse", "move_cursor",
    "launch_app", "open_app", "screenshot", "get_window_state", "get_desktop_state", "list_apps", "list_windows", "set_value", "zoom", "invoke_menu",
];

/// The leftmost verb in the title, any case.
fn verb_of(title: &str) -> &'static str {
    let t = title.to_lowercase();
    (0..t.len()).filter(|&i| t.is_char_boundary(i)).find_map(|i| VERBS.iter().find(|v| t[i..].starts_with(**v)).copied()).unwrap_or("")
}

/// The first of these whole-number fields in a step's input JSON (DeskInfo.Number).
pub fn number(json: Option<&str>, names: &[&str]) -> Option<i64> {
    let text = json.filter(|t| t.starts_with('{'))?;
    let v = hover_core::json::parse(text).ok()?;
    names.iter().find_map(|n| match v.get(n) { Some(Json::Num(s)) => s.parse::<i64>().ok(), _ => None })
}

/// `t[..59] + "…"` past 60 UTF-16 units.
fn cut(t: &str) -> String {
    if t.encode_utf16().count() <= 60 { return t.to_owned(); }
    let (mut used, mut end) = (0, 0);
    for (i, ch) in t.char_indices() {
        if used + ch.len_utf16() > 59 { break; }
        used += ch.len_utf16();
        end = i + ch.len_utf8();
    }
    format!("{}…", &t[..end])
}

static KEYS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""keys"\s*:\s*\[([^\]]*)\]"#).unwrap());
static SPACE_TOOL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"computer_(?:type|key|launch|hotkey|move_cursor|get_window|get_accessibility_tree)\b").unwrap());

/// A Cua Space's tools are computer_* (computer_type, computer_key, computer_launch…); Cua
/// Driver's on the user's desktop are type_text, press_key, launch_app. The title with the
/// first kind named as the second.
fn driver_title(title: &str) -> String {
    let (mut out, mut from) = (String::new(), 0);
    for m in SPACE_TOOL.find_iter(title).flatten() {
        out.push_str(&title[from..m.start()]);
        out.push_str(match &m.as_str()["computer_".len()..] {
            "type" => "type_text", "key" => "press_key", "launch" => "launch_app", "get_window" | "get_accessibility_tree" => "get_window_state", v => v,
        });
        from = m.end();
    }
    out + &title[from..]
}

/// A computer-use step in words, as the screen panel's activity lists it: what was done
/// ("Clicked", "Typed") and on what (the text typed, the key, the app).
pub fn action(x: &DeskStep) -> (String, Option<String>) {
    let input = x.input.as_deref();
    let app = field(input, &["app_name", "appName", "name", "app", "bundle_id"]);
    let (did, on): (&str, Option<String>) = match verb_of(&driver_title(&x.title)) {
        "click" | "left_click" => ("Clicked", field(input, &["label", "element_label", "text"]).map(|t| cut(&t)).or_else(|| match (number(input, &["x"]), number(input, &["y"])) {
            (Some(cx), Some(cy)) => Some(format!("at {cx}, {cy}")),
            _ => app,
        })),
        "double_click" => ("Double-clicked", app),
        "right_click" => ("Right-clicked", app),
        "type_text" | "set_value" => ("Typed", field(input, &["text", "value"]).map(|t| cut(&format!("“{t}”")))),
        "press_key" => ("Pressed", field(input, &["key"])),
        "hotkey" => ("Pressed", input.filter(|j| j.contains("keys")).map(|j| {
            let keys = KEYS.captures(j).ok().flatten().and_then(|c| c.get(1)).map_or("", |m| m.as_str());
            cut(&keys.replace('"', "").replace(',', "+"))
        })),
        "scroll" => ("Scrolled", field(input, &["direction"])),
        "drag" => ("Dragged", app),
        "move_mouse" | "move_cursor" => ("Moved the cursor", None),
        "launch_app" | "open_app" => ("Opened", app.or_else(|| field(input, &["bundle_id"]))),
        "screenshot" | "get_desktop_state" | "zoom" => ("Looked at the screen", app),
        "get_window_state" => ("Read the window", app),
        "list_apps" | "list_windows" => ("Listed the apps", None),
        "invoke_menu" => ("Used a menu", field(input, &["path"]).map(|p| cut(&p))),
        _ => ("Used the computer", Some(cut(&x.title))),
    };
    (did.to_owned(), on)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(title: &str, input: Option<&str>) -> DeskStep {
        DeskStep { id: "s".into(), kind: "other".into(), title: title.into(), input: input.map(str::to_owned), ..Default::default() }
    }

    #[test]
    fn a_computer_use_step_in_words() {
        let a = |t, i| action(&step(t, i));
        assert_eq!(a("cua-driver: click", Some(r#"{"label":"Sign in"}"#)), ("Clicked".into(), Some("Sign in".into())));
        assert_eq!(a("mcp__cua-driver__click", Some(r#"{"x":10,"y":20}"#)), ("Clicked".into(), Some("at 10, 20".into())));
        assert_eq!(a("cua-driver/left_click", Some(r#"{"app_name":"Safari"}"#)), ("Clicked".into(), Some("Safari".into())));
        assert_eq!(a("cua-driver: double_click", Some(r#"{"name":"Finder"}"#)), ("Double-clicked".into(), Some("Finder".into())));
        assert_eq!(a("cua-driver: type_text", Some(r#"{"text":"hi"}"#)), ("Typed".into(), Some("“hi”".into())));
        assert_eq!(a("cua-driver: press_key", Some(r#"{"key":"Return"}"#)), ("Pressed".into(), Some("Return".into())));
        assert_eq!(a("cua-driver: hotkey", Some(r#"{"keys":["cmd","s"]}"#)), ("Pressed".into(), Some("cmd+s".into())));
        assert_eq!(a("cua-driver: hotkey", Some(r#"{"other":1}"#)), ("Pressed".into(), None));
        assert_eq!(a("cua-driver: scroll", Some(r#"{"direction":"down"}"#)), ("Scrolled".into(), Some("down".into())));
        assert_eq!(a("cua-driver: launch_app", Some(r#"{"bundle_id":"com.apple.TextEdit"}"#)), ("Opened".into(), Some("com.apple.TextEdit".into())));
        assert_eq!(a("cua-driver: screenshot", Some("{}")), ("Looked at the screen".into(), None));
        assert_eq!(a("cua-driver: list_windows", None), ("Listed the apps".into(), None));
        assert_eq!(a("cua-driver: move_cursor", None), ("Moved the cursor".into(), None));
        assert_eq!(a("Computer use", None), ("Used the computer".into(), Some("Computer use".into())));
        // Any case, leftmost verb.
        assert_eq!(a("CUA Driver: Right_Click", Some(r#"{"app_name":"Notes"}"#)), ("Right-clicked".into(), Some("Notes".into())));
    }

    #[test]
    fn a_cua_spaces_own_tool_names_read_as_cua_drivers() {
        let a = |t, i| action(&step(t, i));
        assert_eq!(a("mcp__cua-space__computer_type", Some(r#"{"text":"Ada"}"#)), ("Typed".into(), Some("“Ada”".into())));
        assert_eq!(a("mcp__cua-space__computer_key", Some(r#"{"key":"return"}"#)), ("Pressed".into(), Some("return".into())));
        assert_eq!(a("mcp__cua-space__computer_launch", Some(r#"{"app":"Safari"}"#)), ("Opened".into(), Some("Safari".into())));
        assert_eq!(a("mcp__cua-space__computer_screenshot", Some("{}")), ("Looked at the screen".into(), None));
        assert_eq!(a("mcp__cua-space__computer_click", Some(r#"{"x":3,"y":4}"#)), ("Clicked".into(), Some("at 3, 4".into())));
        assert_eq!(a("cua-space/computer_hotkey", Some(r#"{"keys":["cmd","s"]}"#)), ("Pressed".into(), Some("cmd+s".into())));
        assert_eq!(a("cua-space/computer_move_cursor", None), ("Moved the cursor".into(), None));
        assert_eq!(a("cua-space/computer_get_window_state", None), ("Read the window".into(), None));
        assert_eq!(a("cua-space/computer_get_accessibility_tree", Some(r#"{"app":"Notes"}"#)), ("Read the window".into(), Some("Notes".into())));
        // Cua Driver's own names are left as they are.
        assert_eq!(a("cua-driver: type_text", Some(r#"{"text":"hi"}"#)), ("Typed".into(), Some("“hi”".into())));
    }

    #[test]
    fn long_texts_are_cut_at_sixty_units() {
        let long = "x".repeat(80);
        let (_, on) = action(&step("cua-driver: type_text", Some(&format!(r#"{{"text":"{long}"}}"#))));
        let on = on.unwrap();
        assert_eq!(on.encode_utf16().count(), 60);
        assert!(on.ends_with('…') && on.starts_with('“'));
    }

    #[test]
    fn numbers_are_whole_numbers_from_the_input() {
        assert_eq!(number(Some(r#"{"ref":12}"#), &["ref"]), Some(12));
        assert_eq!(number(Some(r#"{"ref":1.5,"x":3}"#), &["ref", "x"]), Some(3));
        assert_eq!(number(Some(r#"{"ref":"4"}"#), &["ref"]), None);
        assert_eq!(number(Some("[1]"), &["ref"]), None);
        assert_eq!(number(None, &["ref"]), None);
    }
}

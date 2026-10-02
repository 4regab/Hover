//! Core/Palette.cs: every colour Settings and the app window draw with, as 0xAARRGGBB.
//! Hover's own light and dark are Apple's system colours; any other theme is worked out
//! from a VS Code colour theme: its editor background for the cards, its side bar (or
//! a darker shade) for the panel they sit on, its text colour, its button colour as the
//! accent and its terminal colours for the rest. The shades in between are the text
//! colour at set strengths, so they suit any background.

use crate::json::{self, Json};
use crate::model::SavedTheme;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub name: String,
    pub dark: bool,
    pub ink: u32,
    pub ink_dim: u32,
    pub ink_faint: u32,
    pub fill: u32,
    pub wash: u32,
    pub wash_strong: u32,
    pub separator: u32,
    pub surface: u32,
    pub sheet: u32,
    pub sheet_edge: u32,
    pub panel: u32,
    pub panel_edge: u32,
    pub thumb: u32,
    pub row_hover: u32,
    pub switch_off: u32,
    pub handle: u32,
    pub blue: u32,
    pub green: u32,
    pub purple: u32,
    pub yellow: u32,
    pub teal: u32,
    pub orange: u32,
    pub red: u32,
}

/// A colour theme another editor has installed (Core.InstalledTheme).
#[derive(Clone, Debug, PartialEq)]
pub struct InstalledTheme { pub label: String, pub path: PathBuf, pub dark: bool, pub from: String }

impl Palette {
    // Apple's dark and light system colour tables: label, secondaryLabel,
    // tertiaryLabel; secondarySystemFill, tertiarySystemFill, systemFill; separator;
    // secondarySystemGroupedBackground (cards) and systemGroupedBackground (panel).
    pub fn hover_dark() -> Palette {
        Palette {
            name: "Hover".into(), dark: true,
            ink: 0xFFFFFFFF, ink_dim: 0x99EBEBF5, ink_faint: 0x4DEBEBF5,
            fill: 0x52787880, wash: 0x3D767680, wash_strong: 0x5C787880, separator: 0x99545458,
            surface: 0xFF1C1C1E, sheet: 0xFF2C2C2E, sheet_edge: 0x1FFFFFFF, panel: 0xFF000000, panel_edge: 0x1AFFFFFF,
            thumb: 0xFF636366, row_hover: 0x14FFFFFF, switch_off: 0xFF39393D, handle: 0x66FFFFFF,
            blue: 0xFF0A84FF, green: 0xFF30D158, purple: 0xFFBF5AF2, yellow: 0xFFFFD60A,
            teal: 0xFF64D2FF, orange: 0xFFFF9F0A, red: 0xFFFF453A,
        }
    }

    pub fn hover_light() -> Palette {
        Palette {
            name: "Hover".into(), dark: false,
            ink: 0xFF000000, ink_dim: 0x993C3C43, ink_faint: 0x4D3C3C43,
            fill: 0x29787880, wash: 0x1F767680, wash_strong: 0x33787880, separator: 0x4A3C3C43,
            surface: 0xFFFFFFFF, sheet: 0xFFFFFFFF, sheet_edge: 0x1A000000, panel: 0xFFF2F2F7, panel_edge: 0x1A000000,
            thumb: 0xFFFFFFFF, row_hover: 0x0D000000, switch_off: 0xFFE9E9EB, handle: 0x40000000,
            blue: 0xFF007AFF, green: 0xFF34C759, purple: 0xFFAF52DE, yellow: 0xFFFFCC00,
            teal: 0xFF32ADE6, orange: 0xFFFF9500, red: 0xFFFF3B30,
        }
    }

    pub fn hover(dark: bool) -> Palette { if dark { Self::hover_dark() } else { Self::hover_light() } }

    pub fn from_theme(theme: &SavedTheme) -> Palette {
        let dark = theme.dark;
        let apple = Self::hover(dark);
        let get = |keys: &[&str]| keys.iter().find_map(|k| theme.colors.iter().find(|(n, _)| n == k).and_then(|(_, v)| parse_color(Some(v))));

        let surface = over(get(&["editor.background"]).unwrap_or(if dark { 0xFF1E1E1E } else { 0xFFFFFFFF }), apple.panel);
        // Apple's order: the panel a step darker than the cards on it, in light and dark.
        let side = get(&["sideBar.background", "activityBar.background"]).map(|s| over(s, surface));
        let panel = match side { Some(p) if luma(p) < luma(surface) - 0.01 => p, _ => mix(surface, 0xFF000000, if dark { 0.35 } else { 0.05 }) };
        let mut ink = over(get(&["foreground", "editor.foreground"]).unwrap_or(apple.ink), surface);
        // A theme whose text barely stands off its background would be unreadable here.
        if (luma(ink) - luma(surface)).abs() < 0.3 { ink = if dark { 0xFFF2F2F2 } else { 0xFF1A1A1A }; }
        let accent = |fallback: u32, keys: &[&str]| over(get(keys).unwrap_or(fallback), surface);
        let red = accent(apple.red, &["terminal.ansiRed", "errorForeground"]);
        let yellow = accent(apple.yellow, &["terminal.ansiYellow"]);
        // The accent colours links, the picked tab and the main buttons, so it has to
        // read as a colour: a theme whose buttons are grey gives its blue instead.
        let blue = ["button.background", "focusBorder", "textLink.foreground", "terminal.ansiBlue"].iter()
            .map(|k| get(&[k]).map(|c| over(c, surface)))
            .find(|c| c.is_some_and(|x| saturation(x) >= 0.3)).flatten().unwrap_or(apple.blue);
        let a = |d: u8, l: u8| alpha(ink, if dark { d } else { l });

        Palette {
            name: theme.name.clone(), dark,
            ink,
            ink_dim: get(&["descriptionForeground"]).map_or(alpha(ink, 0x99), |d| over(d, surface)),
            ink_faint: alpha(ink, 0x4D),
            fill: a(0x29, 0x1A),
            wash: a(0x1C, 0x12),
            wash_strong: a(0x2E, 0x1F),
            separator: a(0x26, 0x1F),
            surface,
            sheet: match get(&["menu.background", "editorWidget.background"]) { Some(m) => over(m, surface), None if dark => mix(surface, ink, 0.06), None => surface },
            sheet_edge: alpha(ink, 0x1F),
            panel,
            panel_edge: alpha(ink, 0x1A),
            thumb: if dark { mix(surface, ink, 0.22) } else { mix(surface, 0xFFFFFFFF, 0.8) },
            row_hover: a(0x14, 0x0D),
            switch_off: mix(surface, ink, if dark { 0.16 } else { 0.1 }),
            handle: a(0x66, 0x40),
            blue,
            green: accent(apple.green, &["terminal.ansiGreen"]),
            purple: accent(apple.purple, &["terminal.ansiMagenta"]),
            yellow,
            teal: accent(apple.teal, &["terminal.ansiCyan"]),
            // Few themes name an orange; halfway between their red and yellow is one.
            orange: mix(red, yellow, 0.5),
            red,
        }
    }
}

/// The VS Code colour ids from_theme reads, most wanted first where several can serve.
pub const KEYS: [&str; 18] = [
    "editor.background", "editor.foreground", "foreground", "descriptionForeground",
    "sideBar.background", "activityBar.background", "menu.background", "editorWidget.background",
    "button.background", "focusBorder", "textLink.foreground", "errorForeground",
    "terminal.ansiBlue", "terminal.ansiGreen", "terminal.ansiMagenta", "terminal.ansiYellow",
    "terminal.ansiCyan", "terminal.ansiRed",
];

/// KeySet.TryGetValue: the id under its own spelling, found in any case.
fn key_of(name: &str) -> Option<&'static str> { KEYS.iter().copied().find(|k| k.eq_ignore_ascii_case(name)) }

// MARK: Reading VS Code theme files

fn read_text(p: &Path) -> std::io::Result<String> { std::fs::read(p).map(|b| json::text_of(&b)) }

/// Path.GetFileNameWithoutExtension.
fn stem(p: &Path) -> String {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match name.rfind('.') { Some(i) => name[..i].to_owned(), None => name }
}

/// A VS Code colour theme file, following its "include" chain (the file's own colours
/// win). Label and dark come from the extension that lists the theme, when there is
/// one; otherwise from the file. None if it can't be read.
pub fn read(path: &Path, label: Option<&str>, dark: Option<bool>) -> Option<SavedTheme> {
    let mut colors: Vec<(String, String)> = vec![];
    let mut name: Option<String> = None;
    let mut kind: Option<String> = None;

    fn load(file: &Path, depth: u32, colors: &mut Vec<(String, String)>, name: &mut Option<String>, kind: &mut Option<String>) -> Result<(), String> {
        if depth > 4 { return Ok(()); }
        let root = json::parse_jsonc(&read_text(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let Json::Obj(_) = root else { return Ok(()) };
        if let Some(inc) = root.get("include").and_then(Json::as_str) {
            let parent = file.parent().unwrap_or(Path::new("")).join(inc);
            if parent.is_file() { load(&parent, depth + 1, colors, name, kind)?; }
        }
        if let Some(n) = root.get("name").and_then(Json::as_str) { *name = Some(n.to_owned()); }
        if let Some(t) = root.get("type").and_then(Json::as_str) { *kind = Some(t.to_owned()); }
        if let Some(Json::Obj(cs)) = root.get("colors") {
            for (k, v) in cs {
                // Kept under the spelling above, so a lookup after a reload finds it.
                if let (Some(key), Some(v)) = (key_of(k), v.as_str()) {
                    match colors.iter_mut().find(|(n, _)| n == key) { Some(slot) => slot.1 = v.to_owned(), None => colors.push((key.to_owned(), v.to_owned())) }
                }
            }
        }
        Ok(())
    }

    if let Err(e) = load(path, 0, &mut colors, &mut name, &mut kind) {
        crate::log::line(&format!("theme {} unreadable — {e}", path.display()));
        return None;
    }
    if colors.is_empty() { return None; }
    let is_dark = dark.unwrap_or_else(|| match kind.as_deref() {
        Some("light" | "hcLight") => false,
        Some("dark" | "hc" | "hcDark" | "hc-black") => true,
        _ => !colors.iter().find(|(k, _)| k == "editor.background").and_then(|(_, v)| parse_color(Some(v))).is_some_and(|c| luma(c) > 0.5),
    });
    Some(SavedTheme { name: label.map(str::to_owned).or(name).unwrap_or_else(|| stem(path)), dark: is_dark, colors })
}

/// Where each editor keeps its extensions: the user's own first, then each editor's
/// built-in ones. Windows: the C#'s list. Linux: the same user folders, then where the
/// editors' .deb, .rpm, tarball and snap packages put their built-in extensions. macOS:
/// the same user folders, then inside the editors' .app bundles.
pub fn roots() -> Vec<(&'static str, PathBuf)> {
    let home = crate::platform::home().unwrap_or_default();
    let mut r = vec![
        ("VS Code", home.join(".vscode").join("extensions")),
        ("Cursor", home.join(".cursor").join("extensions")),
        ("Kiro", home.join(".kiro").join("extensions")),
        ("Windsurf", home.join(".windsurf").join("extensions")),
    ];
    let app = |base: PathBuf| base.join("resources").join("app").join("extensions");
    if cfg!(windows) {
        let local = crate::platform::local_app_data().unwrap_or_default();
        let programs = crate::platform::program_files().unwrap_or_default();
        r.push(("VS Code", app(local.join("Programs").join("Microsoft VS Code"))));
        r.push(("VS Code", app(programs.join("Microsoft VS Code"))));
        r.push(("Cursor", app(local.join("Programs").join("cursor"))));
        r.push(("Kiro", app(local.join("Programs").join("Kiro"))));
        r.push(("Windsurf", app(local.join("Programs").join("Windsurf"))));
    } else if cfg!(target_os = "macos") {
        r.extend(macos_builtin(&home));
    } else {
        for (from, dir) in [("VS Code", "/usr/share/code"), ("VS Code", "/opt/visual-studio-code"), ("VS Code", "/snap/code/current/usr/share/code"),
            ("Cursor", "/usr/share/cursor"), ("Cursor", "/opt/cursor"), ("Kiro", "/usr/share/kiro"), ("Kiro", "/opt/kiro"),
            ("Windsurf", "/usr/share/windsurf"), ("Windsurf", "/opt/windsurf")] {
            r.push((from, app(PathBuf::from(dir))));
        }
    }
    r
}

/// macOS: each editor is an .app bundle, in /Applications or in the user's own
/// ~/Applications, with its built-in extensions inside it.
pub fn macos_builtin(home: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut r = vec![];
    for (from, bundle) in [("VS Code", "Visual Studio Code.app"), ("Cursor", "Cursor.app"), ("Kiro", "Kiro.app"), ("Windsurf", "Windsurf.app")] {
        for apps in [PathBuf::from("/Applications"), home.join("Applications")] {
            r.push((from, apps.join(bundle).join("Contents").join("Resources").join("app").join("extensions")));
        }
    }
    r
}

/// The colour themes VS Code, Cursor, Kiro and Windsurf have installed: the
/// extensions the user added first, then each editor's own. One per name.
pub fn installed() -> Vec<InstalledTheme> { installed_in(&roots()) }

pub fn installed_in(roots: &[(&str, PathBuf)]) -> Vec<InstalledTheme> {
    let mut found = vec![];
    let mut names: Vec<String> = vec![];
    for (from, dir) in roots {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut exts: Vec<PathBuf> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
        // Newest version first when an update left the old folder behind
        // (OrderByDescending, OrdinalIgnoreCase).
        exts.sort_by_key(|p| std::cmp::Reverse(p.to_string_lossy().to_uppercase()));
        for ext in exts {
            if ext.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) { continue; }
            if let Err(e) = read_extension(&ext, from, &mut found, &mut names) {
                crate::log::line(&format!("theme extension {} skipped — {e}", ext.display()));
            }
        }
    }
    // OrderBy(Label, CurrentCultureIgnoreCase); stable.
    found.sort_by_key(|t: &InstalledTheme| t.label.to_lowercase());
    found
}

/// JsonElement.GetString: a string, null for null, and a throw (Err) for the rest.
fn get_string(v: &Json) -> Result<Option<String>, String> {
    match v { Json::Str(s) => Ok(Some(s.clone())), Json::Null => Ok(None), _ => Err("the value isn't a string".into()) }
}

/// TryGetProperty on something that isn't an object throws, as it does in .NET.
fn prop<'a>(v: &'a Json, name: &str) -> Result<Option<&'a Json>, String> {
    match v { Json::Obj(_) => Ok(v.get(name)), _ => Err("the element isn't an object".into()) }
}

fn read_extension(ext: &Path, from: &str, found: &mut Vec<InstalledTheme>, names: &mut Vec<String>) -> Result<(), String> {
    let manifest = ext.join("package.json");
    if !manifest.is_file() { return Ok(()); }
    let text = read_text(&manifest).map_err(|e| e.to_string())?;
    if !text.contains("\"themes\"") { return Ok(()); }
    let doc = json::parse_jsonc(&text).map_err(|e| e.to_string())?;
    let Some(contributes) = prop(&doc, "contributes")? else { return Ok(()) };
    let Some(Json::Arr(themes)) = prop(contributes, "themes")? else { return Ok(()) };
    let mut nls: Option<Option<Json>> = None;
    for t in themes {
        let Some(p) = prop(t, "path")? else { continue };
        let Some(rel) = get_string(p)? else { continue };
        let mut label = match prop(t, "label")? { Some(l) => get_string(l)?, None => None };
        // "%themeLabel%" is a key into the extension's package.nls.json.
        if let Some(l) = label.clone().filter(|l| l.len() >= 2 && l.starts_with('%') && l.ends_with('%')) {
            if nls.is_none() {
                let path = ext.join("package.nls.json");
                nls = Some(if path.is_file() { Some(json::parse_jsonc(&read_text(&path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?) } else { None });
            }
            label = match nls.as_ref().unwrap() {
                Some(root) => match prop(root, &l[1..l.len() - 1])? {
                    Some(v @ Json::Str(_)) => get_string(v)?,
                    Some(v) => match prop(v, "message")? { Some(m) => get_string(m)?, None => None },
                    None => None,
                },
                None => None,
            };
        }
        let file = crate::platform::full_path(&ext.join(&rel));
        let mut label = label.unwrap_or_else(|| stem(&file));
        // Some labels pad with runs of spaces, or of invisible Hangul and Braille
        // fillers, to line up in VS Code's picker.
        for filler in ['\u{115F}', '\u{1160}', '\u{3164}', '\u{FFA0}', '\u{2800}'] { label = label.replace(filler, " "); }
        let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
        if !file.is_file() { continue; }
        let up = label.to_uppercase();
        if names.contains(&up) { continue; }
        names.push(up);
        let ui = match prop(t, "uiTheme")? { Some(u) => get_string(u)?, None => None };
        found.push(InstalledTheme { label, path: file, dark: matches!(ui.as_deref(), Some("vs-dark" | "hc-black")), from: from.to_owned() });
    }
    Ok(())
}

// MARK: Colour arithmetic

/// "#rgb", "#rgba", "#rrggbb" or "#rrggbbaa", as VS Code writes them.
pub fn parse_color(s: Option<&str>) -> Option<u32> {
    let hex = s?.strip_prefix('#')?;
    let mut hex: String = if matches!(hex.chars().count(), 3 | 4) { hex.chars().flat_map(|c| [c, c]).collect() } else { hex.to_owned() };
    if hex.chars().count() == 6 { hex.push_str("FF"); }
    if hex.chars().count() != 8 { return None; }
    // uint.TryParse with NumberStyles.HexNumber: white space may lead and trail.
    let t = hex.trim_matches(|c: char| matches!(c, '\t'..='\r' | ' '));
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_hexdigit()) { return None; }
    let rgba = u32::from_str_radix(t, 16).ok()?;
    // RRGGBBAA to AARRGGBB: (rgba >> 8) | (rgba << 24).
    Some(rgba.rotate_right(8))
}

pub fn alpha(argb: u32, a: u8) -> u32 { (argb & 0x00FF_FFFF) | ((a as u32) << 24) }

/// Channel by channel, rounded as Math.Round does (half to even).
pub fn mix(a: u32, b: u32, k: f64) -> u32 {
    let ch = |shift: u32| {
        let x = ((a >> shift) & 0xFF) as f64;
        let y = ((b >> shift) & 0xFF) as f64;
        ((x + (y - x) * k).round_ties_even() as i64 as u8) as u32
    };
    (ch(24) << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// A see-through colour laid over an opaque one, so what is drawn is predictable.
pub fn over(top: u32, under: u32) -> u32 { alpha(mix(under, top | 0xFF00_0000, (top >> 24) as f64 / 255.0), 0xFF) }

/// Relative brightness, 0 to 1, weighted as the eye sees it.
pub fn luma(argb: u32) -> f64 {
    (0.2126 * ((argb >> 16) & 0xFF) as f64 + 0.7152 * ((argb >> 8) & 0xFF) as f64 + 0.0722 * (argb & 0xFF) as f64) / 255.0
}

/// How far from grey, 0 to 1 (HSV saturation).
pub fn saturation(argb: u32) -> f64 {
    let (r, g, b) = (((argb >> 16) & 0xFF) as i32, ((argb >> 8) & 0xFF) as i32, (argb & 0xFF) as i32);
    let max = r.max(g).max(b);
    if max == 0 { 0.0 } else { (max - r.min(g).min(b)) as f64 / max as f64 }
}

/// Theme.Resolve: the saved theme, else Hover's own in the appearance asked for, with
/// System following the platform.
pub fn resolve(theme: Option<&SavedTheme>, appearance: crate::model::Appearance, system_dark: impl FnOnce() -> bool) -> Palette {
    use crate::model::Appearance;
    if let Some(t) = theme { return Palette::from_theme(t); }
    Palette::hover(match appearance { Appearance::Light => false, Appearance::Dark => true, Appearance::System => system_dark() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On a Mac the editors are .app bundles; the built-in themes are inside them.
    #[test]
    fn macos_looks_inside_the_editors_app_bundles() {
        let r = macos_builtin(Path::new("/Users/u"));
        assert_eq!(r.len(), 8);
        assert_eq!(r[0], ("VS Code", PathBuf::from("/Applications/Visual Studio Code.app/Contents/Resources/app/extensions")));
        assert_eq!(r[1], ("VS Code", PathBuf::from("/Users/u/Applications/Visual Studio Code.app/Contents/Resources/app/extensions")));
        assert!(r.iter().any(|(from, p)| *from == "Cursor" && p.starts_with("/Applications/Cursor.app")));
        assert!(r.iter().any(|(from, p)| *from == "Windsurf" && p.starts_with("/Users/u/Applications/Windsurf.app")));
    }

    fn theme(dark: bool, colors: &[(&str, &str)]) -> SavedTheme {
        SavedTheme { name: "T".into(), dark, colors: colors.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }

    /// ParseColor's cases: #rgb and #rgba doubled, #rrggbb made opaque, then RGBA to ARGB.
    #[test]
    fn colours_parse_as_vs_code_writes_them() {
        assert_eq!(parse_color(Some("#1e1e1e")), Some(0xFF1E1E1E));
        assert_eq!(parse_color(Some("#abc")), Some(0xFFAABBCC));
        assert_eq!(parse_color(Some("#abc8")), Some(0x88AABBCC));
        assert_eq!(parse_color(Some("#11223344")), Some(0x44112233));
        for bad in [None, Some(""), Some("#"), Some("1e1e1e"), Some("#12345"), Some("#ggg"), Some("#1234567890")] {
            assert_eq!(parse_color(bad), None, "{bad:?}");
        }
    }

    /// Mix rounds half to even (Math.Round); Over lays a see-through colour on an opaque one.
    #[test]
    fn colour_arithmetic_as_the_csharp_does_it() {
        assert_eq!(mix(0xFF1E1E1E, 0xFF000000, 0.35), 0xFF141414); // 19.5 -> 20
        assert_eq!(mix(0xFFCD3131, 0xFFE5E510, 0.5), 0xFFD98B20); // 32.5 -> 32
        assert_eq!(over(0x80FFFFFF, 0xFF000000), 0xFF808080);
        assert_eq!(over(0x00FFFFFF, 0xFF123456), 0xFF123456);
        assert_eq!(alpha(0xFF112233, 0x4D), 0x4D112233);
        assert!((luma(0xFFFFFFFF) - 1.0).abs() < 1e-9 && luma(0xFF000000) == 0.0);
        assert_eq!(saturation(0xFF3C3C3C), 0.0);
        assert!((saturation(0xFF0E639C) - 142.0 / 156.0).abs() < 1e-9);
    }

    /// Palette.From on a Dark+-like theme; each value worked out from its line.
    #[test]
    fn a_vs_code_theme_becomes_a_palette() {
        let p = Palette::from_theme(&theme(true, &[("editor.background", "#1e1e1e"), ("foreground", "#cccccc"), ("sideBar.background", "#252526"),
            ("button.background", "#0e639c"), ("terminal.ansiRed", "#cd3131"), ("terminal.ansiYellow", "#e5e510")]));
        assert_eq!((p.surface, p.panel, p.ink, p.ink_dim, p.ink_faint), (0xFF1E1E1E, 0xFF141414, 0xFFCCCCCC, 0x99CCCCCC, 0x4DCCCCCC));
        assert_eq!((p.fill, p.wash, p.wash_strong, p.separator, p.row_hover, p.handle), (0x29CCCCCC, 0x1CCCCCCC, 0x2ECCCCCC, 0x26CCCCCC, 0x14CCCCCC, 0x66CCCCCC));
        assert_eq!((p.sheet, p.thumb, p.switch_off), (0xFF282828, 0xFF444444, 0xFF3A3A3A));
        assert_eq!((p.blue, p.red, p.yellow, p.orange, p.green), (0xFF0E639C, 0xFFCD3131, 0xFFE5E510, 0xFFD98B20, 0xFF30D158));
        // The side bar darker than the cards is the panel.
        let q = Palette::from_theme(&theme(false, &[("editor.background", "#ffffff"), ("sideBar.background", "#f3f3f3")]));
        assert_eq!((q.surface, q.panel, q.ink, q.sheet, q.thumb), (0xFFFFFFFF, 0xFFF3F3F3, 0xFF000000, 0xFFFFFFFF, 0xFFFFFFFF));
        // Grey buttons give the next colour that reads as one; text too close to its
        // background is replaced.
        let g = Palette::from_theme(&theme(true, &[("editor.background", "#1e1e1e"), ("foreground", "#222222"), ("button.background", "#3c3c3c"), ("textLink.foreground", "#3794ff")]));
        assert_eq!((g.blue, g.ink), (0xFF3794FF, 0xFFF2F2F2));
        assert_eq!(Palette::from_theme(&theme(true, &[("button.background", "#3c3c3c")])).blue, Palette::hover_dark().blue);
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-palette-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Palette.Read: JSONC, the include chain (own colours win, depth 4 at most), the
    /// names under their own spelling, the name and type from the files.
    #[test]
    fn a_theme_file_is_read_with_its_includes() {
        let d = temp("read");
        std::fs::write(d.join("base.json"), "{ // base\n \"name\": \"Base\", \"type\": \"light\", \"colors\": { \"EDITOR.background\": \"#101010\", \"foreground\": \"#eeeeee\", \"tab.border\": \"#ff0000\", }, }").unwrap();
        std::fs::write(d.join("mine.json"), "{ \"include\": \"./base.json\", \"name\": \"Mine\", /* no type */ \"colors\": { \"foreground\": \"#dddddd\" } }").unwrap();
        let t = read(&d.join("mine.json"), None, None).unwrap();
        assert_eq!(t.name, "Mine");
        // The type came from the include: light, whatever the background says.
        assert!(!t.dark);
        assert_eq!(t.colors, vec![("editor.background".to_string(), "#101010".to_string()), ("foreground".to_string(), "#dddddd".to_string())]);
        let t = read(&d.join("mine.json"), Some("Label"), Some(true)).unwrap();
        assert_eq!((t.name.as_str(), t.dark), ("Label", true));
        // No type anywhere: dark unless the background is bright.
        std::fs::write(d.join("x.theme.json"), "{\"colors\":{\"editor.background\":\"#fafafa\"}}").unwrap();
        let t = read(&d.join("x.theme.json"), None, None).unwrap();
        assert_eq!((t.name.as_str(), t.dark), ("x.theme", false));
        // Nothing Hover reads, not JSON, or a loop of includes.
        std::fs::write(d.join("none.json"), "{\"colors\":{\"tab.border\":\"#fff\"}}").unwrap();
        std::fs::write(d.join("bad.json"), "{").unwrap();
        std::fs::write(d.join("loop.json"), "{\"include\":\"loop.json\",\"colors\":{\"foreground\":\"#fff\"}}").unwrap();
        assert!(read(&d.join("none.json"), None, None).is_none());
        assert!(read(&d.join("bad.json"), None, None).is_none());
        assert!(read(&d.join("missing.json"), None, None).is_none());
        assert_eq!(read(&d.join("loop.json"), None, None).unwrap().colors.len(), 1);
    }

    fn extension(root: &Path, dir: &str, manifest: &str, nls: Option<&str>, files: &[&str]) {
        let e = root.join(dir);
        std::fs::create_dir_all(e.join("themes")).unwrap();
        std::fs::write(e.join("package.json"), manifest).unwrap();
        if let Some(n) = nls { std::fs::write(e.join("package.nls.json"), n).unwrap(); }
        for f in files { std::fs::write(e.join("themes").join(f), "{\"colors\":{\"foreground\":\"#fff\"}}").unwrap(); }
    }

    /// Palette.Installed and ReadExtension: labels from package.nls.json, the fillers
    /// taken out, one theme per name (the newest folder first), a broken extension
    /// skipped, the list sorted by label.
    #[test]
    fn installed_themes_are_found_in_the_editors_folders() {
        let root = temp("installed");
        let user = root.join("user");
        let builtin = root.join("builtin");
        extension(&user, "acme.night-1.0.0", r#"{"contributes":{"themes":[{"label":"Night  Owl","uiTheme":"vs-dark","path":"./themes/old.json"}]}}"#, None, &["old.json"]);
        extension(&user, "acme.night-1.2.0", r#"{"contributes":{"themes":[{"label":"Night Owl","uiTheme":"vs-dark","path":"./themes/new.json"}]}}"#, None, &["new.json"]);
        extension(&user, ".obsolete", r#"{"contributes":{"themes":[{"label":"Hidden","path":"./themes/h.json"}]}}"#, None, &["h.json"]);
        extension(&user, "broken", r#"{"contributes":{"themes":[{"label":"Fine","path":"./themes/f.json"},{"path":5}]}}"#, None, &["f.json"]);
        extension(&builtin, "theme-defaults", r#"{"contributes":{"themes":[{"label":"%light%","uiTheme":"vs","path":"./themes/light.json"},{"label":"%dark%","uiTheme":"hc-black","path":"./themes/dark.json"},{"label":"Missing","path":"./themes/gone.json"},{"path":"./themes/Plain Name.json"}]}}"#,
            Some(r#"{"light":"Light\u3164\u3164Modern","dark":{"message":"Dark High Contrast"}}"#), &["light.json", "dark.json", "Plain Name.json"]);
        extension(&builtin, "no-themes", r#"{"name":"x"}"#, None, &[]);
        let found = installed_in(&[("VS Code", user.clone()), ("Kiro", builtin.clone()), ("Cursor", root.join("absent"))]);
        let got: Vec<(&str, bool, &str, String)> = found.iter().map(|t| (t.label.as_str(), t.dark, t.from.as_str(), t.path.file_name().unwrap().to_string_lossy().into_owned())).collect();
        assert_eq!(got, vec![
            ("Dark High Contrast", true, "Kiro", "dark.json".into()),
            // The broken extension's first theme was already listed when the second threw.
            ("Fine", false, "VS Code", "f.json".into()),
            ("Light Modern", false, "Kiro", "light.json".into()),
            ("Night Owl", true, "VS Code", "new.json".into()),
            ("Plain Name", false, "Kiro", "Plain Name.json".into()),
        ]);
    }
}

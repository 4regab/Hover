//! The places voice may start work: the projects the user registered (each a folder,
//! the words that name it, whether voice may use it, and its own tool access), the
//! default workspace for everything else, and the voice settings themselves. Kept in
//! settings.json beside the 2.x keys; 2.x ignores keys it doesn't know.

use crate::json::{Json, JsonError, Result};
use crate::model::{opt_text, text};
use crate::shortcut::{Key, Modifiers, Shortcut};
use std::path::{Path, PathBuf};

/// The access ids AgentOptions::with_access takes. A new target asks first: being
/// registered never grants full access on its own.
pub const ACCESS_IDS: [&str; 4] = ["full", "risky", "always", "read"];
pub const NEW_TARGET_ACCESS: &str = "risky";

fn access_or_default(v: Option<String>) -> String {
    v.filter(|a| ACCESS_IDS.contains(&a.as_str())).unwrap_or_else(|| NEW_TARGET_ACCESS.into())
}

/// A registered project.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// Stable, made once (a GUID's hex); the name and folder can change.
    pub id: String,
    pub name: String,
    pub folder: String,
    pub aliases: Vec<String>,
    /// Voice may start tasks here.
    pub voice: bool,
    pub access: String,
}

impl Project {
    pub fn new(name: &str, folder: &str) -> Project {
        Project { id: crate::guid_n(), name: name.trim().into(), folder: folder.into(), aliases: vec![], voice: true, access: NEW_TARGET_ACCESS.into() }
    }

    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Id", Json::str(&self.id)), ("Name", Json::str(&self.name)), ("Folder", Json::str(&self.folder)),
            ("Aliases", Json::Arr(self.aliases.iter().map(Json::str).collect())), ("Voice", Json::Bool(self.voice)), ("Access", Json::str(&self.access))])
    }

    pub fn from_json(v: &Json) -> Result<Project> {
        v.props()?;
        let id = text(v.get("Id"))?;
        Ok(Project {
            id: if id.is_empty() { crate::guid_n() } else { id },
            name: text(v.get("Name"))?,
            folder: text(v.get("Folder"))?,
            aliases: v.get("Aliases").map(|a| a.opt_list(|x| Ok(x.opt_str()?.unwrap_or_default()))).transpose()?.flatten().unwrap_or_default()
                .into_iter().map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect(),
            voice: v.get("Voice").map(Json::bool).transpose()?.unwrap_or(true),
            access: access_or_default(opt_text(v.get("Access"))?),
        })
    }
}

/// Where a voice task goes when no project is clearly named. None is the user's home
/// plus "Hover", found when it is needed.
#[derive(Clone, Debug, PartialEq)]
pub struct Workspace { pub folder: Option<String>, pub access: String }

impl Default for Workspace {
    fn default() -> Self { Workspace { folder: None, access: NEW_TARGET_ACCESS.into() } }
}

impl Workspace {
    /// The folder, the configured one or home + Hover; None only with no home at all.
    pub fn path(&self) -> Option<PathBuf> {
        match self.folder.as_deref().filter(|f| !f.trim().is_empty()) {
            Some(f) => Some(PathBuf::from(f)),
            None => default_workspace(),
        }
    }

    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Folder", Json::opt_str_of(self.folder.as_deref())), ("Access", Json::str(&self.access))])
    }

    pub fn from_json(v: &Json) -> Result<Workspace> {
        v.props()?;
        Ok(Workspace { folder: opt_text(v.get("Folder"))?.filter(|f| !f.trim().is_empty()), access: access_or_default(opt_text(v.get("Access"))?) })
    }
}

/// The user's home plus Hover: C:\Users\<name>\Hover, ~/Hover.
pub fn default_workspace() -> Option<PathBuf> { crate::platform::home().map(|h| h.join("Hover")) }

/// Which service tidies a transcript, when cleanup is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CleanupProvider { #[default] Gemini, OpenAi, Custom }

impl CleanupProvider {
    pub const ALL: [CleanupProvider; 3] = [CleanupProvider::Gemini, CleanupProvider::OpenAi, CleanupProvider::Custom];
    const NAMES: [&'static str; 3] = ["Gemini", "OpenAI", "Custom"];
    pub fn name(self) -> &'static str { Self::NAMES[self as usize] }
    /// The OpenAI-compatible base each preset uses (Gemini's from its compatibility
    /// docs). Custom has the user's own.
    pub fn base(self) -> Option<&'static str> {
        match self {
            CleanupProvider::Gemini => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
            CleanupProvider::OpenAi => Some("https://api.openai.com/v1"),
            CleanupProvider::Custom => None,
        }
    }
    /// The secret store's name for its key.
    pub fn secret(self) -> &'static str { ["cleanup.gemini", "cleanup.openai", "cleanup.custom"][self as usize] }
}

/// Groq's transcription models (console.groq.com/docs/speech-to-text, checked
/// 2026-10-01). Both detect the language when none is given.
pub const TRANSCRIBE_MODELS: [(&str, &str); 2] = [("whisper-large-v3-turbo", "Whisper Large V3 Turbo"), ("whisper-large-v3", "Whisper Large V3")];
pub const GROQ_SECRET: &str = "voice.groq";

/// Where speech becomes text. A file without the key (2.x, or voice set up before Local
/// existed) reads as Cloud, so nothing is downloaded or changed on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpeechMode { Local, #[default] Cloud }

impl SpeechMode {
    pub const ALL: [SpeechMode; 2] = [SpeechMode::Local, SpeechMode::Cloud];
    const NAMES: [&'static str; 2] = ["Local", "Cloud"];
    pub fn id(self) -> &'static str { Self::NAMES[self as usize] }
    pub fn label(self) -> &'static str { ["Local (Phonon)", "Cloud (Groq)"][self as usize] }
}

/// The local model that passed its check: what it is, which pinned release, and where
/// it was put. Written only once it is Ready.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalModel { pub id: String, pub version: String, pub folder: String }

impl LocalModel {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Id", Json::str(&self.id)), ("Version", Json::str(&self.version)), ("Folder", Json::str(&self.folder))])
    }
    pub fn from_json(v: &Json) -> Result<LocalModel> {
        v.props()?;
        Ok(LocalModel { id: text(v.get("Id"))?, version: text(v.get("Version"))?, folder: text(v.get("Folder"))? })
    }
}

/// Voice: off until switched on.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub shortcut: Shortcut,
    /// The input device's name; None is the system's default.
    pub microphone: Option<String>,
    pub speech: SpeechMode,
    pub local: Option<LocalModel>,
    /// Groq's transcription model (Cloud).
    pub model: String,
    pub cleanup: bool,
    pub cleanup_provider: CleanupProvider,
    pub cleanup_model: Option<String>,
    /// Custom's base URL (…/v1).
    pub cleanup_base: Option<String>,
}

impl VoiceSettings {
    /// Ctrl+Alt+Space, the mockup's.
    pub const SHORTCUT: Shortcut = Shortcut { key: Key(18), modifiers: Modifiers(Modifiers::CONTROL.0 | Modifiers::ALT.0) };
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings { enabled: false, shortcut: Self::SHORTCUT, microphone: None, speech: SpeechMode::Cloud, local: None, model: TRANSCRIBE_MODELS[0].0.into(), cleanup: false,
            cleanup_provider: CleanupProvider::Gemini, cleanup_model: None, cleanup_base: None }
    }
}

impl VoiceSettings {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Enabled", Json::Bool(self.enabled)), ("Shortcut", self.shortcut.to_json()), ("Microphone", Json::opt_str_of(self.microphone.as_deref())),
            ("Speech", Json::str(self.speech.id())), ("Local", self.local.as_ref().map(LocalModel::to_json).unwrap_or(Json::Null)),
            ("Model", Json::str(&self.model)), ("Cleanup", Json::Bool(self.cleanup)), ("CleanupProvider", Json::str(self.cleanup_provider.name())),
            ("CleanupModel", Json::opt_str_of(self.cleanup_model.as_deref())), ("CleanupBase", Json::opt_str_of(self.cleanup_base.as_deref()))])
    }

    pub fn from_json(v: &Json) -> Result<VoiceSettings> {
        v.props()?;
        let d = VoiceSettings::default();
        let model = opt_text(v.get("Model"))?.filter(|m| TRANSCRIBE_MODELS.iter().any(|t| t.0 == m)).unwrap_or(d.model);
        Ok(VoiceSettings {
            enabled: v.get("Enabled").map(Json::bool).transpose()?.unwrap_or(false),
            shortcut: match v.get("Shortcut") { None | Some(Json::Null) => d.shortcut, Some(s) => Shortcut::from_json(s)? },
            microphone: opt_text(v.get("Microphone"))?.filter(|m| !m.is_empty()),
            speech: match v.get("Speech") {
                None | Some(Json::Null) => SpeechMode::Cloud,
                Some(x) => x.enum_of(&SpeechMode::NAMES)?.map(|i| SpeechMode::ALL[i]).ok_or_else(|| JsonError("not a SpeechMode".into()))?,
            },
            local: match v.get("Local") { None | Some(Json::Null) => None, Some(l) => Some(LocalModel::from_json(l)?) },
            model,
            cleanup: v.get("Cleanup").map(Json::bool).transpose()?.unwrap_or(false),
            cleanup_provider: match v.get("CleanupProvider") {
                None | Some(Json::Null) => d.cleanup_provider,
                Some(x) => x.enum_of(&CleanupProvider::NAMES)?.map(|i| CleanupProvider::ALL[i]).ok_or_else(|| JsonError("not a CleanupProvider".into()))?,
            },
            cleanup_model: opt_text(v.get("CleanupModel"))?.map(|m| m.trim().to_owned()).filter(|m| !m.is_empty()),
            cleanup_base: opt_text(v.get("CleanupBase"))?.map(|m| m.trim().to_owned()).filter(|m| !m.is_empty()),
        })
    }
}

/// A folder as the system resolves it: absolute, links followed, and on Windows
/// without the \\?\ prefix canonicalize adds. Err says why it can't be used.
pub fn resolve_folder(p: &str) -> std::result::Result<PathBuf, String> {
    let t = p.trim();
    if t.is_empty() { return Err("No folder is set.".into()); }
    let path = Path::new(t);
    if !path.is_absolute() { return Err(format!("{t} isn’t a full path.")); }
    let c = std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("{t} isn’t there any more."),
        std::io::ErrorKind::PermissionDenied => format!("Hover isn’t allowed to open {t}."),
        _ => format!("{t} can’t be opened: {e}"),
    })?;
    if !c.is_dir() { return Err(format!("{t} isn’t a folder.")); }
    std::fs::read_dir(&c).map_err(|e| format!("Hover can’t read {t}: {e}"))?;
    Ok(plain(c))
}

/// canonicalize's \\?\C:\x as C:\x and \\?\UNC\h\s as \\h\s, which every tool takes.
#[cfg(windows)]
fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(r) = s.strip_prefix(r"\\?\UNC\") { return PathBuf::from(format!(r"\\{r}")); }
    if let Some(r) = s.strip_prefix(r"\\?\") { return PathBuf::from(r); }
    p
}
#[cfg(not(windows))]
fn plain(p: PathBuf) -> PathBuf { p }

/// The same folder: their resolved paths are equal, any case on Windows (where the
/// file system ignores it), exactly on Linux. A folder that can't be resolved is
/// compared by its text.
pub fn same_folder(a: &str, b: &str) -> bool {
    let r = |p: &str| resolve_folder(p).map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|_| p.trim().trim_end_matches(['\\', '/']).to_owned());
    let (x, y) = (r(a), r(b));
    if cfg!(windows) { x.to_lowercase() == y.to_lowercase() } else { x == y }
}

/// The default workspace made when first needed (never a repository, only the folder).
pub fn ensure_folder(p: &Path) -> std::result::Result<PathBuf, String> {
    if !p.exists() { std::fs::create_dir_all(p).map_err(|e| format!("Hover couldn’t make {}: {e}", p.display()))?; }
    resolve_folder(&p.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_bad_values_take_safe_defaults() {
        let mut p = Project::new(" Hover site ", "/x");
        p.aliases = vec!["the site".into()];
        let back = Project::from_json(&p.to_json()).unwrap();
        assert_eq!(back, Project { name: "Hover site".into(), ..p.clone() });
        assert_eq!(back.access, "risky", "registering never grants full access");
        let odd = crate::json::parse(r#"{"Name":"A","Folder":"/a","Access":"everything","Aliases":[" b ",""]}"#).unwrap();
        let o = Project::from_json(&odd).unwrap();
        assert_eq!((o.access.as_str(), o.aliases.clone(), o.voice, o.id.len()), ("risky", vec!["b".to_string()], true, 32));
        let v = VoiceSettings::default();
        assert!(!v.enabled, "voice is off until switched on");
        assert_eq!(v.shortcut.label(), "Ctrl+Alt+Space");
        assert_eq!(VoiceSettings::from_json(&v.to_json()).unwrap(), v);
        let m = VoiceSettings::from_json(&crate::json::parse(r#"{"Model":"made-up","CleanupProvider":"openai"}"#).unwrap()).unwrap();
        assert_eq!((m.model.as_str(), m.cleanup_provider), ("whisper-large-v3-turbo", CleanupProvider::OpenAi));
        assert_eq!((m.speech, m.local.clone()), (SpeechMode::Cloud, None), "Groq-only settings stay Cloud and nothing is installed");
        let l = VoiceSettings { speech: SpeechMode::Local, local: Some(LocalModel { id: "phonon-2".into(), version: "x".into(), folder: "/p".into() }), ..v.clone() };
        assert_eq!(VoiceSettings::from_json(&l.to_json()).unwrap(), l);
        assert_eq!(Workspace::from_json(&Workspace::default().to_json()).unwrap(), Workspace::default());
    }

    #[test]
    fn folders_resolve_once_whatever_way_they_are_written() {
        let base = std::env::temp_dir().join(format!("hover-proj ü {}", crate::guid_n()));
        let sub = base.join("Dir with space");
        std::fs::create_dir_all(&sub).unwrap();
        let s = sub.to_string_lossy().into_owned();
        assert!(resolve_folder(&s).is_ok());
        assert!(same_folder(&s, &format!("{s}{}", std::path::MAIN_SEPARATOR)));
        assert!(same_folder(&s, &base.join("Dir with space").join("..").join("Dir with space").to_string_lossy()));
        if cfg!(windows) { assert!(same_folder(&s, &s.to_uppercase())); } else { assert!(!same_folder(&s, &s.to_uppercase())); }
        #[cfg(unix)]
        {
            let link = base.join("link");
            std::os::unix::fs::symlink(&sub, &link).unwrap();
            assert!(same_folder(&s, &link.to_string_lossy()), "a link is the folder it points at");
        }
        assert!(resolve_folder(&base.join("gone").to_string_lossy()).unwrap_err().contains("isn’t there"));
        assert!(resolve_folder("relative/x").is_err());
        let made = ensure_folder(&base.join("Hover")).unwrap();
        assert!(made.is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }
}

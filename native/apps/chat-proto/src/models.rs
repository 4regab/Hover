//! The model pill and its menu, as main.js builds them from the state's `tools`
//! (renderPill, openMenu, EFFORT), and the pick the page posts (`setModel`).

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub models: Vec<(String, String)>,
    pub model: Option<String>,
    pub efforts: Vec<String>,
    pub effort: Option<String>,
}

pub fn tools(state: &Value) -> Vec<Tool> {
    let s = |v: &Value| v.as_str().map(str::to_string);
    state["tools"].as_array().map(|a| a.iter().map(|t| Tool {
        id: s(&t["id"]).unwrap_or_default(),
        name: s(&t["name"]).unwrap_or_default(),
        models: t["models"].as_array().map(|m| m.iter().map(|m| (s(&m["id"]).unwrap_or_default(), s(&m["name"]).unwrap_or_default())).collect()).unwrap_or_default(),
        model: s(&t["model"]),
        efforts: t["efforts"].as_array().map(|e| e.iter().filter_map(s).collect()).unwrap_or_default(),
        effort: s(&t["effort"]),
    }).collect()).unwrap_or_default()
}

/// main.js EFFORT: 'xhigh' is X-High; anything else, its first letter capitalised.
pub fn effort_name(e: &str) -> String {
    if e == "xhigh" { return "X-High".into(); }
    let mut c = e.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

impl Tool {
    /// The current model: the one whose id is `model` (or ''), else the first.
    pub fn current(&self) -> usize {
        let want = self.model.clone().unwrap_or_default();
        self.models.iter().position(|(id, _)| *id == want).unwrap_or(0)
    }

    /// The pill: the model's name (or Default), and the effort when the tool has efforts.
    pub fn pill(&self) -> (String, String) {
        let name = self.models.get(self.current()).map(|m| m.1.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Default".into());
        let effort = match (&self.effort, self.efforts.is_empty()) { (Some(e), false) if !e.is_empty() => effort_name(e), _ => String::new() };
        (name, effort)
    }

    /// The menu's rows: (label, checked) for models, then efforts. The current model is
    /// `model || models[0].id`, so with no match nothing is checked, as in the page.
    pub fn menu(&self) -> (Vec<(String, bool)>, Vec<(String, bool)>) {
        let cur = self.model.clone().filter(|m| !m.is_empty()).or_else(|| self.models.first().map(|m| m.0.clone())).unwrap_or_default();
        let models = self.models.iter().map(|(id, n)| (n.clone(), *id == cur)).collect();
        let efforts = self.efforts.iter().map(|e| (effort_name(e), self.effort.as_deref() == Some(e.as_str()))).collect();
        (models, efforts)
    }

    /// Picks a model or an effort, and gives what the page posts to the host.
    pub fn pick(&mut self, model: Option<usize>, effort: Option<usize>) -> Value {
        if let Some(m) = model.and_then(|i| self.models.get(i)) { self.model = Some(m.0.clone()); }
        if let Some(e) = effort.and_then(|i| self.efforts.get(i)) { self.effort = Some(e.clone()); }
        serde_json::json!({ "type": "setModel", "tool": self.id, "model": self.model, "effort": self.effort })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_and_menu_as_the_page_builds_them() {
        let st: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../golden/fixtures/office-state.json")).unwrap()).unwrap();
        let mut t = tools(&st["state"]);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].pill(), ("Auto".into(), "High".into()));
        assert_eq!(t[1].pill(), ("Default".into(), String::new()), "codex: no efforts, model ''");
        assert!(t[2].models.is_empty(), "cursor's pill is hidden");
        let (m, e) = t[0].menu();
        assert_eq!(m, vec![("Auto".into(), true), ("Claude Opus 5.5".into(), false)]);
        assert_eq!(e.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Low", "Medium", "High"]);
        assert!(e[2].1);
        let msg = t[0].pick(Some(1), None);
        assert_eq!(msg, serde_json::json!({"type": "setModel", "tool": "kiro", "model": "claude-opus-5.5", "effort": "high"}));
        assert_eq!(t[0].pill().0, "Claude Opus 5.5");
        t[0].pick(None, Some(0));
        assert_eq!(t[0].pill().1, "Low");
        assert_eq!(effort_name("xhigh"), "X-High");
    }
}

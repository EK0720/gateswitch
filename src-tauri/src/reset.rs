//! "Khôi phục mặc định": take every app back to how it was before this tool,
//! no matter how many setups happened since. Undo per change is not enough
//! for that: after several setups (and edits by the user in between) undoing
//! them one by one in the right order is hard.
//!
//! Two modes, both recorded as ONE change, so the reset itself can be undone:
//! - "clean": remove only what this tool adds (routing keys, our provider
//!   tables, our catalog, our picker, our Claude Desktop entries, our env vars).
//!   Everything else in the files is kept, including edits made later.
//! - "original": put back the exact file the tool saw the first time it wrote
//!   there (encrypted copy in backups/first). Later edits by the user are lost.

use crate::secrets::Env;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Item {
    /// "claude-code" | "codex" | "orca" | "claude-desktop"
    pub app: String,
    pub path: String,
    /// "edit" | "delete" | "env"
    pub action: String,
    /// What would change, in plain words. Never contains a value.
    pub changes: Vec<String>,
    /// A first original exists, so "original" mode can be used for this file.
    pub has_original: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub apps: Vec<String>,
    /// "clean" (default) or "original"
    #[serde(default)]
    pub mode: String,
}

/// One planned edit: file writes, deletes and env removals.
#[derive(Default)]
struct Plan {
    items: Vec<Item>,
    writes: Vec<(PathBuf, Vec<u8>)>,
    deletes: Vec<PathBuf>,
    env: Vec<(String, Option<String>)>,
}

/// Remove our keys from Claude Code's settings.json. Returns new text and what changed.
pub fn clean_claude(text: &str, owned_headers: &[String]) -> Result<(String, Vec<String>), String> {
    let mut doc: Map<String, Value> = if text.trim().is_empty() {
        Map::new()
    } else {
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("settings.json has a syntax error: {e}"))?
    };
    let mut changes = vec![];
    let had_gateway = doc.get("env").and_then(|e| e.get("ANTHROPIC_BASE_URL")).is_some();
    if let Some(Value::Object(env)) = doc.get_mut("env") {
        for k in crate::claude::ROUTING_ENV {
            if env.remove(*k).is_some() {
                changes.push(format!("Remove env {k}"));
            }
        }
        if crate::claude::is_owned_headers(env.get("ANTHROPIC_CUSTOM_HEADERS"), owned_headers) {
            env.remove("ANTHROPIC_CUSTOM_HEADERS");
            changes.push("Remove env ANTHROPIC_CUSTOM_HEADERS".into());
        }
        if env.is_empty() {
            doc.remove("env");
        }
    }
    // Setup pins the model at the top level; drop it only together with the gateway.
    if had_gateway && doc.remove("model").is_some() {
        changes.push("Remove the selected model".into());
    }
    if crate::claude::is_owned_picker(doc.get("modelPicker")) {
        doc.remove("modelPicker");
        changes.push("Remove the gateway's /model menu".into());
    }
    let mut out = serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok((out, changes))
}

/// ANTHROPIC_CUSTOM_HEADERS values the saved providers would write.
pub(crate) fn owned_header_values(saved: &[crate::providers::Provider]) -> Vec<String> {
    saved.iter().filter(|p| !p.headers.is_empty()).map(|p| crate::headers::to_env(&p.headers)).collect()
}

/// Remove our provider tables, routing and catalog from a Codex config.toml.
/// Returns new text, what changed and the env vars our providers used.
pub fn clean_codex(text: &str, data: &Path, owned: &[String]) -> Result<(String, Vec<String>, Vec<String>), String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("config.toml has a syntax error: {e}"))?;
    let mut changes = vec![];
    let mut env_keys = vec![];
    let ours = |s: &str| crate::codex::is_owned_key(s, owned);
    let active_ours = doc.get("model_provider").and_then(|i| i.as_str()).map(ours).unwrap_or(false);
    if active_ours {
        doc.remove("model_provider");
        changes.push("Remove model_provider (back to OpenAI default)".into());
        if doc.remove("model").is_some() {
            changes.push("Remove model".into());
        }
        if doc.remove("model_reasoning_effort").is_some() {
            changes.push("Remove model_reasoning_effort".into());
        }
    }
    let owned_catalog = doc.get("model_catalog_json").and_then(|i| i.as_str()).map(|s| crate::codex::is_owned_catalog(s, data)).unwrap_or(false);
    if owned_catalog {
        doc.remove("model_catalog_json");
        changes.push("Remove the /model menu catalog".into());
    }
    if let Some(t) = doc.get_mut("model_providers").and_then(|i| i.as_table_mut()) {
        let names: Vec<String> = t.iter().map(|(k, _)| k.to_string()).filter(|k| ours(k)).collect();
        for n in names {
            if let Some(env) = t.get(&n).and_then(|p| p.get("env_key")).and_then(|v| v.as_str()) {
                env_keys.push(env.to_string());
            }
            t.remove(&n);
            changes.push(format!("Remove provider {n}"));
        }
        if t.is_empty() {
            doc.remove("model_providers");
        }
    }
    Ok((doc.to_string(), changes, env_keys))
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn push_file(plan: &mut Plan, app: &str, path: &Path, before: &str, after: String, changes: Vec<String>, original: bool) {
    if changes.is_empty() || after == before {
        return;
    }
    plan.items.push(Item { app: app.into(), path: path.display().to_string(), action: "edit".into(), changes, has_original: original });
    plan.writes.push((path.to_path_buf(), after.into_bytes()));
}

fn same_file(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

fn build(apps: &[String], mode: &str) -> Result<Plan, String> {
    let store = crate::store();
    let data = crate::data_dir();
    let want = |a: &str| apps.iter().any(|x| x == a);
    let original_mode = mode == "original";
    let mut plan = Plan::default();
    let saved = crate::providers().list(&crate::secrets::Keychain);
    let owned = crate::codex::owned_keys(&saved);
    let owned_headers = owned_header_values(&saved);

    // In "original" mode a file with a saved original goes back byte for byte;
    // files without one fall back to "clean" (the tool created them, or never wrote).
    let file = |plan: &mut Plan, app: &str, path: PathBuf, clean: &dyn Fn(&str) -> Result<(String, Vec<String>), String>| -> Result<(), String> {
        let Some(before) = read(&path) else { return Ok(()) };
        let orig = store.original(&path);
        if original_mode {
            if let Some(o) = orig {
                if o != before.as_bytes() {
                    plan.items.push(Item { app: app.into(), path: path.display().to_string(), action: "edit".into(), changes: vec!["Restore the original from before the tool's first write".into()], has_original: true });
                    plan.writes.push((path, o));
                }
                return Ok(());
            }
        }
        let (after, changes) = clean(&before)?;
        push_file(plan, app, &path, &before, after, changes, orig.is_some());
        Ok(())
    };

    if want("claude-code") {
        file(&mut plan, "claude-code", crate::claude::settings_path(), &|t| clean_claude(t, &owned_headers))?;
    }
    let mut env_keys: Vec<String> = vec![];
    if want("codex") {
        let path = crate::codex::config_path();
        if let Some(t) = read(&path) {
            env_keys.extend(clean_codex(&t, &data, &owned)?.2);
        }
        file(&mut plan, "codex", path, &|t| clean_codex(t, &data, &owned).map(|(a, c, _)| (a, c)))?;
        // Orca kept as is still needs its env vars: never remove one it points at.
        if !want("orca") {
            let codex_path = crate::codex::config_path();
            let orca = crate::orca::config_path().filter(|p| !same_file(p, &codex_path));
            if let Some(t) = orca.and_then(|p| read(&p)) {
                if let Ok((_, _, used)) = clean_codex(&t, &data, &owned) {
                    env_keys.retain(|k| !used.contains(k));
                }
            }
        }
        // Env vars our provider tables pointed at.
        for k in env_keys.iter() {
            if crate::secrets::UserEnv.get(k).is_some() && !plan.env.iter().any(|(n, _)| n == k) {
                plan.env.push((k.clone(), None));
            }
        }
        if !plan.env.is_empty() {
            plan.items.push(Item {
                app: "codex".into(),
                path: "User environment variables".into(),
                action: "env".into(),
                changes: plan.env.iter().map(|(n, _)| format!("Remove {n}")).collect(),
                has_original: false,
            });
        }
    }
    if want("orca") {
        // CODEX_HOME may point Codex at Orca's home: same file, one write.
        if let Some(path) = crate::orca::config_path().filter(|p| !plan.writes.iter().any(|(w, _)| same_file(w, p))) {
            file(&mut plan, "orca", path, &|t| clean_codex(t, &data, &owned).map(|(a, c, _)| (a, c)))?;
        }
    }
    if want("codex") {
        let path = crate::image::agents_path(&crate::codex::codex_home());
        file(&mut plan, "codex", path, &|t| {
            Ok(match crate::image::remove(t)? {
                Some(after) => (after, vec!["Remove the image generation guidance block".into()]),
                None => (t.to_string(), vec![]),
            })
        })?;
        let models = crate::image::models_path(&crate::codex::codex_home());
        if models.exists() {
            plan.items.push(Item { app: "codex".into(), path: models.display().to_string(), action: "delete".into(), changes: vec!["Delete the image model list".into()], has_original: false });
            plan.deletes.push(models);
        }
    }
    if want("claude-desktop") {
        if let Some(dir) = crate::desktop::library_dir() {
            desktop(&mut plan, &dir, original_mode, &store)?;
        }
    }
    Ok(plan)
}

/// Claude Desktop: delete our entries and unlink them from _meta.json.
fn desktop(plan: &mut Plan, dir: &Path, original_mode: bool, store: &crate::store::Store) -> Result<(), String> {
    let meta_path = dir.join("_meta.json");
    let Some(text) = read(&meta_path) else { return Ok(()) };
    let mut meta: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("_meta.json error: {e}"))?;
    let entries = meta.get("entries").and_then(|e| e.as_array()).cloned().unwrap_or_default();
    let ours: Vec<String> = entries
        .iter()
        .filter_map(|e| e.get("id").and_then(|v| v.as_str()))
        .filter(|id| {
            crate::store::read_json(&dir.join(format!("{id}.json")))
                .ok()
                .and_then(|c| c.get(crate::desktop::MARKER).and_then(|m| m.as_bool()))
                == Some(true)
        })
        .map(String::from)
        .collect();
    if ours.is_empty() {
        return Ok(());
    }
    let orig = store.original(&meta_path);
    let mut changes = vec![];
    if original_mode && orig.is_some() {
        plan.writes.push((meta_path.clone(), orig.clone().unwrap()));
        changes.push("Restore _meta.json to the original".into());
    } else {
        let obj = meta.as_object_mut().ok_or("_meta.json must be an object")?;
        if let Some(Value::Array(a)) = obj.get_mut("entries") {
            a.retain(|e| !e.get("id").and_then(|v| v.as_str()).map(|id| ours.iter().any(|o| o == id)).unwrap_or(false));
        }
        let applied_ours = obj.get("appliedId").and_then(|v| v.as_str()).map(|id| ours.iter().any(|o| o == id)).unwrap_or(false);
        if applied_ours {
            obj.remove("appliedId");
            changes.push("Deselect the applied gateway configuration".into());
        }
        let mut bytes = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        plan.writes.push((meta_path.clone(), bytes));
    }
    for id in &ours {
        plan.deletes.push(dir.join(format!("{id}.json")));
        changes.push(format!("Delete configuration {id}.json created by the tool"));
    }
    plan.items.push(Item { app: "claude-desktop".into(), path: dir.display().to_string(), action: "edit".into(), changes, has_original: orig.is_some() });
    Ok(())
}

pub fn preview(req: &Request) -> Result<Vec<Item>, String> {
    Ok(build(&req.apps, &req.mode)?.items)
}

pub fn apply(req: &Request) -> Result<crate::store::Change, String> {
    let plan = build(&req.apps, &req.mode)?;
    if plan.items.is_empty() {
        return Err("Nothing to restore: the apps are already at default".into());
    }
    let detail = plan.items.iter().map(|i| i.app.as_str()).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(", ");
    let title = if req.mode == "original" { "Restore originals" } else { "Restore defaults" };
    crate::store().apply_full(title, &detail, &plan.writes, &plan.deletes, &plan.env, Some((&crate::secrets::UserEnv, &crate::secrets::Keychain)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_clean_keeps_user_settings() {
        let before = r#"{"permissions":{"allow":["Bash(ls)"]},"model":"cc/x","env":{"FOO":"1","ANTHROPIC_BASE_URL":"http://g","ANTHROPIC_AUTH_TOKEN":"sk-x","CLAUDE_CODE_EFFORT_LEVEL":"low"},"modelPicker":{"options":[{"model":"a","description":"Từ gateway (Model Switcher)"}]}}"#;
        let (out, changes) = clean_claude(before, &[]).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["env"]["FOO"], "1");
        assert!(v["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
        assert!(v.get("model").is_none());
        assert!(v.get("modelPicker").is_none());
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
        assert!(changes.iter().all(|c| !c.contains("sk-")));
    }

    #[test]
    fn claude_clean_leaves_plain_settings_alone() {
        let (_, changes) = clean_claude(r#"{"model":"opus","env":{"FOO":"1"}}"#, &[]).unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn codex_clean_removes_only_ours() {
        let data = Path::new("C:/data");
        let cat = crate::codex::catalog_dir(data).join(format!("{}.json", "a".repeat(64)));
        let before = format!(
            "model = \"cx/x\"\nmodel_provider = \"ms-acme\"\nmodel_catalog_json = '{}'\napproval_policy = \"on-request\"\n\n[model_providers.ms-acme]\nname = \"ms-acme\"\nenv_key = \"MS_ACME_API_KEY\"\n\n[model_providers.other]\nname = \"other\"\n",
            cat.display()
        );
        let (out, changes, env) = clean_codex(&before, data, &[]).unwrap();
        assert!(!out.contains("ms-acme"));
        assert!(!out.contains("model_catalog_json"));
        assert!(out.contains("approval_policy"));
        assert!(out.contains("[model_providers.other]"));
        assert_eq!(env, vec!["MS_ACME_API_KEY"]);
        assert!(changes.len() >= 3);
    }

    /// Read-only: what a reset would do on this machine. Prints no values.
    #[test]
    #[ignore]
    fn print_preview() {
        let req = Request { apps: vec!["claude-code".into(), "codex".into(), "orca".into(), "claude-desktop".into()], mode: "clean".into() };
        for i in preview(&req).unwrap() {
            println!("{} | {} | {} | original={} | {:?}", i.app, i.action, i.path, i.has_original, i.changes);
        }
    }

    #[test]
    fn codex_clean_keeps_foreign_provider_routing() {
        let (out, changes, _) = clean_codex("model = \"gpt\"\nmodel_provider = \"openai\"\n", Path::new("C:/d"), &["gw".to_string()]).unwrap();
        assert!(changes.is_empty());
        assert!(out.contains("model_provider"));
    }
}

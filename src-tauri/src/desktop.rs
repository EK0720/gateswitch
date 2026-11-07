//! Claude Desktop (3P "gateway" mode): write an entry into the local
//! configLibrary, which the in-app configuration window (Developer →
//! Configure Third-Party Inference…) uses too. Configuration takes effect at
//! launch, so the page asks the user to restart the app afterwards.
//!
//! Format source: claude.com/docs/third-party/claude-desktop/configuration
//! and ./configuration-format:
//! - `<dir>/_meta.json` = {"appliedId": ..., "entries": [{id, name}]}
//! - one `<uuid>.json` per entry, keys at top level.
//! - A managed source (HKLM\\SOFTWARE\\Policies\\Claude, a plist or the Linux
//!   file) that sets any inference key overrides the local file — we refuse and
//!   point at it instead of writing something the app will ignore.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Values the in-app window writes for a keyless static gateway credential.
const CRED_KIND: &str = "static";
/// Marks entries this tool created, so Setup updates them instead of adding.
pub(crate) const MARKER: &str = "modelSwitcher";

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub provider_id: String,
    pub model: String,
    /// "bearer" (default) or "x-api-key".
    #[serde(default)]
    pub auth: Option<String>,
    /// Add a 1M-context toggle for models whose id has the [1m] suffix.
    #[serde(default)]
    pub one_m: bool,
    /// Put every model of the provider in the picker (first entry = default).
    #[serde(default)]
    pub all_models: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Entry {
    pub id: String,
    /// Created by this tool (has the marker).
    pub ours: bool,
    pub name: String,
    pub base_url: String,
    /// "bearer" | "x-api-key" | "" (key absent / different kind).
    pub auth: String,
    pub has_key: bool,
    pub default_model: String,
    pub model_count: u32,
    pub discovery: Option<bool>,
    /// Number of inferenceCustomHeaders (names only are read, never values).
    pub headers: u32,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Current {
    pub entries: Vec<Entry>,
    /// id of the applied entry, or reasons it isn't in effect.
    pub effective: Option<String>,
    pub warnings: Vec<String>,
}

/// The configLibrary dir: docs' local location, or the MSIX redirected one.
pub fn library_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "windows")]
    {
        candidates.push(dirs::data_local_dir()?.join("Claude-3p").join("configLibrary"));
        if let Some(pkgs) = dirs::data_local_dir().map(|d| d.join("Packages")) {
            if let Ok(list) = std::fs::read_dir(&pkgs) {
                let mut pkg: Vec<PathBuf> = list
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("Claude_")).unwrap_or(false))
                    .collect();
                pkg.sort();
                for p in pkg {
                    candidates.push(p.join("LocalCache").join("Roaming").join("Claude-3p").join("configLibrary"));
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = dirs::home_dir()?;
        if cfg!(target_os = "macos") {
            candidates.push(home.join("Library/Application Support/Claude-3p/configLibrary"));
        } else {
            candidates.push(home.join(".config/Claude-3p/configLibrary"));
        }
    }
    candidates.into_iter().find(|d| d.join("_meta.json").exists())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", path.display()))
}

/// Machine policy beats the local file, so setup there would be a no-op.
pub fn policy_override() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        for (label, hive) in [("HKLM", winreg::enums::HKEY_LOCAL_MACHINE), ("HKCU", winreg::enums::HKEY_CURRENT_USER)] {
            if let Ok(k) = winreg::RegKey::predef(hive).open_subkey(r"SOFTWARE\Policies\Claude") {
                // The app reads values only (never subkeys). Inference/bootstrap
                // values mean the managed config wins over the local file.
                let routing: Vec<String> = k
                    .enum_values()
                    .filter_map(|r| r.ok())
                    .map(|(n, _)| n)
                    .filter(|n| n.starts_with("inference") || n.starts_with("bootstrap"))
                    .collect();
                if !routing.is_empty() {
                    return Some(format!(r"{label}\SOFTWARE\Policies\Claude: {}", routing.join(", ")));
                }
                // HKCU is ignored entirely when machine policy exists.
                if label == "HKLM" && k.enum_values().next().is_some() {
                    return None;
                }
            }
        }
    }
    None
}

fn read_one(dir: &Path, id: &str) -> Result<Entry, String> {
    let mut doc = read_json(&dir.join(format!("{id}.json")))?;
    let obj = doc.as_object_mut().ok_or("Entry is not a JSON object")?;
    let s = |k: &str| obj.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok(Entry {
        id: id.into(),
        name: String::new(),
        base_url: s("inferenceGatewayBaseUrl"),
        auth: match obj.get("inferenceGatewayAuthScheme").and_then(|v| v.as_str()) {
            Some("x-api-key") => "x-api-key",
            Some("bearer") | None => "bearer",
            _ => "",
        }
        .into(),
        has_key: obj.contains_key("inferenceGatewayApiKey"),
        default_model: obj
            .get("inferenceModels")
            .and_then(|m| m.as_array())
            .and_then(|a| a.first())
            .and_then(|m| match m {
                Value::String(x) => Some(x.clone()),
                Value::Object(o) => o.get("name").and_then(|v| v.as_str()).map(String::from),
                _ => None,
            })
            .unwrap_or_default(),
        model_count: obj.get("inferenceModels").and_then(|m| m.as_array()).map(|a| a.len() as u32).unwrap_or(0),
        discovery: obj.get("modelDiscoveryEnabled").and_then(|v| v.as_bool()),
        headers: obj.get("inferenceCustomHeaders").and_then(|v| v.as_object()).map(|o| o.len() as u32).unwrap_or(0),
        ours: obj.get(MARKER).and_then(|v| v.as_bool()) == Some(true),
        applied: false,
    })
}

pub fn read_current(dir: Option<&Path>) -> Current {
    let Some(dir) = dir else {
        return Current { warnings: vec!["Claude Desktop (3P) is not installed, or Developer → Configure Third-Party Inference has never been opened.".into()], ..Default::default() };
    };
    let meta = match read_json(&dir.join("_meta.json")) {
        Ok(v) => v,
        Err(e) => return Current { warnings: vec![e], ..Default::default() },
    };
    let applied = meta.get("appliedId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut entries = vec![];
    let mut warnings = vec![];
    for e in meta.get("entries").and_then(|v| v.as_array()).unwrap_or(&vec![]) {
        let Some(id) = e.get("id").and_then(|v| v.as_str()) else { continue };
        match read_one(dir, id) {
            Ok(mut en) => {
                en.name = e.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                en.applied = en.id == applied;
                entries.push(en);
            }
            Err(e) => warnings.push(e),
        }
    }
    let effective = entries.iter().find(|e| e.id == applied).map(|e| e.id.clone());
    if applied.is_empty() {
        warnings.push("No gateway configuration is applied.".into());
    }
    Current { entries, effective, warnings }
}

/// Claude tier a gateway model id stands in for, so bare alias like `sonnet`
/// resolves. `cc/claude-opus-5.5[1m]` -> opus.
fn family_tier(name: &str) -> Option<&'static str> {
    let m = name.to_lowercase();
    let m = m.strip_suffix("[1m]").unwrap_or(&m);
    if m.contains("opus") {
        Some("opus")
    } else if m.contains("sonnet") {
        Some("sonnet")
    } else if m.contains("haiku") {
        Some("haiku")
    } else if m.contains("fable") {
        Some("fable")
    } else {
        None
    }
}

pub fn label(name: &str) -> String {
    let l = name.replace("[1m]", " · 1M");
    l.split(['/', '-'])
        .filter_map(|w| {
            let c = w.chars().next()?;
            Some(c.to_uppercase().collect::<String>() + &w[c.len_utf8()..])
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// New settings.json entry text.
pub fn render_entry(req: &Request, base_url: &str, key: &str, models: &[String], headers: &crate::headers::Headers) -> Result<Map<String, Value>, String> {
    crate::codex::validate_model(&req.model)?;
    if key.trim().is_empty() {
        return Err("This provider has no key".into());
    }
    let b = base_url.trim_end_matches('/');
    let base = b.strip_suffix("/v1").unwrap_or(b);
    let mut list: Vec<String> = models.to_vec();
    if !list.contains(&req.model) {
        list.insert(0, req.model.clone());
    }
    let mut o = Map::new();
    o.insert("inferenceProvider".into(), json!("gateway"));
    o.insert("inferenceGatewayBaseUrl".into(), json!(base));
    o.insert("inferenceGatewayApiKey".into(), json!(key.trim()));
    if req.auth.as_deref() == Some("x-api-key") {
        o.insert("inferenceGatewayAuthScheme".into(), json!("x-api-key"));
    }
    o.insert("inferenceCredentialKind".into(), json!(CRED_KIND));
    if !headers.is_empty() {
        crate::headers::validate(headers)?;
        o.insert("inferenceCustomHeaders".into(), json!(headers));
    }
    o.insert(MARKER.into(), json!(true));
    if req.all_models || list.len() > 1 {
        let items: Vec<Value> = list.iter().map(|m| {
            let mut e = Map::new();
            e.insert("name".into(), json!(m));
            let l = label(m);
            if l != *m {
                e.insert("labelOverride".into(), json!(l));
            }
            if let Some(t) = family_tier(m) {
                e.insert("anthropicFamilyTier".into(), json!(t));
            }
            if req.one_m && m.ends_with("[1m]") {
                e.insert("supports1m".into(), json!(true));
            }
            Value::Object(e)
        }).collect();
        o.insert("inferenceModels".into(), Value::Array(items));
    }
    Ok(o)
}

/// Files to write for a new entry (fresh uuid) that becomes the applied one.
/// Returned instead of written so the store can back up `_meta.json` and undo.
pub fn plan(dir: &Path, name: &str, entry: &Map<String, Value>) -> Result<(String, Vec<(PathBuf, Vec<u8>)>), String> {
    if let Some(p) = policy_override() {
        return Err(format!("This machine is managed by a policy ({p}). Claude Desktop ignores local configuration, so the tool did not write it."));
    }
    let meta_path = dir.join("_meta.json");
    let mut meta: Value = if meta_path.exists() { read_json(&meta_path)? } else { json!({}) };
    let obj = meta.as_object_mut().ok_or("_meta.json must be an object")?;
    let arr = obj
        .entry("entries")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("entries in _meta.json must be an array")?;
    // Update our own entry for this gateway when one exists; never touch
    // entries the user or their admin made in the app.
    let base = entry.get("inferenceGatewayBaseUrl").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let ours = |id: &str| -> Option<bool> {
        let c = crate::store::read_json(&dir.join(format!("{id}.json"))).ok()?;
        Some(c.get(MARKER).and_then(|m| m.as_bool())? && c.get("inferenceGatewayBaseUrl").and_then(|v| v.as_str()) == Some(base.as_str()))
    };
    let id = match arr.iter().find_map(|e| {
        let id = e.get("id").and_then(|v| v.as_str())?;
        (dir.join(format!("{id}.json")).exists() && ours(id) == Some(true)).then(|| id.to_string())
    }) {
        Some(id) => id,
        None => {
            let id = uuid_v4();
            arr.push(json!({ "id": id, "name": name }));
            id
        }
    };
    obj.insert("appliedId".into(), json!(id));
    let mut entry_bytes = serde_json::to_vec_pretty(entry).map_err(|e| e.to_string())?;
    entry_bytes.extend_from_slice(b"\n");
    let mut meta_bytes = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
    meta_bytes.extend_from_slice(b"\n");
    // Entry first: if the meta write fails, the app never points at a missing file.
    Ok((id.clone(), vec![(dir.join(format!("{id}.json")), entry_bytes), (meta_path, meta_bytes)]))
}

/// Files to write/delete to drop `ids` from the library. `appliedId` is cleared
/// when it points at a removed entry, so the app falls back to its default.
pub fn plan_remove(dir: &Path, ids: &[String]) -> Result<(Vec<(PathBuf, Vec<u8>)>, Vec<PathBuf>, Vec<String>), String> {
    if ids.is_empty() {
        return Err("No configuration selected".into());
    }
    let meta_path = dir.join("_meta.json");
    let mut meta = read_json(&meta_path)?;
    let obj = meta.as_object_mut().ok_or("_meta.json must be an object")?;
    let arr = obj.get_mut("entries").and_then(|e| e.as_array_mut()).ok_or("_meta.json has no entries")?;
    let mut names = vec![];
    for id in ids {
        // Ids come from the UI: only accept ones listed in _meta.json (no paths).
        let pos = arr
            .iter()
            .position(|e| e.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
            .ok_or("The configuration is no longer in the list")?;
        let e = arr.remove(pos);
        names.push(e.get("name").and_then(|v| v.as_str()).filter(|n| !n.is_empty()).unwrap_or(id).to_string());
    }
    if obj.get("appliedId").and_then(|v| v.as_str()).map(|a| ids.iter().any(|i| i == a)).unwrap_or(false) {
        obj.remove("appliedId");
    }
    let mut bytes = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    let deletes = ids.iter().map(|id| dir.join(format!("{id}.json"))).filter(|p| p.exists()).collect();
    Ok((vec![(meta_path, bytes)], deletes, names))
}

#[cfg(test)]
fn apply(dir: &Path, name: &str, entry: &Map<String, Value>) -> Result<String, String> {
    let (id, writes) = plan(dir, name, entry)?;
    for (p, b) in writes {
        crate::store::atomic_write(&p, &b)?;
    }
    Ok(id)
}

/// RFC-4122 v4 uuid from the OS random source.
fn uuid_v4() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}", h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7],h[8],h[9],h[10],h[11],h[12],h[13],h[14],h[15])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_render_and_family() {
        let req = Request { provider_id: "9router".into(), model: "cc/claude-opus-5.5[1m]".into(), auth: None, one_m: true, all_models: true };
        let models: Vec<String> = ["cc/claude-opus-5.5[1m]", "cc/claude-sora-1", "cx/gpt-6-astra", "combo-smart"].map(String::from).to_vec();
        let e = render_entry(&req, "http://localhost:20128/v1/", "sk-x", &models, &Default::default()).unwrap();
        assert_eq!(e["inferenceGatewayBaseUrl"], "http://localhost:20128");
        assert_eq!(e["inferenceProvider"], "gateway");
        assert_eq!(e["inferenceCredentialKind"], "static");
        assert!(e.get("inferenceGatewayAuthScheme").is_none());
        let list = e["inferenceModels"].as_array().unwrap();
        assert_eq!(list.len(), 4);
        assert_eq!(list[0]["name"], "cc/claude-opus-5.5[1m]");
        assert_eq!(list[0]["supports1m"], true);
        assert_eq!(list[0]["anthropicFamilyTier"], "opus");
        assert!(list[0]["labelOverride"].as_str().unwrap().contains("1M"));
        assert!(list[2].get("anthropicFamilyTier").is_none());
        // x-api-key variant
        let mut r2 = req.clone();
        r2.auth = Some("x-api-key".into());
        assert_eq!(render_entry(&r2, "u/v1", "k", &[], &Default::default()).unwrap()["inferenceGatewayAuthScheme"], "x-api-key");
        let h: crate::headers::Headers = [("x-app".to_string(), "demo".to_string())].into();
        assert_eq!(render_entry(&r2, "u/v1", "k", &[], &h).unwrap()["inferenceCustomHeaders"]["x-app"], "demo");
        assert!(render_entry(&r2, "u/v1", "k", &[], &Default::default()).unwrap().get("inferenceCustomHeaders").is_none());
    }

    fn temp_lib() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("configLibrary");
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::atomic_write(&dir.join("_meta.json"), br#"{"appliedId":"","entries":[]}"#).unwrap();
        (d, dir)
    }

    #[test]
    fn apply_writes_entry_and_meta() {
        let (_d, dir) = temp_lib();
        let cur = read_current(Some(&dir));
        assert!(cur.entries.is_empty());
        assert!(cur.warnings.iter().any(|w| w.contains("No gateway configuration")));
        let entry = render_entry(&Request { provider_id: "9router".into(), model: "cc/claude-sonnet-5".into(), auth: None, one_m: false, all_models: true }, "http://localhost:20128/v1", "sk-k", &["cc/claude-sonnet-5".to_string(), "cx/gpt-6-astra".to_string()], &Default::default()).unwrap();
        let id = apply(&dir, "", &entry).unwrap();
        let cur = read_current(Some(&dir));
        assert_eq!(cur.effective.as_deref(), Some(id.as_str()));
        assert_eq!(cur.entries.len(), 1);
        assert!(cur.entries[0].applied);
        assert_eq!(cur.entries[0].name, "");
        assert_eq!(cur.entries[0].base_url, "http://localhost:20128");
        assert_eq!(cur.entries[0].auth, "bearer");
        assert_eq!(cur.entries[0].default_model, "cc/claude-sonnet-5");
        assert_eq!(cur.entries[0].model_count, 2);
        // Setup again for the same gateway: updates our entry, no new one.
        let again = render_entry(&Request { provider_id: "9router".into(), model: "cx/gpt-6-astra".into(), auth: None, one_m: false, all_models: true }, "http://localhost:20128/v1", "sk-k", &["cx/gpt-6-astra".to_string()], &Default::default()).unwrap();
        let id2 = apply(&dir, "", &again).unwrap();
        assert_eq!(id, id2, "cùng gateway phải dùng lại entry");
        let cur = read_current(Some(&dir));
        assert_eq!(cur.entries.len(), 1);
        assert_eq!(cur.entries[0].default_model, "cx/gpt-6-astra");
        // A different gateway gets its own entry.
        let other = render_entry(&Request { provider_id: "x".into(), model: "m".into(), auth: None, one_m: false, all_models: false }, "http://other/v1", "k", &[], &Default::default()).unwrap();
        let id3 = apply(&dir, "x", &other).unwrap();
        assert_ne!(id, id3);
        let cur = read_current(Some(&dir));
        assert_eq!(cur.entries.len(), 2);
        assert!(cur.entries.iter().find(|e| e.id == id3).unwrap().applied);
        assert!(!cur.entries.iter().find(|e| e.id == id).unwrap().applied);
        assert!(cur.entries.iter().all(|e| e.ours));
        // Remove the applied one: gone from meta, appliedId cleared, file deleted.
        let (w, d, names) = plan_remove(&dir, &[id3.clone()]).unwrap();
        assert_eq!(names, vec!["x"]);
        for (p, b) in w { crate::store::atomic_write(&p, &b).unwrap(); }
        for p in d { std::fs::remove_file(p).unwrap(); }
        let cur = read_current(Some(&dir));
        assert_eq!(cur.entries.len(), 1);
        assert!(cur.effective.is_none());
        assert!(plan_remove(&dir, &["../evil".into()]).is_err());
    }
}

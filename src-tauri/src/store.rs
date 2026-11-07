//! Safe file writes with per-change backups and undo.
//!
//! Every change is recorded in `history.jsonl`. Before a file is overwritten its
//! previous bytes are copied into `backups/<change-id>/`. Undo restores those
//! bytes only if the file still matches what we wrote (otherwise someone else
//! edited it and we refuse instead of clobbering their work).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::secrets::{Env, Vault};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileChange {
    pub path: PathBuf,
    /// Backup of the previous content; None when the file did not exist.
    pub backup: Option<PathBuf>,
    pub after_sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Change {
    pub id: String,
    pub time: String,
    pub title: String,
    pub detail: String,
    pub files: Vec<FileChange>,
    #[serde(default)]
    pub env: Vec<EnvChange>,
    #[serde(default)]
    pub undone: bool,
}

/// A user environment variable change. Secret values are never stored here;
/// the previous value is kept in the keychain under `backup_account`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvChange {
    pub name: String,
    pub existed: bool,
    pub backup_account: Option<String>,
    pub after_sha: Option<String>,
}

pub struct Store {
    root: PathBuf,
}

fn sha(bytes: &[u8]) -> String {
    let mut out = String::new();
    for b in Sha256::digest(bytes) {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Write via a temp file in the same folder, flush, then rename over the target.
pub(crate) fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(text.trim_start_matches("﻿")).map_err(|e| e.to_string())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("Invalid path")?;
    fs::create_dir_all(dir).map_err(|e| format!("Could not create folder {}: {e}", dir.display()))?;
    let name = path.file_name().ok_or("Invalid path")?.to_string_lossy();
    let tmp = dir.join(format!(".{name}.ms-tmp-{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("Could not write {}: {e}", tmp.display()))?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Could not replace {}: {e}", path.display())
    })
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Store { root }
    }

    fn history_path(&self) -> PathBuf {
        self.root.join("history.jsonl")
    }

    pub fn history(&self) -> Vec<Change> {
        let Ok(text) = fs::read_to_string(self.history_path()) else { return vec![] };
        let mut items: Vec<Change> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        items.reverse();
        items
    }

    fn save_history(&self, newest_first: &[Change]) -> Result<(), String> {
        let mut text = String::new();
        for c in newest_first.iter().rev() {
            text.push_str(&serde_json::to_string(c).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        atomic_write(&self.history_path(), text.as_bytes())
    }

    /// Apply a set of writes as one undoable change.
    pub fn apply(&self, title: &str, detail: &str, writes: &[(PathBuf, Vec<u8>)]) -> Result<Change, String> {
        self.apply_with_env(title, detail, writes, &[], None)
    }

    /// Files and user env vars as one undoable change. Previous env values go to the vault.
    pub fn apply_with_env(
        &self,
        title: &str,
        detail: &str,
        writes: &[(PathBuf, Vec<u8>)],
        env_sets: &[(String, Option<String>)],
        sys: Option<(&dyn Env, &dyn Vault)>,
    ) -> Result<Change, String> {
        self.apply_full(title, detail, writes, &[], env_sets, sys)
    }

    /// Like `apply_with_env`, plus files to delete (backed up, restored on undo).
    pub fn apply_full(
        &self,
        title: &str,
        detail: &str,
        writes: &[(PathBuf, Vec<u8>)],
        deletes: &[PathBuf],
        env_sets: &[(String, Option<String>)],
        sys: Option<(&dyn Env, &dyn Vault)>,
    ) -> Result<Change, String> {
        let id = format!("{}-{:04x}", chrono::Local::now().format("%Y%m%d%H%M%S%3f"), std::process::id() & 0xffff);
        let backup_dir = self.root.join("backups").join(&id);
        let mut files = Vec::new();
        for (i, (path, bytes)) in writes.iter().enumerate() {
            let backup = if path.exists() {
                fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
                let b = backup_dir.join(format!("{i}.bak"));
                let prev = fs::read(path).map_err(|e| format!("Could not back up {}: {e}", path.display()))?;
                atomic_write(&b, &crate::seal::seal(&prev)?)?;
                self.first_write_backup(path)?;
                Some(b)
            } else {
                None
            };
            files.push(FileChange { path: path.clone(), backup, after_sha: sha(bytes) });
        }
        for (i, path) in deletes.iter().enumerate().filter(|(_, p)| p.exists()) {
            fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
            let b = backup_dir.join(format!("d{i}.bak"));
            let prev = fs::read(path).map_err(|e| format!("Could not back up {}: {e}", path.display()))?;
            atomic_write(&b, &crate::seal::seal(&prev)?)?;
            // Empty after_sha = "the change left no file here".
            files.push(FileChange { path: path.clone(), backup: Some(b), after_sha: String::new() });
        }
        let mut env = Vec::new();
        if !env_sets.is_empty() {
            let (e, v) = sys.ok_or("Missing permission to write environment variables")?;
            for (name, value) in env_sets {
                let prev = e.get(name);
                let backup_account = match &prev {
                    Some(old) => {
                        let account = format!("backup:{id}:{name}");
                        v.put(&account, old)?;
                        Some(account)
                    }
                    None => None,
                };
                env.push(EnvChange {
                    name: name.clone(),
                    existed: prev.is_some(),
                    backup_account,
                    after_sha: value.as_deref().map(|x| sha(x.as_bytes())),
                });
            }
        }
        for (path, bytes) in writes {
            atomic_write(path, bytes)?;
        }
        for path in deletes.iter().filter(|p| p.exists()) {
            fs::remove_file(path).map_err(|e| format!("Could not delete {}: {e}", path.display()))?;
        }
        if let Some((e, _)) = sys {
            for (name, value) in env_sets {
                e.set(name, value.as_deref())?;
            }
        }
        let change = Change {
            id,
            time: chrono::Local::now().to_rfc3339(),
            title: title.into(),
            detail: detail.into(),
            files,
            env,
            undone: false,
        };
        let mut all = self.history();
        all.insert(0, change.clone());
        self.save_history(&all)?;
        Ok(change)
    }

    /// Keep the very first original of each file forever, apart from per-change backups.
    fn first_write_backup(&self, path: &Path) -> Result<(), String> {
        let key = sha(path.to_string_lossy().as_bytes());
        let dest = self.root.join("backups").join("first").join(&key[..16]);
        if !dest.exists() {
            let prev = fs::read(path).map_err(|e| e.to_string())?;
            atomic_write(&dest, &crate::seal::seal(&prev)?)?;
        }
        Ok(())
    }

    /// The first original of `path` the tool ever saw, decrypted. None if the
    /// tool never overwrote an existing file there (it created it, or never touched it).
    pub fn original(&self, path: &Path) -> Option<Vec<u8>> {
        let key = sha(path.to_string_lossy().as_bytes());
        let bytes = fs::read(self.root.join("backups").join("first").join(&key[..16])).ok()?;
        crate::seal::open(&bytes).ok()
    }

    /// Re-encrypt plain backups written by earlier builds. Safe to run often.
    pub fn seal_old_backups(&self) {
        let mut stack = vec![self.root.join("backups")];
        while let Some(d) = stack.pop() {
            let Ok(rd) = fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Ok(b) = fs::read(&p) {
                    if !b.starts_with(crate::seal::MAGIC) {
                        if let Ok(s) = crate::seal::seal(&b) {
                            let _ = atomic_write(&p, &s);
                        }
                    }
                }
            }
        }
    }

    pub fn undo(&self, id: &str) -> Result<Change, String> {
        self.undo_with(id, None)
    }

    pub fn undo_with(&self, id: &str, sys: Option<(&dyn Env, &dyn Vault)>) -> Result<Change, String> {
        let mut all = self.history();
        let idx = all.iter().position(|c| c.id == id).ok_or("This change was not found")?;
        if all[idx].undone {
            return Err("This change has already been undone".into());
        }
        for f in &all[idx].files {
            let current = fs::read(&f.path).ok();
            let matches = if f.after_sha.is_empty() { current.is_none() } else { current.as_deref().map(sha) == Some(f.after_sha.clone()) };
            if !matches {
                return Err(format!(
                    "{} was modified after this setup. Undo newer changes first, or fix it manually.",
                    f.path.display()
                ));
            }
        }
        if !all[idx].env.is_empty() {
            let (e, _) = sys.ok_or("Missing permission to write environment variables")?;
            for c in &all[idx].env {
                if e.get(&c.name).map(|x| sha(x.as_bytes())) != c.after_sha {
                    return Err(format!("Variable {} was changed after this setup. Fix it manually or set up again.", c.name));
                }
            }
        }
        for f in &all[idx].files {
            match &f.backup {
                Some(b) => {
                    let bytes = fs::read(b).map_err(|e| format!("Backup {} is missing: {e}", b.display()))?;
                    let bytes = crate::seal::open(&bytes)?;
                    atomic_write(&f.path, &bytes)?;
                }
                None => fs::remove_file(&f.path).map_err(|e| e.to_string())?,
            }
        }
        if let Some((e, v)) = sys {
            for c in &all[idx].env {
                let old = match &c.backup_account {
                    Some(a) => Some(v.get(a).ok_or(format!("Backup of variable {} is missing", c.name))?),
                    None => None,
                };
                e.set(&c.name, old.as_deref())?;
                if let Some(a) = &c.backup_account {
                    let _ = v.delete(a);
                }
            }
        }
        all[idx].undone = true;
        self.save_history(&all)?;
        Ok(all[idx].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path().join("data"));
        (dir, s)
    }

    #[test]
    fn apply_then_undo_restores_previous_content() {
        let (dir, s) = store();
        let f = dir.path().join("cfg/config.toml");
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(&f, "old").unwrap();
        let c = s.apply("Setup Codex", "", &[(f.clone(), b"new".to_vec())]).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "new");
        s.undo(&c.id).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "old");
        assert!(s.history()[0].undone);
    }

    #[test]
    fn undo_removes_file_that_did_not_exist() {
        let (dir, s) = store();
        let f = dir.path().join("new.json");
        let c = s.apply("t", "", &[(f.clone(), b"{}".to_vec())]).unwrap();
        s.undo(&c.id).unwrap();
        assert!(!f.exists());
    }

    #[test]
    fn undo_refuses_when_file_changed_externally() {
        let (dir, s) = store();
        let f = dir.path().join("a.json");
        fs::write(&f, "1").unwrap();
        let c = s.apply("t", "", &[(f.clone(), b"2".to_vec())]).unwrap();
        fs::write(&f, "user edit").unwrap();
        assert!(s.undo(&c.id).is_err());
        assert_eq!(fs::read_to_string(&f).unwrap(), "user edit");
    }

    #[test]
    fn delete_is_backed_up_encrypted_and_undone() {
        let (dir, s) = store();
        let keep = dir.path().join("meta.json");
        let gone = dir.path().join("entry.json");
        fs::write(&keep, "m1").unwrap();
        fs::write(&gone, "secret-entry").unwrap();
        let c = s.apply_full("reset", "", &[(keep.clone(), b"m2".to_vec())], &[gone.clone()], &[], None).unwrap();
        assert!(!gone.exists());
        let bak = c.files.iter().find(|f| f.path == gone).unwrap().backup.clone().unwrap();
        if cfg!(windows) {
            assert!(!fs::read_to_string(&bak).unwrap_or_default().contains("secret-entry"));
        }
        s.undo(&c.id).unwrap();
        assert_eq!(fs::read_to_string(&gone).unwrap(), "secret-entry");
        assert_eq!(fs::read_to_string(&keep).unwrap(), "m1");
    }

    #[test]
    fn env_change_is_undone_and_secret_not_in_history() {
        use crate::secrets::memory::Mem;
        let (dir, s) = store();
        let (env, vault) = (Mem::default(), Mem::default());
        Env::set(&env, "GW_KEY", Some("old-secret")).unwrap();
        let f = dir.path().join("c.toml");
        let c = s.apply_with_env("t", "", &[(f.clone(), b"x".to_vec())],
            &[("GW_KEY".into(), Some("new-secret".into()))], Some((&env, &vault))).unwrap();
        assert_eq!(Env::get(&env, "GW_KEY").as_deref(), Some("new-secret"));
        let hist = fs::read_to_string(dir.path().join("data/history.jsonl")).unwrap();
        assert!(!hist.contains("secret"));
        s.undo_with(&c.id, Some((&env, &vault))).unwrap();
        assert_eq!(Env::get(&env, "GW_KEY").as_deref(), Some("old-secret"));
        assert!(vault.0.borrow().is_empty());
    }

    #[test]
    fn history_is_newest_first_and_first_backup_kept() {
        let (dir, s) = store();
        let f = dir.path().join("a.json");
        fs::write(&f, "orig").unwrap();
        s.apply("one", "", &[(f.clone(), b"1".to_vec())]).unwrap();
        s.apply("two", "", &[(f.clone(), b"2".to_vec())]).unwrap();
        let h = s.history();
        assert_eq!(h[0].title, "two");
        assert_eq!(s.original(&f).unwrap(), b"orig");
    }
}

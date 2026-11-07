//! Secrets live in the OS keychain; user environment variables are only written
//! when an app (Codex) can read its key nowhere else.

pub const SERVICE: &str = "ModelSwitcher";

pub trait Vault {
    fn get(&self, account: &str) -> Option<String>;
    fn put(&self, account: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

pub trait Env {
    fn get(&self, name: &str) -> Option<String>;
    /// `None` removes the variable.
    fn set(&self, name: &str, value: Option<&str>) -> Result<(), String>;
}

pub struct Keychain;

impl Vault for Keychain {
    fn get(&self, account: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, account).ok()?.get_password().ok()
    }
    fn put(&self, account: &str, secret: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| format!("Could not save the key to the keychain: {e}"))
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Could not delete the key from the keychain: {e}")),
        }
    }
}

/// Persistent per-user environment variables.
pub struct UserEnv;

#[cfg(windows)]
impl Env for UserEnv {
    fn get(&self, name: &str) -> Option<String> {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment").ok()?.get_value(name).ok()
    }
    fn set(&self, name: &str, value: Option<&str>) -> Result<(), String> {
        use winreg::{enums::*, RegKey};
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
            .map_err(|e| format!("Could not open user environment variables: {e}"))?;
        let result = match value {
            Some(v) => key.set_value(name, &v.to_string()),
            None => match key.delete_value(name) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        result.map_err(|e| format!("Could not write variable {name}: {e}"))?;
        broadcast_env_change();
        Ok(())
    }
}

#[cfg(windows)]
fn broadcast_env_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let text: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, text.as_ptr() as isize,
            SMTO_ABORTIFHUNG, 2000, &mut result);
    }
}

#[cfg(not(windows))]
impl Env for UserEnv {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn set(&self, _name: &str, _value: Option<&str>) -> Result<(), String> {
        Err("Writing persistent environment variables on macOS/Linux is not supported in this version.".into())
    }
}

#[cfg(test)]
pub mod memory {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    pub struct Mem(pub RefCell<HashMap<String, String>>);

    impl Vault for Mem {
        fn get(&self, a: &str) -> Option<String> { self.0.borrow().get(a).cloned() }
        fn put(&self, a: &str, s: &str) -> Result<(), String> { self.0.borrow_mut().insert(a.into(), s.into()); Ok(()) }
        fn delete(&self, a: &str) -> Result<(), String> { self.0.borrow_mut().remove(a); Ok(()) }
    }

    impl Env for Mem {
        fn get(&self, n: &str) -> Option<String> { self.0.borrow().get(n).cloned() }
        fn set(&self, n: &str, v: Option<&str>) -> Result<(), String> {
            match v {
                Some(v) => { self.0.borrow_mut().insert(n.into(), v.into()); }
                None => { self.0.borrow_mut().remove(n); }
            }
            Ok(())
        }
    }
}

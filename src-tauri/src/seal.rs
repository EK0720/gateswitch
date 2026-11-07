//! Encryption at rest for backups (they hold API keys from config files).
//!
//! Windows: DPAPI, bound to this Windows user, same as the Python app.
//! macOS/Linux: AES-256-GCM with a random key kept in the OS keychain.
//! Sealed files start with MAGIC; anything else is read as plain bytes so
//! backups made before encryption still restore.

pub const MAGIC: &[u8; 4] = b"MSE1";

pub fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = MAGIC.to_vec();
    out.extend(imp::seal(plain)?);
    Ok(out)
}

pub fn open(bytes: &[u8]) -> Result<Vec<u8>, String> {
    match bytes.strip_prefix(MAGIC.as_slice()) {
        Some(rest) => imp::open(rest),
        None => Ok(bytes.to_vec()),
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

    fn run(data: &[u8], protect: bool) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            if protect {
                CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
            } else {
                CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
            }
        };
        if ok == 0 {
            return Err(if protect { "Could not encrypt the backup".into() } else { "Could not decrypt the backup (created by another Windows user?)".into() });
        }
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe { LocalFree(output.pbData as _) };
        Ok(bytes)
    }

    pub fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
        run(plain, true)
    }
    pub fn open(sealed: &[u8]) -> Result<Vec<u8>, String> {
        run(sealed, false)
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::secrets::{Keychain, Vault};
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
    use ring::rand::{SecureRandom, SystemRandom};

    const ACCOUNT: &str = "backup-key";

    fn key(create: bool) -> Result<LessSafeKey, String> {
        let hex = match Keychain.get(ACCOUNT) {
            Some(h) => h,
            None if create => {
                let mut k = [0u8; 32];
                SystemRandom::new().fill(&mut k).map_err(|_| "Could not create a key")?;
                let h: String = k.iter().map(|b| format!("{b:02x}")).collect();
                Keychain.put(ACCOUNT, &h)?;
                h
            }
            None => return Err("The backup decryption key is missing from the keychain".into()),
        };
        let raw: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect();
        Ok(LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &raw).map_err(|_| "The backup key is corrupted")?))
    }

    pub fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
        let k = key(true)?;
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce).map_err(|_| "Could not create a nonce")?;
        let mut buf = plain.to_vec();
        k.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut buf).map_err(|_| "Could not encrypt the backup")?;
        let mut out = nonce.to_vec();
        out.extend(buf);
        Ok(out)
    }

    pub fn open(sealed: &[u8]) -> Result<Vec<u8>, String> {
        if sealed.len() < NONCE_LEN {
            return Err("The backup is corrupted".into());
        }
        let k = key(false)?;
        let (n, body) = sealed.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(n).map_err(|_| "The backup is corrupted")?;
        let mut buf = body.to_vec();
        let plain = k.open_in_place(nonce, Aad::empty(), &mut buf).map_err(|_| "Could not decrypt the backup")?;
        Ok(plain.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn round_trip_and_hides_plaintext() {
        let secret = b"ANTHROPIC_AUTH_TOKEN=sk-very-secret";
        let s = seal(secret).unwrap();
        assert!(s.starts_with(MAGIC));
        assert!(!s.windows(secret.len()).any(|w| w == secret));
        assert_eq!(open(&s).unwrap(), secret);
    }

    #[test]
    fn plain_legacy_bytes_pass_through() {
        assert_eq!(open(b"old backup").unwrap(), b"old backup");
    }
}

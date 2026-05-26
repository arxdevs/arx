use arx_core::{Error, Result};
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use std::path::Path;

const NONCE_LEN: usize = 12;
pub const KEY_LEN: usize = 32;

#[derive(Clone)]
pub struct MasterKey(Key);

impl MasterKey {
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let bytes = std::fs::read(path)?;
            if bytes.len() != KEY_LEN {
                return Err(Error::Internal(format!(
                    "master key at {} has wrong length ({} bytes, want {KEY_LEN})",
                    path.display(),
                    bytes.len()
                )));
            }
            let key = Key::from_slice(&bytes);
            Ok(MasterKey(*key))
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut buf = [0u8; KEY_LEN];
            OsRng.fill_bytes(&mut buf);

            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)?;
                std::io::Write::write_all(&mut f, &buf)?;
            }
            #[cfg(not(unix))]
            {
                std::fs::write(path, &buf)?;
            }
            let key = Key::from_slice(&buf);
            Ok(MasterKey(*key))
        }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let cipher = ChaCha20Poly1305::new(&self.0);
        // 96-bit OsRng nonce. ChaCha20-Poly1305 random nonces are safe up to
        // ~2^32 writes per key (birthday bound); arx variable writes per
        // instance stay well below that. Switch to counter+rotation if we ever
        // approach those volumes.
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| Error::Internal(format!("encrypt: {e}")))?;
        Ok((ct, nonce_bytes.to_vec()))
    }

    pub fn decrypt(&self, ciphertext: &[u8], nonce_bytes: &[u8]) -> Result<Vec<u8>> {
        if nonce_bytes.len() != NONCE_LEN {
            return Err(Error::Internal(format!(
                "bad nonce length: {}",
                nonce_bytes.len()
            )));
        }
        let cipher = ChaCha20Poly1305::new(&self.0);
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| Error::Internal(format!("decrypt: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("master.key");
        let key = MasterKey::load_or_create(&path).unwrap();

        let plaintext = b"postgres://user:hunter2@db:5432/app";
        let (ct, nonce) = key.encrypt(plaintext).unwrap();
        let recovered = key.decrypt(&ct, &nonce).unwrap();
        assert_eq!(plaintext.as_slice(), recovered.as_slice());

        let key2 = MasterKey::load_or_create(&path).unwrap();
        let recovered2 = key2.decrypt(&ct, &nonce).unwrap();
        assert_eq!(plaintext.as_slice(), recovered2.as_slice());
    }

    #[test]
    fn distinct_nonces() {
        let tmp = tempfile::TempDir::new().unwrap();
        let key = MasterKey::load_or_create(&tmp.path().join("k")).unwrap();
        let (_, n1) = key.encrypt(b"x").unwrap();
        let (_, n2) = key.encrypt(b"x").unwrap();
        assert_ne!(n1, n2);
    }
}

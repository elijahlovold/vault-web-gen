//! Client-side encryption for password-protected slices.
//!
//! Key = PBKDF2-HMAC-SHA256(password, salt, ITERATIONS) → AES-256-GCM. Each blob is
//! `nonce(12) || ciphertext || tag(16)`, which is exactly what WebCrypto's
//! `decrypt({name: "AES-GCM", iv})` expects after slicing off the nonce.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use sha2::{Digest, Sha256};

pub const ITERATIONS: u32 = 600_000;

pub struct SliceKey {
    cipher: Aes256Gcm,
    pub salt: [u8; 16],
}

impl SliceKey {
    /// The salt is derived from the slice id so it stays stable across builds;
    /// visitors who unlocked the slice don't have to log in again after a rebuild.
    pub fn derive(password: &str, slice_id: &str) -> SliceKey {
        let digest = Sha256::digest(format!("vault-web-gen:{slice_id}").as_bytes());
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&digest[..16]);
        let key: [u8; 32] = pbkdf2::pbkdf2_hmac_array::<sha2::Sha256, 32>(password.as_bytes(), &salt, ITERATIONS);
        let key = Key::<Aes256Gcm>::from(key);
        SliceKey { cipher: Aes256Gcm::new(&key), salt }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; 12];
        rand::fill(&mut nonce);
        let ct = self.cipher.encrypt(&Nonce::from(nonce), plaintext).expect("AES-GCM encryption failed");
        let mut out = Vec::with_capacity(12 + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        out
    }
}

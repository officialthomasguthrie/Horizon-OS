use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngCore;

use crate::error::{Error, Result};
use crate::object::ObjectId;

const NONCE_LEN: usize = 24;

// Two keys derived from one master: one to address objects, one to encrypt them.
// Splitting them keeps the content hash independent of the cipher key.
pub struct Keys {
    addr: [u8; 32],
    enc: [u8; 32],
}

impl Keys {
    pub fn derive(master: &[u8; 32]) -> Keys {
        Keys {
            addr: blake3::derive_key("horizon lifestream addr v1", master),
            enc: blake3::derive_key("horizon lifestream enc v1", master),
        }
    }

    // Content address: keyed hash of the plaintext, so equal data dedups but
    // an attacker without the key cannot confirm a guess.
    pub fn id_of(&self, plaintext: &[u8]) -> ObjectId {
        ObjectId(*blake3::keyed_hash(&self.addr, plaintext).as_bytes())
    }

    // On-disk record is nonce || ciphertext. The object id is bound in as AAD
    // so a record cannot be moved to a different id.
    pub fn seal(&self, id: &ObjectId, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.enc));
        let mut nonce = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ct = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &id.0,
                },
            )
            .map_err(|_| Error::Crypto)?;
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn open(&self, id: &ObjectId, record: &[u8]) -> Result<Vec<u8>> {
        if record.len() < NONCE_LEN {
            return Err(Error::Corrupt("short record".into()));
        }
        let (nonce, ct) = record.split_at(NONCE_LEN);
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.enc));
        cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ct,
                    aad: &id.0,
                },
            )
            .map_err(|_| Error::Crypto)
    }
}

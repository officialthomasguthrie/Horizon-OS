use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::crypto::Keys;
use crate::error::{Error, Result};
use crate::object::ObjectId;

const MARKER_NAME: &str = "store";
const MARKER_BODY: &str = "horizon-lifestream v1\n";

// Distinguishes concurrent writers' temp files. A process-global counter plus
// the pid keeps every in-flight temp name unique, even across two processes
// writing one store, so no two writers ever share a temp path.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

// Flat on-disk object store. Objects live at objects/<2 hex>/<rest>, refs are
// small text files under refs/. The store never sees plaintext: put/get seal
// and open through Keys.
pub struct ObjectStore {
    root: PathBuf,
    keys: Keys,
}

impl ObjectStore {
    pub fn init(root: impl AsRef<Path>, master: &[u8; 32]) -> Result<ObjectStore> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("objects"))?;
        fs::create_dir_all(root.join("refs"))?;
        let marker = root.join(MARKER_NAME);
        if !marker.exists() {
            fs::write(&marker, MARKER_BODY)?;
        }
        Ok(ObjectStore {
            root,
            keys: Keys::derive(master),
        })
    }

    pub fn open(root: impl AsRef<Path>, master: &[u8; 32]) -> Result<ObjectStore> {
        let root = root.as_ref().to_path_buf();
        if !root.join(MARKER_NAME).exists() {
            return Err(Error::Store(format!(
                "{} is not a lifestream store",
                root.display()
            )));
        }
        Ok(ObjectStore {
            root,
            keys: Keys::derive(master),
        })
    }

    fn path_for(&self, id: &ObjectId) -> PathBuf {
        let hex = id.to_hex();
        self.root.join("objects").join(&hex[0..2]).join(&hex[2..])
    }

    pub fn has(&self, id: &ObjectId) -> bool {
        self.path_for(id).exists()
    }

    pub fn put(&self, plaintext: &[u8]) -> Result<ObjectId> {
        let id = self.keys.id_of(plaintext);
        let path = self.path_for(&id);
        if path.exists() {
            return Ok(id); // already stored, dedup
        }
        let record = self.keys.seal(&id, plaintext)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        commit_record(&path, &record)?;
        Ok(id)
    }

    pub fn get(&self, id: &ObjectId) -> Result<Vec<u8>> {
        let path = self.path_for(id);
        let record = match fs::read(&path) {
            Ok(r) => r,
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(Error::NotFound(id.to_hex())),
            Err(e) => return Err(e.into()),
        };
        let plaintext = self.keys.open(id, &record)?;
        if self.keys.id_of(&plaintext) != *id {
            return Err(Error::Corrupt(format!("hash mismatch for {id}")));
        }
        Ok(plaintext)
    }

    pub fn delete(&self, id: &ObjectId) -> Result<()> {
        match fs::remove_file(self.path_for(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    // The sealed record for an id, exactly as it sits on disk (nonce ||
    // ciphertext). This is what crosses the wire during sync: a peer relays
    // ciphertext and never sees plaintext.
    pub fn read_record(&self, id: &ObjectId) -> Result<Vec<u8>> {
        match fs::read(self.path_for(id)) {
            Ok(r) => Ok(r),
            Err(e) if e.kind() == ErrorKind::NotFound => Err(Error::NotFound(id.to_hex())),
            Err(e) => Err(e.into()),
        }
    }

    // Accept a sealed record from a peer. We hold the same key, so before
    // trusting it we open it (which checks the AEAD tag and binds the id as AAD)
    // and confirm the plaintext addresses back to id. A record we cannot open or
    // that lies about its id is refused, not written. Returns false if the
    // object was already present.
    pub fn write_record(&self, id: &ObjectId, record: &[u8]) -> Result<bool> {
        let path = self.path_for(id);
        if path.exists() {
            return Ok(false); // already stored, dedup
        }
        let plaintext = self.keys.open(id, record)?;
        if self.keys.id_of(&plaintext) != *id {
            return Err(Error::Corrupt(format!("hash mismatch for {id}")));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        commit_record(&path, record)?;
        Ok(true)
    }

    pub fn list_ids(&self) -> Result<Vec<ObjectId>> {
        let mut out = Vec::new();
        for shard in fs::read_dir(self.root.join("objects"))? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            let prefix = shard.file_name().to_string_lossy().to_string();
            for f in fs::read_dir(shard.path())? {
                let rest = f?.file_name().to_string_lossy().to_string();
                if rest.ends_with(".tmp") {
                    continue;
                }
                if let Some(id) = ObjectId::from_hex(&format!("{prefix}{rest}")) {
                    out.push(id);
                }
            }
        }
        Ok(out)
    }

    pub fn set_ref(&self, name: &str, id: &ObjectId) -> Result<()> {
        fs::write(self.root.join("refs").join(name), id.to_hex())?;
        Ok(())
    }

    pub fn get_ref(&self, name: &str) -> Result<Option<ObjectId>> {
        match fs::read_to_string(self.root.join("refs").join(name)) {
            Ok(s) => Ok(ObjectId::from_hex(s.trim())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn list_refs(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for f in fs::read_dir(self.root.join("refs"))? {
            out.push(f?.file_name().to_string_lossy().to_string());
        }
        Ok(out)
    }
}

// Publish a sealed record at `path` atomically: write it to a temp file in the
// same directory, then rename into place, so a reader sees either the whole
// record or nothing. The temp name carries the pid and a process-global counter,
// so two writers committing the same object at once never share one temp file.
// Rename is atomic, and because the id is a keyed hash of the plaintext every
// writer's record opens to the same object, so whichever writer renames last the
// file left in place is always a valid record for this id. The temp name ends in
// .tmp so list_ids skips any that a crash leaves behind.
fn commit_record(path: &Path, record: &[u8]) -> Result<()> {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), seq));
    fs::write(&tmp, record)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

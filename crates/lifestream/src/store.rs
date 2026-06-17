use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::crypto::Keys;
use crate::error::{Error, Result};
use crate::object::ObjectId;

const MARKER_NAME: &str = "store";
const MARKER_BODY: &str = "horizon-lifestream v1\n";

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
        // write to a temp file then rename so a reader never sees a half record
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, &record)?;
        fs::rename(&tmp, &path)?;
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

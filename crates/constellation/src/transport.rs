use std::collections::HashSet;

use lifestream::{Lifestream, Object, ObjectId};

use crate::error::Result;

// A peer in the Constellation: somewhere Lifestream objects and refs can be
// read from and written to. The sync engine speaks only this trait, so the same
// algorithm runs over the in-process LocalTransport today and a QUIC + Noise
// link later. Everything that crosses it is a sealed record (ciphertext); a
// transport never carries plaintext, which is what lets a relaying peer hold
// only opaque objects.
pub trait Transport {
    // The ids this peer already holds.
    fn have(&self) -> Result<HashSet<ObjectId>>;

    // The sealed record for an id, exactly as stored.
    fn read_record(&self, id: &ObjectId) -> Result<Vec<u8>>;

    // Accept a sealed record. The receiver verifies it before committing.
    // Returns false if the object was already present.
    fn write_record(&self, id: &ObjectId, record: &[u8]) -> Result<bool>;

    // Named refs (HEAD and friends) and their targets.
    fn refs(&self) -> Result<Vec<(String, ObjectId)>>;
    fn get_ref(&self, name: &str) -> Result<Option<ObjectId>>;
    fn set_ref(&self, name: &str, id: &ObjectId) -> Result<()>;

    // Parents of a generation, for fast-forward ref checks. None when the id is
    // not a generation this peer can read.
    fn parents(&self, id: &ObjectId) -> Result<Option<Vec<ObjectId>>>;
}

// In-process transport: a Lifestream reached by direct calls. The "wire" is a
// function call, so it is the natural stand-in for a network peer in tests and
// the real path for syncing two stores on one host.
pub struct LocalTransport {
    ls: Lifestream,
}

impl LocalTransport {
    pub fn new(ls: Lifestream) -> LocalTransport {
        LocalTransport { ls }
    }

    pub fn lifestream(&self) -> &Lifestream {
        &self.ls
    }

    pub fn into_inner(self) -> Lifestream {
        self.ls
    }
}

impl Transport for LocalTransport {
    fn have(&self) -> Result<HashSet<ObjectId>> {
        Ok(self.ls.list_ids()?.into_iter().collect())
    }

    fn read_record(&self, id: &ObjectId) -> Result<Vec<u8>> {
        Ok(self.ls.read_record(id)?)
    }

    fn write_record(&self, id: &ObjectId, record: &[u8]) -> Result<bool> {
        Ok(self.ls.write_record(id, record)?)
    }

    fn refs(&self) -> Result<Vec<(String, ObjectId)>> {
        let mut out = Vec::new();
        for name in self.ls.list_refs()? {
            if let Some(id) = self.ls.get_ref(&name)? {
                out.push((name, id));
            }
        }
        Ok(out)
    }

    fn get_ref(&self, name: &str) -> Result<Option<ObjectId>> {
        Ok(self.ls.get_ref(name)?)
    }

    fn set_ref(&self, name: &str, id: &ObjectId) -> Result<()> {
        Ok(self.ls.set_ref(name, id)?)
    }

    fn parents(&self, id: &ObjectId) -> Result<Option<Vec<ObjectId>>> {
        match self.ls.get(id) {
            Ok(Object::Generation(g)) => Ok(Some(g.parents)),
            Ok(_) => Ok(None),
            Err(lifestream::Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

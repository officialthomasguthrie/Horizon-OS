//! Lifestream: the content-addressed, encrypted, versioned state store.
//!
//! Every change is an immutable object addressed by a keyed hash of its
//! plaintext, so identical data is stored once and the whole tree forms a
//! Merkle DAG. A Generation names a complete system state, which is what makes
//! snapshots, rollback, history, and sync the same mechanism.

mod chunker;
mod crypto;
mod error;
mod object;
mod store;

pub use error::{Error, Result};
pub use object::{Generation, NodeKind, Object, ObjectId, TreeEntry};

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use chunker::Chunker;
use store::ObjectStore;

pub const HEAD: &str = "HEAD";

pub struct Lifestream {
    store: ObjectStore,
    chunker: Chunker,
}

impl Lifestream {
    pub fn init(root: impl AsRef<Path>, master: &[u8; 32]) -> Result<Lifestream> {
        Ok(Lifestream {
            store: ObjectStore::init(root, master)?,
            chunker: Chunker::new(),
        })
    }

    pub fn open(root: impl AsRef<Path>, master: &[u8; 32]) -> Result<Lifestream> {
        Ok(Lifestream {
            store: ObjectStore::open(root, master)?,
            chunker: Chunker::new(),
        })
    }

    pub fn put(&self, obj: &Object) -> Result<ObjectId> {
        self.store.put(&obj.encode())
    }

    pub fn get(&self, id: &ObjectId) -> Result<Object> {
        Object::decode(&self.store.get(id)?)
    }

    pub fn has(&self, id: &ObjectId) -> bool {
        self.store.has(id)
    }

    pub fn object_count(&self) -> Result<usize> {
        Ok(self.store.list_ids()?.len())
    }

    // Store raw bytes as a chunked File object and return its id.
    pub fn write_bytes(&self, data: &[u8]) -> Result<ObjectId> {
        let mut chunks = Vec::new();
        for r in self.chunker.split(data) {
            chunks.push(self.put(&Object::Chunk(data[r].to_vec()))?);
        }
        self.put(&Object::File { chunks })
    }

    pub fn read_bytes(&self, file: &ObjectId) -> Result<Vec<u8>> {
        match self.get(file)? {
            Object::File { chunks } => {
                let mut out = Vec::new();
                for c in chunks {
                    match self.get(&c)? {
                        Object::Chunk(d) => out.extend_from_slice(&d),
                        _ => return Err(Error::Corrupt("expected chunk".into())),
                    }
                }
                Ok(out)
            }
            _ => Err(Error::Corrupt("expected file".into())),
        }
    }

    // Walk a directory into a tree object, return the root tree id.
    pub fn snapshot_dir(&self, dir: &Path) -> Result<ObjectId> {
        let mut entries = Vec::new();
        let mut items: Vec<_> = fs::read_dir(dir)?.collect::<std::result::Result<_, _>>()?;
        items.sort_by_key(|e| e.file_name());
        for item in items {
            let name = item
                .file_name()
                .into_string()
                .map_err(|_| Error::Store("non-utf8 filename".into()))?;
            let md = fs::symlink_metadata(item.path())?;
            let ft = md.file_type();
            let mode = mode_of(&md);
            if ft.is_symlink() {
                let target = fs::read_link(item.path())?;
                let id = self.write_bytes(target.to_string_lossy().as_bytes())?;
                entries.push(TreeEntry {
                    name,
                    kind: NodeKind::Symlink,
                    id,
                    mode,
                });
            } else if ft.is_dir() {
                let id = self.snapshot_dir(&item.path())?;
                entries.push(TreeEntry {
                    name,
                    kind: NodeKind::Tree,
                    id,
                    mode,
                });
            } else if ft.is_file() {
                let data = fs::read(item.path())?;
                let id = self.write_bytes(&data)?;
                entries.push(TreeEntry {
                    name,
                    kind: NodeKind::File,
                    id,
                    mode,
                });
            }
            // sockets, fifos and devices are skipped on purpose
        }
        self.put(&Object::Tree { entries })
    }

    pub fn restore_tree(&self, tree: &ObjectId, dest: &Path) -> Result<()> {
        fs::create_dir_all(dest)?;
        match self.get(tree)? {
            Object::Tree { entries } => {
                for e in entries {
                    let p = dest.join(&e.name);
                    match e.kind {
                        NodeKind::Tree => {
                            self.restore_tree(&e.id, &p)?;
                            set_mode(&p, e.mode);
                        }
                        NodeKind::File => {
                            let data = self.read_bytes(&e.id)?;
                            fs::write(&p, &data)?;
                            set_mode(&p, e.mode);
                        }
                        NodeKind::Symlink => {
                            let target = self.read_bytes(&e.id)?;
                            make_symlink(&target, &p)?;
                        }
                    }
                }
                Ok(())
            }
            _ => Err(Error::Corrupt("expected tree".into())),
        }
    }

    // Wrap a root tree in a Generation and move HEAD to it.
    pub fn commit(&self, root: ObjectId, parents: Vec<ObjectId>, label: &str) -> Result<ObjectId> {
        let time_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let gen = Generation {
            root,
            parents,
            time_unix,
            label: label.to_string(),
            meta: String::new(),
        };
        let id = self.put(&Object::Generation(gen))?;
        self.store.set_ref(HEAD, &id)?;
        Ok(id)
    }

    pub fn head(&self) -> Result<Option<ObjectId>> {
        self.store.get_ref(HEAD)
    }

    pub fn set_ref(&self, name: &str, id: &ObjectId) -> Result<()> {
        self.store.set_ref(name, id)
    }

    pub fn get_ref(&self, name: &str) -> Result<Option<ObjectId>> {
        self.store.get_ref(name)
    }

    pub fn list_refs(&self) -> Result<Vec<String>> {
        self.store.list_refs()
    }

    // History from a generation, newest first, following the first parent.
    pub fn history(&self, gen: &ObjectId) -> Result<Vec<(ObjectId, Generation)>> {
        let mut out = Vec::new();
        let mut cur = Some(*gen);
        while let Some(id) = cur {
            match self.get(&id)? {
                Object::Generation(g) => {
                    cur = g.parents.first().copied();
                    out.push((id, g));
                }
                _ => return Err(Error::Corrupt("expected generation".into())),
            }
        }
        Ok(out)
    }

    // Mark and sweep: keep everything reachable from roots, delete the rest.
    pub fn gc(&self, roots: &[ObjectId]) -> Result<usize> {
        let mut live: HashSet<ObjectId> = HashSet::new();
        let mut stack = roots.to_vec();
        while let Some(id) = stack.pop() {
            if !live.insert(id) {
                continue;
            }
            // a stale root may point at a missing object, just skip it
            if let Ok(obj) = self.get(&id) {
                for l in obj.links() {
                    if !live.contains(&l) {
                        stack.push(l);
                    }
                }
            }
        }
        let mut removed = 0;
        for id in self.store.list_ids()? {
            if !live.contains(&id) {
                self.store.delete(&id)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[cfg(unix)]
fn mode_of(md: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    md.permissions().mode()
}
#[cfg(not(unix))]
fn mode_of(_: &fs::Metadata) -> u32 {
    0o644
}

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(mode));
}
#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) {}

#[cfg(unix)]
fn make_symlink(target: &[u8], p: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(target), p)?;
    Ok(())
}
#[cfg(not(unix))]
fn make_symlink(_: &[u8], _: &Path) -> Result<()> {
    Ok(())
}

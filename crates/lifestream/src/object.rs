use std::fmt;

use crate::error::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub [u8; 32]);

impl ObjectId {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Option<ObjectId> {
        let v = hex::decode(s).ok()?;
        if v.len() != 32 {
            return None;
        }
        let mut a = [0u8; 32];
        a.copy_from_slice(&v);
        Some(ObjectId(a))
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({})", &self.to_hex()[..16])
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    File,
    Tree,
    Symlink,
}

impl NodeKind {
    fn tag(self) -> u8 {
        match self {
            NodeKind::File => 0,
            NodeKind::Tree => 1,
            NodeKind::Symlink => 2,
        }
    }
    fn from_tag(t: u8) -> Result<NodeKind> {
        match t {
            0 => Ok(NodeKind::File),
            1 => Ok(NodeKind::Tree),
            2 => Ok(NodeKind::Symlink),
            _ => Err(Error::Corrupt(format!("bad node kind {t}"))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub name: String,
    pub kind: NodeKind,
    pub id: ObjectId,
    pub mode: u32,
}

#[derive(Clone, Debug)]
pub struct Generation {
    pub root: ObjectId,
    pub parents: Vec<ObjectId>,
    pub time_unix: u64,
    pub label: String,
    pub meta: String,
}

// The four object kinds. Everything in the store is one of these, encoded to
// bytes, content-addressed, then encrypted.
#[derive(Clone, Debug)]
pub enum Object {
    Chunk(Vec<u8>),
    File { chunks: Vec<ObjectId> },
    Tree { entries: Vec<TreeEntry> },
    Generation(Generation),
}

impl Object {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        match self {
            Object::Chunk(d) => {
                o.push(1);
                o.extend_from_slice(d);
            }
            Object::File { chunks } => {
                o.push(2);
                put_u32(&mut o, chunks.len() as u32);
                for c in chunks {
                    o.extend_from_slice(&c.0);
                }
            }
            Object::Tree { entries } => {
                o.push(3);
                put_u32(&mut o, entries.len() as u32);
                for e in entries {
                    put_u16(&mut o, e.name.len() as u16);
                    o.extend_from_slice(e.name.as_bytes());
                    o.push(e.kind.tag());
                    put_u32(&mut o, e.mode);
                    o.extend_from_slice(&e.id.0);
                }
            }
            Object::Generation(g) => {
                o.push(4);
                o.extend_from_slice(&g.root.0);
                o.push(g.parents.len() as u8);
                for p in &g.parents {
                    o.extend_from_slice(&p.0);
                }
                put_u64(&mut o, g.time_unix);
                put_u32(&mut o, g.label.len() as u32);
                o.extend_from_slice(g.label.as_bytes());
                put_u32(&mut o, g.meta.len() as u32);
                o.extend_from_slice(g.meta.as_bytes());
            }
        }
        o
    }

    pub fn decode(buf: &[u8]) -> Result<Object> {
        let mut r = Reader::new(buf);
        let tag = r.u8()?;
        match tag {
            1 => Ok(Object::Chunk(r.rest().to_vec())),
            2 => {
                let n = r.u32()?;
                let mut chunks = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    chunks.push(ObjectId(r.arr32()?));
                }
                Ok(Object::File { chunks })
            }
            3 => {
                let n = r.u32()?;
                let mut entries = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let nl = r.u16()? as usize;
                    let name = String::from_utf8(r.bytes(nl)?.to_vec())
                        .map_err(|_| Error::Corrupt("name not utf8".into()))?;
                    let kind = NodeKind::from_tag(r.u8()?)?;
                    let mode = r.u32()?;
                    let id = ObjectId(r.arr32()?);
                    entries.push(TreeEntry {
                        name,
                        kind,
                        id,
                        mode,
                    });
                }
                Ok(Object::Tree { entries })
            }
            4 => {
                let root = ObjectId(r.arr32()?);
                let pc = r.u8()?;
                let mut parents = Vec::with_capacity(pc as usize);
                for _ in 0..pc {
                    parents.push(ObjectId(r.arr32()?));
                }
                let time_unix = r.u64()?;
                let ll = r.u32()? as usize;
                let label = String::from_utf8(r.bytes(ll)?.to_vec())
                    .map_err(|_| Error::Corrupt("label not utf8".into()))?;
                let ml = r.u32()? as usize;
                let meta = String::from_utf8(r.bytes(ml)?.to_vec())
                    .map_err(|_| Error::Corrupt("meta not utf8".into()))?;
                Ok(Object::Generation(Generation {
                    root,
                    parents,
                    time_unix,
                    label,
                    meta,
                }))
            }
            _ => Err(Error::Corrupt(format!("bad object tag {tag}"))),
        }
    }

    // Ids this object points at. Used by gc to walk the graph.
    pub fn links(&self) -> Vec<ObjectId> {
        match self {
            Object::Chunk(_) => Vec::new(),
            Object::File { chunks } => chunks.clone(),
            Object::Tree { entries } => entries.iter().map(|e| e.id).collect(),
            Object::Generation(g) => {
                let mut v = vec![g.root];
                v.extend_from_slice(&g.parents);
                v
            }
        }
    }
}

fn put_u16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_be_bytes());
}
fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_be_bytes());
}
fn put_u64(o: &mut Vec<u8>, v: u64) {
    o.extend_from_slice(&v.to_be_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Reader<'a> {
        Reader { b, p: 0 }
    }
    fn need(&self, n: usize) -> Result<()> {
        if self.p + n > self.b.len() {
            return Err(Error::Corrupt("unexpected end of object".into()));
        }
        Ok(())
    }
    fn u8(&mut self) -> Result<u8> {
        self.need(1)?;
        let v = self.b[self.p];
        self.p += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16> {
        self.need(2)?;
        let v = u16::from_be_bytes([self.b[self.p], self.b[self.p + 1]]);
        self.p += 2;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32> {
        self.need(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(&self.b[self.p..self.p + 4]);
        self.p += 4;
        Ok(u32::from_be_bytes(a))
    }
    fn u64(&mut self) -> Result<u64> {
        self.need(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(&self.b[self.p..self.p + 8]);
        self.p += 8;
        Ok(u64::from_be_bytes(a))
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.need(n)?;
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn arr32(&mut self) -> Result<[u8; 32]> {
        let s = self.bytes(32)?;
        let mut a = [0u8; 32];
        a.copy_from_slice(s);
        Ok(a)
    }
    fn rest(&mut self) -> &'a [u8] {
        let s = &self.b[self.p..];
        self.p = self.b.len();
        s
    }
}

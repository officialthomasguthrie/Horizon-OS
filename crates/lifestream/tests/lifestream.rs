use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use lifestream::{Lifestream, Object};
use tempfile::tempdir;

const KEY: [u8; 32] = [7u8; 32];

// deterministic pseudo-random bytes so tests are reproducible
fn pseudo(seed: u64, n: usize) -> Vec<u8> {
    let mut x = seed | 1;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.push((x & 0xff) as u8);
    }
    v
}

fn collect(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn rec(base: &Path, dir: &Path, m: &mut BTreeMap<String, Vec<u8>>) {
        let mut items: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).collect();
        items.sort_by_key(|e| e.file_name());
        for it in items {
            let p = it.path();
            let md = fs::symlink_metadata(&p).unwrap();
            if md.is_dir() {
                rec(base, &p, m);
            } else if md.is_file() {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().to_string();
                m.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    rec(root, root, &mut m);
    m
}

#[test]
fn object_roundtrip() {
    let d = tempdir().unwrap();
    let ls = Lifestream::init(d.path(), &KEY).unwrap();
    let id = ls.put(&Object::Chunk(b"hello".to_vec())).unwrap();
    match ls.get(&id).unwrap() {
        Object::Chunk(b) => assert_eq!(b, b"hello"),
        _ => panic!("wrong object kind"),
    }
}

#[test]
fn write_and_read_various_sizes() {
    let d = tempdir().unwrap();
    let ls = Lifestream::init(d.path(), &KEY).unwrap();
    for n in [0usize, 1, 100, 8 * 1024, 200 * 1024] {
        let data = pseudo(n as u64 + 1, n);
        let id = ls.write_bytes(&data).unwrap();
        assert_eq!(ls.read_bytes(&id).unwrap(), data, "size {n}");
    }
}

#[test]
fn identical_data_dedups() {
    let d = tempdir().unwrap();
    let ls = Lifestream::init(d.path(), &KEY).unwrap();
    let data = pseudo(42, 100 * 1024);
    let a = ls.write_bytes(&data).unwrap();
    let before = ls.object_count().unwrap();
    let b = ls.write_bytes(&data).unwrap();
    let after = ls.object_count().unwrap();
    assert_eq!(a, b);
    assert_eq!(before, after);
}

#[test]
fn edit_only_rewrites_touched_chunks() {
    let d = tempdir().unwrap();
    let ls = Lifestream::init(d.path(), &KEY).unwrap();
    let mut data = pseudo(99, 300 * 1024);
    ls.write_bytes(&data).unwrap();
    let before = ls.object_count().unwrap();
    for byte in data.iter_mut().skip(150 * 1024).take(16) {
        *byte ^= 0xff;
    }
    let id = ls.write_bytes(&data).unwrap();
    let after = ls.object_count().unwrap();
    assert_eq!(ls.read_bytes(&id).unwrap(), data);
    let added = after - before;
    assert!(added < 8, "edit added {added} objects, expected only a few");
}

#[test]
fn snapshot_and_restore_roundtrip() {
    let d = tempdir().unwrap();
    let ls = Lifestream::init(d.path(), &KEY).unwrap();
    let src = tempdir().unwrap();
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("a.txt"), b"hello").unwrap();
    fs::write(src.path().join("sub/b.bin"), pseudo(5, 120 * 1024)).unwrap();
    fs::write(src.path().join("sub/c.txt"), b"world").unwrap();
    let tree = ls.snapshot_dir(src.path()).unwrap();
    let dst = tempdir().unwrap();
    ls.restore_tree(&tree, dst.path()).unwrap();
    assert_eq!(collect(src.path()), collect(dst.path()));
}

#[test]
fn commit_and_history() {
    let d = tempdir().unwrap();
    let ls = Lifestream::init(d.path(), &KEY).unwrap();
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), b"1").unwrap();
    let t1 = ls.snapshot_dir(src.path()).unwrap();
    let g1 = ls.commit(t1, vec![], "v1").unwrap();
    fs::write(src.path().join("a.txt"), b"2").unwrap();
    let t2 = ls.snapshot_dir(src.path()).unwrap();
    let g2 = ls.commit(t2, vec![g1], "v2").unwrap();

    let h = ls.history(&g2).unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(h[0].0, g2);
    assert_eq!(h[0].1.label, "v2");
    assert_eq!(h[1].0, g1);
    assert_eq!(ls.head().unwrap(), Some(g2));
}

#[test]
fn gc_keeps_reachable_drops_garbage() {
    let d = tempdir().unwrap();
    let ls = Lifestream::init(d.path(), &KEY).unwrap();
    let src = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), pseudo(3, 40 * 1024)).unwrap();
    let tree = ls.snapshot_dir(src.path()).unwrap();
    let g = ls.commit(tree, vec![], "v1").unwrap();

    let junk = ls.write_bytes(&pseudo(123, 50 * 1024)).unwrap();
    let removed = ls.gc(&[g]).unwrap();
    assert!(removed >= 1);
    assert!(!ls.has(&junk));

    let root = match ls.get(&g).unwrap() {
        Object::Generation(gen) => gen.root,
        _ => panic!("expected generation"),
    };
    let dst = tempdir().unwrap();
    ls.restore_tree(&root, dst.path()).unwrap();
    assert_eq!(collect(src.path()), collect(dst.path()));
}

#[test]
fn wrong_key_cannot_read() {
    let d = tempdir().unwrap();
    let ls = Lifestream::init(d.path(), &[1u8; 32]).unwrap();
    let id = ls.write_bytes(b"secret data here").unwrap();
    let ls2 = Lifestream::open(d.path(), &[2u8; 32]).unwrap();
    assert!(ls2.read_bytes(&id).is_err());
}

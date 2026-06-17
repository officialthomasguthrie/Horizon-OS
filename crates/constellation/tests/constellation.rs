use std::collections::HashSet;
use std::fs;

use constellation::{sync, LocalTransport, Transport};
use lifestream::{Lifestream, NodeKind, Object, ObjectId, TreeEntry};
use tempfile::{tempdir, TempDir};

const KEY: [u8; 32] = [9u8; 32];
const OTHER_KEY: [u8; 32] = [3u8; 32];

fn peer(dir: &TempDir, key: &[u8; 32]) -> LocalTransport {
    LocalTransport::new(Lifestream::init(dir.path(), key).unwrap())
}

// Non-repeating bytes so chunks are distinct and do not silently dedup, which
// keeps transferred-byte counts meaningful in tests.
fn varied(n: usize) -> Vec<u8> {
    (0..n as u32)
        .map(|i| {
            let z = i.wrapping_mul(2_654_435_761);
            (z ^ (z >> 15)) as u8
        })
        .collect()
}

// A file object plus a tree pointing at it, returning the tree id.
fn tree_with(ls: &Lifestream, entries: &[(&str, ObjectId)]) -> ObjectId {
    let entries = entries
        .iter()
        .map(|(name, id)| TreeEntry {
            name: (*name).to_string(),
            kind: NodeKind::File,
            id: *id,
            mode: 0o644,
        })
        .collect();
    ls.put(&Object::Tree { entries }).unwrap()
}

#[test]
fn sync_transfers_every_missing_object_and_sets_head() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let a = peer(&da, &KEY);
    let b = peer(&db, &KEY);

    let file = a.lifestream().write_bytes(&varied(200_000)).unwrap();
    let tree = tree_with(a.lifestream(), &[("big", file)]);
    let gen = a.lifestream().commit(tree, vec![], "first").unwrap();

    let report = sync(&a, &b).unwrap();

    assert_eq!(report.dest_objects, 0);
    assert_eq!(
        report.source_objects,
        a.lifestream().object_count().unwrap()
    );
    assert_eq!(report.transferred, a.lifestream().object_count().unwrap());
    assert!(report.bytes > 200_000);
    assert_eq!(report.refs_set, vec!["HEAD".to_string()]);

    // Every id on a is now on b, and HEAD points at the same generation.
    let have_a: HashSet<_> = a.have().unwrap();
    let have_b: HashSet<_> = b.have().unwrap();
    assert_eq!(have_a, have_b);
    assert_eq!(b.lifestream().head().unwrap(), Some(gen));
}

#[test]
fn sync_is_idempotent() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let a = peer(&da, &KEY);
    let b = peer(&db, &KEY);

    let file = a.lifestream().write_bytes(b"hello constellation").unwrap();
    let tree = tree_with(a.lifestream(), &[("note", file)]);
    a.lifestream().commit(tree, vec![], "first").unwrap();

    let first = sync(&a, &b).unwrap();
    assert!(first.moved_anything());

    let second = sync(&a, &b).unwrap();
    assert_eq!(second.transferred, 0);
    assert_eq!(second.bytes, 0);
    assert!(second.refs_set.is_empty());
    assert!(second.refs_advanced.is_empty());
    assert!(!second.moved_anything());
}

#[test]
fn dedup_means_only_new_objects_cross() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let a = peer(&da, &KEY);
    let b = peer(&db, &KEY);

    // Round one: a big file in its own generation, synced over in full.
    let big = a.lifestream().write_bytes(&varied(300_000)).unwrap();
    let t1 = tree_with(a.lifestream(), &[("big", big)]);
    let g1 = a.lifestream().commit(t1, vec![], "g1").unwrap();
    sync(&a, &b).unwrap();

    let before: HashSet<_> = a.have().unwrap();

    // Round two: add a tiny file alongside the unchanged big one.
    let small = a.lifestream().write_bytes(b"tiny").unwrap();
    let t2 = tree_with(a.lifestream(), &[("big", big), ("tiny", small)]);
    a.lifestream().commit(t2, vec![g1], "g2").unwrap();

    let after: HashSet<_> = a.have().unwrap();
    let genuinely_new = after.difference(&before).count();

    let report = sync(&a, &b).unwrap();

    // Only the new objects move, and that is far fewer than the whole store.
    assert_eq!(report.transferred, genuinely_new);
    assert!(report.transferred < after.len());
    assert_eq!(a.have().unwrap(), b.have().unwrap());
}

#[test]
fn head_fast_forwards_along_history() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let a = peer(&da, &KEY);
    let b = peer(&db, &KEY);

    let f1 = a.lifestream().write_bytes(b"one").unwrap();
    let g1 = a
        .lifestream()
        .commit(tree_with(a.lifestream(), &[("f", f1)]), vec![], "g1")
        .unwrap();
    sync(&a, &b).unwrap();
    assert_eq!(b.lifestream().head().unwrap(), Some(g1));

    let f2 = a.lifestream().write_bytes(b"two").unwrap();
    let g2 = a
        .lifestream()
        .commit(tree_with(a.lifestream(), &[("f", f2)]), vec![g1], "g2")
        .unwrap();

    let report = sync(&a, &b).unwrap();
    assert_eq!(report.refs_advanced, vec!["HEAD".to_string()]);
    assert!(report.refs_set.is_empty());
    assert!(report.refs_conflicted.is_empty());
    assert_eq!(b.lifestream().head().unwrap(), Some(g2));
}

#[test]
fn divergent_head_is_reported_not_clobbered() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let a = peer(&da, &KEY);
    let b = peer(&db, &KEY);

    // Shared base generation on both sides.
    let f0 = a.lifestream().write_bytes(b"base").unwrap();
    let g0 = a
        .lifestream()
        .commit(tree_with(a.lifestream(), &[("f", f0)]), vec![], "g0")
        .unwrap();
    sync(&a, &b).unwrap();

    // Each side commits its own child of g0: the histories diverge.
    let fa = a.lifestream().write_bytes(b"a-side").unwrap();
    let g_a = a
        .lifestream()
        .commit(tree_with(a.lifestream(), &[("f", fa)]), vec![g0], "a")
        .unwrap();
    let fb = b.lifestream().write_bytes(b"b-side").unwrap();
    let g_b = b
        .lifestream()
        .commit(tree_with(b.lifestream(), &[("f", fb)]), vec![g0], "b")
        .unwrap();

    let report = sync(&a, &b).unwrap();

    // Objects still replicate; only the ref move is refused.
    assert!(report.transferred > 0);
    assert!(b.lifestream().has(&g_a));
    assert_eq!(report.refs_conflicted, vec!["HEAD".to_string()]);
    assert!(report.refs_advanced.is_empty());
    assert_eq!(b.lifestream().head().unwrap(), Some(g_b));
}

#[test]
fn synced_generation_restores_on_the_other_side() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let src = tempdir().unwrap();
    let dst = tempdir().unwrap();
    fs::write(src.path().join("a.txt"), b"alpha").unwrap();
    fs::write(src.path().join("b.txt"), vec![1u8; 120_000]).unwrap();

    let a = peer(&da, &KEY);
    let b = peer(&db, &KEY);

    let tree = a.lifestream().snapshot_dir(src.path()).unwrap();
    let gen = a.lifestream().commit(tree, vec![], "snapshot").unwrap();

    sync(&a, &b).unwrap();

    // b can decrypt and restore what only a ever wrote.
    let root = match b.lifestream().get(&gen).unwrap() {
        Object::Generation(g) => g.root,
        _ => panic!("not a generation"),
    };
    b.lifestream().restore_tree(&root, dst.path()).unwrap();
    assert_eq!(fs::read(dst.path().join("a.txt")).unwrap(), b"alpha");
    assert_eq!(
        fs::read(dst.path().join("b.txt")).unwrap(),
        vec![1u8; 120_000]
    );
}

#[test]
fn a_peer_with_the_wrong_key_cannot_accept_records() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let a = peer(&da, &KEY);
    let b = peer(&db, &OTHER_KEY); // a different identity

    let file = a.lifestream().write_bytes(b"secret").unwrap();
    let tree = tree_with(a.lifestream(), &[("s", file)]);
    a.lifestream().commit(tree, vec![], "first").unwrap();

    // The sealed records will not open under b's key, so sync fails loudly
    // rather than writing objects b can never read.
    let err = sync(&a, &b).unwrap_err();
    assert!(matches!(err, constellation::Error::Lifestream(_)));
    // Nothing was committed past the failed record.
    assert!(b.lifestream().object_count().unwrap() < a.lifestream().object_count().unwrap());
}

#[test]
fn two_way_sync_converges_both_stores() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let a = peer(&da, &KEY);
    let b = peer(&db, &KEY);

    let fa = a.lifestream().write_bytes(b"only on a").unwrap();
    a.lifestream().set_ref("a-ref", &fa).unwrap();
    let fb = b.lifestream().write_bytes(b"only on b").unwrap();
    b.lifestream().set_ref("b-ref", &fb).unwrap();

    sync(&a, &b).unwrap();
    sync(&b, &a).unwrap();

    assert_eq!(a.have().unwrap(), b.have().unwrap());
    assert_eq!(a.lifestream().get_ref("b-ref").unwrap(), Some(fb));
    assert_eq!(b.lifestream().get_ref("a-ref").unwrap(), Some(fa));
}

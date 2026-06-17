// The QUIC + Noise transport, exercised over loopback. These run the real sync
// engine across a real network stack: one side is a Server, the other reaches it
// through a NetworkTransport, and both are just `Transport`s to `sync`.
#![cfg(feature = "net")]

use std::collections::HashSet;
use std::net::SocketAddr;

use constellation::{sync, LocalTransport, NetworkTransport, Server, Transport};
use lifestream::{Lifestream, NodeKind, Object, ObjectId, TreeEntry};
use tempfile::{tempdir, TempDir};

const KEY: [u8; 32] = [9u8; 32];
const OTHER_KEY: [u8; 32] = [3u8; 32];

fn store(dir: &TempDir, key: &[u8; 32]) -> Lifestream {
    Lifestream::init(dir.path(), key).unwrap()
}

fn loopback() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

// Non-repeating bytes so chunks stay distinct and a few records grow past the
// 64 KiB cut, which forces the channel to segment them under the Noise message
// limit and reassemble on the far side.
fn varied(n: usize) -> Vec<u8> {
    (0..n as u32)
        .map(|i| {
            let z = i.wrapping_mul(2_654_435_761);
            (z ^ (z >> 13)) as u8
        })
        .collect()
}

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

// Push from a local store to a remote Server over the wire. Large content here
// makes the client send multi-segment record frames.
#[test]
fn push_replicates_to_a_remote_server() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();

    // The far side: a Server wrapping store B, listening on a chosen-by-OS port.
    let server = Server::start(loopback(), KEY, store(&db, &KEY)).unwrap();
    let addr = server.local_addr();

    // The near side: store A with real content in one generation.
    let a = LocalTransport::new(store(&da, &KEY));
    let big = a.lifestream().write_bytes(&varied(300_000)).unwrap();
    let small = a.lifestream().write_bytes(b"hello over quic").unwrap();
    let tree = tree_with(a.lifestream(), &[("big", big), ("note", small)]);
    let gen = a.lifestream().commit(tree, vec![], "first").unwrap();

    let remote = NetworkTransport::connect(addr, KEY).unwrap();
    let report = sync(&a, &remote).unwrap();

    assert_eq!(report.transferred, a.lifestream().object_count().unwrap());
    assert!(report.bytes > 300_000);
    assert_eq!(report.refs_set, vec!["HEAD".to_string()]);

    // Seen through the wire, the remote now holds exactly what A holds.
    assert_eq!(a.have().unwrap(), remote.have().unwrap());
    assert_eq!(remote.get_ref("HEAD").unwrap(), Some(gen));

    // And on B's actual disk, a fresh handle decrypts the pushed generation.
    let b = Lifestream::open(db.path(), &KEY).unwrap();
    assert_eq!(
        b.object_count().unwrap(),
        a.lifestream().object_count().unwrap()
    );
    assert_eq!(b.head().unwrap(), Some(gen));
    drop(server);
}

// Pull from a remote Server into a local store. Large content here makes the
// server send multi-segment record frames the client reassembles.
#[test]
fn pull_replicates_from_a_remote_server() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();

    // The far side holds the content this time.
    let source = store(&da, &KEY);
    let payload = varied(250_000);
    let f = source.write_bytes(&payload).unwrap();
    let tree = tree_with(&source, &[("data", f)]);
    let gen = source.commit(tree, vec![], "src").unwrap();
    let want: HashSet<ObjectId> = source.list_ids().unwrap().into_iter().collect();

    let server = Server::start(loopback(), KEY, source).unwrap();
    let addr = server.local_addr();

    // The near side starts empty and pulls everything down.
    let local = LocalTransport::new(store(&db, &KEY));
    let remote = NetworkTransport::connect(addr, KEY).unwrap();
    let report = sync(&remote, &local).unwrap();

    assert_eq!(report.transferred, want.len());
    assert_eq!(local.have().unwrap(), want);
    assert_eq!(local.lifestream().head().unwrap(), Some(gen));

    // The pulled bytes round-trip back to the original payload.
    let root = match local.lifestream().get(&gen).unwrap() {
        Object::Generation(g) => g.root,
        _ => panic!("not a generation"),
    };
    let entries = match local.lifestream().get(&root).unwrap() {
        Object::Tree { entries } => entries,
        _ => panic!("not a tree"),
    };
    assert_eq!(
        local.lifestream().read_bytes(&entries[0].id).unwrap(),
        payload
    );
    drop(server);
}

// Several peers push into one Server at the same time. The server spawns a task
// per connection over a single shared store, so concurrent peers writing the
// same object ids land on the store's concurrent-write path. Nothing should be
// lost or corrupted: the store ends with the union of every peer's objects, each
// still decryptable.
#[test]
fn serves_several_peers_at_once() {
    use std::sync::Arc;
    use std::thread;

    const PEERS: usize = 6;
    const SHARED: usize = 20;
    const UNIQUE: usize = 2;

    // Distinct shared payloads (every peer holds these, so they race to write the
    // same ids), with one oversized so its record must segment on the wire while
    // several peers push it at once.
    let shared: Arc<Vec<Vec<u8>>> = Arc::new(
        (0..SHARED)
            .map(|i| {
                if i == 0 {
                    varied(200_000)
                } else {
                    varied(8_000 + i * 137)
                }
            })
            .collect(),
    );

    // The far side starts empty and receives from every peer concurrently.
    let db = tempdir().unwrap();
    let server = Server::start(loopback(), KEY, store(&db, &KEY)).unwrap();
    let addr = server.local_addr();

    let mut handles = Vec::new();
    for p in 0..PEERS {
        let shared = shared.clone();
        handles.push(thread::spawn(move || {
            let dir = tempdir().unwrap();
            let local = LocalTransport::new(Lifestream::init(dir.path(), &KEY).unwrap());
            for payload in shared.iter() {
                local
                    .lifestream()
                    .put(&Object::Chunk(payload.clone()))
                    .unwrap();
            }
            for j in 0..UNIQUE {
                local
                    .lifestream()
                    .put(&Object::Chunk(format!("peer-{p}-uniq-{j}").into_bytes()))
                    .unwrap();
            }
            let remote = NetworkTransport::connect(addr, KEY).unwrap();
            sync(&local, &remote).unwrap();
            remote.close().ok();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // Read the server's store fresh from disk: it holds the union of every peer's
    // objects, each decryptable, with nothing dropped or corrupted.
    let b = Lifestream::open(db.path(), &KEY).unwrap();
    assert_eq!(b.object_count().unwrap(), SHARED + PEERS * UNIQUE);

    let refdir = tempdir().unwrap();
    let reference = Lifestream::init(refdir.path(), &KEY).unwrap();
    for payload in shared.iter() {
        let id = reference.put(&Object::Chunk(payload.clone())).unwrap();
        match b.get(&id).unwrap() {
            Object::Chunk(bytes) => assert_eq!(&bytes, payload),
            _ => panic!("wrong object kind on server"),
        }
    }
    drop(server);
}

// A wrong identity cannot even connect: the Noise handshake fails on the first
// message, so the peer is turned away before any object moves.
#[test]
fn a_wrong_identity_is_refused_at_the_handshake() {
    let db = tempdir().unwrap();
    let server = Server::start(loopback(), KEY, store(&db, &KEY)).unwrap();
    let addr = server.local_addr();

    let err = NetworkTransport::connect(addr, OTHER_KEY);
    assert!(err.is_err(), "a different identity must not connect");

    // The right identity still connects to the same server.
    assert!(NetworkTransport::connect(addr, KEY).is_ok());
    drop(server);
}

// A second push moves only the genuinely new objects: content addressing dedups
// across the wire exactly as it does in process.
#[test]
fn second_push_only_moves_new_objects() {
    let da = tempdir().unwrap();
    let db = tempdir().unwrap();
    let server = Server::start(loopback(), KEY, store(&db, &KEY)).unwrap();
    let addr = server.local_addr();

    let a = LocalTransport::new(store(&da, &KEY));
    let big = a.lifestream().write_bytes(&varied(120_000)).unwrap();
    let g1 = a
        .lifestream()
        .commit(tree_with(a.lifestream(), &[("big", big)]), vec![], "g1")
        .unwrap();

    let remote = NetworkTransport::connect(addr, KEY).unwrap();
    let first = sync(&a, &remote).unwrap();
    assert!(first.moved_anything());

    // Idempotent: nothing new the second time.
    let again = sync(&a, &remote).unwrap();
    assert_eq!(again.transferred, 0);
    assert!(!again.moved_anything());

    // Add a small file alongside the unchanged big one; only the new objects
    // cross, and HEAD fast-forwards rather than being set fresh.
    let small = a.lifestream().write_bytes(b"second generation").unwrap();
    let tree = tree_with(a.lifestream(), &[("big", big), ("note", small)]);
    let g2 = a.lifestream().commit(tree, vec![g1], "g2").unwrap();

    let third = sync(&a, &remote).unwrap();
    assert!(third.transferred > 0);
    assert!(third.transferred < a.lifestream().object_count().unwrap());
    assert_eq!(third.refs_advanced, vec!["HEAD".to_string()]);
    assert_eq!(remote.get_ref("HEAD").unwrap(), Some(g2));
    drop(server);
}

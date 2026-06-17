// Hole punching, exercised over loopback. The punch coordination is plain QUIC
// brokered by a rendezvous, so the whole wait-broker-fire-then-sync path runs in
// CI: a Rendezvous, a serving Server waiting to be punched under its identity
// fingerprint, and a second peer that asks the rendezvous to broker a punch and
// then syncs over the direct connection that forms. On one loopback host there is
// no NAT to traverse, so the punch always opens (the observed address is directly
// dialable); what this proves is the machinery around it (rendezvous brokering,
// the simultaneous fire from a shared socket, accept-on-the-punched-socket, the
// Noise handshake, the sync). Crossing a real NAT is real-host work the loopback
// cannot stand in for.
#![cfg(feature = "net")]

use std::net::SocketAddr;
use std::time::Duration;

use constellation::{sync, LocalTransport, NetworkTransport, Rendezvous, Server};
use lifestream::{Lifestream, NodeKind, Object, ObjectId, TreeEntry};
use tempfile::{tempdir, TempDir};

const KEY: [u8; 32] = [61u8; 32];
const OTHER_KEY: [u8; 32] = [62u8; 32];

fn store(dir: &TempDir, key: &[u8; 32]) -> Lifestream {
    Lifestream::init(dir.path(), key).unwrap()
}

fn loopback() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

// Non-repeating bytes so a few records grow past the 64 KiB cut, which forces the
// channel to segment them under the Noise message limit on the punched link.
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

// The headline path: a peer that knows only the rendezvous address and the shared
// identity asks for a punch, and over the direct connection that opens it pulls
// the serving peer's objects. Large content makes the server send multi-segment
// record frames the punched channel reassembles.
#[test]
fn punch_pull_replicates_directly() {
    let rz = Rendezvous::start(loopback()).unwrap();
    let rz_addr = rz.local_addr();

    // A serving peer with real content. It waits to be punched at the rendezvous;
    // the dialer never learns its direct address, only punches to it.
    let ds = tempdir().unwrap();
    let source = store(&ds, &KEY);
    let payload = varied(200_000);
    let f = source.write_bytes(&payload).unwrap();
    let gen = source
        .commit(tree_with(&source, &[("data", f)]), vec![], "src")
        .unwrap();
    let server = Server::start(loopback(), KEY, source).unwrap();
    let _listener = server.punch_via_rendezvous(rz_addr).unwrap();

    // A second peer of the same identity punches in and pulls everything.
    let dd = tempdir().unwrap();
    let local = LocalTransport::new(store(&dd, &KEY));
    let remote = NetworkTransport::connect_via_punch(rz_addr, KEY).unwrap();
    let report = sync(&remote, &local).unwrap();

    assert!(report.transferred > 0);
    assert_eq!(local.lifestream().head().unwrap(), Some(gen));

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

    remote.close().ok();
    drop(server);
    drop(rz);
}

// The punched link is symmetric: a peer can push to the waiting server over it
// too, not only pull. Large content here makes the client send multi-segment
// frames.
#[test]
fn punch_push_replicates_directly() {
    let rz = Rendezvous::start(loopback()).unwrap();
    let rz_addr = rz.local_addr();

    // The far side starts empty and is reachable only by punching.
    let db = tempdir().unwrap();
    let server = Server::start(loopback(), KEY, store(&db, &KEY)).unwrap();
    let _listener = server.punch_via_rendezvous(rz_addr).unwrap();

    // The near side holds the content and pushes it across the punched link.
    let da = tempdir().unwrap();
    let a = LocalTransport::new(store(&da, &KEY));
    let big = a.lifestream().write_bytes(&varied(150_000)).unwrap();
    let note = a.lifestream().write_bytes(b"pushed over a punch").unwrap();
    let gen = a
        .lifestream()
        .commit(
            tree_with(a.lifestream(), &[("big", big), ("note", note)]),
            vec![],
            "g1",
        )
        .unwrap();

    let remote = NetworkTransport::connect_via_punch(rz_addr, KEY).unwrap();
    let report = sync(&a, &remote).unwrap();
    assert_eq!(report.transferred, a.lifestream().object_count().unwrap());
    assert_eq!(report.refs_set, vec!["HEAD".to_string()]);
    remote.close().ok();

    // On the server's actual disk, a fresh handle decrypts the pushed generation.
    let b = Lifestream::open(db.path(), &KEY).unwrap();
    assert_eq!(
        b.object_count().unwrap(),
        a.lifestream().object_count().unwrap()
    );
    assert_eq!(b.head().unwrap(), Some(gen));
    drop(server);
    drop(rz);
}

// With no peer waiting to be punched, a dialer is told there is none rather than
// left hanging, so the caller can fall back to a relay.
#[test]
fn a_dialer_with_no_waiter_is_refused() {
    let rz = Rendezvous::start(loopback()).unwrap();
    let rz_addr = rz.local_addr();

    let err = NetworkTransport::connect_via_punch(rz_addr, KEY);
    assert!(
        err.is_err(),
        "no peer is waiting, so no punch can be brokered"
    );
    drop(rz);
}

// A different identity cannot be brokered onto a waiting peer: the rendezvous
// matches by fingerprint, and a different identity has a different fingerprint,
// so the rendezvous has no waiter for it. (Even if a hostile rendezvous forced
// the pairing, the Noise handshake would refuse it, as net.rs covers on a direct
// link; the rendezvous only ever brokers addresses.)
#[test]
fn a_different_identity_finds_no_waiter() {
    let rz = Rendezvous::start(loopback()).unwrap();
    let rz_addr = rz.local_addr();

    let db = tempdir().unwrap();
    let server = Server::start(loopback(), KEY, store(&db, &KEY)).unwrap();
    let _listener = server.punch_via_rendezvous(rz_addr).unwrap();

    // A stranger of another identity finds no peer to punch to.
    assert!(NetworkTransport::connect_via_punch(rz_addr, OTHER_KEY).is_err());
    // The right identity still punches in.
    let ok = NetworkTransport::connect_via_punch(rz_addr, KEY).unwrap();
    ok.close().ok();

    drop(server);
    drop(rz);
}

// Dropping the listener withdraws the wait from the rendezvous: a later dialer of
// the same identity then finds no one to punch. The rendezvous learns the wait is
// gone when the listener's connection closes, so we give that close a moment to
// propagate.
#[test]
fn wait_withdraws_on_drop() {
    let rz = Rendezvous::start(loopback()).unwrap();
    let rz_addr = rz.local_addr();

    let db = tempdir().unwrap();
    let server = Server::start(loopback(), KEY, store(&db, &KEY)).unwrap();
    let listener = server.punch_via_rendezvous(rz_addr).unwrap();

    // While waiting, a dialer punches in.
    let up = NetworkTransport::connect_via_punch(rz_addr, KEY).unwrap();
    up.close().ok();

    // After dropping the listener, the rendezvous forgets the waiter.
    drop(listener);
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        NetworkTransport::connect_via_punch(rz_addr, KEY).is_err(),
        "a withdrawn wait leaves no peer to punch to"
    );

    drop(server);
    drop(rz);
}

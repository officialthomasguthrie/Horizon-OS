// mDNS LAN discovery. The roundtrip uses real multicast, which is not reliable
// in a CI sandbox, so it is #[ignore]d: run it on a real machine with
//   cargo test -p constellation --test discovery -- --ignored
#![cfg(feature = "discovery")]

use std::time::Duration;

use constellation::{discover, fingerprint, Beacon};

const KEY: [u8; 32] = [13u8; 32];

// Cheap and network-free, so it runs in CI: the public fingerprint is stable
// and identity-specific.
#[test]
fn fingerprint_is_deterministic() {
    assert_eq!(fingerprint(&KEY), fingerprint(&KEY));
    assert_ne!(fingerprint(&KEY), fingerprint(&[14u8; 32]));
}

// Announce a beacon, then browse and confirm we resolve it: an end-to-end check
// that the advertisement and the fingerprint match line up over the wire. A
// different identity browsing at the same time must not match it.
#[test]
#[ignore = "uses real LAN multicast; run locally with --ignored"]
fn announce_then_discover_finds_the_beacon() {
    let port = 47777;
    let _beacon = Beacon::announce(&KEY, port).unwrap();

    let mine = discover(&KEY, Duration::from_secs(6)).unwrap();
    assert!(
        mine.iter().any(|a| a.port() == port),
        "expected to discover our own beacon on port {port}, got {mine:?}"
    );

    // A peer of a different identity sees no match: the fingerprint gates it.
    let other = discover(&[99u8; 32], Duration::from_secs(2)).unwrap();
    assert!(
        other.iter().all(|a| a.port() != port),
        "a different identity must not match our beacon, got {other:?}"
    );
}

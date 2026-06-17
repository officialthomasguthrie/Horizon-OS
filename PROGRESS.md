# Horizon build progress

The design lives in `docs/`. This file tracks what actually exists in code.

Repo: https://github.com/officialthomasguthrie/horizon-os

## Phases

- Phase 0  Proof of life: bootable encrypted base on a Key, boots on x86-64 UEFI, persistent state
- Phase 1  Lifestream: content-addressed encrypted state store, generations, time travel
- Phase 2  Weave + Glass: capability broker, Cells, audit log, transparency
- Phase 3  Shell + compositor (Wayland, Smithay/iced)
- Phase 4  Aura: local model runtime, voice, semantic search, capability tools
- Phase 5  Constellation + Reconstitution: P2P sync, Shamir recovery
- Phase 6  Website + installer (Tauri)

## Done

- Design docs (docs/00-09, README, SUMMARY)
- Workspace scaffold (Cargo workspace, toolchain, license, editorconfig)
- Phase 1 lifestream crate: FastCDC chunker, encrypted content-addressed store
  (XChaCha20-Poly1305 with keyed BLAKE3 addressing), generations, history,
  time-travel restore, mark-and-sweep gc. 8 integration tests passing.
- horizon CLI: lifestream init / snapshot / log / restore / gc / refs / stat,
  with Argon2id passphrase key derivation.
- CI: fmt, clippy (-D warnings), test on push and PR.
- Phase 2 weave crate: object-capability broker over the Lifestream. Unforgeable
  capability handles scoped to a resource (file/net/device/service) and rights
  (r/w/x); grants are time- and use-limited and revocable; a request policy
  (allow/deny/rules) decides unsolicited asks. The audit log is an append-only,
  hash-chained sequence of entries stored as Lifestream Trees, so it is
  tamper-evident, gc-safe (reachable from one ref), and replayed on open to
  rebuild broker state. 11 tests passing.
- horizon weave CLI: grant / revoke / use / grants / audit / verify, plus a
  scripted `weave demo` that walks the full grant-use-deny-revoke lifecycle and
  prints the resulting audit log.
- Phase 5 constellation crate: object sync between two Lifestream stores of one
  identity. A Transport trait abstracts a peer; sync diffs the two id sets and
  ships only the sealed records the other side lacks (content addressing makes
  shared history free), then carries refs forward fast-forward-only, reporting
  divergence rather than clobbering it. Records cross as ciphertext and the
  receiver verifies each against its own key before committing, so a wrong
  identity is refused, not corrupted. LocalTransport is the in-process transport;
  a QUIC+Noise network skin implements the same trait later. 8 tests passing.
- horizon sync CLI: `horizon sync <from> <to> [--both]`. Creates the destination
  as a replica of the source identity when absent, refuses a foreign one, and
  reports objects moved and refs set / advanced / diverged.
- Phase 5 reconstitution crate: Shamir k-of-n recovery of the identity master
  key over GF(2^8). split turns the key into n shares (any k rebuild it, any k-1
  reveal nothing); combine interpolates back and verifies the result against a
  domain-separated tag carried on every share, so a corrupted or wrong-set share
  is caught instead of silently returning a wrong key. Shares are versioned,
  self-describing, and hex-portable. 11 tests passing, including every k-subset.
- horizon reconstitute CLI: `split <store> --k --n` cuts recovery shares from a
  store's master key; `open <store> --share ...` rebuilds the key from k shares
  and opens the store with no passphrase, decrypting HEAD to prove the key.

## Next

- Finish Phase 2 on a Linux host: Cells, process confinement via namespaces +
  seccomp (bubblewrap-class), so the broker hands real fds/sockets to confined
  principals. Linux-only, so build and test there, not on darwin.
- Glass: the live transparency surface over the weave audit log. It lands with
  the shell in Phase 3 (it is an L5 compositor surface); `horizon weave
  audit/grants` is the headless stand-in until then.
- Phase 3: shell + Wayland compositor (Smithay/iced). Linux-only.
- Phase 5 Constellation network transport: a QUIC + Noise skin implementing the
  same Transport trait the in-process sync already runs on, with peer discovery
  and NAT traversal. Network/Linux-host work; the sync core and CLI are done and
  cross-platform.
- Phase 5 Reconstitution boot/identity wiring: bind recovery shares to FIDO2
  re-enrollment and the boot-time unlock path, and a phone as a post-boot trusted
  device. Linux-only; the secret-sharing core and CLI are done and cross-platform.

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

## Next

- Phase 2: the Weave. crates/weave: capability type, broker, scoped and
  revocable grants, append-only audit log (persisted through lifestream).
  Then Cells (process confinement on Linux) and Glass over the audit log.
- Add a `horizon weave` CLI surface and an audit demo.

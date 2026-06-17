# Horizon build progress

The design lives in `docs/`. This file tracks what actually exists in code.

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
- Workspace scaffold (Cargo workspace, toolchain, license, CI config)

## In progress

- crates/lifestream: content-addressed object store

## Next

- Finish lifestream: chunker, object store, generations, gc, with tests
- horizon CLI exposing lifestream commands
- CI: build, test, clippy, fmt

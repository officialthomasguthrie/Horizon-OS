# 07 · Roadmap

How Horizon gets built, in what order, and what "done" means at each step. This is sequenced by **dependency and risk**, not calendar dates, timelines depend entirely on team size. The honest framing: this is a **multi-year effort**, and it's structured so that something real, demoable, and *useful* exists early and keeps growing.

**Sequencing principle:** prove the riskiest, most-novel things first (boot-anywhere + Lifestream + Weave), because everything else assumes them.

---

## Phase 0, Proof of life: "it boots anywhere and remembers"

*The smallest thing that demonstrates the core promise.*

- Reproducible, immutable Linux base image (Nix/OSTree-style) on a Key (NVMe-in-UASP-enclosure).
- Universal boot: isohybrid (UEFI + legacy), dual-signed shim for Secure Boot.
- Generic initramfs + full firmware + auto-probe -> boots on a spread of real x86-64 laptops.
- LUKS2 (AES-256-XTS, capped Argon2id) encrypted persistence; FIDO2 + PIN unlock.
- First-boot per-Surface hardware profiling + cache.

**Done when:** you can plug the same Key into ~5 different laptops, unlock with a hardware key, reach a (minimal) desktop, save a file on one machine, and see it on the next. *This alone is a compelling demo.*

**Top risks:** day-one GPU/Wi-Fi gaps; Secure Boot signing; the 2026 cert transition.

---

## Phase 1, The Lifestream

*Turn persistence into the rewindable Merkle DAG.*

- Content-addressed, encrypted object store (FastCDC chunking, HMAC addressing, XChaCha20-Poly1305).
- btrfs/bcachefs CoW live tier + capture into the durable object tier.
- **Generations:** signed roots; atomic updates; one-reboot rollback.
- **Time-travel UI:** whole-system slider + per-file history.

**Done when:** you can break the system, slide back 15 minutes, and be whole; updates apply atomically and roll back cleanly.

**Top risks:** performance of the live<->durable bridge; chunking/dedup efficiency on a pocket drive.

---

## Phase 2, The Weave + Glass

*No ambient authority, and you can see everything.*

- Capability broker + tightly-typed IPC + tamper-evident audit log (stored in the Lifestream).
- Cells: bubblewrap-class confinement (namespaces/seccomp); disposable Cells; KVM-microVM Cells where supported.
- Portals for user-mediated grants (file pickers, devices).
- **Glass:** live per-process network/data view with kill switches + 7-day timeline.

**Done when:** every app runs with zero ambient authority, all grants/uses are logged and revocable, and Glass shows (and can sever) live egress.

**Top risks:** making confinement airtight on a monolithic kernel; keeping grant prompts legible, not naggy.

---

## Phase 3, The experience layer

*A desktop people actually want to use.*

- Smithay-based Wayland compositor; iced + wgpu shell; the Aura intent line as the launcher/command palette.
- Surface Trust Dial UI (Home/Known/Foreign) wired to real posture changes.
- App ecosystem on day one: Flatpak-in-Cells, XWayland-in-Cells, PWAs.

**Done when:** a non-developer can do real daily work (browse, write, files, media) comfortably.

**Top risks:** desktop polish is a deep, long tail; app compatibility expectations.

---

## Phase 4, Aura

*The on-device intent layer, on a leash.*

- llama.cpp embedded; tiered model selection by Surface; Q4_K_M models.
- whisper.cpp + Piper/Kokoro voice; embeddings + sqlite-vec semantic search.
- **OS-as-capabilities (MCP-style) tool interface**; preview/confirm/audit/undo rails.
- Begin **Aura-Core** fine-tune (QLoRA on a permissive base for safe tool-calling).

**Done when:** "find my cattle deck and chart the Q3 numbers" works end-to-end, locally, with preview + undo, on a Standard-tier Surface.

**Top risks:** quality/latency on weak Surfaces; prompt-injection containment (mitigated by capabilities, never "solved").

---

## Phase 5, Constellation & Reconstitution

*Cloudless mesh, and surviving device loss.*

- QUIC+Noise P2P sync of Lifestream objects between your devices (ciphertext-only at peers).
- **Reconstitution:** Shamir/SLIP-39 *k-of-n* recovery flow; second FIDO2 enrollment; phone as post-boot trusted device.
- *Stretch:* compute offload, borrow your home machine's horsepower for Aura, or a full remote session (waypipe-style), trust-bounded to your own devices.

**Done when:** you can lose your Key and reconstitute your identity from shares + a peer device, with no cloud involved.

**Top risks:** NAT traversal/reliability; making recovery UX feel safe, not scary.

---

## Phase 6, Website & installer (parallelizable from Phase 0)

*Make getting Horizon effortless and verifiable.*

- Tauri flasher (Raspberry-Pi-Imager model): download latest signed image -> verify -> write -> re-verify -> **filter out system disks**.
- One-page site: detect OS, serve the right tiny flasher, show signed checksums.
- Distribution: zstd images, zsync/casync deltas, torrent + CDN, cosign/GPG signatures.
- *Stretch:* "Try Horizon in your browser" WASM/VM demo.

**Done when:** a non-expert can go from website -> flashed Key -> booted Horizon in minutes, with integrity verified. Details: [`08-WEBSITE-AND-INSTALLER.md`](08-WEBSITE-AND-INSTALLER.md).

---

## Phase 7+, The moonshots

*Where Horizon becomes genuinely unprecedented.*

- **seL4 trusted core:** migrate the Weave/Lifestream-gate/identity onto the formally-verified seL4 microkernel, with **Linux as a contained driver VM**, provable isolation under the same Rust userland (the design was kept microkernel-shaped for exactly this; [`02`](02-ARCHITECTURE.md) §8).
- **CHERI/CHERIoT** hardware capabilities as silicon matures.
- **ARM build** (Snapdragon X et al.) and a **per-Mac Apple-Silicon install** flow.
- **First-party Horizon Key hardware** (secure element, hardware kill switch, tamper evidence).
- **Verifiable privacy receipts** (exportable cryptographic proof of a session's network behavior).

---

## What it takes (skills)

| Area | Needed for |
|---|---|
| Linux integration / distro engineering | Phases 0, 3, 6 |
| Rust systems programming | Phases 1-5 (the bulk) |
| Applied cryptography | Phases 0, 1, 5 (encryption, Merkle store, Shamir) |
| Wayland / graphics | Phase 3 |
| ML / LLM fine-tuning & inference | Phase 4 |
| Distributed systems / networking | Phase 5 |
| Web + design | Phase 6 (and brand throughout) |
| Formal methods / seL4 | Phase 7 moonshot |

A tiny team can reach Phase 0-1 (the most compelling early demo). The full vision is a funded, multi-year, community-backed open-source project.

---

## The first 90 days (concrete starter)

1. **Scaffold the workspace** in [`06-TECH-STACK.md`](06-TECH-STACK.md) §5 (Cargo workspace + image-build tree + site).
2. **Build the immutable base image** and get it booting on **one** laptop (UEFI), unsigned, unencrypted, pure boot.
3. **Add LUKS2 + FIDO2 unlock** and **persistence**; boot on a **second, different** laptop with state intact.
4. **Prototype the Lifestream object store** (chunk/tree/generation; one rollback).
5. **Stand up the Tauri flasher** writing the image to a Key with verification.
6. **Demo:** plug into two laptops, unlock with a hardware key, see persisted state, roll back one change.

That demo *is* the pitch, boot-anywhere + encrypted persistence + time-travel, and it's reachable fast.

-> Next: the install experience in detail, [`08-WEBSITE-AND-INSTALLER.md`](08-WEBSITE-AND-INSTALLER.md).

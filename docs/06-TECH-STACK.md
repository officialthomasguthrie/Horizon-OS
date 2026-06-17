# 06 · Technology Stack

This answers the "what languages and frameworks?" question directly, with the reasoning and sources behind each choice. The meta-principle (tenet #5): **reuse proven substrate, write original code where it's felt.**

---

## 1. The kernel decision (recap, because it dominates everything)

**Use the Linux kernel.** A from-scratch kernel cannot reach broad laptop hardware on any realistic timeline:

- Linux is **~40M lines, ~60% drivers** ([stackscale](https://www.stackscale.com/blog/linux-kernel-surpasses-40-million-lines-code/)); recreating it is thousands of person-years ([cost study](https://dwheeler.com/essays/linux-kernel-cost.html)).
- Even the best from-scratch Rust OS, **Redox**, is still pre-1.0 with no Wi-Fi/Bluetooth and Intel-GPU-only after ~a decade ([Redox](https://www.phoronix.com/news/Redox-OS-April-2026)).
- Every "new" OS with real reach (Android, ChromeOS, SteamOS, postmarketOS) is Linux underneath.

Horizon's originality goes **above** the kernel. The seL4 moonshot ([`02`](02-ARCHITECTURE.md) §8) keeps Linux as a driver VM, so we never re-fight drivers.

---

## 2. Languages

### Rust, the primary language of Horizon (L2-L6)

Everything we write, the Weave broker, the Lifestream engine, system services, the compositor and shell, the Aura runtime glue, the Constellation mesh, the installer, is **Rust**.

**Why:**
- **Memory safety without a GC** via ownership/borrow-checking, eliminates the ~70% of CVEs that are memory-safety bugs ([Chromium memory safety](https://www.chromium.org/Home/chromium-security/memory-safety/)), with `no_std` for bare-metal paths.
- It's **proven for systems work in 2026**: Rust-for-Linux drivers are in-tree (the Nova GPU driver, Rust Binder), and at the Dec 2025 Maintainers Summit Rust was declared **no longer experimental** ([LWN](https://lwn.net/Articles/1049831/)); Ubuntu 25.10 ships Rust `uutils` coreutils by default ([uutils](https://github.com/uutils/coreutils)).
- A **whole Rust desktop already shipped**: System76's **COSMIC** hit 1.0 in Dec 2025 (Smithay + iced), Horizon follows a *validated* path, not a speculative one ([COSMIC](https://en.wikipedia.org/wiki/COSMIC_desktop)).

**Honest limits:** `unsafe` is unavoidable at the hardware boundary; the borrow checker fights some data structures; a from-scratch *general-purpose* Rust OS is still unrealistic for a small team (Redox is the cautionary tale), which is exactly why we write a Rust **userland/system layer**, not a Rust kernel.

### C, at the kernel boundary

The Linux kernel, most drivers, and many libraries are **C**. We touch C where we must interface with the kernel, write or patch a driver, or bind to C libraries. We minimize new C; we don't pretend we can avoid reading it.

### C++, inherited via key dependencies

`llama.cpp` and `whisper.cpp` are **C++** (the basis of Google's Zircon kernel too, a restricted C++ subset). We consume them as libraries with Rust bindings rather than writing new C++.

### Zig, a candidate for low-level glue & cross-compilation

**Zig** has best-in-class cross-compilation (a bundled C/C++ cross-compiler), `comptime`, and no hidden allocations, attractive for build tooling and portable low-level glue. **But it's pre-1.0 and churny** (0.16-era in 2026), so we keep it *optional and non-load-bearing* until it stabilizes. Not a core dependency.

### Go & Python, tooling, services, automation

**Go** for network/daemon tooling and parts of the Constellation/CI infra where its concurrency and deployment story shine (wrong for kernels, GC/runtime, right for infra). **Python** for ML fine-tuning pipelines (Aura-Core training), scripting, and data tooling.

**Summary:** **Rust first**, C at the kernel edge, C++ via deps, Zig as optional glue, Go/Python for tooling and ML.

---

## 3. Frameworks & components

| Layer | Choice | Why | Source |
|---|---|---|---|
| **Kernel** | Linux LTS, generic initramfs, full `linux-firmware` | Only realistic path to boot-anywhere hardware support | [kernel licensing](https://docs.kernel.org/process/license-rules.html) |
| **Boot** | `shim` (dual-signed 2011+2023) -> systemd-boot/GRUB | Secure Boot + legacy BIOS coverage; mind 2026 cert expiry | [MS cert expiry](https://www.redhat.com/en/blog/expiration-secure-boot-signing-certificates-2026) |
| **Image model** | Immutable, content-addressed, reproducible (Nix- or OSTree-style) + dm-verity | Atomic updates, rollback, "same everything everywhere" | [NixOS](https://nixos.org/), [rpm-ostree](https://coreos.github.io/rpm-ostree/) |
| **Live filesystem** | **btrfs** or **bcachefs** (CoW) on LUKS2 | Fast snapshots = instant recent time-travel (Lifestream Tier A) | |
| **Encryption** | **LUKS2 / dm-crypt**, AES-256-XTS (`--key-size 512`), Argon2id (capped) | Kernel-native, portable, multi-keyslot | [cryptsetup](https://gitlab.com/cryptsetup/cryptsetup) |
| **Auth tokens** | **FIDO2** via `systemd-cryptenroll` (+ PIN) | Hardware-key unlock; secret never leaves token | [systemd-cryptenroll](https://0pointer.net/blog/unlocking-luks2-volumes-with-tpm2-fido2-pkcs11-security-hardware-on-systemd-248.html) |
| **Recovery** | **Shamir / SLIP-39** secret sharing | Cloudless *k-of-n* identity recovery | [SLIP-39](https://trezor.io/learn/advanced/standards-proposals/what-is-shamir-backup) |
| **Capability layer (Weave)** | Custom **Rust** broker + **bubblewrap**-class confinement (namespaces/seccomp) | Object-capability model on Linux; microkernel-portable later | [bubblewrap](https://github.com/containers/bubblewrap) |
| **Compositor** | **Smithay** (Rust) | All-Rust Wayland compositor; COSMIC-proven | [Smithay](https://github.com/Smithay/smithay) |
| **UI toolkit** | **iced** (shell) + **wgpu** (GPU); egui/Slint for smaller surfaces | Only Rust toolkit proven at full-shell scale (COSMIC) | [iced](https://github.com/iced-rs/iced) |
| **App sandboxing** | Flatpak-in-Cells, XWayland-in-Cells, KVM microVMs | Day-one app ecosystem + isolation | |
| **AI inference** | **llama.cpp** (GGUF), Metal/CUDA/Vulkan/CPU | Bundleable, runs everywhere, no daemon | [llama.cpp](https://github.com/ggml-org/llama.cpp) |
| **Voice** | **whisper.cpp** (STT) + **Piper/Kokoro** (TTS) | Fast, offline, CPU-capable | [whisper.cpp](https://github.com/ggml-org/whisper.cpp) |
| **Semantic search** | embeddings + **sqlite-vec** | Laptop-scale vector search, simple | [sqlite-vec](https://alexgarcia.xyz/blog/2024/sqlite-vec-stable-release/index.html) |
| **AI tool protocol** | **MCP** (Model Context Protocol) | Cross-vendor standard for typed tools = capabilities | [MCP](https://www.anthropic.com/news/donating-the-model-context-protocol-and-establishing-of-the-agentic-ai-foundation) |
| **Mesh (Constellation)** | **QUIC + Noise** P2P (Syncthing-class) | Authenticated, encrypted, NAT-traversing, cloudless sync | [Syncthing](https://docs.syncthing.net/) |
| **Installer (desktop)** | **Tauri** (Rust + native webview) | ~5-10 MB, secure-by-default, matches our Rust stack | [Tauri](https://tauri.app/) |
| **Image distribution** | **zstd** compression, **zsync/casync** deltas, torrent + CDN, **cosign/GPG** signatures | Fast decompress, cheap updates, verifiable downloads | [zstd](https://github.com/facebook/zstd), [Sigstore](https://docs.sigstore.dev/) |

---

## 4. Why all-Rust above the kernel is the right bet (and the risk)

**The bet:** a single, memory-safe, modern language across the Weave, Lifestream, compositor, shell, services, mesh, and installer means one toolchain, shared libraries, no FFI seams between our own components, and security properties by construction. COSMIC proves the desktop half is real; Rust-for-Linux proves the systems half is real.

**The risk:** Rust's hiring pool is smaller than C/C++; some low-level patterns are awkward; toolchain/ABI churn exists. We accept this, memory safety in a security-first OS is worth it, and the ecosystem crossed the credibility threshold in 2025-2026.

---

## 5. Proposed repository / workspace layout

A Cargo workspace (plus an image-build tree and the website), so the "impressive codebase" is also a *navigable* one:

```
horizon/
├── kernel/            # kernel config, patches, firmware manifest, initramfs
├── boot/              # shim/bootloader config, Secure Boot signing pipeline
├── image/             # reproducible image build (Nix/OSTree), dm-verity, generations
├── crates/
│   ├── weave/         # capability broker + IPC + audit log        (Rust)
│   ├── lifestream/    # content-addressed store + CoW snapshot bridge (Rust)
│   ├── identity/      # LUKS2/FIDO2/Shamir identity & unlock         (Rust)
│   ├── constellation/ # QUIC+Noise P2P mesh, sync, offload           (Rust)
│   ├── cells/         # sandbox/Cell lifecycle (namespaces/seccomp/KVM) (Rust)
│   ├── compositor/    # Smithay-based Wayland compositor             (Rust)
│   ├── shell/         # desktop shell + Aura intent line (iced/wgpu) (Rust)
│   ├── glass/         # transparency UI over the audit log          (Rust)
│   └── aura/          # llama.cpp/whisper.cpp bindings + tool runtime (Rust)
├── models/            # Aura-Core fine-tune pipeline + model manifests (Python)
├── installer/         # Tauri cross-platform flasher                 (Rust)
├── website/           # landing + download + verify                 (web)
└── docs/              # this design set
```

-> Next: how we actually build it, in what order, [`07-ROADMAP.md`](07-ROADMAP.md).

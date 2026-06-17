# 02 · Architecture

This document is the engineering core: the layered stack, and deep dives on the two pillars that make Horizon novel, **the Lifestream** (state engine) and **the Weave** (capability fabric), plus how Aura, the shell, the Constellation, and the boot flow fit together.

Guiding principle (tenet #5): **reuse the proven substrate, innovate in the layers the user feels.** Linux is the commodity beneath us; everything above is Horizon's.

---

## 1. The layered stack

```
╔════════════════════════════════════════════════════════════════════════╗
║ L6  AURA, Intent Layer                                                 ║
║     llama.cpp · tiered GGUF models · whisper.cpp · Piper · sqlite-vec    ║
║     speaks ONLY through Weave capabilities (MCP-style typed tools)       ║
╠════════════════════════════════════════════════════════════════════════╣
║ L5  EXPERIENCE, Shell · Apps · Glass · Cells UI                        ║
║     Rust · Wayland · Smithay compositor · iced + wgpu  (COSMIC blueprint)║
║     each app is a confined Wayland client in a Cell                     ║
╠════════════════════════════════════════════════════════════════════════╣
║ L4  THE WEAVE, object-capability broker + audited IPC  (Rust)          ║
║     no ambient authority · scoped/revocable handles · one log for all    ║
║     the single gate for users, apps, AND Aura                          ║
╠════════════════════════════════════════════════════════════════════════╣
║ L3  LIFESTREAM, content-addressed, encrypted, versioned state engine    ║
║     Tier A: btrfs/bcachefs CoW (fast, live, instant recent snapshots)   ║
║     Tier B: content-addressed object store (durable history, dedup,     ║
║             sync, backup), the Merkle DAG you carry & rewind           ║
╠════════════════════════════════════════════════════════════════════════╣
║ L2  IDENTITY & MESH                                                     ║
║     LUKS2 (AES-256-XTS) · Argon2id · FIDO2+PIN · Shamir Reconstitution  ║
║     Constellation: QUIC + Noise P2P mesh (cloudless sync & offload)     ║
╠════════════════════════════════════════════════════════════════════════╣
║ L1  BOOT & TRUST                                                        ║
║     shim (dual-signed) -> systemd-boot/GRUB -> kernel + generic initramfs  ║
║     dm-verity verified immutable base · Surface Trust Dial · profile cache║
╠════════════════════════════════════════════════════════════════════════╣
║ L0  LINUX KERNEL (substrate)                                            ║
║     drivers · scheduling · namespaces · seccomp · KVM · full firmware    ║
╚════════════════════════════════════════════════════════════════════════╝
        runs on -> SURFACE (x86-64 UEFI host, untrusted)
        stored on -> HORIZON KEY (NVMe in UASP USB-C enclosure)
```

The crucial architectural discipline: **L4-L6 never talk to L0 directly.** All authority is mediated by the Weave (L4) over the Lifestream (L3). That discipline is what makes the system auditable, sandboxable, AI-safe, *and* portable to a microkernel later (see §8).

---

## 2. L0-L1: Substrate & boot

We ship a recent **Linux LTS kernel**, a **generic (host-agnostic) initramfs**, and the **complete `linux-firmware` set**, because that combination is precisely what lets one image adapt to unknown hardware via Linux's auto-probe (`modalias` -> `modprobe`) path. (Full reasoning and the driver-problem analysis: [`03-PORTABILITY-AND-BOOT.md`](03-PORTABILITY-AND-BOOT.md).)

**Boot chain (x86-64 UEFI, with legacy fallback):**

```
UEFI firmware
  └─ shim            (Microsoft-signed; dual 2011+2023 certs, see §note)
       └─ bootloader (systemd-boot for UEFI; GRUB when legacy BIOS needed)
            └─ kernel + generic initramfs
                 ├─ probe hardware, load modules + firmware
                 ├─ UNLOCK: passphrase + FIDO2 token + PIN  -> open LUKS2
                 ├─ verify immutable base with dm-verity (tamper = refuse)
                 ├─ Surface Trust Dial:
                 │     Home/Known -> mount persistent encrypted overlay
                 │     Foreign    -> tmpfs overlay (amnesiac; nothing written)
                 ├─ apply cached per-Surface profile (if Home/Known & seen)
                 └─ hand off to the Weave broker (the trusted-core init)
```

> **Secure Boot note:** Microsoft's 2011 signing CAs expire **June 2026**; Horizon's shim must be **dual-signed** (2011 + 2023) and track SBAT revocations. This is a hard, dated requirement baked into the build pipeline.

The base image is **immutable and reproducible**, verified by **dm-verity** so a tampered base simply won't boot. It is, itself, just a generation in the Lifestream (§3), which is why updates and rollback fall out for free.

---

## 3. L3: The Lifestream, your computer as a rewindable Merkle DAG

The Lifestream is Horizon's signature idea: **persistence, time-travel, updates, sync, backup, and recovery are one mechanism**, not six.

### 3.1 The model

Everything is an immutable, content-addressed, encrypted **object**:

- **Chunk**, a piece of file data, split by content-defined chunking (FastCDC-style rolling hash) so that editing a file re-stores only the changed chunks, and identical data anywhere is stored once.
- **Tree**, a directory: names -> hashes of chunks/subtrees. (The whole filesystem is a tree of trees.)
- **Generation**, a signed root: a pointer to the complete system tree (immutable base subtree + your state subtree + config), plus parent generation(s), timestamp, and metadata. **A generation hash names your entire computer at an instant.**

```
Generation g42  (signed root)
   ├─ parent -> g41
   ├─ base/   -> [dm-verity immutable OS image subtree]   (shared, dedup'd)
   ├─ home/   -> tree
   │             ├─ docs/  -> tree -> {chunk h1, chunk h2, ...}
   │             └─ ...
   └─ config/ -> tree (capability grants, Surface profiles, settings)
```

Because objects are addressed by the hash of their *content*, the structure is a **Merkle DAG**: any change anywhere changes its parent hashes up to the root, so **tampering is mathematically detectable** and **shared data is automatically deduplicated**.

### 3.2 Encryption & privacy of the store

- Chunks are addressed by a **keyed hash (HMAC)** of their plaintext, then sealed with **authenticated encryption (XChaCha20-Poly1305)** before they ever hit disk. Keyed addressing gives dedup *without* the confirmation-attack leak that naive convergent encryption suffers.
- Generation roots are **signed** by your identity key so a peer (or a restored backup) can verify authenticity, not just integrity.
- The object keys derive from your identity, which is itself protected by LUKS2 + FIDO2 (L2). A peer storing your objects (backup target, Constellation member) sees only opaque ciphertext.

### 3.3 Two tiers (so it's actually fast)

A pure object store would be too slow for a live OS. The Lifestream is implemented in two cooperating tiers:

- **Tier A, Live (fast):** a CoW filesystem (**btrfs** or **bcachefs**) on the LUKS2 volume. The running system reads/writes here at full NVMe speed. CoW snapshots give *instant* recent time-travel ("back to 20 minutes ago") with near-zero cost.
- **Tier B, Durable (content-addressed):** periodic and event-driven capture of Tier-A snapshots into the content-addressed object store. This is what gives long history, cross-device dedup, sync, backup, and the portable "carry your whole DAG" property.

This mirrors how robust systems already work (CoW filesystem for speed + content-addressed snapshots for durability), Horizon's contribution is *unifying* them behind one user-facing concept and wiring them to the Weave's audit log and the Constellation's sync.

### 3.4 What you get from the one mechanism

| Capability | How it falls out of the DAG |
|---|---|
| **Persistence** | The current generation root *is* your saved state. |
| **Time-travel (whole system)** | Boot/mount an older generation. |
| **Time-travel (per file)** | Walk the DAG for that path's history. |
| **Atomic updates** | A new base subtree -> a new generation; switch the root atomically. |
| **Rollback** | The previous generation is always intact and bootable. |
| **Reproducibility** | A generation is a recipe; rebuild it bit-identically elsewhere. |
| **Sync** | Replicate missing objects (by hash) to a peer; dedup makes it cheap. |
| **Backup** | Same as sync, targeting a drive you own. |
| **Integrity** | Verify hashes down the Merkle tree; signatures on roots. |
| **Recovery** | Protect one secret (the identity/root key) -> Reconstitution (L2). |

---

## 4. L4: The Weave, one capability fabric for all actors

The Weave enforces tenet #3: **no ambient authority.** A process can do *nothing* by virtue of "running as you." It can only exercise **capabilities** it has been explicitly handed.

### 4.1 What a capability is

> A **capability** = an unforgeable handle that *both* names a resource *and* carries the rights to it. (The object-capability model used by seL4 and Fuchsia.)

Examples: "read-only access to `~/Documents/cattle/`", "outbound network to `api.example.com:443`", "use the microphone for 30s", "send an email via the Mail service." Each is scoped, revocable, time-or-use-limited, and **audited on every use.**

### 4.2 How it's enforced on the Linux substrate (today)

Linux has ambient authority by default, so the Weave *manufactures* an ocap model on top:

- **Confinement:** every principal (app, service, Aura) runs in a **Cell**, Linux namespaces + seccomp filters + an empty default world: **no network namespace, no filesystem mounts, no devices** unless granted. (bubblewrap-class isolation, chosen over Firejail for its unprivileged, no-SUID design.)
- **The Broker:** a small trusted service, the Weave broker, is the *only* path to resources. A confined process requests a capability over a tightly-typed IPC; the broker checks policy, possibly prompts the user, and hands back a handle (e.g., an open file descriptor passed over a Unix socket, a brokered network socket, a portal to a device).
- **Portals:** user-mediated grants (à la xdg-desktop-portal) for things like "pick a file", the app receives access *only* to what the user picked, nothing more.
- **One audit log:** every grant and every use is appended to a tamper-evident log stored in the Lifestream. **Glass** (L5) renders it live; the user can revoke anything.

```
   App / Service / AURA   (confined Cell: no ambient FS, net, or devices)
          │  "I need: read ~/Documents/cattle/, net->api.x:443"

     ┌───────────────┐   policy + (maybe) user prompt
     │  WEAVE BROKER │ ───────────────────────────────── USER (grant once / always / no)
     └───────────────┘
          │ returns scoped handle (fd / brokered socket / portal)
                                             every use ->  AUDIT LOG -> Glass
     resource access happens ONLY through the handle
```

This is a faithful *approximation* of object-capabilities on a monolithic kernel. It is not as airtight as a verified microkernel, a kernel-level exploit bypasses userland confinement, which is exactly why the design is kept microkernel-shaped for the seL4 future (§8), where the same model becomes hardware/kernel-enforced and the broker's guarantees become provable.

### 4.3 Why this is the keystone

The Weave makes three normally-separate problems into **one**:

1. **App sandboxing**, apps get least authority by construction.
2. **User permission**, grants are explicit, legible, revocable, and logged.
3. **Safe AI agency**, Aura is just another principal. It can only do what you granted; everything it does is scoped, audited, and (via the Lifestream) reversible. "Excessive agency," the central risk of AI agents, is structurally bounded.

---

## 5. L6: Aura's place in the architecture

Aura is a **principal in the Weave**, not a privileged subsystem. Its anatomy:

- **Inference:** `llama.cpp` embedded as a library (GGUF models), auto-selecting Metal/CUDA/Vulkan/CPU. Tiered model selection by Surface/Constellation capacity (see [`05-AI-LAYER.md`](05-AI-LAYER.md)).
- **Senses:** `whisper.cpp` (speech-in), Piper/Kokoro (speech-out), local embeddings + `sqlite-vec` (semantic search), each itself a capability-gated service.
- **Hands:** the OS is exposed to Aura as **typed, permissioned tools = Weave capabilities** (MCP-style). Aura never clicks pixels; it calls `open_file`, `launch_app`, `send_email`, `set_display_temp`, each individually scoped and confirmable. Accessibility-API control is a *fallback* only for apps with no capability interface.
- **Safety rails:** preview-before-act for anything with effects; explicit confirmation for destructive ops; full audit trail; Lifestream undo. Aura cannot acquire a capability silently, acquiring a new one prompts you.

```
 "find the cattle deck and chart the Q3 numbers"
        │  (typed or spoken -> whisper.cpp)

   AURA (llama.cpp)  ── plans ── [ open_file(cattle.odp), read_sheet(...), insert_chart(...) ]
        │                                   each = a Weave capability call
          preview to user -> approve
   THE WEAVE  -> checks Aura holds these caps -> executes -> AUDIT LOG -> Glass
        │  result
          every step undoable via LIFESTREAM
```

---

## 6. L5: Experience layer (shell, apps, Glass, Cells)

- **Compositor:** a Wayland compositor built on **Smithay** (Rust), the path System76's COSMIC validated by shipping a from-scratch Rust desktop in production. **Wayland-only** (X11's input model breaks sandboxing); XWayland only inside Cells for legacy apps.
- **Toolkit & rendering:** **iced** for the shell, **wgpu** for GPU rendering. egui/Slint for smaller surfaces.
- **The shell** is keyboard-first: the **Aura intent line** *is* the launcher/command palette, one place to type a command, a search, or a sentence.
- **Glass** is a built-in surface, not an app: it visualizes the Weave audit log live (per-process network + data access), with kill switches and a timeline.
- **Cells** are the unit of running software: every app is a confined Wayland client. *Disposable* Cells (for untrusted links/files) are destroyed on close; heavier KVM-microVM Cells are used where hardware allows and the trust tier warrants.
- **App ecosystem on day one:** Linux apps via Flatpak-in-Cells and XWayland, plus PWAs, so users aren't staring at an empty store while native Horizon apps mature.

---

## 7. L2: Identity & the Constellation

- **Identity** is a keypair sealed inside the LUKS2 volume, unlocked by passphrase + FIDO2 + PIN, and recoverable via **Reconstitution** (Shamir/SLIP-39 *k-of-n* shares). Details in [`04-SECURITY-AND-PRIVACY.md`](04-SECURITY-AND-PRIVACY.md).
- **Constellation** is your private mesh of *your own* devices, built on **QUIC + the Noise protocol** for authenticated, encrypted, NAT-traversing P2P (Syncthing-class, but integrated). It does two jobs:
  1. **Cloudless sync/backup:** replicate Lifestream objects between your Key, phone, and home machine. Encrypted end-to-end; peers store only ciphertext.
  2. **Compute offload (stretch):** a weak Surface offloads heavy Aura inference to your strong home machine, or runs a full remote session (Wayland-over-network, waypipe-style), turning "the laptop is just a screen for my home GPU" into a real, *trust-bounded* feature (only your devices, never strangers).

---

## 8. The microkernel-portable shape (the moonshot, designed-for now)

Every cross-L4-boundary interaction is **capability-mediated IPC**, deliberately the same shape as a microkernel's. That means the long-term migration is an *evolution*, not a rewrite:

- **Today:** Weave broker + Cells approximate object-capabilities on the Linux kernel (userland-enforced).
- **Moonshot:** the trusted core (broker, Lifestream gatekeeper, identity) moves onto the **formally-verified seL4 microkernel**; **Linux runs as a contained driver VM** (proven pattern, seL4 can host Linux to reuse its drivers), keeping all hardware support. The Weave's capabilities become *kernel-enforced and provable* instead of userland-approximated.
- **Further out:** **CHERI/CHERIoT** hardware capabilities push the model into the ISA (unforgeable pointers, spatial memory safety) as that silicon reaches production.

Because L3-L6 only ever spoke "capabilities over IPC," they barely change. This is the entire reason for the discipline in §1.

---

## 9. End-to-end example: "download Spotify"

Tracing one sentence through every layer shows the architecture working as a whole:

1. **L6 Aura** transcribes/parses the intent -> plan: `find_app("Spotify")`, `install_app(<pkg>)`. Shows you the plan.
2. **L4 Weave**: Aura holds a `discover_apps` capability but **not** `install_app` -> the broker prompts you: *"Aura wants to install Spotify (Flatpak, sandboxed). Allow once / always / no."*
3. You approve once. The broker grants a scoped, time-limited install capability.
4. **L3 Lifestream**: the install lands in a new **generation** (so it's atomic and undoable). The app's data lives in a content-addressed subtree.
5. **L5**: Spotify launches inside a **Cell**, its own namespace, network capability only to Spotify's domains, **no** filesystem access beyond its sandbox.
6. **Glass** shows Spotify's live connections; you can sever them. The whole action is in the **audit log**, and one Lifestream step **undoes** it.

Bold capability (talk to your computer, it acts) + hard guarantees (scoped, visible, reversible), from the same architecture.

-> Deep dives follow: [portability & boot](03-PORTABILITY-AND-BOOT.md) · [security & privacy](04-SECURITY-AND-PRIVACY.md) · [the AI layer](05-AI-LAYER.md) · [tech stack](06-TECH-STACK.md).

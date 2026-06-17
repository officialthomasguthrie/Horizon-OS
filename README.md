# Horizon

**Your computer is not a machine. It's a file you carry.**

Horizon is a private, local-first, AI-native operating system that lives entirely on a drive in your pocket. Plug it into almost any laptop and your entire computer, your logins, your files, your apps, your settings, your AI, your half-finished sentence, appears in seconds. Unplug it, and you leave nothing behind. The laptop was just borrowed glass and silicon. **You** were never in it.

> The hardware becomes a disposable *Surface*. Your identity, state, and trust live on the *Horizon Key* in your pocket, never on the machine.

This repository is the design: the vision, the genuinely new concepts, the research-backed architecture, the technology choices, and the build roadmap. It is deliberately **bold about the ideas and honest about the hard parts**, including the parts of the original dream that the laws of physics and silicon won't allow, and what we do instead.

---

## The one-paragraph pitch

Today your "computer" is a tangle: an account in someone's cloud, a laptop that knows your secrets, files scattered across services, an AI that ships your life to a datacenter. Horizon collapses all of that into a single encrypted object you own and carry. The OS image is immutable and reproducible; your life is an encrypted, versioned, content-addressed object store you can **rewind like a video**; the AI runs **on the metal in front of you**, not in a datacenter; and every program, including the AI, can only touch what you explicitly grant, through one unified permission fabric. It boots on commodity laptops because it stands on the Linux kernel's enormous hardware base, but everything you actually *touch*, the security model, the state engine, the AI layer, the shell, is new.

---

## Four ideas that make Horizon new

Most "new OSes" are a fresh coat of paint on old foundations. Horizon's novelty is in four load-bearing concepts that, combined, don't exist in any shipping system:

### 1. Trust lives in the Key, not the machine
Every laptop you plug into is treated as **untrusted, ephemeral, and disposable**, a *Surface*. This is the founding axiom, not an afterthought. A single gesture, the **Surface Trust Dial**, reconfigures the entire security posture between *"this is my own machine"* (verified boot, persistent, anti-evil-maid) and *"this is a stranger's laptop in an airport"* (amnesiac, hardware-key-only unlock, nothing written, nothing remembered). No consumer OS is designed *from the boot sector up* around the assumption that the computer is hostile.

### 2. Your whole computer is a Merkle DAG you can rewind
Horizon's state engine, the **Lifestream**, stores every change to your system as an encrypted, deduplicated, content-addressed object. The *entire state of your machine* at any instant is a single hash. From this one idea you get, for free and unified: **time-travel** (scrub the whole OS back to 9am, or just one file), **atomic updates with instant rollback**, **cloudless sync** (replicate objects to your other devices), **backup** (push objects to drives you own), **integrity** (it's a Merkle tree, tampering is mathematically visible), and **recovery** (the root key is the only secret to protect). Persistence, updates, sync, backup, and undo are normally five separate, fragile subsystems. In Horizon they are *one* mechanism.

### 3. One permission fabric for humans, apps, and AI
Everything in Horizon acts through **the Weave**, an object-capability system where nothing has "ambient authority." A program can't open your files just because it's running as you; it can only use *capabilities* you handed it: unforgeable, scoped, revocable, audited tokens. The radical part: **the AI is not special-cased.** It's just another principal holding capabilities. The same mechanism that sandboxes a sketchy app is what lets you safely tell an AI "clean up my downloads folder" without it being able to touch anything else. Security and AI agency become the *same problem with the same solution.*

### 4. Identity floats above fungible compute
Your identity is not your laptop, and increasingly it isn't even one device. Your Horizon Keys, your phone, and your home machine form a private, encrypted, **cloudless mesh**, the *Constellation*. Compute becomes a pooled resource: a weak borrowed laptop can borrow horsepower from your strong machine at home over the mesh, so even "the computer is just a CPU and a monitor" becomes literally true when you want it to be. Lose your Key entirely? **Reconstitute** your whole identity from secret-shares you pre-distributed, no cloud, no company, ever holding the keys to you.

---

## What it feels like (a day with Horizon)

- **7:45am, home.** You plug your Key into your own laptop. It recognizes a *Home Surface*, verifies its own boot, and you're at your desktop in ~8 seconds, exactly as you left it last night, cursor still blinking mid-sentence.
- **9:10am, you broke something.** A bad config update. You drag the **time slider** back fifteen minutes. The whole machine, not just one file, is as it was. No reinstall, no panic.
- **12:30pm, a borrowed laptop at a café.** You plug in and twist the Surface Trust Dial to *Foreign*. Horizon boots **amnesiac**: it unlocks only with your hardware key + PIN (a keylogger alone is useless), routes traffic through Tor, refuses to decrypt your most sensitive vault, and when you yank the Key, the RAM is wiped and the café's laptop remembers nothing.
- **2:00pm, you talk to your computer.** "Find the deck about the cattle-grazing project and turn the Q3 numbers into a chart." **Aura**, the on-device AI, shows you the three steps it's about to take, *open file, read sheet, insert chart*, each a scoped capability. You tap approve. It never left the laptop; nothing went to a cloud.
- **6:00pm, weak hotel PC.** It's slow, but your beefy desktop is online at home. Horizon quietly turns the hotel PC into a thin window onto your home machine's compute over the Constellation. Same desktop, borrowed muscle.
- **Every moment, you can see.** A live pane called **Glass** shows every byte trying to leave the machine, per app. One tap cuts any of them off.

---

## The architecture, at a glance

Horizon is a **new system layer on a proven substrate.** We do *not* rewrite the 40-million-line Linux kernel (≈60% of which is drivers), that's the only realistic way to "boot on almost any laptop." Everything above the kernel is ours.

```
┌──────────────────────────────────────────────────────────────────────┐
│  AURA, the Intent Layer (on-device AI)                               │
│  natural language / voice  ->  capability calls (never blind clicking)  │
├──────────────────────────────────────────────────────────────────────┤
│  HORIZON SHELL & APPS  (Rust · Wayland · iced/wgpu, COSMIC blueprint) │
│  Glass (transparency)   ·   Cells (disposable sandboxes)              │
├──────────────────────────────────────────────────────────────────────┤
│  THE WEAVE, object-capability fabric  (Rust)                         │
│  no ambient authority · every action scoped, revocable, audited       │
│  the one gate for humans, apps, AND the AI                            │
├──────────────────────────────────────────────────────────────────────┤
│  LIFESTREAM, content-addressed, versioned, encrypted state engine     │
│  persistence + time-travel + sync + backup + recovery = one Merkle DAG │
├──────────────────────────────────────────────────────────────────────┤
│  IDENTITY & CRYPTO  ·  LUKS2 (AES-256-XTS) · FIDO2+PIN · Argon2id      │
│  Constellation mesh (P2P, cloudless) · Shamir Reconstitution          │
├──────────────────────────────────────────────────────────────────────┤
│  LINUX KERNEL  (the commodity substrate, drivers, scheduling, HW)    │
│  generic initramfs · full firmware · per-Surface profile cache        │
│  shim->boot->kernel, Secure-Boot signed · isohybrid (UEFI + legacy)     │
└──────────────────────────────────────────────────────────────────────┘
         runs on
   SURFACE: any x86-64 UEFI laptop, untrusted, ephemeral, disposable
         stored on
   HORIZON KEY: NVMe SSD in a fast USB-C/UASP enclosure (your pocket)
```

> **Long-term moonshot:** migrate Horizon's trusted core onto the **formally-verified seL4 microkernel**, running Linux as a contained *driver VM* to keep the hardware support, a provably-isolated base under the same Rust userland, without ever re-fighting the driver war. See [`docs/07-ROADMAP.md`](docs/07-ROADMAP.md).

---

## The honest part (read this)

A design is only as trustworthy as the things it admits it *can't* do. The full accounting is in [`docs/09-OPEN-QUESTIONS.md`](docs/09-OPEN-QUESTIONS.md), but the headlines:

- **"Any laptop" honestly means any x86-64 UEFI PC.** Apple Silicon Macs cannot boot a generic stick (their bootloader is per-machine, signed by Apple's Secure Enclave). ARM Windows laptops are a separate, immature target. Both are post-v1.
- **A cheap USB flash stick can't run a real OS well.** Flash sticks have terrible random I/O (single-digit MB/s), which is exactly what an OS hammers. The "Horizon Key" is a small **NVMe SSD in a UASP USB-C enclosure**, pocket-sized, but a real disk. We're honest about this everywhere.
- **You cannot make a malicious host trustworthy.** If a borrowed laptop has a hardware keylogger or compromised firmware, it can capture what you type and see while you use it. What Horizon *guarantees* is narrower and real: a lost or stolen Key is cryptographically useless to a finder; nothing persists to a foreign machine; and you can *see and sever* everything leaving the network. Ghost mode minimizes, not eliminates, what a hostile Surface can learn.
- **We are not training a frontier AI from scratch.** That costs tens to hundreds of millions of dollars. We ship a curated open model, *fine-tuned by us* for safe OS control and tool-calling. On-device AI is genuinely useful but will trail cloud frontier models in raw quality, a tradeoff we make deliberately for privacy.
- **A website can't flash your USB directly.** Browsers forbid raw writes to mass-storage devices (a good security rule). The web flow is a polished page that hands you a tiny (~5-10 MB) verified flasher app. The "one click on the website" dream is *one click in a featherweight downloader.*

None of these sink the project. They sharpen it.

---

## Repository map

| Document | What's inside |
|---|---|
| [`docs/00-IDEA-BOARD.md`](docs/00-IDEA-BOARD.md) | The unfiltered idea board, every concept, feature, and wild idea, including the "never-built-before" ones |
| [`docs/01-VISION.md`](docs/01-VISION.md) | Vision, principles, the paradigm shift, and the Horizon vocabulary |
| [`docs/02-ARCHITECTURE.md`](docs/02-ARCHITECTURE.md) | Full technical architecture: the Weave, the Lifestream, the layered stack |
| [`docs/03-PORTABILITY-AND-BOOT.md`](docs/03-PORTABILITY-AND-BOOT.md) | Boot-anywhere, the driver problem, persistence, hardware & performance reality |
| [`docs/04-SECURITY-AND-PRIVACY.md`](docs/04-SECURITY-AND-PRIVACY.md) | Threat model, encryption, Surfaces, Glass, Ghost mode, cloudless recovery |
| [`docs/05-AI-LAYER.md`](docs/05-AI-LAYER.md) | Aura: on-device models by hardware tier, voice, and *safe* agentic OS control |
| [`docs/06-TECH-STACK.md`](docs/06-TECH-STACK.md) | Languages & frameworks, each choice justified with research and sources |
| [`docs/07-ROADMAP.md`](docs/07-ROADMAP.md) | Phased build plan, milestones, team/skills, and the seL4 moonshot track |
| [`docs/08-WEBSITE-AND-INSTALLER.md`](docs/08-WEBSITE-AND-INSTALLER.md) | The flashing website + cross-platform installer + image distribution |
| [`docs/09-OPEN-QUESTIONS.md`](docs/09-OPEN-QUESTIONS.md) | The honest ledger: hard problems, risks, and decisions still open |

---

## Status & license

**Status:** Design phase. This repo currently contains the architecture and plan. No code yet, the next step is scaffolding the workspace described in [`docs/07-ROADMAP.md`](docs/07-ROADMAP.md).

**License:** Intended to be fully open source (the privacy promise is only credible if the code is auditable). Proposed: **GPLv3** for the OS/system layer, **Apache-2.0** for libraries and the SDK so others can build on the Weave. To be finalized.

**Name of everything (so the docs make sense):**

| Term | Meaning |
|---|---|
| **Horizon** | the operating system |
| **Horizon Key** | the pocket NVMe drive holding your encrypted OS + identity |
| **Surface** | any host machine you plug into, untrusted, ephemeral |
| **The Weave** | the object-capability fabric (security + AI agency) |
| **Lifestream** | the content-addressed, versioned, encrypted state engine |
| **Aura** | the on-device AI intent layer |
| **Constellation** | your private, cloudless mesh of devices |
| **Glass** | the live privacy/transparency surface |
| **Cells** | disposable, sandboxed compartments |
| **Ghost mode** | amnesiac/paranoid mode for untrusted Surfaces |
| **Reconstitution** | cloudless, Shamir-based identity recovery |

> Horizon's bet, in one line: **own your computer by carrying it, trust no machine, see everything, and let an AI you actually control do the boring parts.**

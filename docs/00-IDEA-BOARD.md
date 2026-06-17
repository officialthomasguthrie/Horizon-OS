# 00 · The Idea Board

This is the unfiltered concept space for Horizon, the brainstorm before the engineering discipline. Some of these are load-bearing pillars; some are wild swings we may never build. They're tagged:

- [CORE] **Core**, a defining pillar; Horizon isn't Horizon without it.
- [STRONG] **Strong**, high-value, very buildable, planned.
- [STRETCH] **Stretch**, exciting, harder, later.
- [MOONSHOT] **Moonshot**, research-grade or far-future.
- [ANTI] **Anti-feature**, something we deliberately refuse to do.

---

## Part A, The big concepts (the "never built before" ones)

### [CORE] A1. The computer is a file you carry
The mental model flip: a "computer" stops being a machine you own and becomes an **encrypted object** you own. Hardware is rented by the minute, emotionally speaking. You can lose a laptop the way you lose a pen. Your *self*, files, logins, AI, muscle-memory of your desktop, is portable, encrypted, and yours.

### [CORE] A2. Surfaces: hardware is disposable and untrusted
Every machine is a **Surface**, a temporary CPU + screen + keyboard you borrow. The OS is built from the boot sector up around the assumption that *the Surface might be hostile.* This is the inversion no consumer OS makes. From it flows the Trust Dial, Ghost mode, amnesiac boot, and hardware-key unlock.

### [CORE] A3. The Lifestream: your whole machine as a rewindable Merkle DAG
One content-addressed, encrypted, versioned object store underneath everything. The state of your entire computer is a single hash. **Five subsystems collapse into one:**
- Persistence = the current root of the DAG.
- Time-travel = check out an older root (whole-system *or* per-file).
- Updates = the base image is just another subtree; swapping it is atomic; rollback is instant.
- Sync = replicate objects to your other devices.
- Backup & recovery = push objects to drives you own; protect one root key.

### [CORE] A4. The Weave: one capability fabric for humans, apps, and AI
Object-capability security everywhere. No ambient authority, nothing can act just because it's "running as you." The AI is **not** a privileged special case; it's a principal holding scoped capabilities, exactly like an app. Security and AI safety become the *same* mechanism.

### [CORE] A5. Aura: AI as a system layer, not an app
The AI sits between intent and the system as a first-class layer, like a shell, but for natural language. It does **not** drive the machine by clicking pixels; it calls typed, permissioned capabilities through the Weave. Every action is previewable, reversible (Lifestream!), and audited.

### [CORE] A6. Identity floats above fungible compute (the Constellation)
Your devices form a private encrypted mesh with **no cloud**. Compute is pooled: a weak Surface can borrow a strong device's horsepower. Your identity is independent of any one piece of hardware, and survives the loss of all of them via Reconstitution.

### [STRETCH] A7. Trust is measurable and visible, not assumed
Horizon never says "you're secure." It *shows* you your exact security posture right now: which Surface, what's verified, what's leaking, what the AI can touch. Security as a **live readout**, not a marketing claim.

---

## Part B, Portability & "boot anywhere" features

- [CORE] **Plug-in-and-you're-home.** Boot to your exact desktop on a new Surface in seconds, state intact.
- [STRONG] **Per-Surface profiles.** First boot on a machine, Horizon profiles its hardware and caches a tuned config (the NixOS `hardware-configuration` idea, automated). Next time on that Surface: instant and optimized.
- [STRONG] **Surface Trust Dial.** One control: *Home* (persistent, verified, optimized) <-> *Known* (trusted friend's machine) <-> *Foreign* (amnesiac, paranoid). It reconfigures encryption, persistence, networking, and AI scope in one move.
- [STRONG] **Universal boot image.** One image boots both UEFI and legacy BIOS (isohybrid), 64-bit with a 32-bit-UEFI shim for older machines.
- [STRONG] **"Will it run here?" pre-flight.** A tiny tool (web or app) reads a Surface's specs and tells you, honestly, how well Horizon will run before you reboot it.
- [STRETCH] **Hot-unplug safety.** Yank the Key and Horizon fails *safe*: RAM scrubbed, no half-written state, the Surface clean.
- [STRETCH] **Hibernate-to-Key.** Suspend on one Surface, resume the *exact* RAM state on another (encrypted hibernation image in the Lifestream). Walk from desk to café mid-task.
- [MOONSHOT] **Crowd-sourced Surface database.** Opt-in, anonymized "this exact laptop model works great / has a WiFi quirk" so the community maps real-world compatibility.

---

## Part C, Security & privacy features

- [CORE] **Lost-Key-is-useless guarantee.** Full-volume LUKS2 AES-256; a finder gets ciphertext.
- [CORE] **Hardware-key unlock with PIN.** FIDO2 token + PIN + passphrase; a host keylogger alone can't unlock you.
- [STRONG] **Ghost mode.** Amnesiac boot for foreign Surfaces, nothing persists, Tor-forced, sensitive vaults stay sealed.
- [STRONG] **Glass.** A live, per-process map of every network connection and data access. Tap to kill. Privacy you can *watch happen*.
- [STRONG] **Cells.** Disposable, sandboxed compartments (bubblewrap / microVM). Open a sketchy link or untrusted file in a Cell that's incinerated on close.
- [STRONG] **Vaults with tiered trust.** Mark some data "never decrypt on a Foreign Surface." Your tax records simply won't open on a stranger's laptop, by policy.
- [STRONG] **Reconstitution.** Lose the Key? Rebuild your identity from *k-of-n* secret shares (Shamir/SLIP-39) on steel cards + your phone. No company can do this *for* you, and none can do it *to* you.
- [STRONG] **Per-app network kill-switch & egress prompts** (OpenSnitch-style), mic/cam hardware-style indicators, a 7-day access timeline.
- [STRETCH] **Anti-evil-maid on Home Surfaces.** On your *own* machine, detect boot tampering by sealing a secret you'll recognize.
- [STRETCH] **MAC randomization + blank hostname** by default; you're not broadcasting "Thomas's Horizon" on every café WiFi.
- [STRETCH] **Decoy / duress mode.** A duress PIN opens a plausible, empty environment. (Be honest about limits, see security doc.)
- [MOONSHOT] **Phone-as-trust-anchor.** Post-boot, your phone over the Constellation co-signs sensitive actions, acting as a second, *trusted* device next to the untrusted Surface.

---

## Part D, The AI layer (Aura)

- [CORE] **Intent shell.** Type or speak: *"open my deck about cows," "install Spotify," "find Sarah's email and draft a reply," "make my screen warmer."* Aura maps it to capability calls.
- [CORE] **Capability-scoped & previewed.** Aura shows the exact steps before acting; destructive actions need a tap; everything is undoable via the Lifestream.
- [STRONG] **Fully offline voice.** whisper.cpp (speech-in) + Piper/Kokoro (speech-out). Talk to your computer on a plane.
- [STRONG] **Semantic everything.** On-device embeddings + vector search: find files/emails/notes by *meaning*, not filename. "That thing about grazing rotations from spring."
- [STRONG] **Tiered models by hardware.** A small model (3-4B) for intent routing on weak Surfaces; a larger one (8-14B) when the Surface or your Constellation can run it.
- [STRONG] **Aura learns your patterns locally.** "You usually do X after Y, want me to?" All on-device; nothing profiled in a cloud.
- [STRETCH] **Ambient automations.** "Every time I plug in at work, open these three apps and mute notifications." Natural-language macros backed by capabilities.
- [STRETCH] **Explain-this-system.** Point Aura at any setting, file, or process: "what is this, is it safe, what does it touch?" The AI as a guide to your own machine.
- [MOONSHOT] **Aura over the Constellation.** Weak Surface runs the small model; offloads hard reasoning to the big model on your home machine, privately, over your mesh.
- [ANTI] **No cloud inference, ever, by default.** Aura is local. Any optional cloud model is opt-in, clearly labeled, and never the default.

---

## Part E, The state engine & "undo for your life"

- [CORE] **Whole-system time-travel.** A slider for your entire computer. Scrub back to before you broke it.
- [STRONG] **Per-file history, automatic.** Every file is versioned by default (it's content-addressed anyway). No "save as v2 final FINAL."
- [STRONG] **Atomic updates with one-tap rollback.** Update can never brick you; the previous generation is always one reboot away.
- [STRONG] **Reproducible system.** Your exact environment is a recipe; rebuild it bit-identically on a fresh Key.
- [STRETCH] **"Fork your computer."** Branch your whole environment (like git) to try something risky, then keep it or throw it away.
- [STRETCH] **Deduplicated by design.** Content-addressing means identical data is stored once across versions, apps, and the base image, crucial on a pocket drive.
- [MOONSHOT] **Shareable system snapshots.** Hand a friend a hash; they boot *your* exact setup (minus your secrets) to debug or collaborate.

---

## Part F, The desktop, shell & apps

- [STRONG] **All-new Rust desktop** built on the COSMIC blueprint (Smithay + iced + wgpu), Wayland-only.
- [STRONG] **Command palette as a first-class citizen**, keyboard-driven, AI-augmented; the Aura intent line *is* the launcher.
- [STRETCH] **App model = capabilities.** Apps ship a manifest of what they want; you grant per-capability, like the best of mobile but for a real desktop.
- [STRETCH] **Web-app-first compatibility.** Run Linux apps (via Wayland/XWayland/Flatpak) and PWAs in Cells, so the ecosystem isn't empty on day one.
- [STRETCH] **Focus & calm by default.** Notifications are opt-in, batched; the system respects attention. A privacy OS should also be a *peace* OS.
- [MOONSHOT] **"Surfaces as peripherals."** Pair a tablet over the Constellation as a second screen / control surface for your Horizon session.

---

## Part G, The website & install experience

- [STRONG] **Beautiful one-page site** that detects your OS and hands you the right tiny flasher.
- [STRONG] **Raspberry-Pi-Imager-class flasher** (built in Tauri): pick your Key, it downloads the latest signed image, verifies it, writes it, re-verifies, and filters out your system disks so you can't nuke your main drive.
- [STRONG] **Signed everything.** GPG/cosign-signed checksums; the flasher refuses tampered images.
- [STRETCH] **Delta updates.** Returning users download only the diff (zsync/casync).
- [STRETCH] **Torrent + CDN.** Community-mirrored, resumable, censorship-resistant distribution.
- [STRETCH] **"Try Horizon in your browser."** A WASM/VM demo of the desktop so people can feel it before committing a drive.
- [ANTI] **No account to download.** You don't sign up to get a private OS. That would be absurd.

---

## Part H, Genuinely wild / moonshot ideas

- [MOONSHOT] **seL4 trusted core.** Move Horizon's security-critical core onto a *formally verified* microkernel, with Linux demoted to a contained driver VM. A consumer OS with a mathematically proven trust base.
- [MOONSHOT] **CHERI hardware capabilities.** As CHERI/CHERIoT silicon matures, push the Weave's capabilities into *hardware*, unforgeable pointers, spatial memory safety at the ISA level.
- [MOONSHOT] **Compute-borrowing marketplace (trust-bounded).** Borrow compute only from devices in *your* Constellation, never strangers, but make it seamless enough that "my laptop is just a screen for my home GPU" is a daily reality.
- [MOONSHOT] **Self-healing system.** Aura watches the audit log; when something misbehaves, it proposes a Lifestream rollback or a capability revocation automatically.
- [MOONSHOT] **Verifiable privacy receipts.** Cryptographic proof, exportable, that "during this session, these processes made exactly these connections", privacy you can *prove* to an auditor, a journalist's editor, a court.
- [MOONSHOT] **Hardware: the official Horizon Key.** A purpose-built NVMe-in-enclosure device with a hardware kill switch, secure element, and tamper evidence. The dream object.
- [MOONSHOT] **Plan-9-style "everything is a capability-mediated resource"** networking: your remote devices' files/devices appear as local, brokered through the Weave.

---

## Part I, Anti-features (what we refuse to do)

- [ANTI] **No telemetry. No analytics. No "anonymous usage data."** Not even the well-intentioned kind.
- [ANTI] **No mandatory cloud, no mandatory account.** The OS is fully functional with zero servers in the loop.
- [ANTI] **No ad tech, no data brokering, ever.** The business model can never be you.
- [ANTI] **No silent AI.** Aura never acts without a visible trace; no background model is quietly reading your files.
- [ANTI] **No security theater.** We don't claim to defend against threats we can't (a malicious host's keylogger). We say so plainly and design around it.
- [ANTI] **No locked bootloader / no anti-ownership.** You can read the code, fork it, and rebuild your own Key. It's *yours.*

---

## Part J, Open creative questions

These are unresolved on purpose, good fuel for the next design pass:

- What's the *first ten seconds* like? The boot-to-desktop ritual is the product's handshake.
- Should the Lifestream timeline be a literal video-scrubber UI? A calendar? A git-graph?
- How does Aura ask for a capability it doesn't have yet, and how do we make granting it feel safe, not nagging?
- What does Glass look like such that a non-expert *enjoys* watching their privacy?
- What's the emotional story of Reconstitution, handing someone steel cards feels heavy; can we make it feel like a gift to your future self?
- Is the brand "Horizon" calm/sunrise, or sharp/cyber? (It changes every visual choice.)

-> Next: the disciplined version of all this lives in [`01-VISION.md`](01-VISION.md) and the deep-dive docs.

# 01 · Vision & Principles

## The problem Horizon exists to solve

Three quiet defaults have hardened into "just how computers are":

1. **Your computer knows you, and so does everyone who touches it.** Your laptop stores your secrets; your accounts live in clouds you don't control; your AI ships your life to a datacenter. Privacy is something you *request*, badly, from companies whose income is your data.
2. **You are chained to hardware.** Your "computer" is a specific physical object. Lose it, break it, leave it at home, travel without it, and you're locked out of your own life. Migrating to a new machine is a dreaded ritual.
3. **You don't actually control what runs.** Any app you launch inherits your full authority. You can't see what's leaving your machine. The "AI assistant" is a black box with the keys to everything.

Horizon rejects all three as inevitable. Not by adding features to the old model, by **changing the model.**

## The paradigm shift

> **From:** a computer is a *machine you own and log into.*
> **To:** a computer is an *encrypted object you carry, that borrows hardware to exist.*

When the computer becomes a portable, encrypted, self-contained object:

- **Privacy** becomes structural, not requested, there's no cloud and no company in the loop by default.
- **Portability** becomes total, your whole environment goes where you go, on any Surface.
- **Control** becomes visible and granular, one capability fabric governs every actor, including the AI.
- **Resilience** becomes built-in, your state is a versioned Merkle DAG you can rewind, replicate, and reconstitute.

Everything in Horizon is downstream of that one shift.

## Design tenets

These are the rules we use to settle arguments. When two features conflict, the higher tenet wins.

### 1. The Key is the root of trust. The Surface is never trusted.
Security decisions assume the host may be hostile. We protect what we *can* (data at rest, secrets, network egress, persistence) and are loud about what we *can't* (a host's keylogger). We never pretend a borrowed machine is safe.

### 2. Local by default; cloud by explicit, labeled choice.
No feature may *require* a server we run. The AI is on-device. Sync is peer-to-peer between *your* devices. If we ever offer an optional hosted service, it is opt-in, clearly marked, and the OS is fully whole without it.

### 3. No ambient authority. Ever.
Nothing acts merely because it's "running as the user." Every action flows through an explicit, scoped, revocable capability. This single rule is what makes both app sandboxing *and* safe AI agency possible, they're the same mechanism (the Weave).

### 4. The user can always see, and always undo.
Transparency (Glass) and reversibility (Lifestream) are not features bolted on, they're guarantees. Any action a program or the AI takes is visible and, wherever physically possible, undoable.

### 5. Stand on giants; innovate where it's felt.
We do not rewrite the Linux kernel, llama.cpp, or a Wayland compositor from scratch to prove a point. We reuse the best proven substrate and pour our originality into the layers the user actually experiences: identity, state, capabilities, AI, and the security model. (See the honest cost analysis in [`06-TECH-STACK.md`](06-TECH-STACK.md).)

### 6. Honesty is a feature.
We document what's hard, what's impossible, and what we chose not to do. A privacy product that overpromises is worse than useless, it's dangerous. The [Open Questions](09-OPEN-QUESTIONS.md) doc is a first-class part of the product.

### 7. Open or it isn't private.
"Trust us" is not a privacy model. The code is open source and auditable. You can rebuild your own Key from source. The privacy claim is only as good as the public's ability to verify it.

### 8. Calm by default.
A system that respects your data should respect your attention. Notifications are opt-in and batched; the AI is quiet unless invoked; the defaults are serene. Privacy and peace come from the same place: nothing acting on you without your say.

## Who Horizon is for

- **The privacy-serious**, journalists, activists, lawyers, doctors, researchers, who need *real*, auditable, local guarantees, not promises.
- **The mobile-by-necessity**, people who work across many machines, travel, use shared/library/work computers, or simply don't want to carry a laptop everywhere.
- **The sovereignty-minded**, people who want to *own* their computing, not rent it from clouds and platforms.
- **The tinkerers and builders**, who want a genuinely new, open, hackable system to extend (the Weave is an SDK, not just a feature).

And, aspirationally, **everyone**, because "carry your whole private computer in your pocket and trust no machine" is a better deal than the status quo for almost anyone, once it's easy enough.

## What success looks like

- You can walk up to a random x86-64 laptop, plug in your Key, and be working in your own environment in under 15 seconds, then leave it with no trace.
- A security researcher can read the code and agree the lost-Key and no-cloud guarantees hold.
- You can tell your computer what you want in plain language and trust that it can only do what you allowed, and that you can undo it.
- You can lose your Key and not lose yourself.
- Nothing about your computing life lives anywhere you don't control.

## The Horizon vocabulary

A precise shared language so the rest of the docs are unambiguous:

| Term | Definition |
|---|---|
| **Horizon** | The operating system itself. |
| **Horizon Key** (the *Key*) | The portable device, an NVMe SSD in a fast USB-C/UASP enclosure, holding your encrypted OS image and your identity/state. |
| **Surface** | Any host machine you boot Horizon on. Untrusted and ephemeral by definition. |
| **Home / Known / Foreign Surface** | Trust tiers set by the **Surface Trust Dial**, governing persistence, verification, networking, and AI scope. |
| **The Weave** | Horizon's object-capability fabric. All authority flows through it; no ambient authority exists. |
| **Capability** | An unforgeable, scoped, revocable token granting a specific right to a specific resource. The atom of the Weave. |
| **Lifestream** | The content-addressed, encrypted, versioned state engine. The whole system's state is one root hash in it. |
| **Generation** | A named root of the Lifestream, a complete, bootable system state you can return to. |
| **Aura** | The on-device AI intent layer. A principal in the Weave, not a privileged component. |
| **Constellation** | Your private, cloudless, encrypted mesh of your own devices (Keys, phone, home machine). |
| **Glass** | The live transparency surface: per-process network and data-access visibility with kill switches. |
| **Cell** | A disposable, sandboxed compartment for risky or untrusted activity. |
| **Ghost mode** | The amnesiac/paranoid posture for Foreign Surfaces. |
| **Reconstitution** | Cloudless recovery of your identity from *k-of-n* secret shares. |

-> Now the engineering: [`02-ARCHITECTURE.md`](02-ARCHITECTURE.md).

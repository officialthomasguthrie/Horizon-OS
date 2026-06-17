# 09 · Open Questions & The Honest Ledger

A design is only trustworthy if it names what's hard, what's unresolved, and what was decided on assumption. This is that ledger. (Tenet #6: honesty is a feature.)

---

## 1. Hard technical problems (best current answer + residual risk)

| Problem | Best answer we have | Residual risk |
|---|---|---|
| **Boot on truly *any* machine** | Linux substrate + generic initramfs + full firmware + recent kernel | Day-one GPUs/Wi-Fi, NVIDIA Optimus, fingerprint readers, flaky s2idle suspend. Apple Silicon impossible from a generic stick; ARM is a separate build. |
| **Fast OS off a "USB stick"** | NVMe-in-UASP-enclosure, USB 3.2 Gen2+; immutable base to cut writes | Cheap flash sticks are genuinely unusable; we must message "Key = real disk" without losing the "USB" simplicity. Cost/perception hurdle. |
| **Capability security on Linux** | Weave broker + bubblewrap-class confinement (userland ocap) | A kernel-level exploit bypasses userland confinement. Only the seL4 moonshot makes it airtight/provable. |
| **Trusting an untrusted host** | Can't. We protect at-rest + minimize/illuminate runtime leak; Ghost mode | A malicious host can keylog/screen-capture during the session; we cannot detect hostile firmware/TPM from a guest boot. *Unsolvable in principle.* |
| **Lifestream performance** | Two tiers: CoW live + content-addressed durable | The live<->durable bridge and chunk dedup must stay fast on a pocket drive under real workloads, unproven at scale yet. |
| **AI prompt injection** | Capability confinement bounds damage; confirm/audit/undo | No robust general defense exists; a fooled model is contained, not prevented. Must be documented in-product. |
| **Local AI quality on weak HW** | Tiered models; offload to Constellation | 3-4B on a GPU-less laptop is slow and error-prone for multi-step tasks; local trails cloud quality. |
| **Recovery without a cloud** | Shamir/SLIP-39 *k-of-n* + your own devices | Secret exists whole at reconstruction (mitigated by "trusted-boot-only" rule); UX of steel cards is heavy for normal users. |
| **Browser can't flash USB** | Tiny Tauri flasher | Not the literal "flash from the website" dream; an extra (small) step. |
| **Secure Boot 2026 cert expiry** | Dual-signed shim (2011+2023), SBAT tracking | Hard external deadline (June 2026); shim-review board gating adds lead time. |

---

## 2. The honest limits, consolidated

If someone reads only one section, read this:

1. **"Any laptop" = any x86-64 UEFI PC.** Not Apple Silicon (impossible from a generic stick). Not ARM Windows (separate, immature build). Stated plainly in-product.
2. **The Key is a fast NVMe drive, not a $5 thumb stick.** Honest hardware requirement.
3. **A hostile host can spy on your live session.** We make a lost Key useless, leave no trace, and let you see/sever network egress, but we cannot secure a machine that's actively against you.
4. **Aura is a fine-tuned open model, not a from-scratch frontier model**, and local AI trails cloud quality. A deliberate privacy trade.
5. **Capability confinement is userland-enforced until seL4.** Strong, not yet provable.
6. **Prompt injection is contained, not solved.**
7. **You flash with a small downloaded tool, not the browser.**

None of these kill the vision. Stating them is what makes the rest credible.

---

## 3. Open product & design decisions

- **Time-travel UI:** literal video-scrubber, calendar, or git-style graph? Affects how "rewind your computer" *feels.*
- **Capability-grant UX:** how does Aura ask for a new capability without becoming naggy? What are good "grant once / always / for this Cell" defaults?
- **Glass for normal people:** how do we make watching your own privacy *pleasant*, not alarming?
- **Default trust tier on unknown machines:** Foreign (safe, more friction) vs Known (smoother, riskier)? Leaning Foreign.
- **Duress/decoy mode:** ship it (with honest caveats) or omit it to avoid false confidence?
- **App ecosystem stance:** how much to lean on Flatpak/XWayland compatibility vs pushing native capability-aware apps?
- **Brand & tone:** calm/sunrise *Horizon* vs sharp/cyber? Drives every visual and copy choice.

---

## 4. Open strategic / project questions

- **License:** proposed GPLv3 (OS) + Apache-2.0 (libs/SDK). Confirm. (Privacy credibility *requires* open source.)
- **Funding & sustainability:** the anti-features (no telemetry, no data business, no mandatory cloud) rule out the usual models. Options: donations/foundation (Tails/Signal model), optional paid hosted *extras* (sync relay, never required), first-party hardware Keys, support/enterprise. Needs a real plan.
- **Governance:** solo -> core team -> open foundation? Affects contribution model early.
- **Name & trademark:** is "Horizon" available/clear in this space? (Several products use it.) Decide before brand investment.
- **Threat-model scope:** who is the *primary* user we optimize for first, privacy professionals (deep features) or mainstream portability (polish/ease)? It changes prioritization.

---

## 5. Decisions made *for* you (override freely)

To produce a coherent plan I committed to defaults. Each is reversible:

- **Linux substrate over from-scratch kernel** (the single most consequential call, see [`03`](03-PORTABILITY-AND-BOOT.md), [`06`](06-TECH-STACK.md)).
- **Rust as the primary language** above the kernel.
- **NVMe-in-enclosure as the canonical "Key."**
- **Capabilities as the unifying model** for apps + AI (the Weave).
- **Lifestream (content-addressed DAG)** as the state engine.
- **On-device AI by default**, fine-tuned open model.
- **seL4 as the long-term trusted core**, not v1.
- **Tauri flasher** instead of chasing browser flashing.
- **Honesty-in-product** as a hard rule (we ship the limits, not just the promises).

---

## 6. The biggest single risk

**Scope.** This is, fully realized, the work of a funded multi-year team. The mitigation is baked into [`07-ROADMAP.md`](07-ROADMAP.md): the **Phase 0-1 demo** (boot-anywhere + encrypted persistence + time-travel) is independently compelling, reachable by a small team, and *is* the pitch for everything after. Build the core that proves the idea; let the vision pull the rest.

---

*This ledger is a living document. Every honest "we can't" here is also a research frontier, and the places Horizon could, eventually, become genuinely unprecedented.*

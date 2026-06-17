# 04 · Security & Privacy

> **The hardest truth, stated first:** when Horizon boots on a machine you don't own, *you cannot make a malicious computer trustworthy.* The host's firmware and CPU run beneath anything you boot. So Horizon's real, honest goals are: **protect your data at rest**, **make a lost or stolen Key useless to a finder**, **minimize and illuminate what leaks at runtime**, and **survive device loss without a cloud.** Anyone who promises more on borrowed hardware is selling security theater.

This document defines the threat model, the layered defenses, and, crucially, the boundaries.

---

## 1. Threat model

**What we defend against:**

- **Loss/theft of the Key** -> data at rest must be cryptographically useless to a finder.
- **A finder/attacker trying to unlock the Key** -> strong KDF, hardware-key + PIN, rate-limiting.
- **A passive/curious host** -> minimize what's written, leak as little as possible, leave no trace on Foreign Surfaces.
- **Network surveillance** -> encrypted-by-default, optional Tor, MAC randomization, visible/severable egress.
- **Malicious apps & an over-eager AI** -> capability confinement (the Weave), disposable Cells, full audit.
- **Device loss without backup** -> cloudless Shamir recovery.

**What we *cannot* fully defend against (and say so):**

- **A compromised host while you use it**, hardware keyloggers, malicious firmware (SMM/ME ring -2), a tampered screen. It can capture what you type and see *during the session.* ([System Management Mode](https://en.wikipedia.org/wiki/System_Management_Mode); [hardware USB keyloggers](https://www.keelog.com/usb-keylogger/))
- **Cold-boot / DMA attacks** on RAM, which still work to varying degrees on modern hardware. ([RAM remanence tests, 2025](https://blog.3mdeb.com/2025/2025-02-20-conclusions-from-ram-data-remanence-tests/); [Thunderspy](https://thunderspy.io/))
- **Trust attestation on hardware you don't own**, measured boot / TPM attestation are meaningless when the host controls the TPM and firmware (and discrete-TPM keys can be bus-sniffed). ([sniffing BitLocker/TPM keys](https://blog.nviso.eu/2024/11/26/wake-up-and-smell-the-bitlocker-keys/))

Ghost mode (§5) *minimizes* exposure to these; it cannot eliminate them.

---

## 2. Layer 0, Encryption & keys (the foundation)

**Full-volume encryption: LUKS2 / dm-crypt**, kernel-native, multi-keyslot, Argon2id by default ([LUKS](https://en.wikipedia.org/wiki/Linux_Unified_Key_Setup)). Two concrete, easy-to-get-wrong settings we hard-code:

- **Force AES-256.** cryptsetup's default 256-bit XTS key is *two* 128-bit halves, effectively AES-128. We format with `--key-size 512` for true AES-256-XTS. ([cryptsetup FAQ](https://gitlab.com/cryptsetup/cryptsetup/-/blob/main/FAQ.md))
- **Cap Argon2id memory for portability.** cryptsetup benchmarks the *formatting* machine and can pick ~1 GiB, which then **fails to unlock on a weaker laptop**. We cap `--pbkdf-memory` (~256-512 MiB), with RFC 9106's memory-constrained profile as the floor. ([RFC 9106](https://www.rfc-editor.org/info/rfc9106/), [Ubuntu bug](https://bugs.launchpad.net/ubuntu/+source/cryptsetup/+bug/1820049))

**Unlock factors (multi-keyslot):**

- **Passphrase** (always available as a fallback).
- **FIDO2 hardware key + on-token PIN** via `systemd-cryptenroll`, the token computes an HMAC; the secret never leaves it, and the **PIN means a host keylogger alone can't unlock you.** ([FIDO2 LUKS](https://kudelskisecurity.com/research/luks-disk-encryption-with-fido2/), [systemd-cryptenroll](https://0pointer.net/blog/unlocking-luks2-volumes-with-tpm2-fido2-pkcs11-security-hardware-on-systemd-248.html))
- **A printed recovery passphrase** (for the worst case).

**Deliberately avoided: TPM2 unlock.** TPM PCRs bind to *one host's* firmware, so a TPM-sealed key won't unlock elsewhere, the exact opposite of boot-anywhere, *and* the host's TPM is untrusted anyway. We don't use it for the portable volume.

**The phone is a post-boot factor only.** The initramfs has no Bluetooth/network stack, and phone passkeys use BLE/internet "hybrid" transport, so a phone can't be a *boot-time* unlock factor. It becomes a trusted second device *after* boot, over the Constellation. ([fido2luks discussion](https://github.com/bertogg/fido2luks))

---

## 3. Layer 1, The Surface Trust Dial

One control reconfigures the entire posture by declaring what the host *is*:

| Tier | Persistence | Verification | Unlock | Network | AI / vault scope |
|---|---|---|---|---|---|
| **Home** (your own machine) | Full, encrypted | **Anti-evil-maid** boot check (works *only* on your own machine, you pre-seal a secret to a TPM *you* trust, [Qubes AEM](https://doc.qubes-os.org/en/latest/user/security-in-qubes/anti-evil-maid.html)) | Passphrase or FIDO2 | Normal | Full |
| **Known** (a trusted friend's machine) | Optional, encrypted | None claimed | FIDO2 + PIN | Normal or Tor | Reduced |
| **Foreign** (a stranger's / public machine) | **None (amnesiac)** | None possible | **FIDO2 + PIN only** | **Tor-forced** | **High-value vaults stay sealed** |

The dial is honest about *verification*: boot integrity is something we can meaningfully check **only on your own machine.** On a Foreign Surface we explicitly tell you it's **unverifiable**, and design around that with amnesia and minimization rather than false assurance.

---

## 4. Layer 2, Runtime isolation (the Weave + Cells)

(Mechanism detailed in [`02-ARCHITECTURE.md`](02-ARCHITECTURE.md) §4. Security-relevant choices here.)

- **No ambient authority:** every app/service/AI runs confined; resources come only via capabilities from the Weave broker. This structurally bounds both malicious apps and an over-eager Aura ("excessive agency").
- **Cells** use **bubblewrap-class** unprivileged namespaces + seccomp, chosen over Firejail, whose SUID-root design caused privilege-escalation CVEs. ([bubblewrap](https://github.com/containers/bubblewrap))
- **Disposable Cells** for untrusted links/files, incinerated on close (Qubes disposable-VM pattern, made lightweight, [Qubes disposables](https://doc.qubes-os.org/en/latest/user/how-to-guides/how-to-use-disposables.html)).
- **Wayland-only** so a sandboxed app can't keylog other windows the way X11 allows.
- **KVM microVM Cells** for the highest-trust separation where the Surface's hardware (and trust tier) permit. We can't ship Qubes itself, Xen needs IOMMU/VT-d often locked on borrowed machines plus 6 GB+ RAM ([Qubes requirements](https://doc.qubes-os.org/en/latest/user/hardware/system-requirements.html)), so we borrow its *patterns* at a weight that runs on real Surfaces.

---

## 5. Ghost mode (the Foreign-Surface posture)

When you set the dial to Foreign, Horizon:

- boots **fully amnesiac**, overlay in tmpfs, **nothing** written to the Key or host;
- **forces Tor** with stream isolation (the Tails model, [Tails Tor](https://tails.net/doc/about/warnings/tor/index.en.html));
- unlocks **only** with FIDO2 token + PIN, so a lone host keylogger can't capture a reusable secret;
- **refuses to decrypt high-value vaults** (you marked them "never on Foreign");
- **scrubs RAM on removal** and assumes the screen and RAM may be observed.

**Honest limits of Ghost mode:** a compromised host can still capture what you *type and see during the session*, and Horizon **cannot** detect malicious host firmware/TPM from inside a guest boot. Ghost mode reduces the blast radius; it does not make a hostile machine safe. We will say this in the UI, not bury it.

---

## 6. Layer 3, Network privacy you can see (Glass)

Local-first removes the cloud honeypot, but that alone isn't privacy, it needs the encryption above *and* visibility. So Horizon makes privacy **observable and interactive**:

- **Glass:** a live, per-process map of every network connection and data access, with **one-tap kill switches** and a 7-day timeline, combining OpenSnitch-style egress control, Android's Privacy Dashboard, and GrapheneOS's per-app network toggle. ([OpenSnitch](https://opensnitch.org/), [GrapheneOS](https://grapheneos.org/features))
- **Defaults:** **MAC randomization + blank DHCP hostname** (don't broadcast "Thomas's Horizon" on every café AP, [NetworkManager trackability](https://privsec.dev/posts/linux/networkmanager-trackability-reduction/)); encrypted DNS; **ECH** where available (hides SNI, though *not* the destination IP, [RFC 9849](https://www.rfc-editor.org/info/rfc9849/)).
- **Optional forced Tor** per Surface/Cell. We're honest that Tor is not magic: end-to-end timing correlation is a real, demonstrated risk. ([Tor deanonymization, 2024](https://www.schneier.com/blog/archives/2024/10/law-enforcement-deanonymizes-tor-users.html))
- **Mic/cam indicators** that are part of the compositor, not a dismissible app.

The point: most OSes ask you to *trust* their privacy. Horizon lets you *watch* it and *cut it off.*

---

## 7. Cloudless recovery (Reconstitution)

Losing the Key must not mean losing your life, *without* reintroducing a cloud that defeats the whole point.

- **Identity key split with Shamir / SLIP-39** ("Shamir Backup"): *k-of-n* shares where any fewer than *k* reveal **zero** information ([SLIP-39 / Shamir Backup](https://trezor.io/learn/advanced/standards-proposals/what-is-shamir-backup)). Defaults: **2-of-3** or **3-of-5**, on fire/water-resistant steel cards, stored in separate places.
- **Data backup** replicated to **your own** devices (phone, NAS, second Key) via the Constellation in **untrusted-device mode**, peers hold only ciphertext (Syncthing's model, [untrusted devices](https://docs.syncthing.net/users/untrusted.html)).
- **A second FIDO2 key** enrolled from day one.
- **The one real weakness** of secret-sharing, the key exists *whole* at the moment of reconstruction, is mitigated by a rule: **reconstitute only on a trusted, offline Horizon boot**, never on a Foreign Surface. ([SSS shortcomings](https://blog.casa.io/shamirs-secret-sharing-security-shortcomings/))
- **Optional social recovery** (guardians, à la crypto wallets) for better UX, offered, never required, because it reintroduces contacts/online dependence. ([Vitalik on social recovery](https://vitalik.eth.limo/general/2021/01/11/recovery.html))

---

## 8. What about a duress / decoy mode?

A **duress PIN** can open a plausible, empty environment instead of your real one (hidden-volume idea, VeraCrypt-style). We'll likely offer it, but with an honest caveat in the docs: against a sophisticated forensic adversary, deniable encryption has known weaknesses, and it's a *social/legal* gamble, not a cryptographic guarantee. Offered as a tool, not a promise.

---

## 9. The honest summary

**Horizon can credibly promise:**
- A lost or stolen Key is cryptographically useless to whoever finds it.
- On a Foreign Surface, nothing persists and your high-value secrets never decrypt.
- You can *see* and *sever* everything your machine sends to the network.
- No app or AI has any authority you didn't explicitly grant, and every action is logged and (where possible) reversible.
- You can recover your entire identity after losing every device, with no company or cloud ever holding your keys.

**Horizon cannot promise (and won't pretend):**
- Safety from a host that is actively malicious *while you use it* (keyloggers, firmware implants, a watched screen/RAM).
- Detection of a compromised host's firmware/TPM from inside a guest boot.
- That Tor or deniable encryption defeats a determined, well-resourced adversary.

That gap between the two lists is stated in the product itself. **Honesty is the security feature** (tenet #6).

-> Next: the AI layer, and how the Weave keeps it on a leash, [`05-AI-LAYER.md`](05-AI-LAYER.md).

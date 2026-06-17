# 03 · Portability & Boot

**The promise:** plug your Key into almost any laptop and boot your whole computer, state intact.
**The honest scope:** "almost any laptop" = **any x86-64 UEFI PC**. This document explains exactly how that works, where it breaks, and the hardware reality that shapes the whole project.

---

## 1. The central decision: build on the Linux kernel

Every broadly-compatible boot-from-USB OS in existence, Tails, Kali Live, NixOS live, Puppy, Slax, is **Linux-based**, and so is every "new" OS that achieved real hardware reach: Android, ChromeOS, SteamOS, Qubes, postmarketOS. There is a brutal reason.

- The Linux kernel is **~40 million lines, roughly 60% of which is device drivers** ([Tom's Hardware](https://www.tomshardware.com/software/linux/linux-kernel-source-expands-beyond-40-million-lines-it-has-doubled-in-size-in-a-decade)).
- Replicating that driver base from scratch is estimated at **thousands of person-years** ([Linux kernel cost study, D. Wheeler](https://dwheeler.com/essays/linux-kernel-cost.html)).
- The most advanced from-scratch OS, Rust's **Redox**, still has **no Wi-Fi/Bluetooth, Intel-GPU-only graphics, and no I2C touchpad support** after ~a decade ([Redox hardware support](https://doc.redox-os.org/book/hardware-support.html)).

So "boot on any laptop" and "from-scratch kernel" are mutually exclusive on any realistic timeline. **Horizon uses the Linux kernel as a commodity substrate** and makes its product the layers above it. (The GPLv2 user-space syscall exception lets us ship our own licensed userland on top, [kernel licensing](https://docs.kernel.org/process/license-rules.html).) The long-term seL4 path (see [`02-ARCHITECTURE.md`](02-ARCHITECTURE.md) §8) *keeps* Linux as a driver VM precisely so we never re-fight this war.

---

## 2. One image that boots almost anywhere

### 2.1 Firmware & boot mode

- **UEFI + legacy BIOS from one image** via the isohybrid/El Torito layout ([Syslinux isohybrid](https://wiki.syslinux.org/wiki/index.php?title=Isohybrid)). Modern machines take the UEFI path; old ones fall back to BIOS.
- **64-bit kernel with a 32-bit-UEFI shim** (`bootia32.efi`) to cover the awkward class of 64-bit CPUs that shipped 32-bit firmware (older Bay Trail tablets/netbooks).
- Boot chain: `shim -> systemd-boot (UEFI) or GRUB (when legacy BIOS is needed) -> kernel + generic initramfs`. systemd-boot is UEFI-only, so GRUB is retained for BIOS coverage.

### 2.2 Secure Boot (and a hard 2026 deadline)

Most laptops ship with Secure Boot **on**. Third-party OSes boot under it via a **Microsoft-signed `shim`** that then trusts the distro's own key; shim signing is gated by the community [shim-review board](https://github.com/rhboot/shim-review).

> Partial **Dated requirement:** Microsoft's 2011 signing CAs **expire in 2026**, the third-party UEFI CA that signs shim expires **2026-06-27**, and are replaced by 2023 CAs ([Microsoft: Secure Boot certificate expiration & CA updates](https://support.microsoft.com/en-us/topic/windows-secure-boot-certificate-expiration-and-ca-updates-7ff40d33-95dc-4c3c-8725-a9b95457578e)). Horizon's `shim` must be **dual-signed (2011 + 2023)** and track **SBAT** generation revocations ([shim SBAT.md](https://github.com/rhboot/shim/blob/main/SBAT.md)). This is wired into the build pipeline as a non-negotiable.

Fallback for locked or hostile firmware: instruct the user to disable Secure Boot for that Surface, or (Home Surface) enroll Horizon's key via MOK.

---

## 3. The driver problem, the real hard part

"Boot anywhere" lives or dies on drivers for hardware the image has never seen.

### 3.1 How Linux makes it work

Linux **auto-probes**: each device exposes a `modalias` string, udev matches it, and `modprobe` loads the right module; coldplug re-triggers everything at boot ([Arch: mkinitcpio](https://wiki.archlinux.org/title/Mkinitcpio)). This is the machinery that lets a single image adapt to unknown hardware, *provided the module and its firmware blob are present.* Therefore Horizon:

- ships a **generic, non-host-only initramfs** (dracut explicitly recommends generic images for portable/external installs, [dracut.conf](https://man7.org/linux/man-pages/man5/dracut.conf.5.html));
- bundles the **full `linux-firmware`** set;
- runs a **recent kernel** so new hardware is covered.

### 3.2 What still breaks (be honest)

Even done right, day-one-new hardware has rough edges:

- **GPUs:** newest AMD/Intel GPUs need very recent firmware; **NVIDIA** is the perennial pain (open modules are Turing+ only; Optimus + Secure Boot signing friction). ([NVIDIA open-gpu-kernel-modules](https://github.com/NVIDIA/open-gpu-kernel-modules))
- **Wi-Fi:** the latest chips (e.g., Wi-Fi 7 `mt7925`, Intel BE200) need very recent kernels.
- **Fingerprint readers, some webcams, vendor hotkeys:** hit-or-miss.
- **Suspend/resume:** modern s2idle can drain battery or misbehave on some laptops. ([Arch power management](https://wiki.archlinux.org/title/Power_management/Suspend_and_hibernate))

### 3.3 Horizon's answer: per-Surface profiles + honesty

- **First boot on a machine:** Horizon profiles the hardware and caches a tuned config, an automated version of NixOS's `hardware-configuration.nix`. **Next time on that Surface:** instant, optimized, no re-probing.
- **"Will it run here?" pre-flight:** a small tool reports expected compatibility *before* you commit to rebooting a machine.
- **Opt-in crowd-sourced Surface database:** anonymized "this exact model works / has quirk X," so real-world compatibility is mapped by the community.
- Profiles are cached only on **Home/Known** Surfaces (a Foreign Surface leaves no trace, see [`04`](04-SECURITY-AND-PRIVACY.md)).

---

## 4. Persistence: immutable base + encrypted writable overlay

Horizon's on-disk model (which is also the Lifestream's Tier-A; see [`02`](02-ARCHITECTURE.md) §3):

- **Read-only immutable base image**, verified by **dm-verity**. Tamper-evident, can't be corrupted by a runaway process, resets clean, and minimizes flash wear. (This is the OverlayFS "lower" layer; [kernel OverlayFS docs](https://www.kernel.org/doc/html/latest/filesystems/overlayfs.html).)
- **Writable layer** on top:
  - **Home/Known Surface:** a **LUKS2-encrypted** CoW filesystem (btrfs/bcachefs), your persistent state. (Tails' persistent storage is the reference design: LUKS2 + Argon2id, [Tails persistence](https://tails.net/contribute/design/persistence/).)
  - **Foreign Surface (Ghost mode):** the overlay is **tmpfs in RAM**, nothing is ever written to the Key or the host; on power-off it's gone.

The base image is itself a Lifestream **generation**, which is what makes updates atomic and rollback instant (an old generation is always bootable). The conceptual model, immutable, content-addressed, reproducible, atomically rolled-back, is the **NixOS/OSTree** family ([NixOS](https://nixos.wiki/wiki/NixOS), [rpm-ostree](https://coreos.github.io/rpm-ostree/)), which is *the* natural fit for "same everything, everywhere."

---

## 5. The hardware reality that reshapes "USB stick"

This is the most important honest correction in the whole project.

> **A cheap USB flash stick cannot run a real OS acceptably.** Even premium sticks do ~400+ MB/s *sequential* but only **single-digit to ~17 MB/s random 4K** read/write, and OS responsiveness is dominated by random I/O. They also wear out fast. (This is why Microsoft discontinued flash-based Windows To Go.)

The fix, and the actual definition of a **Horizon Key**:

- **An NVMe SSD in a quality USB-C enclosure with UASP** (not the older BOT protocol, UASP adds command queuing and ~30% throughput; [UASP vs BOT](https://www.electronicdesign.com/technologies/embedded/article/21800348/whats-the-difference-between-usb-uasp-and-bot)). Over USB it behaves like a real disk (hundreds of thousands of random-read IOPS measured over USB4, [enclosure review](https://www.techporn.ph/minisopuru-me808m-nvme-ssd-enclosure-review/)).
- **Interface tiers:** USB 3.2 Gen2 (10 Gbps) **minimum**; USB4 / Thunderbolt ideal (3,000-7,000 MB/s sequential ceilings, [USB4/TB5 speeds](https://www.owc.com/blog/can-usb4-v2-and-thunderbolt-5-enclosures-deliver-pcie-gen5-speeds)).
- **Enable TRIM**; use the immutable-base design to minimize writes and extend flash life.

A Horizon Key is still pocket-sized and looks like "a USB stick" to a user, it's just a *fast* one. We message it honestly as **"a real disk in your pocket,"** not a $5 thumb drive. (Recommended/optional first-party hardware is a moonshot in [`00`](00-IDEA-BOARD.md) §H.)

---

## 6. The boundaries of "any laptop"

| Target | Status | Why |
|---|---|---|
| **x86-64 UEFI PC** (most Windows/Linux laptops & desktops) | Yes **v1 target** | Shared image, isohybrid boot, Linux drivers, shim/Secure Boot path. |
| **64-bit CPU w/ 32-bit UEFI** (old netbooks/tablets) | Yes with `bootia32.efi` shim | Covered by the universal image. |
| **Legacy BIOS PCs** | Yes via GRUB/isohybrid | Older but supported. |
| **Apple Silicon Macs (M-series)** | No **post-v1, hard** | No UEFI; per-machine bootloader signed by Apple's Secure Enclave, installed *from macOS* (the Asahi model, [asahilinux.org](https://asahilinux.org/docs/platform/introduction/)). A generic stick fundamentally cannot boot one. Would require a separate, per-Mac install flow. |
| **Intel Macs** | Partial partial | Closer to PC UEFI but with quirks; case-by-case. |
| **ARM Windows laptops (Snapdragon X, etc.)** | Partial **separate, immature build** | UEFI exists but needs per-device device trees and Windows-extracted firmware; ARM ≠ x86, so it's a *different image* entirely. ([Linux on Snapdragon X](https://www.linaro.org/blog/linux-on-snapdragon-x-elite/)) |

**We will say this plainly in the product:** Horizon v1 is for x86-64 UEFI machines. Mac and ARM support are explicitly roadmap items, not silent gaps.

---

## 7. Updates (atomic, reversible, bandwidth-cheap)

Because the base is an immutable Lifestream generation:

- **Atomic image updates with one-generation rollback**, the previous version is always one reboot away (the ChromeOS/Android/Silverblue/NixOS model, [Android A/B](https://source.android.com/docs/core/ota/ab)). For a flash device we favor the **content-addressed/CoW-dedup** approach (OSTree/btrfs) over full A/B duplication to save space and writes.
- **Verified boot** via **dm-verity** ties the running base to a signed hash ([dm-verity](https://source.android.com/docs/security/features/verifiedboot/dm-verity)).
- **Delta downloads** so returning users fetch only the diff (**zsync** over plain HTTP, or **casync** content-addressed chunks, [casync](https://0pointer.net/blog/casync-a-tool-for-distributing-file-system-images.html)). Distribution & signing details in [`08-WEBSITE-AND-INSTALLER.md`](08-WEBSITE-AND-INSTALLER.md).

---

## 8. Summary

The Linux kernel is the **commodity** that makes boot-anywhere possible; Horizon's value is the encrypted-portable identity, the immutable/rewindable state, the capability fabric, and the AI, all the layers above. The honest scope (x86-64 UEFI, NVMe-in-enclosure, day-one-hardware caveats, Mac/ARM later) doesn't weaken the vision; it makes it *shippable*.

-> Next: how we protect all this on machines we don't trust, [`04-SECURITY-AND-PRIVACY.md`](04-SECURITY-AND-PRIVACY.md).

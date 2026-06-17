# 08 · Website & Installer

The goal: getting Horizon onto a Key should feel like magic, *go to a website, end up with a bootable encrypted computer in your pocket.* This document covers what's genuinely possible, the honest browser limitation, and the architecture that makes the experience as close to one-click as physics allows.

---

## 1. The honest limitation up front

> **A website cannot write a raw OS image to your USB drive.** This is a deliberate, structural browser security rule, not a gap we can engineer around.

- **WebUSB** is Chromium-only and its spec keeps a **"protected interface class" list that includes Mass Storage**, `claimInterface()` on a storage device throws `SecurityError`. The only override (`usb-unrestricted`) "can only be used with Isolated Web Apps," never a normal web page, and the OS has already mounted the drive, denying raw access anyway. ([WebUSB spec](https://wicg.github.io/webusb/), [unrestricted-USB explainer](https://github.com/WICG/webusb/blob/main/unrestricted-usb-explainer.md))
- **File System Access API** writes only into user-picked files inside a mounted filesystem; system/raw locations are blocked. ([File System Access](https://developer.chrome.com/docs/capabilities/web-apis/file-system-access))
- ESP Web Tools' "install from browser" only works because it streams firmware to a microcontroller over **Web Serial**, a byte stream to a serial bootloader, **not** a block-device write. Irrelevant to writing an OS image to a disk. ([ESP Web Tools](https://esphome.github.io/esp-web-tools/))

So "click a button on the website and it flashes your USB" is impossible **in the browser**. The honest, excellent version: **one click in a featherweight downloaded flasher.**

---

## 2. The experience we actually ship

A two-step flow that *feels* like one:

```
   ┌────────────────────────────────────────────────────────────┐
   │  horizon-os.org                                             │
   │  • detects your OS (Win/Mac/Linux)                          │
   │  • "Get Horizon" -> downloads a ~5-10 MB verified flasher    │
   │  • shows the signed checksum + how to verify                │
   └────────────────────────────────────────────────────────────┘
                         │  run the tiny flasher

   ┌────────────────────────────────────────────────────────────┐
   │  Horizon Flasher (Tauri app)                               │
   │  1. picks the latest signed image (or your chosen version) │
   │  2. downloads it (zstd, resumable, torrent-or-CDN)         │
   │  3. VERIFIES signature + checksum (refuses if tampered)    │
   │  4. shows ONLY removable disks (system drives filtered out)│
   │  5. writes the image to your Key                           │
   │  6. RE-VERIFIES the written bytes                          │
   │  7. optional: pre-seed your FIDO2 enrollment / passphrase  │
   └────────────────────────────────────────────────────────────┘
                         │
                            reboot into Horizon
```

This is the **Raspberry Pi Imager model**, download + verify + write + verify + pre-configure, which is the closest existing tool to Horizon's needs. ([rpi-imager](https://github.com/raspberrypi/rpi-imager))

---

## 3. The flasher: built in Tauri

**Tauri (Rust + the OS-native webview)** over Electron:

- **~5-10 MB** bundle vs Electron's ~80-150 MB, it must be a quick, trustworthy download. ([Tauri](https://tauri.app/))
- **Secure-by-default** and **Rust**, same language and security posture as the rest of Horizon.
- Cross-platform: Windows/macOS/Linux from one codebase.

**Privilege & safety:**
- Elevation per-OS: UAC (Windows), a privileged helper + Full Disk Access (macOS), polkit (Linux), following balenaEtcher's pattern of a separate elevated writer process. ([Etcher](https://en.wikipedia.org/wiki/Etcher_(software)))
- **Hard guardrail:** the flasher lists **only removable disks** and refuses anything that looks like a system/boot drive, you cannot accidentally overwrite your main drive. (This is the single most important safety property of any flasher.)
- A **power-user "prepare once" mode** (Ventoy-style): set up a Key once, then drop new image versions on without a full reflash. ([Ventoy](https://www.ventoy.net/en/doc_start.html))

---

## 4. Image distribution

- **Compression: zstd**, far faster decompress than xz at comparable ratios, near-constant decompress speed across levels (ideal for stream-while-downloading). Arch's xz->zstd switch gave ~13× faster decompress for +0.8% size. ([zstd](https://github.com/facebook/zstd), [Arch news](https://archlinux.org/news/now-using-zstandard-instead-of-xz-for-package-compression/))
- **Delta updates: zsync** (rolling checksums over plain HTTP, no special server) for simplicity, or **casync/desync** content-addressed chunks for dedup. Returning users fetch only the diff. ([casync](https://0pointer.net/blog/casync-a-tool-for-distributing-file-system-images.html))
- **Resilient delivery: official torrents + HTTPS CDN mirrors** with HTTP range-resume, the Debian/Ubuntu model, censorship-resistant and community-mirrorable. ([Debian torrents](https://www.debian.org/CD/torrent-cd/))

---

## 5. Integrity & authenticity (non-negotiable)

A privacy OS that ships unverifiable images is a contradiction. Every image is verifiable two ways:

- **Classic chain:** a **GPG-signed `SHA256SUMS`** file, the signature authenticates the checksum list, the checksum authenticates the image. ([Ubuntu verification](https://ubuntu.com/tutorials/how-to-verify-ubuntu))
- **Modern chain:** **Sigstore / cosign** keyless signing with a Rekor transparency log. ([Sigstore](https://docs.sigstore.dev/))
- The **flasher verifies automatically** and refuses tampered images; the **website shows the checksum and the verify command** for the paranoid (who are our core users, they *should* verify).
- For Secure Boot, ship the **Microsoft-signed, dual-signed (2011+2023) shim** (mind the June 2026 cert expiry). ([cert expiry](https://www.redhat.com/en/blog/expiration-secure-boot-signing-certificates-2026))

---

## 6. The website itself

- **One clear page:** what Horizon is, the honest scope (x86-64 UEFI, NVMe-Key recommendation), and a single "Get Horizon" action that detects the OS and serves the right flasher.
- **No account, no email, no telemetry to download** (tenet-level, a private OS you must sign up for is absurd).
- **"Will it run here?" pre-flight** and a hardware guide (what makes a good Key: NVMe + UASP + USB 3.2 Gen2/USB4).
- **Stretch: "Try Horizon in your browser"**, a WASM/VM demo of the desktop so people feel it before committing a drive.
- Built as a static, fast, privacy-respecting site (no third-party trackers, practice what we preach).

---

## 7. Summary

The dream of "flash from the browser" is blocked by (correct) browser security on raw block devices. The honest, polished reality is a **~5-10 MB Tauri flasher** that downloads, **verifies**, writes, and **re-verifies** a signed image, refusing to touch your system disks, distributed via zstd images, delta updates, and torrents+CDN, all cryptographically signed. It's one click in a tiny trusted tool, and it's the right way to do it.

-> Finally, the honest ledger of what's still hard and unresolved, [`09-OPEN-QUESTIONS.md`](09-OPEN-QUESTIONS.md).

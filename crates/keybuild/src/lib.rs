//! Keybuild: assemble the filesystems of a Horizon Key.
//!
//! A Horizon Key carries two filesystems (see `docs/03-PORTABILITY-AND-BOOT.md`): an
//! immutable base image, mounted read-only, holding the OS, and a persistent data
//! partition holding the writable overlay layer and the identity store. This crate is
//! the host-side tool that builds them. It is the producer side of the contract the
//! `init` crate consumes: keybuild writes the partition labels and emits the kernel
//! command line, and init finds the partitions by those labels and parses that command
//! line, so the two agree by sharing `init`'s types rather than by convention.
//!
//! [`build_base`] materializes a minimal base skeleton (the standard mount directories
//! and an os-release) and, when a spec names userland binaries, populates the real
//! userland: each binary at `/usr/bin/<name>` together with its shared-library closure
//! ([`ldd_closure`]) and an `ld.so.cache`, so the base actually runs a program. It then
//! packs the tree into a reproducible squashfs, so the same inputs yield byte-identical
//! bytes and the base can be verified by hash. The build shells out to `mksquashfs` (and
//! `ldd`/`ldconfig` when populating), doing no kernel work itself, so the crate builds
//! and the pure parts ([`parse_ldd`], the install-path mapping) test on every host;
//! only the tests that mount and run the result need a Linux kernel and are gated, run
//! for real in a privileged container. Kernel modules and firmware, the persistent data
//! partition, and the bootloader come next.

mod error;

pub use error::{Error, Result};

use std::path::{Path, PathBuf};
use std::process::Command;

pub use init::ModeChoice;
use init::{BASE_LABEL, DATA_LABEL};

/// The immutable base image's filename under a spec's output directory.
pub const BASE_IMAGE: &str = "base.squashfs";

/// The persistent data image's filename under a spec's output directory.
pub const DATA_IMAGE: &str = "data.img";

/// The parameters of a Key to build: where to write it, the partition labels and
/// filesystems init looks for, the default boot mode, and how the system names itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpec {
    /// The directory build artifacts are written into.
    pub out: PathBuf,
    /// The label written on the base partition and named on the boot command line.
    pub base_label: String,
    /// The label written on the data partition and named on the boot command line.
    pub data_label: String,
    pub basefs: String,
    pub datafs: String,
    /// The size of the data partition image, in mebibytes.
    pub data_size_mb: u64,
    /// The boot mode the command line requests (Auto picks Home or Ghost at boot).
    pub mode: ModeChoice,
    pub os_name: String,
    pub os_id: String,
    pub os_version: String,
    /// Host binaries to install into the base's `/usr/bin`, each with its shared-library
    /// closure. Empty builds a skeleton-only base (the reproducible default the pure
    /// tests use); naming `horizon` and `horizon-init` here is what makes the base boot.
    pub userland: Vec<PathBuf>,
}

impl KeySpec {
    /// A spec writing into `out` with Horizon's standard labels and filesystems, the
    /// ones `init`'s defaults look for, so a Key built this way boots with no explicit
    /// command line.
    pub fn new(out: impl Into<PathBuf>) -> KeySpec {
        KeySpec {
            out: out.into(),
            base_label: BASE_LABEL.to_string(),
            data_label: DATA_LABEL.to_string(),
            basefs: "squashfs".to_string(),
            datafs: "ext4".to_string(),
            data_size_mb: 64,
            mode: ModeChoice::Auto,
            os_name: "Horizon OS".to_string(),
            os_id: "horizon".to_string(),
            os_version: env!("CARGO_PKG_VERSION").to_string(),
            userland: Vec::new(),
        }
    }
}

/// The kernel command line a bootloader passes so `init` finds this Key's partitions.
/// It names the base and data by label, their filesystems, and the boot mode; the
/// `init` parser reads exactly these tokens back, so a build and a boot cannot drift.
pub fn boot_cmdline(spec: &KeySpec) -> String {
    let mode = match spec.mode {
        ModeChoice::Auto => "auto",
        ModeChoice::Home => "home",
        ModeChoice::Ghost => "ghost",
    };
    format!(
        "horizon.base=LABEL={} horizon.basefs={} horizon.data=LABEL={} horizon.datafs={} horizon.mode={}",
        spec.base_label, spec.basefs, spec.data_label, spec.datafs, mode
    )
}

/// The minimal contents of the immutable base: the standard mountpoint directories the
/// init moves the kernel filesystems onto, plus an os-release naming the system. Kept
/// pure (no filesystem touched) so it is asserted with no build tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skeleton {
    pub dirs: Vec<String>,
    pub os_release: String,
}

pub fn base_skeleton(spec: &KeySpec) -> Skeleton {
    let dirs = [
        "dev", "proc", "sys", "run", "tmp", "etc", "var", "usr", "usr/bin",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    Skeleton {
        dirs,
        os_release: os_release(spec),
    }
}

fn os_release(spec: &KeySpec) -> String {
    format!(
        "NAME=\"{name}\"\nID={id}\nVERSION=\"{ver}\"\nPRETTY_NAME=\"{name} {ver}\"\n",
        name = spec.os_name,
        id = spec.os_id,
        ver = spec.os_version
    )
}

/// Build the immutable base image: materialize the [`base_skeleton`] into a staging
/// tree and pack it into a reproducible squashfs at `<out>/base.squashfs`. The squashfs
/// is built root-owned, without xattrs, and with fixed timestamps, so the same skeleton
/// always yields byte-identical bytes. Returns the path to the image.
pub fn build_base(spec: &KeySpec) -> Result<PathBuf> {
    std::fs::create_dir_all(&spec.out)?;

    // A clean staging tree each time, so the input to mksquashfs is deterministic.
    let staging = spec.out.join("base.staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    materialize(&base_skeleton(spec), &staging)?;

    // Populate the real userland (the binaries plus their shared-library closure) when
    // the spec names any; an empty userland leaves the reproducible skeleton untouched.
    if !spec.userland.is_empty() {
        populate_userland(&staging, &spec.userland)?;
    }

    let out = spec.out.join(BASE_IMAGE);
    if out.exists() {
        std::fs::remove_file(&out)?;
    }

    let mut cmd = Command::new("mksquashfs");
    cmd.arg(&staging)
        .arg(&out)
        // -noappend overwrites; the rest pin uid/gid, xattrs, and every timestamp so the
        // bytes are reproducible.
        .args([
            "-noappend",
            "-all-root",
            "-no-xattrs",
            "-mkfs-time",
            "0",
            "-all-time",
            "0",
        ])
        .stdout(std::process::Stdio::null());
    run(cmd, "mksquashfs")?;

    let _ = std::fs::remove_dir_all(&staging);
    Ok(out)
}

/// Build the persistent data partition: a labeled ext4 image at `<out>/data.img`, sized
/// per the spec. This is the writable side of the Key: the init lays the overlay upper
/// and work directories and the identity store onto it, so unlike the base it is not
/// reproducible, it is mutable state. Shells out to `mkfs.ext4`. Returns the image path.
pub fn build_data(spec: &KeySpec) -> Result<PathBuf> {
    std::fs::create_dir_all(&spec.out)?;
    let out = spec.out.join(DATA_IMAGE);

    // A fresh file sized to the partition; mkfs.ext4 lays the filesystem into it.
    let file = std::fs::File::create(&out)?;
    file.set_len(spec.data_size_mb * 1024 * 1024)?;
    drop(file);

    let mut cmd = Command::new("mkfs.ext4");
    cmd.args(["-F", "-q", "-L"])
        .arg(&spec.data_label)
        .arg(&out)
        .stdout(std::process::Stdio::null());
    run(cmd, "mkfs.ext4")?;
    Ok(out)
}

fn materialize(skeleton: &Skeleton, staging: &Path) -> Result<()> {
    for d in &skeleton.dirs {
        std::fs::create_dir_all(staging.join(d))?;
    }
    std::fs::write(staging.join("etc/os-release"), &skeleton.os_release)?;
    Ok(())
}

/// Parse `ldd` stdout into the absolute paths of the shared objects a binary loads:
/// every `soname => /path` resolution and the bare `/path` interpreter line, dropping
/// the kernel's virtual DSO (linux-vdso / linux-gate) and any unresolved entry. The
/// trailing ` (0x...)` load address ldd prints is stripped, and duplicates are folded.
/// Pure text handling, so it is unit-tested with sample output on every host while the
/// [`ldd_closure`] call that produces the text is Linux-only.
pub fn parse_ldd(output: &str) -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("linux-vdso") || line.starts_with("linux-gate") {
            continue;
        }
        let path = if let Some((_, rhs)) = line.split_once("=>") {
            // "libc.so.6 => /lib/.../libc.so.6 (0x...)"; "=> not found" has no path.
            let rhs = strip_load_address(rhs.trim());
            if rhs.is_empty() || rhs == "not found" {
                continue;
            }
            rhs
        } else if line.starts_with('/') {
            // The interpreter line: "/lib/ld-linux-aarch64.so.1 (0x...)".
            strip_load_address(line)
        } else {
            // "statically linked", a soname header, a blank line: nothing to copy.
            continue;
        };
        let p = PathBuf::from(path);
        if !libs.contains(&p) {
            libs.push(p);
        }
    }
    libs
}

// Drop the trailing " (0x...)" load address ldd prints after a resolved path.
fn strip_load_address(s: &str) -> &str {
    match s.rfind(" (0x") {
        Some(i) => s[..i].trim_end(),
        None => s.trim(),
    }
}

/// The shared-library closure of a dynamically linked binary: every shared object it
/// transitively needs plus the ELF interpreter, as resolved absolute paths. Shells out
/// to `ldd`, whose output [`parse_ldd`] reads; a statically linked or non-ELF input has
/// an empty closure rather than an error. There is no `ldd` on a non-Linux host, so the
/// populate path that calls this runs in the build container.
pub fn ldd_closure(bin: &Path) -> Result<Vec<PathBuf>> {
    let mut cmd = Command::new("ldd");
    cmd.arg(bin);
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Error::Missing("ldd")),
        Err(e) => return Err(Error::Io(e)),
    };
    if !out.status.success() {
        // ldd exits nonzero for a static or non-dynamic ELF; that is an empty closure.
        let text = String::from_utf8_lossy(&out.stdout);
        let err = String::from_utf8_lossy(&out.stderr);
        if text.contains("not a dynamic executable") || err.contains("not a dynamic executable") {
            return Ok(Vec::new());
        }
        return Err(Error::Tool {
            name: "ldd",
            code: out.status.code(),
            stderr: err.trim().to_string(),
        });
    }
    Ok(parse_ldd(&String::from_utf8_lossy(&out.stdout)))
}

// Where a userland binary is installed inside the base: /usr/bin/<name> (relative to
// the base root), which is exactly where init's DEFAULT_INIT points, so installing the
// `horizon` binary here is what makes the pivot's exec target exist.
fn bin_install_path(bin: &Path) -> Result<PathBuf> {
    let name = bin
        .file_name()
        .ok_or_else(|| Error::NotAFile(bin.to_path_buf()))?;
    Ok(Path::new("usr/bin").join(name))
}

/// Install the userland into the staging tree: each binary at /usr/bin/<name>, the
/// transitive shared-library closure of all of them each at its own absolute path, and
/// an ld.so.cache so the loader resolves them. The closure is collected across every
/// binary and deduplicated, so a library shared by two binaries is copied once.
fn populate_userland(staging: &Path, bins: &[PathBuf]) -> Result<()> {
    let mut libs: Vec<PathBuf> = Vec::new();
    for bin in bins {
        copy_file(bin, &staging.join(bin_install_path(bin)?))?;
        for lib in ldd_closure(bin)? {
            if !libs.contains(&lib) {
                libs.push(lib);
            }
        }
    }
    for lib in &libs {
        // Strip the leading slash so /lib/.../libc.so.6 lands under the base root.
        let rel = lib.strip_prefix("/").unwrap_or(lib);
        copy_file(lib, &staging.join(rel))?;
    }
    build_ld_so_cache(staging)
}

// Copy one file into the base, creating parent directories as needed. fs::copy follows
// symlinks (a versioned .so behind its soname) and preserves the mode bits, so an
// executable or the loader stays executable; squashfs then pins ownership and
// timestamps, keeping the base reproducible.
fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

/// Build `/etc/ld.so.cache` inside the staging tree with `ldconfig -r`, so the dynamic
/// loader finds the copied libraries by soname the way it does on a normal system
/// rather than leaning on its compiled-in defaults. The cache is a deterministic
/// function of the libraries present, so the populated base stays reproducible.
fn build_ld_so_cache(staging: &Path) -> Result<()> {
    std::fs::create_dir_all(staging.join("etc"))?;
    let mut cmd = Command::new("ldconfig");
    cmd.arg("-r")
        .arg(staging)
        .stdout(std::process::Stdio::null());
    run(cmd, "ldconfig")
}

fn run(mut cmd: Command, name: &'static str) -> Result<()> {
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(Error::Tool {
            name,
            code: o.status.code(),
            stderr: String::from_utf8_lossy(&o.stderr).trim().to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::Missing(name)),
        Err(e) => Err(Error::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use init::{parse_cmdline, Spec};

    #[test]
    fn cmdline_round_trips_through_the_init_parser() {
        for mode in [ModeChoice::Auto, ModeChoice::Home, ModeChoice::Ghost] {
            let mut spec = KeySpec::new("/tmp/key");
            spec.mode = mode;
            let parsed = parse_cmdline(&boot_cmdline(&spec));
            assert_eq!(parsed.base, Spec::Label(spec.base_label.clone()));
            assert_eq!(parsed.data, Spec::Label(spec.data_label.clone()));
            assert_eq!(parsed.basefs, spec.basefs);
            assert_eq!(parsed.datafs, spec.datafs);
            assert_eq!(parsed.mode, mode);
        }
    }

    #[test]
    fn default_labels_are_the_ones_init_looks_for() {
        // keybuild's defaults write the labels init's own defaults search for, so a Key
        // built with no explicit command line still boots.
        let spec = KeySpec::new("/tmp/key");
        let init_default = parse_cmdline("");
        assert_eq!(init_default.base, Spec::Label(spec.base_label));
        assert_eq!(init_default.data, Spec::Label(spec.data_label));
    }

    #[test]
    fn skeleton_has_the_mount_dirs_and_names_the_system() {
        let spec = KeySpec::new("/tmp/key");
        let sk = base_skeleton(&spec);
        for d in ["dev", "proc", "sys", "run", "etc", "usr/bin"] {
            assert!(sk.dirs.iter().any(|x| x == d), "missing base dir {d}");
        }
        assert!(sk.os_release.contains("Horizon OS"));
        assert!(sk.os_release.contains("ID=horizon"));
    }

    #[test]
    fn parse_ldd_reads_resolved_libraries_and_the_interpreter() {
        // Real aarch64 ldd output: the kernel vdso, a resolved library, the interpreter.
        let out = "\tlinux-vdso.so.1 (0x0000ffff82c4f000)\n\
                   \tlibc.so.6 => /lib/aarch64-linux-gnu/libc.so.6 (0x0000ffff82a20000)\n\
                   \t/lib/ld-linux-aarch64.so.1 (0x0000ffff82c00000)\n";
        let libs = parse_ldd(out);
        assert_eq!(
            libs,
            vec![
                PathBuf::from("/lib/aarch64-linux-gnu/libc.so.6"),
                PathBuf::from("/lib/ld-linux-aarch64.so.1"),
            ]
        );
        // The kernel's virtual DSO is never a real file, so it is dropped.
        assert!(!libs.iter().any(|p| p.to_string_lossy().contains("vdso")));
    }

    #[test]
    fn parse_ldd_skips_unresolved_and_folds_duplicates() {
        // An x86-64 shape, a missing library, and the same soname listed twice.
        let out = "\tlibfoo.so.1 => not found\n\
                   \tlibm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x00007f00)\n\
                   \tlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f10)\n\
                   \tlibm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x00007f20)\n\
                   \t/lib64/ld-linux-x86-64.so.2 (0x00007f30)\n";
        assert_eq!(
            parse_ldd(out),
            vec![
                PathBuf::from("/lib/x86_64-linux-gnu/libm.so.6"),
                PathBuf::from("/lib/x86_64-linux-gnu/libc.so.6"),
                PathBuf::from("/lib64/ld-linux-x86-64.so.2"),
            ]
        );
    }

    #[test]
    fn parse_ldd_of_a_static_binary_is_empty() {
        assert!(parse_ldd("\tstatically linked\n").is_empty());
        assert!(parse_ldd("").is_empty());
    }

    #[test]
    fn a_binary_installs_where_init_execs_it() {
        // The horizon binary must land at exactly init's DEFAULT_INIT, so the pivot's
        // exec target exists in the base no matter what host path it was built at.
        let rel = bin_install_path(Path::new("target/release/horizon")).unwrap();
        assert_eq!(rel, PathBuf::from("usr/bin/horizon"));
        assert_eq!(Path::new("/").join(&rel), Path::new(init::DEFAULT_INIT));
        // A path with no filename is rejected rather than silently misplaced.
        assert!(bin_install_path(Path::new("/")).is_err());
    }
}

// Mounting the built base back, as the init's overlay lower, needs a Linux kernel, so
// it is proven for real where there is one: a privileged container packs a squashfs and
// stacks a writable overlay on it, the immutable-base + writable-overlay model the
// design turns on, now on the real image format the Key uses.
#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;
    use init::{execute, is_unprivileged_error, Layout, MountFlags, Plan, Source, Step};
    use std::process::Command;

    // Attach `file` to a free loop device (read-only for the immutable base, writable
    // for the data partition) and return its path, or None if losetup is not permitted.
    fn losetup(file: &Path, ro: bool) -> Option<String> {
        let mut cmd = Command::new("losetup");
        cmd.args(["--find", "--show"]);
        if ro {
            cmd.arg("-r");
        }
        let out = cmd.arg(file).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let dev = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!dev.is_empty()).then_some(dev)
    }

    fn losetup_d(dev: &str) {
        let _ = Command::new("losetup").arg("-d").arg(dev).output();
    }

    fn umount(p: &Path) {
        let _ = Command::new("umount").arg("-l").arg(p).output();
    }

    // Build a base, skipping if mksquashfs is not installed (CI) rather than failing.
    fn build_or_skip(out: &Path) -> Option<PathBuf> {
        match build_base(&KeySpec::new(out)) {
            Ok(p) => Some(p),
            Err(Error::Missing(_)) => {
                eprintln!("skipping: mksquashfs not installed");
                None
            }
            Err(e) => panic!("build base: {e}"),
        }
    }

    #[test]
    fn base_image_is_reproducible() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let Some(pa) = build_or_skip(a.path()) else {
            return;
        };
        let pb = build_or_skip(b.path()).unwrap();
        let bytes_a = std::fs::read(&pa).unwrap();
        let bytes_b = std::fs::read(&pb).unwrap();
        assert!(!bytes_a.is_empty());
        assert_eq!(
            bytes_a, bytes_b,
            "the immutable base must build byte-for-byte reproducibly"
        );
    }

    #[test]
    fn base_squashfs_mounts_read_only_as_the_overlay_lower() {
        let dir = tempfile::tempdir().unwrap();
        let Some(base) = build_or_skip(dir.path()) else {
            return;
        };

        // A read-only loop device over the squashfs file, the immutable base as the init
        // would see the Key's base partition.
        let Some(loopdev) = losetup(&base, true) else {
            eprintln!("skipping: losetup not permitted here");
            return;
        };

        // Assemble on a private tmpfs so nothing escapes and the lower is isolated.
        let scratch = dir.path().join("run");
        std::fs::create_dir_all(&scratch).unwrap();
        if let Err(e) = execute(&Plan {
            steps: vec![Step::Mount {
                source: Source::tmpfs(),
                target: scratch.clone(),
            }],
        }) {
            losetup_d(&loopdev);
            if is_unprivileged_error(&e) {
                eprintln!("skipping: mounting not permitted here ({e})");
                return;
            }
            panic!("mount tmpfs: {e}");
        }

        let l = Layout::new(&scratch);
        let steps = vec![
            Step::Mkdir(l.lower.clone()),
            Step::Mount {
                source: Source::new(loopdev.as_str(), "squashfs", MountFlags::default())
                    .read_only(),
                target: l.lower.clone(),
            },
            Step::Mkdir(l.over.clone()),
            Step::Mount {
                source: Source::tmpfs(),
                target: l.over.clone(),
            },
            Step::Mkdir(l.upper.clone()),
            Step::Mkdir(l.work.clone()),
            Step::Mkdir(l.root.clone()),
            Step::Overlay {
                lower: l.lower.clone(),
                upper: l.upper.clone(),
                work: l.work.clone(),
                target: l.root.clone(),
            },
        ];
        if let Err(e) = execute(&Plan { steps }) {
            umount(&scratch);
            losetup_d(&loopdev);
            if is_unprivileged_error(&e) {
                eprintln!("skipping: assembling not permitted here ({e})");
                return;
            }
            panic!("assemble: {e}");
        }

        // The immutable base shows through the overlay root.
        let osr = std::fs::read_to_string(l.root.join("etc/os-release")).unwrap();
        assert!(osr.contains("Horizon OS"));
        // A write to the root lands in the writable tmpfs upper.
        std::fs::write(l.root.join("state"), b"session").unwrap();
        assert!(l.upper.join("state").exists());
        // The squashfs lower is genuinely read-only: it cannot be written.
        assert!(
            std::fs::write(l.lower.join("nope"), b"x").is_err(),
            "the immutable base must be read-only"
        );

        umount(&l.root);
        umount(&l.over);
        umount(&l.lower);
        umount(&scratch);
        losetup_d(&loopdev);
    }

    // A base populated with a real userland actually runs it: build a base holding a
    // dynamic host binary and its ldd closure, mount the squashfs, and exec the binary
    // inside a chroot of the base. If any library or the loader were missing or
    // misplaced, the dynamic loader would fail, so this proves the closure is complete
    // and correctly placed on the real image, the part parse_ldd's unit tests cannot.
    #[test]
    fn a_populated_base_runs_its_userland_under_chroot() {
        let dir = tempfile::tempdir().unwrap();
        // A small, ubiquitous dynamic binary whose closure is just libc and the loader.
        let probe = Path::new("/bin/cat");
        if !probe.exists() {
            eprintln!("skipping: no /bin/cat to populate");
            return;
        }
        let mut spec = KeySpec::new(dir.path());
        spec.userland = vec![probe.to_path_buf()];
        let base = match build_base(&spec) {
            Ok(p) => p,
            Err(Error::Missing(t)) => {
                eprintln!("skipping: {t} not installed");
                return;
            }
            Err(e) => panic!("build populated base: {e}"),
        };

        let Some(loopdev) = losetup(&base, true) else {
            eprintln!("skipping: losetup not permitted here");
            return;
        };
        let mnt = dir.path().join("mnt");
        std::fs::create_dir_all(&mnt).unwrap();
        if let Err(e) = execute(&Plan {
            steps: vec![Step::Mount {
                source: Source::new(loopdev.as_str(), "squashfs", MountFlags::default())
                    .read_only(),
                target: mnt.clone(),
            }],
        }) {
            losetup_d(&loopdev);
            if is_unprivileged_error(&e) {
                eprintln!("skipping: mounting not permitted here ({e})");
                return;
            }
            panic!("mount base: {e}");
        }

        // The populated cat reads the skeleton's os-release from inside the chrooted
        // base: the binary, its libc, the loader, and the cache all have to resolve.
        let out = Command::new("chroot")
            .arg(&mnt)
            .args(["/usr/bin/cat", "/etc/os-release"])
            .output();
        umount(&mnt);
        losetup_d(&loopdev);
        let out = out.expect("spawn chroot");
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        if !out.status.success() {
            // chroot needs CAP_SYS_CHROOT; skip where it is not permitted (CI).
            if stderr.contains("Operation not permitted") || stderr.contains("superuser") {
                eprintln!("skipping: chroot not permitted here ({stderr})");
                return;
            }
            panic!("chroot run failed (code {:?}): {stderr}", out.status.code());
        }
        assert!(
            stdout.contains("Horizon OS"),
            "the populated cat must read the base os-release, got: {stdout:?}"
        );
    }

    // The keystone: a complete Key (a real squashfs base, a real ext4 data partition,
    // and an initialized identity store) assembles through the init's plan and horizon
    // boot opens the identity on it, all on the real filesystems keybuild produced. This
    // ties keybuild, init, and boot together end to end, short of the switch_root and
    // the on-screen session that need an actual boot.
    #[test]
    fn a_built_key_assembles_and_boot_opens_its_identity() {
        use boot::{boot as boot_device, derive, Method};
        use identity::{enroll, Keyslots, SoftwareAuthenticator};
        use lifestream::Lifestream;

        const PASS: &str = "correct horse battery staple";
        const SALT: &[u8] = b"horizon-keybuild-keystone-salt!!";
        const SEED: [u8; 32] = [7u8; 32];

        let dir = tempfile::tempdir().unwrap();
        let spec = KeySpec::new(dir.path());

        // Build both filesystems of the Key, skipping if a build tool is absent.
        let Some(base) = build_or_skip(dir.path()) else {
            return;
        };
        let data = match build_data(&spec) {
            Ok(p) => p,
            Err(Error::Missing(_)) => {
                eprintln!("skipping: mkfs.ext4 not installed");
                return;
            }
            Err(e) => panic!("build data: {e}"),
        };

        // The base read-only and the data writable, as the init sees the Key's two
        // partitions.
        let Some(base_loop) = losetup(&base, true) else {
            eprintln!("skipping: losetup not permitted here");
            return;
        };
        let Some(data_loop) = losetup(&data, false) else {
            losetup_d(&base_loop);
            eprintln!("skipping: losetup not permitted here");
            return;
        };

        let scratch = dir.path().join("run");
        std::fs::create_dir_all(&scratch).unwrap();
        let l = Layout::new(&scratch);
        let store = l.over.join("store");
        let booted_store = l.root.join("run/horizon/store");

        let cleanup = || {
            umount(&booted_store);
            umount(&l.root);
            umount(&l.over);
            umount(&l.lower);
            umount(&scratch);
            losetup_d(&data_loop);
            losetup_d(&base_loop);
        };

        // Assemble the writable layer over the immutable base, init's Home-mode plan: a
        // private tmpfs scratch, the squashfs base as the read-only lower, the ext4 data
        // as the writable backing for the overlay upper and work.
        let setup = Plan {
            steps: vec![
                Step::Mount {
                    source: Source::tmpfs(),
                    target: scratch.clone(),
                },
                Step::Mkdir(l.lower.clone()),
                Step::Mount {
                    source: Source::new(base_loop.as_str(), "squashfs", MountFlags::default())
                        .read_only(),
                    target: l.lower.clone(),
                },
                Step::Mkdir(l.over.clone()),
                Step::Mount {
                    source: Source::new(data_loop.as_str(), "ext4", MountFlags::default()),
                    target: l.over.clone(),
                },
            ],
        };
        if let Err(e) = execute(&setup) {
            cleanup();
            if is_unprivileged_error(&e) {
                eprintln!("skipping: mounting not permitted here ({e})");
                return;
            }
            panic!("mount Key: {e}");
        }

        // Initialize the identity store on the data partition, the way the boot crate's
        // own tests build one: a master derived from a passphrase and salt, a HEAD
        // generation to prove, and an enrolled software token (the touch-to-boot path).
        std::fs::create_dir_all(&store).unwrap();
        let master = derive(PASS, SALT);
        let ls = Lifestream::init(&store, &master).unwrap();
        std::fs::write(store.join("keysalt"), SALT).unwrap();
        let seed = dir.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        std::fs::write(seed.join("hello"), b"horizon").unwrap();
        let tree = ls.snapshot_dir(&seed).unwrap();
        ls.commit(tree, vec![], "first").unwrap();
        let mut auth = SoftwareAuthenticator::new(SEED);
        let mut slots = Keyslots::new();
        slots.add(enroll(&mut auth, &master).unwrap());
        std::fs::write(store.join("keyslots"), slots.encode()).unwrap();
        drop(ls);

        // Overlay the root and carry the store into it, exactly as init's Home-mode plan.
        let assemble = Plan {
            steps: vec![
                Step::Mkdir(l.upper.clone()),
                Step::Mkdir(l.work.clone()),
                Step::Mkdir(l.root.clone()),
                Step::Overlay {
                    lower: l.lower.clone(),
                    upper: l.upper.clone(),
                    work: l.work.clone(),
                    target: l.root.clone(),
                },
                Step::Mkdir(booted_store.clone()),
                Step::Bind {
                    from: store.clone(),
                    to: booted_store.clone(),
                },
            ],
        };
        if let Err(e) = execute(&assemble) {
            cleanup();
            panic!("assemble Key: {e}");
        }

        // The whole Key boots: boot finds the carried store, unlocks the master with the
        // enrolled token and no passphrase, and proves HEAD, on the real squashfs + ext4
        // filesystems keybuild produced.
        let mut token = SoftwareAuthenticator::new(SEED);
        let booted = boot_device(&booted_store, Some(&mut token), || {
            panic!("the passphrase must not be requested when the token unlocks")
        });
        let booted = match booted {
            Ok(b) => b,
            Err(e) => {
                cleanup();
                panic!("boot: {e}");
            }
        };
        assert_eq!(booted.method, Method::Keyslot);
        assert_eq!(booted.master, master);
        assert!(booted.head.is_some());
        // The immutable base is also visible through the assembled root.
        assert!(std::fs::read_to_string(l.root.join("etc/os-release"))
            .unwrap()
            .contains("Horizon OS"));

        cleanup();
    }
}

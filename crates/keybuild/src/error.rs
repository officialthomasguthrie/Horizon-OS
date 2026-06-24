use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// A userland binary path has no final component to install under /usr/bin.
    #[error("not a file: {0}")]
    NotAFile(PathBuf),
    /// A build tool ran but failed. The name and its stderr are what a build log needs.
    #[error("{name} failed (exit {code:?}): {stderr}")]
    Tool {
        name: &'static str,
        code: Option<i32>,
        stderr: String,
    },
    /// A build tool is not installed. Separate from a tool failure so a test can skip
    /// gracefully where the tool is absent (CI) while running for real where it is not.
    #[error("{0} is not installed")]
    Missing(&'static str),
    /// Requested modules that no file in the kernel's `modules.dep` matches. A base must
    /// not silently omit a driver it was told to carry, so an unresolved name fails.
    #[error("modules not found in the kernel's modules.dep: {}", .0.join(", "))]
    UnknownModules(Vec<String>),
    /// Modules were requested without naming the kernel version to harvest them from.
    #[error("a kernel version is required to install modules")]
    NoKernelVersion,
    /// A partition image the disk assembly needs has not been built yet.
    #[error("partition image not found (build it first): {0}")]
    NoImage(PathBuf),
    /// A file or directory name placed in the ESP is not usable as an 8.3 short name or a VFAT
    /// long name: an empty or reserved name, an illegal character, or past the length limit.
    #[error("not a usable FAT name: {0}")]
    BadName(String),
    /// A bootable ESP was asked for (a bootloader given) without a kernel, or vice versa: both
    /// are needed, alongside the built initramfs, to write a loadable EFI System Partition.
    #[error("a bootable ESP needs both a kernel and a bootloader")]
    IncompleteEsp,
    /// The bootloader given is not a PE/COFF EFI executable, so its machine type (which
    /// fixes the removable-media boot filename, BOOTX64.EFI vs BOOTAA64.EFI) cannot be read.
    #[error("not a PE/COFF EFI binary: {0}")]
    NotAnEfiBinary(PathBuf),
    /// The bootloader's PE machine type is not one UEFI defines a removable boot path for,
    /// so the right `/EFI/BOOT/BOOT*.EFI` name is unknown.
    #[error("unsupported EFI machine type {0:#06x}")]
    UnknownEfiMachine(u16),
    /// The ESP contents do not fit in the partition: more clusters are needed than it holds.
    #[error("ESP contents do not fit: need {needed} clusters, partition holds {available}")]
    EspFull { needed: u64, available: u64 },
    /// The ESP partition is too small to format as a valid FAT16/FAT32 filesystem.
    #[error("ESP is too small to format as FAT: {0} bytes")]
    EspTooSmall(u64),
    /// A cpio archive could not be built or parsed: a malformed path on write, or a
    /// corrupt header/field on read (the reader half that cross-checks the writer).
    #[error("cpio: {0}")]
    Cpio(&'static str),
    /// `build_initramfs` was called without naming the `/init` binary to install as PID 1.
    #[error("an init binary is required to build the initramfs")]
    NoInitBin,
    /// Provisioning could not derive the Home master from the store passphrase and salt
    /// (the path given is not a store, or its `keysalt` is unreadable).
    #[error("provision: {0}")]
    Provision(String),
}

pub type Result<T> = std::result::Result<T, Error>;

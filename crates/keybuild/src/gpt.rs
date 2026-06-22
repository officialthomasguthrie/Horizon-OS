//! GPT (GUID Partition Table) over a Horizon Key: the partition table that frames the
//! ESP, the immutable base, the plain data store, and the encrypted Home layer into one
//! bootable disk.
//!
//! keybuild builds each of a Key's filesystems as its own image (`base.squashfs`,
//! `data.img`, `home.img`); a real disk needs them laid side by side under one partition
//! table so firmware and the kernel find them. This module is the producer of that table.
//! It owns the GPT format rather than shelling out to `sgdisk`/`sfdisk` for the same
//! reasons the rest of keybuild owns its formats (see [`mod@crate::verity`]): the table is
//! a deterministic function of the partition sizes and the caller's GUIDs, so it builds
//! reproducibly and on any host, and the format is pure logic tested everywhere. The proof
//! it is byte-exact is a gated test that cross-checks the disk against `sgdisk`; the
//! firmware-side read is eye-verified by booting.
//!
//! The layout is the standard one: an LBA-0 protective MBR (one type-0xEE partition so a
//! legacy tool sees the disk as used, not empty), a primary GPT header at LBA 1 with its
//! 128-entry partition array at LBA 2, partitions aligned to 1 MiB, and a backup header at
//! the last LBA with its own array copy just before it. All multi-byte integers are
//! little-endian; GUIDs are stored mixed-endian (the first three fields little-endian, the
//! last two as written), the classic GPT footgun, pinned by a known-answer test.

use sha2::{Digest, Sha256};

/// The logical block (sector) size the table is expressed in. 512 is the universal GPT
/// LBA size; the images keybuild produces and the disks it targets all use it.
pub const SECTOR: u64 = 512;

/// Partition alignment, in sectors: 1 MiB, the modern default that keeps partitions off
/// awkward physical boundaries on flash and SSDs.
pub const ALIGN_SECTORS: u64 = 2048;

/// The number of partition entries the array holds (the GPT standard minimum, what every
/// tool expects), and the size of each entry in bytes.
pub const ENTRY_COUNT: u32 = 128;
pub const ENTRY_SIZE: u32 = 128;

/// The partition entry array's size in sectors: 128 * 128 / 512 = 32.
const ENTRY_ARRAY_SECTORS: u64 = (ENTRY_COUNT as u64 * ENTRY_SIZE as u64) / SECTOR;

/// The GPT header size the spec fixes and tools verify against, in bytes.
const HEADER_SIZE: u32 = 92;

/// The EFI System Partition type GUID: a FAT filesystem firmware loads the bootloader from.
pub const ESP_TYPE: &str = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B";

/// The generic Linux filesystem data type GUID. Horizon resolves its partitions by
/// filesystem label, so the base, data, and Home partitions all carry this neutral type
/// and are told apart by their labels and partition names, not by distinct type GUIDs.
pub const LINUX_FS_TYPE: &str = "0FC63DAF-8483-4772-8E79-3D69D8477DE4";

/// A 16-byte GUID in its on-disk byte order, the form a GPT header and partition entry
/// store. Parse from the canonical `8-4-4-4-12` text form, or derive one deterministically
/// so the table is reproducible.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Guid([u8; 16]);

impl Guid {
    /// Parse the canonical `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` text into on-disk bytes:
    /// the first three groups little-endian, the last two big-endian (stored as written).
    /// Returns `None` for anything not in that exact shape.
    pub fn parse(s: &str) -> Option<Guid> {
        let g: Vec<&str> = s.split('-').collect();
        if g.len() != 5 || [8, 4, 4, 4, 12].iter().zip(&g).any(|(l, p)| p.len() != *l) {
            return None;
        }
        let f1 = u32::from_str_radix(g[0], 16).ok()?;
        let f2 = u16::from_str_radix(g[1], 16).ok()?;
        let f3 = u16::from_str_radix(g[2], 16).ok()?;
        let f4 = u16::from_str_radix(g[3], 16).ok()?;
        let f5 = u64::from_str_radix(g[4], 16).ok()?;
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&f1.to_le_bytes());
        b[4..6].copy_from_slice(&f2.to_le_bytes());
        b[6..8].copy_from_slice(&f3.to_le_bytes());
        b[8..10].copy_from_slice(&f4.to_be_bytes());
        b[10..16].copy_from_slice(&f5.to_be_bytes()[2..8]);
        Some(Guid(b))
    }

    /// A GUID derived deterministically from a seed, so a Key's disk and partition GUIDs are
    /// stable across builds (the table is reproducible). The bytes are the first 16 of a
    /// domain-separated SHA-256; the RFC 4122 version (4) and variant bits are set so the
    /// result reads as a normal random GUID to any tool that inspects it.
    pub fn derive(seed: &str) -> Guid {
        let mut h = Sha256::new();
        h.update(b"horizon-gpt:");
        h.update(seed.as_bytes());
        let d = h.finalize();
        let mut b = [0u8; 16];
        b.copy_from_slice(&d[..16]);
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        Guid(b)
    }

    pub fn bytes(&self) -> [u8; 16] {
        self.0
    }
}

/// A partition to place on the disk: its type and unique GUIDs, its name (the GPT
/// PARTLABEL, up to 36 UTF-16 code units), and the size in bytes of the image that fills
/// it. The size is rounded up to a whole sector; the partition is placed at the next
/// 1 MiB boundary.
#[derive(Debug, Clone)]
pub struct PartSpec {
    pub type_guid: Guid,
    pub unique_guid: Guid,
    pub name: String,
    pub size: u64,
}

/// Where a partition landed on the disk: its first and last LBA and the byte offset and
/// length of the region to copy its image into. `size` is sector-rounded, so it is at
/// least the requested size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub first_lba: u64,
    pub last_lba: u64,
    pub offset: u64,
    pub size: u64,
}

/// A built partition table: the total disk size, where each partition landed, and the
/// table bytes to stamp onto the disk. `front` is the first 34 sectors (protective MBR,
/// primary header, primary entry array); `back` is the 33 trailing sectors (the backup
/// entry array and backup header) to write at `back_offset`.
#[derive(Debug, Clone)]
pub struct Disk {
    pub total_bytes: u64,
    pub parts: Vec<Placed>,
    pub front: Vec<u8>,
    pub back: Vec<u8>,
    pub back_offset: u64,
}

fn align_up(x: u64, a: u64) -> u64 {
    x.div_ceil(a) * a
}

/// Build the GPT for a disk holding `parts` in order, identified by `disk_guid`. Pure: it
/// computes the layout and returns the table bytes, touching no filesystem. The caller
/// sizes the disk to [`Disk::total_bytes`], copies each partition image to its [`Placed`]
/// offset, and writes `front` at offset 0 and `back` at `back_offset`.
pub fn build(disk_guid: Guid, parts: &[PartSpec]) -> Disk {
    let entries_lba = 2u64;
    let first_usable = entries_lba + ENTRY_ARRAY_SECTORS;

    // Place each partition at the next 1 MiB boundary, sized up to a whole alignment unit so
    // partitions both start and end aligned and sit contiguously (no gaps, and no tool
    // caution about an unaligned end). The image fills the start of its region; the slack to
    // the boundary stays zero.
    let mut placed = Vec::with_capacity(parts.len());
    let mut cursor = align_up(first_usable, ALIGN_SECTORS);
    for p in parts {
        let sectors = align_up(p.size.div_ceil(SECTOR).max(1), ALIGN_SECTORS);
        let first = cursor;
        let last = first + sectors - 1;
        placed.push(Placed {
            first_lba: first,
            last_lba: last,
            offset: first * SECTOR,
            size: sectors * SECTOR,
        });
        cursor = last + 1;
    }

    // The backup array and header occupy the last 33 sectors; everything before them up to
    // the last partition is usable.
    let backup_entries_lba = cursor;
    let total_sectors = backup_entries_lba + ENTRY_ARRAY_SECTORS + 1;
    let backup_header_lba = total_sectors - 1;
    let last_usable = backup_entries_lba - 1;

    let entry_array = build_entries(parts, &placed);
    let entry_crc = crc32(&entry_array);

    let primary_header = header(&HeaderArgs {
        my_lba: 1,
        alt_lba: backup_header_lba,
        first_usable,
        last_usable,
        disk_guid,
        entries_lba,
        entry_crc,
    });
    let backup_header = header(&HeaderArgs {
        my_lba: backup_header_lba,
        alt_lba: 1,
        first_usable,
        last_usable,
        disk_guid,
        entries_lba: backup_entries_lba,
        entry_crc,
    });

    // Front: LBA 0 protective MBR, LBA 1 primary header, LBA 2.. primary entry array.
    let mut front = vec![0u8; (first_usable * SECTOR) as usize];
    write_pmbr(&mut front[..SECTOR as usize], total_sectors);
    front[SECTOR as usize..(SECTOR * 2) as usize].copy_from_slice(&primary_header);
    front[(SECTOR * 2) as usize..].copy_from_slice(&entry_array);

    // Back: the entry array again, then the backup header in the very last sector.
    let mut back = vec![0u8; ((ENTRY_ARRAY_SECTORS + 1) * SECTOR) as usize];
    back[..entry_array.len()].copy_from_slice(&entry_array);
    back[(ENTRY_ARRAY_SECTORS * SECTOR) as usize..].copy_from_slice(&backup_header);

    Disk {
        total_bytes: total_sectors * SECTOR,
        parts: placed,
        front,
        back,
        back_offset: backup_entries_lba * SECTOR,
    }
}

struct HeaderArgs {
    my_lba: u64,
    alt_lba: u64,
    first_usable: u64,
    last_usable: u64,
    disk_guid: Guid,
    entries_lba: u64,
    entry_crc: u32,
}

/// One 512-byte sector holding a GPT header in its first 92 bytes. The header CRC is
/// computed over those 92 bytes with the CRC field itself zeroed, the spec's rule.
fn header(a: &HeaderArgs) -> [u8; SECTOR as usize] {
    let mut s = [0u8; SECTOR as usize];
    s[0..8].copy_from_slice(b"EFI PART");
    s[8..12].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]); // revision 1.0
    s[12..16].copy_from_slice(&HEADER_SIZE.to_le_bytes());
    // 16..20 header CRC, filled last.
    s[24..32].copy_from_slice(&a.my_lba.to_le_bytes());
    s[32..40].copy_from_slice(&a.alt_lba.to_le_bytes());
    s[40..48].copy_from_slice(&a.first_usable.to_le_bytes());
    s[48..56].copy_from_slice(&a.last_usable.to_le_bytes());
    s[56..72].copy_from_slice(&a.disk_guid.bytes());
    s[72..80].copy_from_slice(&a.entries_lba.to_le_bytes());
    s[80..84].copy_from_slice(&ENTRY_COUNT.to_le_bytes());
    s[84..88].copy_from_slice(&ENTRY_SIZE.to_le_bytes());
    s[88..92].copy_from_slice(&a.entry_crc.to_le_bytes());
    let crc = crc32(&s[..HEADER_SIZE as usize]);
    s[16..20].copy_from_slice(&crc.to_le_bytes());
    s
}

/// The full 128 * 128-byte partition entry array: one entry per partition in order, the
/// rest left zero (an unused entry is all zeros, which a zero type GUID marks).
fn build_entries(parts: &[PartSpec], placed: &[Placed]) -> Vec<u8> {
    let mut a = vec![0u8; (ENTRY_COUNT * ENTRY_SIZE) as usize];
    for (i, (p, pl)) in parts.iter().zip(placed).enumerate() {
        let e = &mut a[i * ENTRY_SIZE as usize..(i + 1) * ENTRY_SIZE as usize];
        e[0..16].copy_from_slice(&p.type_guid.bytes());
        e[16..32].copy_from_slice(&p.unique_guid.bytes());
        e[32..40].copy_from_slice(&pl.first_lba.to_le_bytes());
        e[40..48].copy_from_slice(&pl.last_lba.to_le_bytes());
        // 48..56 attribute flags, left zero.
        for (j, u) in p.name.encode_utf16().take(36).enumerate() {
            e[56 + j * 2..56 + j * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
    }
    a
}

/// Write the protective MBR into LBA 0: one partition record of type 0xEE spanning the
/// whole disk (clamped to the 32-bit LBA field), and the 0x55AA boot signature.
fn write_pmbr(s: &mut [u8], total_sectors: u64) {
    let span = (total_sectors - 1).min(0xFFFF_FFFF) as u32;
    let rec = &mut s[446..462];
    rec[1..4].copy_from_slice(&[0x00, 0x02, 0x00]); // start CHS (legacy, ignored)
    rec[4] = 0xEE; // type: GPT protective
    rec[5..8].copy_from_slice(&[0xFF, 0xFF, 0xFF]); // end CHS (legacy, ignored)
    rec[8..12].copy_from_slice(&1u32.to_le_bytes()); // first LBA
    rec[12..16].copy_from_slice(&span.to_le_bytes()); // sector count
    s[510] = 0x55;
    s[511] = 0xAA;
}

/// CRC-32 (IEEE 802.3, the reflected polynomial GPT uses for both the header and the entry
/// array). Computed directly so the crate takes no CRC dependency; pinned by a known-answer
/// test.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let m = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & m);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_answer() {
        // The canonical CRC-32 check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn guid_parses_to_on_disk_mixed_endian() {
        // The ESP type GUID's documented on-disk byte order: first three fields
        // little-endian, last two as written.
        let g = Guid::parse(ESP_TYPE).unwrap();
        assert_eq!(
            g.bytes(),
            [
                0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E,
                0xC9, 0x3B
            ]
        );
    }

    #[test]
    fn guid_parse_rejects_malformed() {
        assert!(Guid::parse("not-a-guid").is_none());
        assert!(Guid::parse("C12A7328-F81F-11D2-BA4B").is_none());
        assert!(Guid::parse(&ESP_TYPE.replace('C', "G")).is_none());
    }

    #[test]
    fn derive_is_deterministic_and_distinct() {
        assert_eq!(Guid::derive("HORIZON-BASE"), Guid::derive("HORIZON-BASE"));
        assert_ne!(Guid::derive("HORIZON-BASE"), Guid::derive("HORIZON-DATA"));
        // The derived GUID carries the version-4 nibble and the variant bits.
        let b = Guid::derive("HORIZON-BASE").bytes();
        assert_eq!(b[6] & 0xf0, 0x40);
        assert_eq!(b[8] & 0xc0, 0x80);
    }

    fn sample() -> Disk {
        let parts = vec![
            PartSpec {
                type_guid: Guid::parse(ESP_TYPE).unwrap(),
                unique_guid: Guid::derive("esp"),
                name: "HORIZON-ESP".into(),
                size: 3 * 1024 * 1024,
            },
            PartSpec {
                type_guid: Guid::parse(LINUX_FS_TYPE).unwrap(),
                unique_guid: Guid::derive("base"),
                name: "HORIZON-BASE".into(),
                // Deliberately not a sector multiple, to exercise rounding.
                size: 5 * 1024 * 1024 + 17,
            },
        ];
        build(Guid::derive("disk"), &parts)
    }

    #[test]
    fn partitions_are_mib_aligned_and_contiguous() {
        let d = sample();
        assert_eq!(d.parts.len(), 2);
        // First partition at 1 MiB.
        assert_eq!(d.parts[0].first_lba, ALIGN_SECTORS);
        let align = ALIGN_SECTORS * SECTOR;
        let wants = [3 * 1024 * 1024u64, 5 * 1024 * 1024 + 17];
        for (p, want) in d.parts.iter().zip(wants) {
            // Each partition starts and ends on a 1 MiB boundary and holds at least its
            // image, with less than one alignment unit of slack.
            assert_eq!(p.first_lba % ALIGN_SECTORS, 0);
            assert_eq!((p.last_lba + 1) % ALIGN_SECTORS, 0);
            assert!(p.size >= want);
            assert!(p.size - want < align);
        }
        // Contiguous: the second begins right where the first ends.
        assert_eq!(d.parts[1].first_lba, d.parts[0].last_lba + 1);
    }

    #[test]
    fn header_fields_and_crcs_are_valid() {
        let d = sample();
        let sec = SECTOR as usize;
        // Protective MBR.
        assert_eq!(d.front[446 + 4], 0xEE);
        assert_eq!([d.front[510], d.front[511]], [0x55, 0xAA]);

        // Primary header at LBA 1.
        let h = &d.front[sec..2 * sec];
        assert_eq!(&h[0..8], b"EFI PART");
        assert_eq!(
            u32::from_le_bytes(h[12..16].try_into().unwrap()),
            HEADER_SIZE
        );
        assert_eq!(u64::from_le_bytes(h[24..32].try_into().unwrap()), 1);
        // Header CRC verifies (recompute with the CRC field zeroed).
        let stored = u32::from_le_bytes(h[16..20].try_into().unwrap());
        let mut z = h[..HEADER_SIZE as usize].to_vec();
        z[16..20].fill(0);
        assert_eq!(crc32(&z), stored);

        // Entry array CRC in the header matches the array bytes in `front`.
        let array = &d.front[2 * sec..2 * sec + (ENTRY_COUNT * ENTRY_SIZE) as usize];
        assert_eq!(
            u32::from_le_bytes(h[88..92].try_into().unwrap()),
            crc32(array)
        );
    }

    #[test]
    fn backup_header_mirrors_the_primary() {
        let d = sample();
        let sec = SECTOR as usize;
        let total = d.total_bytes / SECTOR;
        let primary = &d.front[sec..2 * sec];
        // The backup header sits in the last sector of `back`.
        let bh = &d.back[(ENTRY_ARRAY_SECTORS * SECTOR) as usize..];
        assert_eq!(&bh[0..8], b"EFI PART");
        // my/alternate LBA are swapped between the two headers.
        assert_eq!(
            u64::from_le_bytes(bh[24..32].try_into().unwrap()),
            total - 1
        );
        assert_eq!(u64::from_le_bytes(bh[32..40].try_into().unwrap()), 1);
        assert_eq!(
            u64::from_le_bytes(primary[32..40].try_into().unwrap()),
            total - 1
        );
        // Same disk GUID and entry-array CRC in both.
        assert_eq!(primary[56..72], bh[56..72]);
        assert_eq!(primary[88..92], bh[88..92]);
        // The backup is written so its last sector lands in the disk's last sector.
        assert_eq!(d.back_offset + d.back.len() as u64, d.total_bytes);
    }

    #[test]
    fn first_partition_entry_carries_type_and_name() {
        let d = sample();
        let sec = SECTOR as usize;
        let e = &d.front[2 * sec..2 * sec + ENTRY_SIZE as usize];
        assert_eq!(&e[0..16], &Guid::parse(ESP_TYPE).unwrap().bytes());
        assert_eq!(
            u64::from_le_bytes(e[32..40].try_into().unwrap()),
            d.parts[0].first_lba
        );
        // The name is UTF-16LE: "HORIZON-ESP".
        let name: String = (0..11)
            .map(|i| u16::from_le_bytes(e[56 + i * 2..58 + i * 2].try_into().unwrap()))
            .filter_map(|u| char::from_u32(u as u32))
            .collect();
        assert_eq!(name, "HORIZON-ESP");
    }

    #[test]
    fn table_is_reproducible() {
        // Same inputs, byte-identical table (derived GUIDs are deterministic).
        let a = sample();
        let b = sample();
        assert_eq!(a.front, b.front);
        assert_eq!(a.back, b.back);
        assert_eq!(a.total_bytes, b.total_bytes);
    }
}

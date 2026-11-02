//! A master boot record: the first sector of a disk, with up to four primary
//! partitions (the classic MBR layout: entries at byte 446, the `0x55 0xaa`
//! signature at 510).

use anyhow::{Result, bail};

/// Bytes of a sector.
pub const SECTOR_SIZE: usize = 512;

/// One partition table entry, in sectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub bootable: bool,
    /// The partition type byte
    pub kind: u8,
    pub first_sector: u32,
    pub sectors: u32,
}

/// Cylinder, head and sector of `lba` in the usual 255-head, 63-sector
/// geometry, packed as an entry holds them; past cylinder 1023 the largest
/// value, which tells readers to use the LBA fields (firmware does anyway).
#[must_use]
pub const fn chs(lba: u32) -> [u8; 3] {
    const HEADS: u32 = 255;
    const SECTORS: u32 = 63;
    let cylinder = lba / (HEADS * SECTORS);
    if cylinder > 1023 {
        return [0xfe, 0xff, 0xff];
    }
    let head = (lba / SECTORS) % HEADS;
    let sector = lba % SECTORS + 1;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "each part is masked to its field"
    )]
    [
        head as u8,
        (sector as u8 & 0x3f) | (((cylinder >> 8) as u8 & 0x03) << 6),
        cylinder as u8,
    ]
}

/// The boot sector holding `entries` (no boot code), with the disk
/// identifier `signature` at byte 440.
///
/// # Errors
///
/// More than four entries, an empty one, or two that overlap.
pub fn boot_sector(entries: &[Entry], signature: u32) -> Result<[u8; SECTOR_SIZE]> {
    if entries.len() > 4 {
        bail!("an MBR holds at most 4 partitions, not {}", entries.len());
    }
    for (index, entry) in entries.iter().enumerate() {
        if entry.sectors == 0 || entry.first_sector == 0 {
            bail!(
                "partition {} is empty or overlaps the boot sector",
                index + 1
            );
        }
        let end = |entry: &Entry| u64::from(entry.first_sector) + u64::from(entry.sectors);
        if entries[..index].iter().any(|other| {
            u64::from(entry.first_sector) < end(other) && u64::from(other.first_sector) < end(entry)
        }) {
            bail!("partition {} overlaps another", index + 1);
        }
    }
    let mut sector = [0_u8; SECTOR_SIZE];
    sector[440..444].copy_from_slice(&signature.to_le_bytes());
    for (record, entry) in sector[446..510]
        .as_chunks_mut::<16>()
        .0
        .iter_mut()
        .zip(entries)
    {
        let Some(last) = entry.first_sector.checked_add(entry.sectors - 1) else {
            bail!("a partition extends beyond the MBR LBA range");
        };
        // Status, first CHS, type, last CHS, first sector, sector count.
        let bytes = [if entry.bootable { 0x80 } else { 0 }]
            .into_iter()
            .chain(chs(entry.first_sector))
            .chain([entry.kind])
            .chain(chs(last))
            .chain(entry.first_sector.to_le_bytes())
            .chain(entry.sectors.to_le_bytes());
        for (slot, byte) in record.iter_mut().zip(bytes) {
            *slot = byte;
        }
    }
    sector[510] = 0x55;
    sector[511] = 0xaa;
    Ok(sector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chs_of_the_usual_geometry() {
        // LBA 0 is cylinder 0, head 0, sector 1.
        assert_eq!(chs(0), [0, 1, 0]);
        // 1 MiB in: LBA 2048 is cylinder 0, head 32, sector 33.
        assert_eq!(chs(2048), [32, 33, 0]);
        // The first sector of cylinder 1.
        assert_eq!(chs(255 * 63), [0, 1, 1]);
        // Cylinder 256 sets the high bits in the sector byte.
        assert_eq!(chs(256 * 255 * 63), [0, 0x41, 0]);
        assert_eq!(chs(u32::MAX), [0xfe, 0xff, 0xff]);
    }

    #[test]
    fn the_card_boot_sector() {
        // A bootable FAT32 partition of 128 MiB at 1 MiB, as on the card.
        let sector = boot_sector(
            &[Entry {
                bootable: true,
                kind: 0x0c,
                first_sector: 2048,
                sectors: 262_144,
            }],
            0x1234_5678,
        )
        .unwrap();
        assert_eq!(&sector[440..444], &[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(
            &sector[446..462],
            &[
                0x80, 32, 33, 0, // bootable, CHS of LBA 2048
                0x0c, 113, 33, 16, // type, CHS of LBA 264191
                0x00, 0x08, 0, 0, // first sector 2048
                0x00, 0x00, 0x04, 0, // 262144 sectors
            ]
        );
        assert!(sector[462..510].iter().all(|byte| *byte == 0));
        assert_eq!(&sector[510..], &[0x55, 0xaa]);
    }

    #[test]
    fn bad_tables_are_refused() {
        let entry = |first_sector, sectors| Entry {
            bootable: false,
            kind: 0x83,
            first_sector,
            sectors,
        };
        boot_sector(&[entry(2048, 0)], 0).unwrap_err();
        boot_sector(&[entry(0, 10)], 0).unwrap_err();
        boot_sector(&[entry(2048, 100), entry(2100, 100)], 0).unwrap_err();
        boot_sector(&[entry(1, 1); 5], 0).unwrap_err();
        boot_sector(&[entry(2048, 100), entry(2148, 100)], 0).unwrap();
    }
}

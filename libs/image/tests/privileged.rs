//! Host tests of privileged-image loading: layout, zeroing and relocation.

#[cfg(test)]
mod tests {
    use vesper_image::{
        LoadError, LoadableSection, PAGE_BYTES, Permissions, PrivilegedImage, Relocation,
        SectionMeta,
    };

    const TEXT: [u8; 4] = [0xAA, 0xBB, 0xCC, 0xDD];
    /// File-backed part of `.data`: an 8-byte word the relocation rewrites,
    /// then a marker byte.
    const DATA: [u8; 9] = [0x11; 9];

    fn segment(offset: u64, size: usize, writable: bool, data: &'static [u8]) -> LoadableSection {
        LoadableSection {
            meta: SectionMeta {
                name: "segment",
                virt_addr: offset,
                size,
                alignment: 4096,
                permissions: Permissions {
                    readable: true,
                    writable,
                    executable: !writable,
                },
            },
            data,
        }
    }

    fn image(relocations: &'static [Relocation]) -> PrivilegedImage {
        // Leak the segment table: images are 'static in the bundling kernel.
        let segments = Box::leak(Box::new([
            segment(0, TEXT.len(), false, &TEXT),
            // 0x40 bytes in memory, 9 from the file: the rest is `.bss`.
            segment(0x1000, 0x40, true, &DATA),
        ]));
        PrivilegedImage {
            name: "test",
            compatible: &["vendor,root", "vendor,cascade"],
            segments,
            relocations,
            ops_offset: 0x1000,
        }
    }

    #[test]
    fn total_size_covers_every_segment_in_whole_pages() {
        assert_eq!(image(&[]).total_size(), 2 * PAGE_BYTES);
    }

    #[test]
    fn load_copies_zeroes_and_relocates() {
        const LOAD_BASE: u64 = 0xFFFF_8080_0000_0000;
        let image = image(&[Relocation {
            offset: 0x1000,
            addend: 0x24,
        }]);
        let mut memory = vec![0xEE_u8; image.total_size()];
        image.load_into(&mut memory, LOAD_BASE).expect("fits");

        assert_eq!(memory[..4], TEXT);
        // Stale bytes after the file-backed part of a segment are zeroed.
        assert!(memory[4..0x1000].iter().all(|&byte| byte == 0));
        let word = u64::from_le_bytes(memory[0x1000..0x1008].try_into().expect("8 bytes"));
        assert_eq!(word, LOAD_BASE + 0x24);
        assert_eq!(memory[0x1008], 0x11);
        assert!(memory[0x1009..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn load_rejects_a_small_destination_and_stray_relocations() {
        let image = image(&[]);
        let mut small = vec![0_u8; PAGE_BYTES];
        assert_eq!(image.load_into(&mut small, 0), Err(LoadError::TooSmall));

        let stray = self::image(&[Relocation {
            offset: 0x1FFC,
            addend: 0,
        }]);
        let mut memory = vec![0_u8; stray.total_size()];
        assert_eq!(
            stray.load_into(&mut memory, 0),
            Err(LoadError::BadRelocation)
        );
    }

    #[test]
    fn kind_is_the_position_in_the_compatible_list() {
        let image = image(&[]);
        assert_eq!(image.kind_of("vendor,root"), Some(0));
        assert_eq!(image.kind_of("vendor,cascade"), Some(1));
        assert_eq!(image.kind_of("vendor,other"), None);
    }
}

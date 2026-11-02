//! A FAT volume holding a directory tree (`fatfs`): every file and directory
//! under the root, in name order, dated 1980-01-01 so the same tree makes the
//! same image. `fatfs` picks FAT12, FAT16 or FAT32 by the volume's size unless
//! the config names one.

use {
    anyhow::{Context, Result, bail},
    fatfs::{Dir, FatType, FileSystem, FormatVolumeOptions, FsOptions, ReadWriteSeek},
    std::{
        fs,
        io::{self, SeekFrom},
        path::Path,
    },
};

/// The 11 bytes of a volume label: `label` in upper case, padded with spaces.
#[must_use]
pub fn volume_label(label: &str) -> [u8; 11] {
    let mut bytes = [b' '; 11];
    for (byte, character) in bytes.iter_mut().zip(label.bytes()) {
        *byte = character.to_ascii_uppercase();
    }
    bytes
}

/// Format `disk` (as large as the volume) as FAT labelled `label`, FAT12, FAT16
/// or FAT32 as `fat_type` says (by its size when `None`), and copy everything
/// under `root` into it.
///
/// # Errors
///
/// A volume too small for its FAT type, or a file that cannot be read or does
/// not fit.
pub fn build<T: ReadWriteSeek>(
    mut disk: T,
    label: &str,
    fat_type: Option<u8>,
    root: &Path,
) -> Result<()> {
    let mut options = FormatVolumeOptions::new().volume_label(volume_label(label));
    let wanted = match fat_type {
        None => None,
        Some(12) => Some(FatType::Fat12),
        Some(16) => Some(FatType::Fat16),
        Some(32) => Some(FatType::Fat32),
        Some(other) => bail!("FAT{other} is not a FAT type"),
    };
    if let Some(wanted) = wanted {
        options = options.fat_type(wanted);
    }
    fatfs::format_volume(&mut disk, options).context("formatting the FAT volume")?;
    // The file system is read from the start of the volume.
    disk.seek(SeekFrom::Start(0))?;
    let filesystem = FileSystem::new(disk, FsOptions::new()).context("opening the new volume")?;
    // A reader tells the FAT type by the count of clusters, so a volume of
    // the wrong size for its type reads as another one.
    if let Some(wanted) = wanted
        && filesystem.fat_type() != wanted
    {
        bail!(
            "the volume reads as {:?}, not {wanted:?}: its size does not suit {wanted:?}",
            filesystem.fat_type()
        );
    }
    copy_tree(&filesystem.root_dir(), root)?;
    filesystem.unmount().context("closing the volume")?;
    Ok(())
}

/// Copy the entries of the host directory `from` into `into`, in name order.
/// Names starting with a dot (`.gitignore`) are left out.
fn copy_tree<T: ReadWriteSeek>(into: &Dir<'_, T>, from: &Path) -> Result<()> {
    let mut entries = fs::read_dir(from)
        .with_context(|| format!("reading {}", from.display()))?
        .collect::<io::Result<Vec<_>>>()
        .with_context(|| format!("reading {}", from.display()))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let name = name
            .to_str()
            .with_context(|| format!("{} is not UTF-8", entry.path().display()))?;
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            let directory = into
                .create_dir(name)
                .with_context(|| format!("creating {name} on the volume"))?;
            copy_tree(&directory, &path)?;
        } else {
            let mut file = into
                .create_file(name)
                .with_context(|| format!("creating {name} on the volume"))?;
            let mut source =
                fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;
            io::copy(&mut source, &mut file)
                .with_context(|| format!("copying {} (does the volume fit it?)", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {super::*, std::io::Cursor};

    /// A scratch directory of its own for one test.
    fn scratch(test: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("makesd-{test}-{}", std::process::id()));
        let _gone = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn labels_are_padded_upper_case() {
        assert_eq!(&volume_label("boot"), b"BOOT       ");
        assert_eq!(&volume_label("ELEVEN CHAR"), b"ELEVEN CHAR");
    }

    #[test]
    fn a_tree_reads_back_from_the_volume() {
        let root = scratch("tree");
        fs::write(root.join("config.txt"), "arm_64bit=1\n").unwrap();
        fs::write(root.join("bcm2711-rpi-4-b.dtb"), vec![0xd0_u8; 70_000]).unwrap();
        fs::write(root.join(".gitignore"), "*\n").unwrap();
        fs::create_dir_all(root.join("overlays")).unwrap();
        fs::write(root.join("overlays/pwm-2chan.dtbo"), "overlay").unwrap();

        let mut disk = Cursor::new(vec![0_u8; 8 << 20]);
        build(&mut disk, "boot", None, &root).expect("a volume");

        disk.set_position(0);
        let filesystem = FileSystem::new(&mut disk, FsOptions::new()).unwrap();
        assert_eq!(filesystem.volume_label(), "BOOT");
        let read = |path: &str| {
            let mut bytes = Vec::new();
            io::Read::read_to_end(
                &mut filesystem.root_dir().open_file(path).unwrap(),
                &mut bytes,
            )
            .unwrap();
            bytes
        };
        assert_eq!(read("config.txt"), b"arm_64bit=1\n");
        // A long name, and a file of several clusters.
        assert_eq!(read("bcm2711-rpi-4-b.dtb"), vec![0xd0_u8; 70_000]);
        assert_eq!(read("overlays/pwm-2chan.dtbo"), b"overlay");
        let names: Vec<String> = filesystem
            .root_dir()
            .iter()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(!names.iter().any(|name| name == ".gitignore"), "{names:?}");
        drop(filesystem);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_same_tree_makes_the_same_image() {
        let root = scratch("same");
        fs::write(root.join("kernel8.img"), vec![7_u8; 5000]).unwrap();
        let image = || {
            let mut disk = Cursor::new(vec![0_u8; 4 << 20]);
            build(&mut disk, "BOOT", None, &root).unwrap();
            disk.into_inner()
        };
        assert!(image() == image(), "two builds differ");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_larger_than_the_volume_fails() {
        let root = scratch("large");
        fs::write(root.join("big.bin"), vec![1_u8; 3 << 20]).unwrap();
        let mut disk = Cursor::new(vec![0_u8; 2 << 20]);
        build(&mut disk, "BOOT", None, &root).unwrap_err();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_card_volume_is_fat32() {
        // The card's 128 MiB would be FAT16 by size; the config asks for
        // FAT32, which partition type 0x0c names. Too small for FAT32 fails.
        let root = scratch("fat32");
        fs::write(root.join("config.txt"), "arm_64bit=1\n").unwrap();
        let mut disk = Cursor::new(vec![0_u8; 128 << 20]);
        build(&mut disk, "BOOT", Some(32), &root).unwrap();
        disk.set_position(0);
        let filesystem = FileSystem::new(&mut disk, FsOptions::new()).unwrap();
        assert_eq!(filesystem.fat_type(), FatType::Fat32);
        drop(filesystem);
        build(Cursor::new(vec![0_u8; 1 << 20]), "BOOT", Some(32), &root).unwrap_err();
        fs::remove_dir_all(root).unwrap();
    }
}

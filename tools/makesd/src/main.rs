//! makesd: build the Raspberry Pi SD card image.
//!
//! It reads the images to build from a KDL config (`makesd.config`, see
//! [`config`]) and builds them in order into the output directory:
//!
//! - a `fat` image is a FAT volume of its size holding everything under the
//!   root directory ([`fat`]);
//! - a `disk` image is an MBR partition table ([`mbr`]) followed by its
//!   partitions, each a copy of an image of the output directory (one built
//!   earlier in the run, or there already) at its offset. The disk ends where
//!   its last partition does.
//!
//! ```text
//! makesd --config makesd.config --root boot --output ../../target/images
//! ```

mod config;
mod fat;
mod mbr;

use {
    anyhow::{Context, Result, bail},
    config::{DiskImage, FatImage, Image},
    mbr::{Entry, SECTOR_SIZE},
    std::{
        fs::{self, File, OpenOptions},
        io::{self, Seek, SeekFrom, Write},
        path::{Path, PathBuf},
    },
};

/// Build the SD card images a config describes.
#[derive(argh::FromArgs)]
struct Arguments {
    /// the KDL config naming the images
    #[argh(option)]
    config: PathBuf,
    /// the directory whose files the FAT images hold
    #[argh(option)]
    root: PathBuf,
    /// the directory the images are written to, and disk partitions read from
    #[argh(option)]
    output: PathBuf,
}

/// The disk identifier in the MBR (byte 440): a fixed value, so the same
/// partitions make the same image.
const DISK_SIGNATURE: u32 = 0x7665_7370;

/// Build the FAT image `image` at `path`.
fn build_fat(image: &FatImage, root: &Path, path: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.set_len(image.size)?;
    fat::build(file, &image.label, image.fat_type, root)
        .with_context(|| format!("building {}", image.name))
}

/// Build the disk image `image` at `path` from the partitions' images in
/// `output`.
fn build_disk(image: &DiskImage, output: &Path, path: &Path) -> Result<()> {
    let mut entries = Vec::new();
    let mut end = 0;
    for partition in &image.partitions {
        if partition.offset % SECTOR_SIZE as u64 != 0 {
            bail!(
                "partition `{}`: the offset {} is not a multiple of {SECTOR_SIZE}",
                partition.name,
                partition.offset
            );
        }
        let source = output.join(&partition.image);
        let bytes = fs::metadata(&source)
            .with_context(|| format!("partition `{}`: {}", partition.name, source.display()))?
            .len();
        let sector = |bytes: u64| {
            u32::try_from(bytes.div_ceil(SECTOR_SIZE as u64))
                .with_context(|| format!("partition `{}` lies past 2 TiB", partition.name))
        };
        entries.push(Entry {
            bootable: partition.bootable,
            kind: partition.kind,
            first_sector: sector(partition.offset)?,
            sectors: sector(bytes)?,
        });
        end = end.max(partition.offset + bytes.div_ceil(SECTOR_SIZE as u64) * SECTOR_SIZE as u64);
    }
    let boot_sector = mbr::boot_sector(&entries, DISK_SIGNATURE)
        .with_context(|| format!("disk `{}`", image.name))?;

    let mut disk = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    disk.set_len(end)?;
    disk.write_all(&boot_sector)?;
    for partition in &image.partitions {
        disk.seek(SeekFrom::Start(partition.offset))?;
        let source = output.join(&partition.image);
        io::copy(&mut File::open(&source)?, &mut disk)
            .with_context(|| format!("copying {} into {}", source.display(), image.name))?;
    }
    disk.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let arguments: Arguments = argh::from_env();
    let text = fs::read_to_string(&arguments.config)
        .with_context(|| format!("reading {}", arguments.config.display()))?;
    let images = config::parse(&text).with_context(|| arguments.config.display().to_string())?;
    fs::create_dir_all(&arguments.output)
        .with_context(|| format!("creating {}", arguments.output.display()))?;
    for image in &images {
        let path = arguments.output.join(image.name());
        match image {
            Image::Fat(fat) => build_fat(fat, &arguments.root, &path)?,
            Image::Disk(disk) => build_disk(disk, &arguments.output, &path)?,
        }
        println!("makesd: {}", path.display());
    }
    Ok(())
}

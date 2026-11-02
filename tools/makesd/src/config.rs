//! `makesd.config`: the images to build, in KDL.
//!
//! ```kdl
//! fat "boot.vfat" {
//!     label "BOOT"
//!     size "128M"
//!     fat-type 32
//! }
//! disk "sdcard.img" {
//!     partition "boot" {
//!         image "boot.vfat"
//!         type 0x0c
//!         offset "1M"
//!         bootable #true
//!     }
//! }
//! ```
//!
//! A size is a byte count, or a string with a `K`, `M` or `G` suffix (powers
//! of 1024).

use {
    anyhow::{Context, Result, anyhow, bail},
    kdl::{KdlDocument, KdlNode, KdlValue},
};

/// A FAT volume holding a directory's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatImage {
    pub name: String,
    /// At most 11 characters
    pub label: String,
    /// Bytes of the volume
    pub size: u64,
    /// FAT12, FAT16 or FAT32 (`fat-type 12`, `16` or `32`); `None` lets the
    /// volume's size decide
    pub fat_type: Option<u8>,
}

/// One primary MBR partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub name: String,
    /// The image the partition holds, from the output directory
    pub image: String,
    /// The partition type byte (`0x0c`: FAT32 with LBA)
    pub kind: u8,
    /// Bytes from the start of the disk, a multiple of the sector size
    pub offset: u64,
    pub bootable: bool,
}

/// A disk image with an MBR partition table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskImage {
    pub name: String,
    pub partitions: Vec<Partition>,
}

/// One image to build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    Fat(FatImage),
    Disk(DiskImage),
}

impl Image {
    /// The image's file name.
    pub fn name(&self) -> &str {
        match self {
            Self::Fat(image) => &image.name,
            Self::Disk(image) => &image.name,
        }
    }
}

/// Parse `text`, the contents of a config file: its images, in order.
///
/// # Errors
///
/// A KDL syntax error, an unknown node, or a missing or malformed value.
pub fn parse(text: &str) -> Result<Vec<Image>> {
    let document = KdlDocument::parse_v2(text).map_err(|error| anyhow!("{error:?}"))?;
    document
        .nodes()
        .iter()
        .map(|node| match node.name().value() {
            "fat" => fat(node).map(Image::Fat),
            "disk" => disk(node).map(Image::Disk),
            other => bail!("unknown image kind `{other}` (fat or disk)"),
        })
        .collect()
}

/// The node's one argument, its name, as a string.
fn name(node: &KdlNode) -> Result<String> {
    node.get(0)
        .and_then(KdlValue::as_string)
        .map(str::to_owned)
        .with_context(|| format!("`{}` needs a name", node.name().value()))
}

/// The children of `node`: only `known` ones, and none but the `repeatable`
/// ones twice.
fn children<'a>(
    node: &'a KdlNode,
    known: &[&str],
    repeatable: &[&str],
) -> Result<Vec<&'a KdlNode>> {
    let children: Vec<&KdlNode> = node
        .children()
        .map_or_else(Vec::new, |document| document.nodes().iter().collect());
    for (index, child) in children.iter().enumerate() {
        let key = child.name().value();
        if !known.contains(&key) {
            bail!("`{}` has no setting `{key}`", node.name().value());
        }
        if child.get(0).is_none() {
            bail!("`{}` setting `{key}` needs a value", node.name().value());
        }
        let repeated = children[..index]
            .iter()
            .any(|other| other.name().value() == key);
        if repeated && !repeatable.contains(&key) {
            bail!("`{}` sets `{key}` twice", node.name().value());
        }
    }
    Ok(children)
}

/// The value of the setting `key` among `children`, if set.
fn setting<'a>(children: &[&'a KdlNode], key: &str) -> Option<&'a KdlValue> {
    children
        .iter()
        .find(|child| child.name().value() == key)
        .and_then(|child| child.get(0))
}

/// A size: a byte count, or a string with a `K`, `M` or `G` suffix.
///
/// # Errors
///
/// A negative count, an unknown suffix or an overflow.
pub fn size(value: &KdlValue) -> Result<u64> {
    if let Some(count) = value.as_integer() {
        return u64::try_from(count).map_err(|_error| anyhow!("a size of {count} bytes"));
    }
    let text = value
        .as_string()
        .with_context(|| format!("a size is a byte count or a string, not {value}"))?;
    let (digits, shift) = [('K', 10), ('M', 20), ('G', 30)]
        .into_iter()
        .find_map(|(suffix, shift)| text.strip_suffix(suffix).map(|digits| (digits, shift)))
        .unwrap_or((text, 0));
    let count: u64 = digits
        .parse()
        .with_context(|| format!("the size `{text}` (a count with K, M or G)"))?;
    count
        .checked_mul(1 << shift)
        .with_context(|| format!("the size `{text}` overflows"))
}

fn fat(node: &KdlNode) -> Result<FatImage> {
    let name = name(node)?;
    let settings = children(node, &["label", "size", "fat-type"], &[])?;
    let fat_type = setting(&settings, "fat-type")
        .map(|value| {
            value
                .as_integer()
                .and_then(|bits| u8::try_from(bits).ok())
                .filter(|bits| [12, 16, 32].contains(bits))
                .with_context(|| format!("`{name}`: fat-type is 12, 16 or 32"))
        })
        .transpose()?;
    let label = setting(&settings, "label")
        .map(|value| {
            value
                .as_string()
                .map(str::to_owned)
                .with_context(|| format!("`{name}`: the label is a string"))
        })
        .transpose()?
        .unwrap_or_else(|| "NO NAME".to_owned());
    if label.len() > 11 || !label.is_ascii() {
        bail!("`{name}`: the label `{label}` is longer than 11 ASCII characters");
    }
    let size = size(setting(&settings, "size").with_context(|| format!("`{name}` needs a size"))?)
        .with_context(|| format!("`{name}`"))?;
    Ok(FatImage {
        name,
        label,
        size,
        fat_type,
    })
}

fn partition(node: &KdlNode) -> Result<Partition> {
    let name = name(node)?;
    let settings = children(node, &["image", "type", "offset", "bootable"], &[])?;
    let image = setting(&settings, "image")
        .and_then(KdlValue::as_string)
        .map(str::to_owned)
        .with_context(|| format!("partition `{name}` needs an image"))?;
    let kind = setting(&settings, "type")
        .and_then(KdlValue::as_integer)
        .and_then(|kind| u8::try_from(kind).ok())
        .with_context(|| format!("partition `{name}` needs a type byte"))?;
    let offset = setting(&settings, "offset")
        .map(size)
        .transpose()
        .with_context(|| format!("partition `{name}`"))?
        .with_context(|| format!("partition `{name}` needs an offset"))?;
    let bootable = setting(&settings, "bootable")
        .map(|value| {
            value
                .as_bool()
                .with_context(|| format!("partition `{name}`: bootable is #true or #false"))
        })
        .transpose()?
        .unwrap_or(false);
    Ok(Partition {
        name,
        image,
        kind,
        offset,
        bootable,
    })
}

fn disk(node: &KdlNode) -> Result<DiskImage> {
    let name = name(node)?;
    let partitions = children(node, &["partition"], &["partition"])?
        .into_iter()
        .map(partition)
        .collect::<Result<Vec<_>>>()
        .with_context(|| format!("disk `{name}`"))?;
    if partitions.is_empty() {
        bail!("disk `{name}` has no partitions");
    }
    Ok(DiskImage { name, partitions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_config_parses() {
        let images = parse(include_str!("../makesd.config")).expect("the card config");
        assert_eq!(
            images,
            [
                Image::Fat(FatImage {
                    name: "boot.vfat".into(),
                    label: "BOOT".into(),
                    size: 128 << 20,
                    fat_type: Some(32),
                }),
                Image::Disk(DiskImage {
                    name: "sdcard.img".into(),
                    partitions: vec![Partition {
                        name: "boot".into(),
                        image: "boot.vfat".into(),
                        kind: 0x0c,
                        offset: 1 << 20,
                        bootable: true,
                    }],
                }),
            ]
        );
    }

    #[test]
    fn sizes() {
        assert_eq!(size(&KdlValue::Integer(4096)).unwrap(), 4096);
        assert_eq!(size(&KdlValue::String("512".into())).unwrap(), 512);
        assert_eq!(size(&KdlValue::String("64K".into())).unwrap(), 64 << 10);
        assert_eq!(size(&KdlValue::String("2G".into())).unwrap(), 2 << 30);
        size(&KdlValue::String("2T".into())).unwrap_err();
        size(&KdlValue::Integer(-1)).unwrap_err();
        size(&KdlValue::Bool(true)).unwrap_err();
    }

    #[test]
    fn mistakes_are_refused() {
        // An unknown image kind, setting, a repeated setting, a long label,
        // missing fat-type argument,
        // a disk without partitions, a partition without an image,
        // a wrong partition type.
        for text in [
            r#"ext4 "root.img" { size "1M" }"#,
            r#"fat "a.vfat" { size "1M"; colour "red" }"#,
            r#"fat "a.vfat" { size "1M"; size "2M" }"#,
            r#"fat "a.vfat" { size "1M"; label "TWELVE CHARS" }"#,
            r#"fat "a.vfat" { label "A" }"#,
            r#"fat "a.vfat" { size "1M"; fat-type 24 }"#,
            r#"fat "a.vfat" { size "1M"; fat-type }"#,
            r#"disk "d.img""#,
            r#"disk "d.img" { partition "p" { type 12; offset "1M" } }"#,
            r#"disk "d.img" { partition "p" { image "a"; type 300; offset "1M" } }"#,
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }
}

//! Trim a SoundFont to what a set of songs plays (audio decision D9.4).
//!
//! Reads Standard MIDI Files, collects the melodic programs and percussion
//! notes they sound, and writes a SoundFont with only the presets,
//! instruments, samples and sample data those need
//! (`vesper_synth::trim::trim`).
//!
//! The MIDI synthesiser's test bank is made this way from the test song
//! (`userspace/midi-synth/fixtures/README.md`), and Wood's bank from the
//! IWAD's songs, which vesper-doom's `wad-music` exports
//! (`just _wood-soundfont`).

use {
    anyhow::{Context, Result, bail},
    argh::FromArgs,
    std::{
        fs,
        path::{Path, PathBuf},
    },
    vesper_synth::{
        Song,
        sf2::SoundFont,
        trim::{Trimmed, Usage, trim},
    },
};

/// Trim a SoundFont to the programs and percussion notes a set of Standard
/// MIDI Files uses.
#[derive(FromArgs)]
struct Arguments {
    /// the SoundFont 2 file to trim
    #[argh(option)]
    soundfont: PathBuf,
    /// where to write the trimmed SoundFont
    #[argh(option)]
    output: PathBuf,
    /// the Standard MIDI Files whose notes the trimmed SoundFont keeps
    #[argh(positional)]
    songs: Vec<PathBuf>,
}

/// The programs and percussion notes the MIDI files `songs` sound.
fn usage_of(songs: &[Vec<u8>]) -> Result<Usage> {
    let mut usage = Usage::default();
    for (index, midi) in songs.iter().enumerate() {
        match Song::parse(midi) {
            Ok(song) => usage.add_song(&song),
            Err(error) => bail!("song {index}: {error:?}"),
        }
    }
    Ok(usage)
}

/// `source` trimmed to what `songs` play.
fn trimmed(source: &[u8], songs: &[Vec<u8>]) -> Result<(Trimmed, Usage)> {
    let font = match SoundFont::parse(source) {
        Ok(font) => font,
        Err(error) => bail!("{error:?}"),
    };
    let usage = usage_of(songs)?;
    Ok((trim(&font, &usage), usage))
}

/// Write `bytes` to `path`, making its directory.
fn write_output(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// `bank:value` pairs, the bank left out when it is 0.
fn listed<T: ToString>(pairs: impl Iterator<Item = (u16, T)>) -> Vec<String> {
    pairs
        .map(|(bank, value)| {
            if bank == 0 {
                value.to_string()
            } else {
                format!("{bank}:{}", value.to_string())
            }
        })
        .collect()
}

fn main() -> Result<()> {
    let arguments: Arguments = argh::from_env();
    if arguments.songs.is_empty() {
        bail!("no songs: name the MIDI files whose notes to keep");
    }
    let source = fs::read(&arguments.soundfont)
        .with_context(|| format!("reading {}", arguments.soundfont.display()))?;
    let songs = arguments
        .songs
        .iter()
        .map(|path| fs::read(path).with_context(|| format!("reading {}", path.display())))
        .collect::<Result<Vec<_>>>()?;
    let (trimmed, usage) = trimmed(&source, &songs)
        .with_context(|| format!("trimming {}", arguments.soundfont.display()))?;
    write_output(&arguments.output, &trimmed.bytes)?;

    let font = match SoundFont::parse(&source) {
        Ok(font) => font,
        Err(error) => bail!("{error:?}"),
    };
    let programs = listed(usage.melodic.iter().copied());
    let drums = listed(usage.percussion.iter().copied());
    println!("{} songs", songs.len());
    println!(
        "{} melodic programs: {}",
        programs.len(),
        programs.join(" ")
    );
    println!("{} percussion notes: {}", drums.len(), drums.join(" "));
    println!(
        "kept {} of {} presets, {} of {} instruments, {} of {} samples",
        trimmed.presets,
        font.preset_count(),
        trimmed.instruments,
        font.instrument_count(),
        trimmed.samples,
        font.sample_count()
    );
    println!(
        "{} bytes ({} bytes of sample data) from {} bytes: {}",
        trimmed.bytes.len(),
        SoundFont::parse(&trimmed.bytes).map_or(0, |trimmed| trimmed.sample_data.len()),
        source.len(),
        arguments.output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::trimmed,
        std::{fs, path::Path},
    };

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../midi-synth/fixtures")
                .join(name),
        )
        .unwrap()
    }

    /// The committed test bank is the test song's trim: trimming it again by
    /// the song keeps it byte for byte. A changed song needs the bank made
    /// again (`userspace/midi-synth/fixtures/README.md`).
    #[test]
    fn test_bank_is_trimmed_to_the_test_song() {
        let bank = fixture("wtk1-prelude1.sf2");
        let (again, usage) = trimmed(&bank, &[fixture("wtk1-prelude1.mid")]).unwrap();
        assert!(!usage.melodic.is_empty());
        assert_eq!(again.bytes, bank);
    }

    #[test]
    fn refuses_a_song_that_is_not_midi() {
        let bank = fixture("wtk1-prelude1.sf2");
        let error = trimmed(&bank, &[b"not a MIDI file".to_vec()]).unwrap_err();
        assert!(error.to_string().starts_with("song 0:"), "{error}");
    }
}

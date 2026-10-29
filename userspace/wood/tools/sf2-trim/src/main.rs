//! Trim a SoundFont to what an IWAD's music plays (audio decision D9.4).
//!
//! Reads every MUS lump (`D_*`) of the IWAD, converts it with Room's
//! `mus2midi` as the game does, collects the melodic programs and percussion
//! notes the songs sound, and writes a SoundFont with only the presets,
//! instruments, samples and sample data those need
//! (`wood_synth::trim::trim`). The result is a build artifact (under
//! `target/`), rebuilt per IWAD: `just wood-soundfont`.
//!
//! With `--song` and `--midi` it writes one song's MIDI file instead, converted
//! the same way (`just wood-midi`, for the MIDI test image).

use {
    anyhow::{Context, Result, bail, ensure},
    argh::FromArgs,
    std::{
        fs,
        path::{Path, PathBuf},
    },
    wood_synth::{
        Song,
        sf2::SoundFont,
        trim::{Usage, trim},
    },
};

/// Trim a SoundFont to the programs and percussion notes an IWAD's music uses,
/// or export one of its songs as a Standard MIDI File.
#[derive(FromArgs)]
struct Arguments {
    /// the IWAD whose MUS lumps are read
    #[argh(option)]
    wad: PathBuf,
    /// the SoundFont 2 file to trim
    #[argh(option)]
    soundfont: Option<PathBuf>,
    /// where to write the trimmed SoundFont
    #[argh(option)]
    output: Option<PathBuf>,
    /// the MUS lump to export as MIDI instead of trimming (e.g. `D_E1M1`)
    #[argh(option)]
    song: Option<String>,
    /// where to write the exported song's MIDI file
    #[argh(option)]
    midi: Option<PathBuf>,
}

/// The MIDI file of the IWAD's song `name`, converted with Room's `mus2midi`
/// as the game converts it.
fn song_midi(wad: &[u8], name: &str) -> Result<Vec<u8>> {
    let lumps = music_lumps(wad)?;
    let (_, mus) = lumps
        .iter()
        .find(|(lump, _)| lump.eq_ignore_ascii_case(name))
        .with_context(|| format!("no MUS lump {name}"))?;
    let midi = room::engine::sound::mus2midi::mus2midi(mus)
        .with_context(|| format!("{name}: MUS conversion failed"))?;
    if let Err(error) = Song::parse(&midi) {
        bail!("{name}: {error:?}");
    }
    Ok(midi)
}

/// Write `bytes` to `path`, making its directory.
fn write_output(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// The MUS lumps of a WAD: name and bytes.
fn music_lumps(wad: &[u8]) -> Result<Vec<(String, &[u8])>> {
    ensure!(wad.len() >= 12, "WAD too short");
    ensure!(
        &wad[..4] == b"IWAD" || &wad[..4] == b"PWAD",
        "not a WAD file"
    );
    let dword = |offset: usize| -> Result<usize> {
        let bytes: [u8; 4] = wad
            .get(offset..offset + 4)
            .context("WAD directory truncated")?
            .try_into()?;
        Ok(usize::try_from(u32::from_le_bytes(bytes))?)
    };
    let count = dword(4)?;
    let directory = dword(8)?;
    let mut lumps = Vec::new();
    for entry in 0..count {
        let record = directory + entry * 16;
        let position = dword(record)?;
        let size = dword(record + 4)?;
        let name_bytes = wad.get(record + 8..record + 16).context("WAD truncated")?;
        let name: String = name_bytes
            .iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| char::from(byte))
            .collect();
        let data = wad
            .get(position..position + size)
            .context("lump past the end")?;
        if name.starts_with("D_") && data.starts_with(b"MUS\x1a") {
            lumps.push((name, data));
        }
    }
    Ok(lumps)
}

fn main() -> Result<()> {
    let arguments: Arguments = argh::from_env();
    let wad =
        fs::read(&arguments.wad).with_context(|| format!("reading {}", arguments.wad.display()))?;
    match (arguments.song, arguments.midi) {
        (Some(name), Some(midi_path)) => {
            let midi = song_midi(&wad, &name)?;
            write_output(&midi_path, &midi)?;
            println!("{name}: {} bytes of MIDI: {}", midi.len(), midi_path.display());
            return Ok(());
        }
        (None, None) => {}
        _ => bail!("--song and --midi go together"),
    }
    let (Some(soundfont), Some(output)) = (arguments.soundfont, arguments.output) else {
        bail!("trimming needs --soundfont and --output");
    };
    let source =
        fs::read(&soundfont).with_context(|| format!("reading {}", soundfont.display()))?;
    let font = match SoundFont::parse(&source) {
        Ok(font) => font,
        Err(error) => bail!("{}: {error:?}", soundfont.display()),
    };

    let mut usage = Usage::default();
    let lumps = music_lumps(&wad)?;
    for (name, mus) in &lumps {
        let midi = room::engine::sound::mus2midi::mus2midi(mus)
            .with_context(|| format!("{name}: MUS conversion failed"))?;
        let song = match Song::parse(&midi) {
            Ok(song) => song,
            Err(error) => bail!("{name}: {error:?}"),
        };
        usage.add_song(&song);
    }
    let trimmed = trim(&font, &usage);
    write_output(&output, &trimmed.bytes)?;

    let programs: Vec<String> = usage
        .melodic
        .iter()
        .map(|(bank, program)| {
            if *bank == 0 {
                program.to_string()
            } else {
                format!("{bank}:{program}")
            }
        })
        .collect();
    let drums: Vec<String> = usage
        .percussion
        .iter()
        .map(|(kit, key)| {
            if *kit == 0 {
                key.to_string()
            } else {
                format!("{kit}:{key}")
            }
        })
        .collect();
    println!("{} songs", lumps.len());
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
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::{music_lumps, song_midi},
        std::{fs, path::Path},
        wood_synth::Song,
    };

    fn wad() -> Vec<u8> {
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../wad/doom1.wad")).unwrap()
    }

    /// The exported song is the MUS lump through Room's `mus2midi`, byte for
    /// byte, as the midi-synth session tests make it: the start hash
    /// `just test-midi` checks holds for the file the image bundles.
    #[test]
    fn exports_e1m1_as_the_game_converts_it() {
        let wad = wad();
        let midi = song_midi(&wad, "D_E1M1").unwrap();
        let lumps = music_lumps(&wad).unwrap();
        let (_, mus) = lumps.iter().find(|(name, _)| name == "D_E1M1").unwrap();
        assert_eq!(
            midi,
            room::engine::sound::mus2midi::mus2midi(mus).unwrap()
        );
        assert!(midi.starts_with(b"MThd"));
        let song = Song::parse(&midi).unwrap();
        assert_eq!(song.division, 70, "one tick per MUS tick at 120 bpm");
        assert!(!song.events.is_empty());
        // Names match without regard to case, as WAD lump names do.
        assert_eq!(song_midi(&wad, "d_e1m1").unwrap(), midi);
    }

    #[test]
    fn refuses_a_missing_song() {
        let error = song_midi(&wad(), "D_NOSUCH").unwrap_err();
        assert_eq!(error.to_string(), "no MUS lump D_NOSUCH");
    }
}

//! Trim a SoundFont to what an IWAD's music plays (audio decision D9.4).
//!
//! Reads every MUS lump (`D_*`) of the IWAD, converts it with Room's
//! `mus2midi` as the game does, collects the melodic programs and percussion
//! notes the songs sound, and writes a SoundFont with only the presets,
//! instruments, samples and sample data those need
//! (`wood_synth::trim::trim`). The result is a build artifact (under
//! `target/`), rebuilt per IWAD: `just wood-soundfont`.

use {
    anyhow::{Context, Result, bail, ensure},
    argh::FromArgs,
    std::{fs, path::PathBuf},
    wood_synth::{
        Song,
        sf2::SoundFont,
        trim::{Usage, trim},
    },
};

/// Trim a SoundFont to the programs and percussion notes an IWAD's music uses.
#[derive(FromArgs)]
struct Arguments {
    /// the IWAD whose MUS lumps are read
    #[argh(option)]
    wad: PathBuf,
    /// the SoundFont 2 file to trim
    #[argh(option)]
    soundfont: PathBuf,
    /// where to write the trimmed SoundFont
    #[argh(option)]
    output: PathBuf,
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
    let source = fs::read(&arguments.soundfont)
        .with_context(|| format!("reading {}", arguments.soundfont.display()))?;
    let font = match SoundFont::parse(&source) {
        Ok(font) => font,
        Err(error) => bail!("{}: {error:?}", arguments.soundfont.display()),
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
    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&arguments.output, &trimmed.bytes)
        .with_context(|| format!("writing {}", arguments.output.display()))?;

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
        arguments.output.display()
    );
    Ok(())
}

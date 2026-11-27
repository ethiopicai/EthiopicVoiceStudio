//! Offline compressed-audio import. Existing WAV bytes are never rewritten.
use std::{
    io::Cursor,
    time::{Duration, Instant},
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{Limit, MetadataOptions},
    probe::Hint,
};

pub const MAX_BYTES: usize = 100 * 1024 * 1024;
const FORMATS: &[&str] = &["wav", "mp3", "flac", "ogg", "m4a"];
const INVALID: &str = "This recording could not be decoded. Choose a readable WAV, MP3, FLAC, OGG (Vorbis), or M4A (AAC/ALAC) file.";

pub fn extension(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.len() > 240
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err("Choose a recording with a valid filename.".into());
    }
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !FORMATS.contains(&ext.as_str()) {
        return Err(INVALID.into());
    }
    Ok(ext)
}

pub fn prepare(name: String, bytes: Vec<u8>) -> Result<(String, Vec<u8>), String> {
    prepare_bounded(name, bytes, MAX_BYTES)
}

fn prepare_bounded(
    name: String,
    bytes: Vec<u8>,
    limit: usize,
) -> Result<(String, Vec<u8>), String> {
    let ext = extension(&name)?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("Choose a nonempty recording up to 100 MB.".into());
    }
    // This is also essential for sample-identical edits and transfer metadata hashes.
    if ext == "wav" {
        return Ok((name, bytes));
    }
    let mut hint = Hint::new();
    hint.with_extension(&ext);
    let input = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            input,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions {
                limit_metadata_bytes: Limit::Maximum(1024 * 1024),
                limit_visual_bytes: Limit::Maximum(0),
            },
        )
        .map_err(|_| INVALID)?
        .format;
    let track = format.default_track().ok_or(INVALID)?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| INVALID)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let first_packet = loop {
        let packet = format.next_packet().map_err(|_| INVALID)?;
        if packet.track_id() == track_id {
            break packet;
        }
        if Instant::now() > deadline {
            return Err(INVALID.into());
        }
    };
    let first = decoder.decode(&first_packet).map_err(|_| INVALID)?;
    let spec = *first.spec();
    let rate = spec.rate;
    let channel_count = spec.channels.count();
    if !(8000..=192000).contains(&rate) || !(1..=8).contains(&channel_count) {
        return Err(INVALID.into());
    }
    let mut output = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut output,
        hound::WavSpec {
            channels: channel_count as u16,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .map_err(|_| INVALID)?;
    let mut sample_count = 0usize;
    {
        let mut append = |audio: symphonia::core::audio::AudioBufferRef<'_>| -> Result<(), String> {
            let spec = *audio.spec();
            let channels = spec.channels.count();
            if rate != spec.rate || channel_count != channels {
                return Err("This file changes audio format partway through. Export it as a single WAV recording and retry.".into());
            }
            if audio.capacity().saturating_mul(channels) > 1024 * 1024 {
                return Err(INVALID.into());
            }
            let mut samples = SampleBuffer::<i16>::new(audio.capacity() as u64, spec);
            samples.copy_interleaved_ref(audio);
            sample_count = sample_count.saturating_add(samples.samples().len());
            if sample_count.saturating_mul(2).saturating_add(44) > limit {
                return Err("The decoded recording exceeds 100 MB. Choose a shorter recording or trim it before importing.".into());
            }
            for sample in samples.samples() {
                writer.write_sample(*sample).map_err(|_| INVALID)?;
            }
            Ok(())
        };
        append(first)?;
        loop {
            if Instant::now() > deadline {
                return Err("Import took too long. Try a shorter recording.".into());
            }
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break
                }
                Err(_) => return Err(INVALID.into()),
            };
            if packet.track_id() != track_id {
                continue;
            }
            // Reject corrupt packets rather than silently dropping time from a recording.
            append(decoder.decode(&packet).map_err(|_| INVALID)?)?;
        }
    }
    if sample_count == 0 {
        return Err(INVALID.into());
    }
    writer.finalize().map_err(|_| INVALID)?;
    let stem = name.rsplit_once('.').ok_or(INVALID)?.0;
    Ok((format!("{stem}.wav"), output.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    const MP3: &[u8] = include_bytes!("../tests/fixtures/tone.mp3");
    #[test]
    fn decodes_mp3_to_pcm_and_preserves_source() {
        let original = MP3.to_vec();
        let (name, wav) = prepare("ሰላም.MP3".into(), original.clone()).unwrap();
        assert_eq!(name, "ሰላም.wav");
        assert_eq!(original, MP3);
        let reader = hound::WavReader::new(Cursor::new(&wav)).unwrap();
        assert_eq!(reader.spec().sample_rate, 24000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().bits_per_sample, 16);
        assert!((reader.duration() as f64 / 24000. - 0.4).abs() < 0.05);
        assert!(reader.into_samples::<i16>().any(|s| s.unwrap().abs() > 100));
    }
    #[test]
    fn wav_is_byte_identical_and_bad_or_expanding_inputs_fail() {
        let (_, wav) = prepare("a.mp3".into(), MP3.to_vec()).unwrap();
        assert_eq!(prepare("a.wav".into(), wav.clone()).unwrap().1, wav);
        assert!(prepare("a.mp3".into(), vec![0; 100]).is_err());
        assert!(prepare("a.mp3.exe".into(), MP3.to_vec()).is_err());
        assert!(prepare("../a.mp3".into(), MP3.to_vec()).is_err());
        assert!(prepare_bounded("a.mp3".into(), MP3.to_vec(), 1000)
            .unwrap_err()
            .contains("decoded recording"));
    }
    #[test]
    fn advertised_codecs_decode_into_validated_recordings() {
        for (name, bytes) in [
            (
                "tone.flac",
                include_bytes!("../tests/fixtures/tone.flac").as_slice(),
            ),
            (
                "tone.ogg",
                include_bytes!("../tests/fixtures/tone.ogg").as_slice(),
            ),
            (
                "tone.m4a",
                include_bytes!("../tests/fixtures/tone.m4a").as_slice(),
            ),
            (
                "tone-alac.m4a",
                include_bytes!("../tests/fixtures/tone-alac.m4a").as_slice(),
            ),
        ] {
            let (_, wav) =
                prepare(name.into(), bytes.to_vec()).unwrap_or_else(|e| panic!("{name}: {e}"));
            crate::asr::validate_audio(&wav).unwrap();
            let reader = hound::WavReader::new(Cursor::new(&wav)).unwrap();
            assert!(
                (reader.duration() as f64 / 24000. - 0.4).abs() < 0.1,
                "{name}"
            );
        }
    }
    #[test]
    fn compressed_import_and_path_transfer_save_wav_without_modifying_source() {
        let root = std::env::temp_dir().join(format!(
            "ethiopic-mp3-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.mp3");
        std::fs::write(&source, MP3).unwrap();
        let package = crate::recording_transfer::resolve(
            &root,
            crate::recording_transfer::Input::Path {
                path: source.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        assert_eq!(package.name, "source.wav");
        crate::recording_transfer::validate_context(&package.context, &package.bytes).unwrap();
        let imported =
            crate::asr::import_audio(&root, "source.mp3".into(), MP3.to_vec(), None).unwrap();
        assert_eq!(imported.name, "source.wav");
        let stored =
            std::fs::read(root.join("recordings").join(imported.id).join("source.wav")).unwrap();
        assert_eq!(stored, package.bytes);
        crate::asr::validate_audio(&stored).unwrap();
        assert_eq!(std::fs::read(source).unwrap(), MP3);
        let transferred = crate::recording_transfer::resolve(
            &root,
            crate::recording_transfer::Input::File {
                name: "tone.mp3".into(),
                bytes: MP3.to_vec(),
                context: None,
                text: Some("ሰላም".into()),
            },
        )
        .unwrap();
        assert_eq!(transferred.context.text, "ሰላም");
        assert_eq!(transferred.bytes, stored);
        std::fs::remove_dir_all(root).unwrap();
    }
}

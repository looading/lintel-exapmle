//! Short original chimes for the portable Windows build.

use crate::tools::Sound;

const SAMPLE_RATE: u32 = 24_000;

#[cfg(target_os = "windows")]
pub fn play(sound: Sound) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};

    let Some(wave) = chime(sound) else { return };
    // SND_MEMORY interprets the pointer as RIFF bytes instead of a UTF-16
    // filename. SND_SYNC keeps the aligned buffer in scope for playback.
    let played = unsafe {
        PlaySoundW(
            wave.as_ptr(),
            std::ptr::null_mut(),
            SND_MEMORY | SND_SYNC | SND_NODEFAULT,
        )
    };
    if played == 0 {
        eprintln!("Windows could not play the clock alert");
    }
}

fn chime(sound: Sound) -> Option<Vec<u16>> {
    let notes: &[(f64, u32)] = match sound {
        Sound::Ping => &[(880.0, 160), (1_175.0, 230)],
        Sound::Glass => &[(1_317.0, 180), (1_568.0, 290)],
        Sound::Hero => &[(523.0, 150), (659.0, 150), (784.0, 310)],
        Sound::Silent | Sound::StopMedia => return None,
    };
    let sample_count: u32 = notes.iter().map(|&(_, ms)| SAMPLE_RATE * ms / 1000).sum();
    let data_bytes = sample_count * 2;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM
    header.extend_from_slice(&1u16.to_le_bytes()); // mono
    header.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());

    let mut wave = Vec::with_capacity(22 + sample_count as usize);
    wave.extend(
        header
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
    );
    for &(frequency, duration_ms) in notes {
        let count = SAMPLE_RATE * duration_ms / 1000;
        for index in 0..count {
            let time = f64::from(index) / f64::from(SAMPLE_RATE);
            let phase = time * frequency * std::f64::consts::TAU;
            let decay = (1.0 - f64::from(index) / f64::from(count)).powi(2);
            let attack = (time / 0.008).min(1.0);
            let sample = (phase.sin() + 0.18 * (2.0 * phase).sin()) * decay * attack * 6000.0;
            wave.push(sample as i16 as u16);
        }
    }
    Some(wave)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_chime_is_a_complete_pcm_wave() {
        let words = chime(Sound::Ping).unwrap();
        let bytes: Vec<u8> = words.into_iter().flat_map(u16::to_le_bytes).collect();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(&bytes[36..40], b"data");
        let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), 44 + data_len);
        assert!(bytes[44..].iter().any(|&sample| sample != 0));
        assert!(chime(Sound::Silent).is_none());
    }
}

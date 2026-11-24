#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AudioFormat {
    #[default]
    Mp3,
    Wav,
    M4a,
    Flac,
    Opus,
    Aac,
}

impl AudioFormat {
    /// Every format, in the order the interface shows them.
    pub const ALL: [Self; 6] = [
        Self::Mp3,
        Self::Wav,
        Self::M4a,
        Self::Flac,
        Self::Opus,
        Self::Aac,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Wav => "WAV",
            Self::M4a => "M4A",
            Self::Flac => "FLAC",
            Self::Opus => "Opus",
            Self::Aac => "AAC",
        }
    }

    /// The value of the yt-dlp `--audio-format` argument.
    pub fn yt_dlp_name(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::M4a => "m4a",
            Self::Flac => "flac",
            Self::Opus => "opus",
            Self::Aac => "aac",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Mp3 => "Works with most players and editors.",
            Self::Wav => "Uncompressed audio for editing.",
            Self::M4a => "Compact audio for Apple devices.",
            Self::Flac => "Lossless compression.",
            Self::Opus => "Small files with good sound quality.",
            Self::Aac => "Efficient audio for everyday listening.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_formats_keep_their_order_and_text() {
        assert_eq!(AudioFormat::default(), AudioFormat::Mp3);
        assert_eq!(
            AudioFormat::ALL.map(|format| (
                format.label(),
                format.yt_dlp_name(),
                format.description()
            )),
            [
                ("MP3", "mp3", "Works with most players and editors."),
                ("WAV", "wav", "Uncompressed audio for editing."),
                ("M4A", "m4a", "Compact audio for Apple devices."),
                ("FLAC", "flac", "Lossless compression."),
                ("Opus", "opus", "Small files with good sound quality."),
                ("AAC", "aac", "Efficient audio for everyday listening."),
            ]
        );
    }
}

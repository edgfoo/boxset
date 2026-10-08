//! The `type` attribute for a written rendition.

use crate::config::Codec;
use crate::sources::OutputProbe;

pub fn mime(codec: Codec, probe: &OutputProbe) -> String {
    let (container, audio) = match codec {
        Codec::Vp9 => ("video/webm", "opus"),
        _ => ("video/mp4", "mp4a.40.2"),
    };
    match (video_codec_string(codec, probe), probe.has_audio) {
        (Some(video), true) => format!("{container}; codecs=\"{video}, {audio}\""),
        (Some(video), false) => format!("{container}; codecs=\"{video}\""),
        (None, _) => container.to_string(),
    }
}

fn video_codec_string(codec: Codec, probe: &OutputProbe) -> Option<String> {
    let profile = probe.profile.as_deref()?;
    match (codec, profile) {
        // avc1.PPCCLL: profile, constraint flags, level, in hex
        (Codec::H264, "High") => Some(format!("avc1.6400{:02X}", probe.level?)),
        (Codec::H264, "Main") => Some(format!("avc1.4D40{:02X}", probe.level?)),
        (Codec::H264, "Baseline" | "Constrained Baseline") => {
            Some(format!("avc1.42E0{:02X}", probe.level?))
        }
        // ffprobe's level is already ×30. The tier and constraint flags are
        // the ones x265 writes by default.
        (Codec::H265, "Main") => Some(format!("hvc1.1.6.L{}.90", probe.level?)),

        // WebM doesn't record a level. Browsers don't check it against the
        // stream, so the lowest one stands in.
        (Codec::Vp9, "Profile 0") => Some("vp09.00.10.08".to_string()),

        // ffprobe's level for AV1 is the seq_level_idx.
        (Codec::Av1, "Main") => Some(format!("av01.0.{:02}M.08", probe.level?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(profile: &str, level: Option<i32>, has_audio: bool) -> OutputProbe {
        OutputProbe {
            width: 1280,
            height: 720,
            has_audio,
            profile: Some(profile.to_string()),
            level,
        }
    }

    #[test]
    fn codec_strings() {
        let cases = [
            (Codec::H264, probe("High", Some(31), false), "avc1.64001F"),
            (Codec::H264, probe("Main", Some(30), false), "avc1.4D401E"),
            (
                Codec::H264,
                probe("Constrained Baseline", Some(30), false),
                "avc1.42E01E",
            ),
            (
                Codec::H265,
                probe("Main", Some(93), false),
                "hvc1.1.6.L93.90",
            ),
            (Codec::Vp9, probe("Profile 0", None, false), "vp09.00.10.08"),
            (Codec::Av1, probe("Main", Some(5), false), "av01.0.05M.08"),
        ];
        for (codec, probe, expected) in cases {
            assert_eq!(video_codec_string(codec, &probe).unwrap(), expected);
        }
    }

    #[test]
    fn mime_joins_video_and_audio() {
        assert_eq!(
            mime(Codec::H264, &probe("High", Some(40), true)),
            r#"video/mp4; codecs="avc1.640028, mp4a.40.2""#
        );
        assert_eq!(
            mime(Codec::Vp9, &probe("Profile 0", None, true)),
            r#"video/webm; codecs="vp09.00.10.08, opus""#
        );
        assert_eq!(
            mime(Codec::Av1, &probe("Main", Some(8), false)),
            r#"video/mp4; codecs="av01.0.08M.08""#
        );
    }

    #[test]
    fn an_unknown_profile_leaves_out_codecs() {
        assert_eq!(
            mime(Codec::H264, &probe("Extended", Some(30), false)),
            "video/mp4"
        );
    }
}

//! Output paths: the one place a target's filenames are built.

use std::path::{Path, PathBuf};

use crate::config::Codec;

pub struct Naming<'a> {
    pub out_dir: &'a Path,
    pub src: &'a Path,
    pub name: Option<&'a str>,
}

impl Naming<'_> {
    /// Tidied, so an out_dir of `.` doesn't leave a `./` on the front.
    fn join(&self, filename: String) -> PathBuf {
        crate::config::fold_dot_segments(&self.out_dir.join(filename))
    }

    fn name_or_source_stem(&self) -> String {
        match self.name {
            Some(name) => name.to_owned(),
            None => self
                .src
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

/// A codec suffix is added only when more than one `.mp4`-family codec is
/// selected, to avoid a collision; the default `[h264, vp9]` case stays
/// unsuffixed.
fn needs_codec_suffix(codecs: &[Codec], codec: Codec) -> bool {
    if codec == Codec::Vp9 {
        return false;
    }
    codecs.iter().filter(|c| **c != Codec::Vp9).count() > 1
}

pub fn rendition_path(naming: &Naming, codecs: &[Codec], width: u32, codec: Codec) -> PathBuf {
    let stem = naming.name_or_source_stem();
    let ext = match codec {
        Codec::Vp9 => "webm",
        _ => "mp4",
    };
    let filename = if needs_codec_suffix(codecs, codec) {
        format!("{stem}-{width}-{}.{ext}", codec_suffix(codec))
    } else {
        format!("{stem}-{width}.{ext}")
    };
    naming.join(filename)
}

pub fn poster_path(naming: &Naming, width: u32) -> PathBuf {
    naming.join(format!(
        "{}-{width}-poster.jpg",
        naming.name_or_source_stem()
    ))
}

pub fn subtitles_path(naming: &Naming) -> PathBuf {
    naming.join(format!("{}.vtt", naming.name_or_source_stem()))
}

pub fn codec_suffix(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "h264",
        Codec::H265 => "h265",
        Codec::Vp9 => "vp9",
        Codec::Av1 => "av1",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// vp9 is alone in webm, so it never takes the suffix its mp4-family
    /// siblings take on once there is more than one of them.
    #[test]
    fn a_second_mp4_family_codec_suffixes_only_those() {
        let naming = Naming {
            out_dir: Path::new("assets/video"),
            src: Path::new("interview.mp4"),
            name: None,
        };
        let codecs = [Codec::H264, Codec::Vp9, Codec::Av1];

        assert_eq!(
            rendition_path(&naming, &codecs, 960, Codec::H264),
            PathBuf::from("assets/video/interview-960-h264.mp4")
        );
        assert_eq!(
            rendition_path(&naming, &codecs, 960, Codec::Vp9),
            PathBuf::from("assets/video/interview-960.webm")
        );
        assert_eq!(
            rendition_path(&naming, &[Codec::H264, Codec::Vp9], 960, Codec::H264),
            PathBuf::from("assets/video/interview-960.mp4")
        );
    }

    /// `--out-dir .` shouldn't leave a `./` on every path recorded in the
    /// lockfile and printed in the plan.
    #[test]
    fn the_current_directory_leaves_no_prefix_on_a_path() {
        let naming = Naming {
            out_dir: Path::new("."),
            src: Path::new("interview.mp4"),
            name: None,
        };
        assert_eq!(
            rendition_path(&naming, &[Codec::H264], 960, Codec::H264),
            PathBuf::from("interview-960.mp4")
        );
        assert_eq!(subtitles_path(&naming), PathBuf::from("interview.vtt"));
    }
}

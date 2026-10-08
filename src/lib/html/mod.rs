//! `--html`: a `<video>` snippet per target, built from its outputs on disk.

pub mod media;

use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, CONTROLS, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::config::Codec;
use crate::plan::Plan;
use crate::sources::OutputProbe;
use crate::task::TaskKind;

/// The poster `src` for browsers that ignore `srcset`.
const FALLBACK_POSTER_WIDTH: u32 = 960;

const URL_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// `srcset` splits its entries on whitespace.
const BASE_URL: &AsciiSet = &CONTROLS.add(b' ');

/// A rendition ffprobe couldn't read, so its `<source type>` is unknown.
#[derive(Debug)]
pub struct UnreadableOutput {
    pub path: PathBuf,
}

struct Target {
    name: String,
    videos: Vec<Video>,
    posters: Vec<Poster>,
    subtitles: Option<String>,
}

struct Video {
    url: String,
    codec: Codec,
    probe: OutputProbe,
}

struct Poster {
    url: String,
    width: u32,
}

/// `names` is indexed by a task's target. Without a base URL, paths are
/// relative to the out_dir.
pub fn render(
    plan: &Plan,
    names: &[String],
    base_url: Option<&str>,
) -> Result<String, UnreadableOutput> {
    let targets = names
        .iter()
        .enumerate()
        .map(|(index, name)| target(plan, index, name, base_url))
        .collect::<Result<Vec<_>, _>>()?;
    let snippets: Vec<String> = targets.iter().filter_map(snippet).collect();
    Ok(format!("{}\n", snippets.join("\n\n")))
}

fn target(
    plan: &Plan,
    index: usize,
    name: &str,
    base_url: Option<&str>,
) -> Result<Target, UnreadableOutput> {
    let mut target = Target {
        name: name.to_string(),
        videos: Vec::new(),
        posters: Vec::new(),
        subtitles: None,
    };

    for task in plan.tasks.iter().filter(|t| t.id.target == index) {
        let Some(path) = task.output_path() else {
            continue;
        };
        let url = url(base_url, path);

        match task.id.kind {
            TaskKind::Rendition { codec, .. } => {
                let probe = crate::sources::probe_output(path).ok_or_else(|| UnreadableOutput {
                    path: path.to_path_buf(),
                })?;
                target.videos.push(Video { url, codec, probe });
            }
            TaskKind::Poster { width } => target.posters.push(Poster { url, width }),
            TaskKind::Subtitles => target.subtitles = Some(url),
            TaskKind::Loudness => {}
        }
    }

    Ok(target)
}

fn snippet(target: &Target) -> Option<String> {
    let mut videos: Vec<&Video> = target.videos.iter().collect();
    videos.sort_by_key(|v| (std::cmp::Reverse(v.probe.width), codec_rank(v.codec)));

    let mut posters: Vec<&Poster> = target.posters.iter().collect();
    posters.sort_by_key(|p| p.width);

    let widest = &videos.first()?.probe;
    let (width, height) = (widest.width, widest.height);

    let mut widths: Vec<u32> = videos.iter().map(|v| v.probe.width).collect();
    widths.dedup();

    let mut lines = vec![
        format!("<!-- boxset html: {} -->", comment_text(&target.name)),
        format!(r#"<div style="display: grid; aspect-ratio: {width} / {height}">"#),
    ];

    lines.extend(img(&posters, width, height));
    lines.push(r#"  <video style="grid-area: 1 / 1; width: 100%; height: auto""#.to_string());
    lines.push(format!(
        r#"         controls playsinline preload="none" crossorigin="anonymous" width="{width}" height="{height}">"#
    ));

    for video in &videos {
        let media = match media_query(&widths, video.probe.width) {
            Some(query) => format!(r#" media="{query}""#),
            None => String::new(),
        };
        // The type is ours and holds double quotes, so it's single-quoted.
        lines.push(format!(
            r#"    <source{media} src="{}" type='{}'>"#,
            escape(&video.url),
            media::mime(video.codec, &video.probe)
        ));
    }

    if let Some(track) = &target.subtitles {
        lines.push(format!(
            r#"    <track kind="subtitles" src="{}">"#,
            escape(track)
        ));
    }

    lines.push("  </video>".to_string());
    lines.push("</div>".to_string());

    Some(lines.join("\n"))
}

/// `posters` are narrowest first.
fn img(posters: &[&Poster], width: u32, height: u32) -> Vec<String> {
    let Some(fallback) = posters
        .iter()
        .rev()
        .find(|p| p.width <= FALLBACK_POSTER_WIDTH)
        .or(posters.first())
    else {
        return Vec::new();
    };

    let mut lines = vec![
        r#"  <img style="grid-area: 1 / 1; width: 100%; height: auto""#.to_string(),
        format!(r#"       src="{}""#, escape(&fallback.url)),
    ];
    let size = format!(r#"width="{width}" height="{height}" alt="">"#);

    match posters.len() > 1 {
        true => {
            let srcset = posters
                .iter()
                .map(|p| format!("{} {}w", p.url, p.width))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(r#"       srcset="{}""#, escape(&srcset)));
            lines.push(format!(r#"       sizes="100vw" {size}"#));
        }
        false => lines.push(format!("       {size}")),
    }

    lines
}

/// An HTML comment can't contain `--`.
fn comment_text(text: &str) -> String {
    let mut text = text.to_string();
    while text.contains("--") {
        text = text.replace("--", "- -");
    }
    text
}

/// For text inside a double-quoted attribute.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Best-compressing first, so a browser takes the smallest file it can play.
fn codec_rank(codec: Codec) -> u8 {
    match codec {
        Codec::Av1 => 0,
        Codec::H265 => 1,
        Codec::Vp9 => 2,
        Codec::H264 => 3,
    }
}

fn media_query(widths: &[u32], width: u32) -> Option<String> {
    let next = widths.iter().copied().find(|&w| w < width)?;
    Some(format!(
        "(min-width: {}px), (min-resolution: 2dppx) and (min-width: {}px)",
        next + 1,
        next / 2 + 1
    ))
}

/// Outputs sit directly in the out_dir, so a path's file name is its URL
/// relative to the out_dir.
fn url(base: Option<&str>, path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| utf8_percent_encode(&name.to_string_lossy(), URL_SEGMENT).to_string())
        .unwrap_or_default();
    let Some(base) = base else {
        return name;
    };
    let base = utf8_percent_encode(base, BASE_URL).to_string();
    match base.ends_with('/') {
        true => format!("{base}{name}"),
        false => format!("{base}/{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_text_has_no_double_hyphens() {
        assert_eq!(comment_text("a---b"), "a- - -b");
        assert_eq!(comment_text("a----b"), "a- - - -b");
    }

    #[test]
    fn a_silent_video_has_controls_and_no_autoplay() {
        let target = Target {
            name: "clip".to_string(),
            videos: vec![Video {
                url: "clip-320.mp4".to_string(),
                codec: Codec::H264,
                probe: OutputProbe {
                    width: 320,
                    height: 180,
                    has_audio: false,
                    profile: Some("High".to_string()),
                    level: Some(13),
                },
            }],
            posters: vec![],
            subtitles: Some("clip.vtt".to_string()),
        };
        let html = snippet(&target).unwrap();
        assert!(
            html.contains(r#"controls playsinline preload="none""#),
            "{html}"
        );
        assert!(!html.contains("autoplay"), "{html}");
        assert!(
            html.contains(r#"<track kind="subtitles" src="clip.vtt">"#),
            "{html}"
        );
        assert!(!html.contains("<img"), "{html}");
    }

    #[test]
    fn urls_are_file_names_and_percent_encoded() {
        let file = Path::new("site/my video/clip #1&2.mp4");
        assert_eq!(url(None, file), "clip%20%231%262.mp4");
        assert_eq!(
            url(Some("https://cdn.example.com/v"), file),
            "https://cdn.example.com/v/clip%20%231%262.mp4"
        );
        assert_eq!(
            url(Some("https://cdn.example.com/v/"), file),
            "https://cdn.example.com/v/clip%20%231%262.mp4"
        );
        assert_eq!(
            url(Some("https://cdn.example.com/my videos"), file),
            "https://cdn.example.com/my%20videos/clip%20%231%262.mp4"
        );
    }
}

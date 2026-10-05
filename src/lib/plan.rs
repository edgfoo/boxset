//! `plan()` turns resolved settings into the list of tasks a run will do.

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Codec;
use crate::outputs::{self, Naming};
use crate::settings::{PosterSettings, Settings, Timestamp};
use crate::sources::Probe;
use crate::task::{Task, TaskId, TaskKind, TaskWork};

/// Which targets a run covers. Empty means every target.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub names: Vec<String>,
}

impl Selection {
    pub fn all() -> Self {
        Self::default()
    }

    pub fn covers_all(&self) -> bool {
        self.names.is_empty()
    }

    fn covers(&self, settings: &Settings) -> bool {
        if self.covers_all() {
            return true;
        }
        self.names.contains(&target_name(settings))
    }

    pub fn covers_config(&self, config: &crate::config::TargetConfig) -> bool {
        if self.covers_all() {
            return true;
        }
        let name = config.name.clone().or_else(|| {
            config
                .src
                .as_deref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
        });
        name.is_some_and(|name| self.names.contains(&name))
    }
}

pub fn target_name(settings: &Settings) -> String {
    settings.name.clone().unwrap_or_else(|| {
        settings
            .src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub tasks: Vec<Task>,
}

/// `settings` and `probes` are parallel. `probes[i]` is the source probe for
/// `settings[i]`.
pub fn plan(settings: &[Settings], probes: &[Arc<Probe>], selection: &Selection) -> Plan {
    let mut tasks = Vec::new();

    for (target, (settings, probe)) in settings.iter().zip(probes).enumerate() {
        if !selection.covers(settings) {
            continue;
        }
        tasks.extend(plan_target(target, settings, probe));
    }

    Plan { tasks }
}

/// Every output the config names, whatever a run was filtered to.
pub fn all_output_paths(settings: &[Settings]) -> Vec<PathBuf> {
    settings
        .iter()
        .flat_map(|settings| {
            outputs::target_output_paths(
                &naming(settings),
                &settings.widths,
                &settings.codecs,
                settings.poster.is_some(),
                settings.subtitles.is_some(),
            )
        })
        .collect()
}

fn plan_target(target: usize, settings: &Settings, probe: &Arc<Probe>) -> Vec<Task> {
    let mut tasks = Vec::new();
    let naming = naming(settings);

    for &width in &settings.widths {
        for &codec in &settings.codecs {
            let path = outputs::rendition_path(&naming, &settings.codecs, width, codec);
            tasks.push(make_task(
                target,
                probe,
                TaskKind::Rendition { width, codec },
                TaskWork::Rendition {
                    output: output(path),
                    codec,
                    width,
                    options: codec_options(settings, codec),
                    trim: settings.trim,
                    crop: settings.crop,
                    fps: settings.fps,
                    audio: settings.audio.clone(),
                },
            ));
        }

        if let Some(poster) = &settings.poster {
            tasks.push(make_task(
                target,
                probe,
                TaskKind::Poster { width },
                TaskWork::Poster {
                    output: output(outputs::poster_path(&naming, width)),
                    width,
                    at: poster_timestamp(poster, settings),
                    crop: settings.crop,
                },
            ));
        }
    }

    if let Some(subtitles) = &settings.subtitles {
        tasks.push(make_task(
            target,
            probe,
            TaskKind::Subtitles,
            TaskWork::Subtitles {
                output: output(outputs::subtitles_path(&naming)),
                model: subtitles.model,
                trim: settings.trim,
                extra_args: Vec::new(),
            },
        ));
    }

    // Every rendition that runs needs a measurement, so this goes in whenever one of them will normalise.
    if needs_loudness(settings, &tasks) {
        tasks.insert(
            0,
            make_task(
                target,
                probe,
                TaskKind::Loudness,
                TaskWork::Loudness {
                    trim: settings.trim,
                },
            ),
        );
    }

    tasks
}

fn needs_loudness(settings: &Settings, tasks: &[Task]) -> bool {
    let normalizing = settings.audio.as_ref().is_some_and(|a| a.normalize);
    normalizing
        && tasks
            .iter()
            .any(|t| matches!(t.work, TaskWork::Rendition { .. }))
}

fn output(path: PathBuf) -> crate::task::Output {
    crate::task::Output {
        exists: path.exists(),
        path,
    }
}

fn make_task(target: usize, probe: &Arc<Probe>, kind: TaskKind, work: TaskWork) -> Task {
    Task {
        id: TaskId { target, kind },
        probe: Arc::clone(probe),
        work,
    }
}

/// The first frame after trim, unless the poster names an explicit
/// timestamp into the source.
fn poster_timestamp(poster: &PosterSettings, settings: &Settings) -> Timestamp {
    poster
        .at
        .unwrap_or_else(|| Timestamp(settings.trim.map(|t| t.start_secs).unwrap_or(0.0)))
}

fn naming(settings: &Settings) -> Naming<'_> {
    Naming {
        out_dir: &settings.out_dir,
        src: &settings.src,
        name: settings.name.as_deref(),
    }
}

fn codec_options(settings: &Settings, codec: Codec) -> crate::settings::CodecOptions {
    match codec {
        Codec::H264 => settings.h264.clone(),
        Codec::H265 => settings.h265.clone(),
        Codec::Vp9 => settings.vp9.clone(),
        Codec::Av1 => settings.av1.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AudioSettings, SubtitleSettings};
    use crate::sources::Probe;
    use std::path::PathBuf;

    fn probe() -> Arc<Probe> {
        Arc::new(Probe {
            src: PathBuf::from("interview.mp4"),
            width: 1920,
            height: 1080,
            duration_secs: 10.0,
            frame_rate: (30, 1),
            has_audio: true,
            video_codec: "h264".to_string(),
            audio_codec: Some("aac".to_string()),
            size_bytes: 1_000_000,
        })
    }

    fn settings() -> Settings {
        Settings {
            src: PathBuf::from("interview.mp4"),
            name: None,
            out_dir: PathBuf::from("assets/video"),
            codecs: vec![Codec::H264, Codec::Vp9],
            crop: None,
            widths: vec![480, 960],
            trim: None,
            fps: None,
            audio: Some(AudioSettings {
                normalize: false,
                bitrate: None,
                quality: crate::config::Quality::Balanced,
            }),
            poster: Some(PosterSettings { at: None }),
            subtitles: Some(SubtitleSettings {
                model: crate::config::TranscriptionModel::Parakeet(
                    crate::config::ParakeetTier::Multilingual,
                ),
            }),
            h264: Default::default(),
            h265: Default::default(),
            vp9: Default::default(),
            av1: Default::default(),
        }
    }

    #[test]
    fn default_codecs_get_no_suffix() {
        let plan = plan(&[settings()], &[probe()], &Selection::all());
        let h264_task = plan
            .tasks
            .iter()
            .find(|t| {
                matches!(
                    t.id.kind,
                    TaskKind::Rendition {
                        width: 480,
                        codec: Codec::H264
                    }
                )
            })
            .unwrap();
        assert_eq!(
            h264_task.output_path().unwrap(),
            PathBuf::from("assets/video/interview-480.mp4")
        );
    }

    #[test]
    fn three_mp4_family_codecs_get_suffixed() {
        let mut cfg = settings();
        cfg.codecs = vec![Codec::H264, Codec::Vp9, Codec::Av1];
        let plan = plan(&[cfg], &[probe()], &Selection::all());
        let h264_path = plan
            .tasks
            .iter()
            .find(|t| {
                matches!(
                    t.id.kind,
                    TaskKind::Rendition {
                        width: 480,
                        codec: Codec::H264
                    }
                )
            })
            .unwrap()
            .output_path()
            .unwrap()
            .to_path_buf();
        let vp9_path = plan
            .tasks
            .iter()
            .find(|t| {
                matches!(
                    t.id.kind,
                    TaskKind::Rendition {
                        width: 480,
                        codec: Codec::Vp9
                    }
                )
            })
            .unwrap()
            .output_path()
            .unwrap()
            .to_path_buf();
        assert_eq!(
            h264_path,
            PathBuf::from("assets/video/interview-480-h264.mp4")
        );
        assert_eq!(vp9_path, PathBuf::from("assets/video/interview-480.webm"));
    }

    #[test]
    fn name_replaces_the_source_stem_in_output_paths() {
        let mut cfg = settings();
        cfg.name = Some("wide".to_string());
        let plan = plan(&[cfg], &[probe()], &Selection::all());
        let poster = plan
            .tasks
            .iter()
            .find(|t| matches!(t.id.kind, TaskKind::Poster { width: 480 }))
            .unwrap();
        assert_eq!(
            poster.output_path().unwrap(),
            PathBuf::from("assets/video/wide-480-poster.jpg")
        );
    }

    #[test]
    fn no_poster_setting_means_no_poster_task() {
        let mut cfg = settings();
        cfg.poster = None;
        let plan = plan(&[cfg], &[probe()], &Selection::all());
        assert!(
            !plan
                .tasks
                .iter()
                .any(|t| matches!(t.id.kind, TaskKind::Poster { .. }))
        );
    }

    #[test]
    fn selection_filters_to_named_target() {
        let mut first = settings();
        first.name = Some("wide".to_string());
        let mut second = settings();
        second.name = Some("tall".to_string());

        let selection = Selection {
            names: vec!["tall".to_string()],
        };
        let plan = plan(&[first, second], &[probe(), probe()], &selection);
        assert!(plan.tasks.iter().all(|t| t.id.target == 1));
    }

    #[test]
    fn selection_falls_back_to_source_stem_when_unnamed() {
        let selection = Selection {
            names: vec!["interview".to_string()],
        };
        let plan = plan(&[settings()], &[probe()], &selection);
        assert!(!plan.tasks.is_empty());
    }

    #[test]
    fn every_target_gets_exactly_one_subtitles_task() {
        let plan = plan(&[settings()], &[probe()], &Selection::all());
        assert_eq!(
            plan.tasks
                .iter()
                .filter(|t| matches!(t.id.kind, TaskKind::Subtitles))
                .count(),
            1
        );
    }
}

//! Runs the plan's tasks and writes the outputs. Execution decides only
//! ordering and concurrency; each task's executor turns intent into a
//! command or call.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::command;
use crate::config::{Codec, WhisperModel};
use crate::environment::resolve_tool_path;
use crate::error::{BoxsetError, FfmpegError, Tool, TranscribeError, classify_ffmpeg_failure};
use crate::plan::Plan;
use crate::report::{Phase, Reporter, TaskOutcome, TaskReport};
use crate::settings::expand_quality;
use crate::task::{Task, TaskId, TaskWork};
use crate::transcribe;

#[derive(Debug, Default)]
pub struct ExecutionOutcome {
    pub succeeded: usize,
    pub failed: usize,
    /// The tasks that wrote their output. A failed task is absent.
    pub produced: Vec<TaskId>,
}

/// What a worker reports as it goes
enum Event {
    Started(TaskId),
    Stage(TaskId, &'static str, u32, u32),
    Progress(TaskId, &'static str, f32, f32),
    Finished(TaskId, Result<(), BoxsetError>, Duration, Option<u64>),
}

pub fn execute(plan: &Plan, reporter: &mut dyn Reporter, jobs: usize) -> ExecutionOutcome {
    if plan.tasks.is_empty() {
        return ExecutionOutcome::default();
    }

    reporter.phase(Phase::Encoding);

    // The task list never changes, so workers share one cursor into it rather
    // than a locked queue.
    let cursor = AtomicUsize::new(0);
    let tasks = plan.tasks.as_slice();
    let (tx, rx) = mpsc::channel::<Event>();
    let workers = jobs.max(1).min(tasks.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let cursor = &cursor;
            let tx = tx.clone();
            scope.spawn(move || {
                loop {
                    let next = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(task) = tasks.get(next) else {
                        break;
                    };

                    let _ = tx.send(Event::Started(task.id));
                    let started = Instant::now();
                    let result = run_task(task, &tx);
                    let elapsed = started.elapsed();
                    let bytes = result
                        .is_ok()
                        .then(|| std::fs::metadata(&task.output_path).ok().map(|m| m.len()))
                        .flatten();
                    let _ = tx.send(Event::Finished(task.id, result, elapsed, bytes));
                }
            });
        }
        drop(tx);

        let mut outcome = ExecutionOutcome::default();
        for event in rx {
            match event {
                Event::Started(id) => reporter.task_started(id),
                Event::Stage(id, stage, index, total) => {
                    reporter.task_stage(id, stage, index, total)
                }
                Event::Progress(id, stage, overall, stage_done) => {
                    reporter.task_progress(id, stage, overall, stage_done)
                }
                Event::Finished(id, result, elapsed, bytes) => {
                    let outcome_kind = match result {
                        Ok(()) => {
                            outcome.succeeded += 1;
                            outcome.produced.push(id);
                            TaskOutcome::Succeeded
                        }
                        Err(e) => {
                            outcome.failed += 1;
                            TaskOutcome::Failed(e)
                        }
                    };
                    reporter.task_finished(
                        id,
                        TaskReport {
                            outcome: outcome_kind,
                            elapsed,
                            bytes,
                        },
                    );
                }
            }
        }

        outcome
    })
}

/// Written to a temp path and renamed on success, so a failed or interrupted
/// task never leaves a truncated file where a valid one was. The original
/// extension stays last: ffmpeg picks the output format from it.
fn temp_path(output: &Path) -> PathBuf {
    let stem = output.file_stem().unwrap_or_default();
    let mut name = stem.to_os_string();
    name.push(format!(".{}.partial", std::process::id()));
    if let Some(ext) = output.extension() {
        name.push(".");
        name.push(ext);
    }
    output.with_file_name(name)
}

fn run_task(task: &Task, tx: &mpsc::Sender<Event>) -> Result<(), BoxsetError> {
    match &task.work {
        TaskWork::Subtitles {
            language,
            model,
            trim,
            ..
        } => run_subtitles_task(task, language.as_deref(), *model, *trim, tx),
        _ => run_ffmpeg_task(task, tx),
    }
}

/// Extract 16kHz mono PCM, transcribe it, write WebVTT. Two stages, since the
/// extraction is a visible wait of its own on a long source.
fn run_subtitles_task(
    task: &Task,
    language: Option<&str>,
    model: WhisperModel,
    trim: Option<crate::settings::TimeRange>,
    tx: &mpsc::Sender<Event>,
) -> Result<(), BoxsetError> {
    let fail = |source| BoxsetError::TranscribeFailed {
        task: task.id,
        source,
    };

    if !task.probe.has_audio {
        return Err(fail(TranscribeError::NoAudioTrack));
    }

    if let Some(parent) = task.output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| BoxsetError::WriteFailed {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let tmp = temp_path(&task.output_path);
    let audio = tmp.with_extension("pcm");
    let stages = command::subtitle_stages();

    let _ = tx.send(Event::Stage(task.id, stages[0], 1, stages.len() as u32));
    let args = command::audio_extract_args(&task.probe.src, &audio, trim);
    let extracted = run_ffmpeg(&args, |_| {}, 0.0);
    if let Err(source) = extracted {
        let _ = std::fs::remove_file(&audio);
        return Err(fail(TranscribeError::AudioExtract(source)));
    }

    let _ = tx.send(Event::Stage(task.id, stages[1], 2, stages.len() as u32));
    let result = transcribe_extracted(task, &audio, model, language, tx);
    let _ = std::fs::remove_file(&audio);
    let cues = result.map_err(fail)?;

    std::fs::write(&tmp, transcribe::to_webvtt(&cues)).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        BoxsetError::WriteFailed {
            path: tmp.clone(),
            source,
        }
    })?;

    let renamed = std::fs::rename(&tmp, &task.output_path);
    let _ = std::fs::remove_file(&tmp);
    renamed.map_err(|source| BoxsetError::WriteFailed {
        path: task.output_path.clone(),
        source,
    })
}

fn transcribe_extracted(
    task: &Task,
    audio: &Path,
    model: WhisperModel,
    language: Option<&str>,
    tx: &mpsc::Sender<Event>,
) -> Result<Vec<transcribe::Cue>, TranscribeError> {
    let pcm = std::fs::read(audio).map_err(|e| {
        TranscribeError::AudioExtract(FfmpegError {
            kind: crate::error::FfmpegErrorKind::Unclassified,
            stderr: e.to_string(),
        })
    })?;

    let stage = command::subtitle_stages()[1];
    // whisper-rs never drops the callback, so it must not hold a Sender: that
    // clone would keep the channel open and `execute` would never return.
    let percent = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&percent);
    let cues = transcribe::transcribe(
        model,
        &crate::environment::model_path(model),
        &transcribe::pcm_s16le_to_f32(&pcm),
        language,
        move |done| {
            counter.store((done * 100.0) as usize, Ordering::Relaxed);
        },
    )?;

    let done = percent.load(Ordering::Relaxed) as f32 / 100.0;
    let _ = tx.send(Event::Progress(task.id, stage, done, done));
    Ok(cues)
}

fn run_ffmpeg_task(task: &Task, tx: &mpsc::Sender<Event>) -> Result<(), BoxsetError> {
    if let Some(parent) = task.output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| BoxsetError::WriteFailed {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let tmp = temp_path(&task.output_path);
    let src = &task.probe.src;

    let (stage_args, stage_names, passlog, codec) = match &task.work {
        TaskWork::Rendition {
            codec,
            width,
            quality,
            overrides,
            trim,
            crop,
            fps,
            audio,
        } => {
            let expanded = expand_quality(*quality, *codec);
            let built = command::rendition_args(
                src,
                &tmp,
                *codec,
                *width,
                overrides,
                &expanded,
                *trim,
                *crop,
                *fps,
                audio.as_ref(),
                &task.probe,
            );
            (
                built.stages,
                command::stages(*codec),
                built.passlog,
                Some(*codec),
            )
        }
        TaskWork::Poster { width, at, crop } => (
            vec![command::poster_args(
                src,
                &tmp,
                *width,
                *at,
                *crop,
                &task.probe,
            )],
            command::stages(Codec::H264),
            None,
            None,
        ),
        TaskWork::Subtitles { .. } => unreachable!("subtitles do not run through ffmpeg here"),
    };

    let total = stage_args.len() as u32;
    let duration = stage_duration(task);

    let cleanup = |tmp: &Path, passlog: &Option<String>| {
        let _ = std::fs::remove_file(tmp);
        if let Some(log) = passlog {
            // libvpx appends a suffix to the name it is given.
            let _ = std::fs::remove_file(log);
            let _ = std::fs::remove_file(format!("{log}-0.log"));
        }
    };

    for (index, args) in stage_args.iter().enumerate() {
        let name = stage_names.get(index).copied().unwrap_or("encode");
        let _ = tx.send(Event::Stage(task.id, name, index as u32 + 1, total));

        let result = run_ffmpeg(
            args,
            |stage_done| {
                let overall = match codec {
                    Some(codec) => command::overall_progress(codec, index as u32, stage_done),
                    None => stage_done,
                };
                let _ = tx.send(Event::Progress(task.id, name, overall, stage_done));
            },
            duration,
        );

        if let Err(source) = result {
            cleanup(&tmp, &passlog);
            return Err(BoxsetError::EncodeFailed {
                task: task.id,
                stage: (total > 1).then_some(name),
                source,
            });
        }
    }

    let renamed = std::fs::rename(&tmp, &task.output_path);
    cleanup(&tmp, &passlog);
    renamed.map_err(|source| BoxsetError::WriteFailed {
        path: task.output_path.clone(),
        source,
    })
}

/// How much of the source a task encodes, for turning ffmpeg's progress
/// output into a fraction.
fn stage_duration(task: &Task) -> f64 {
    let full = task.probe.duration_secs;
    let TaskWork::Rendition { trim: Some(t), .. } = &task.work else {
        return full;
    };
    t.end_secs.unwrap_or(full) - t.start_secs
}

fn run_ffmpeg(
    args: &[String],
    mut on_progress: impl FnMut(f32),
    duration: f64,
) -> Result<(), FfmpegError> {
    let Some(ffmpeg) = resolve_tool_path(Tool::Ffmpeg) else {
        return Err(FfmpegError {
            kind: crate::error::FfmpegErrorKind::Unclassified,
            stderr: "ffmpeg not found".to_string(),
        });
    };

    let mut child = match Command::new(ffmpeg)
        .args(args)
        .args(["-progress", "pipe:1", "-nostats"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return Err(FfmpegError {
                kind: crate::error::FfmpegErrorKind::Unclassified,
                stderr: e.to_string(),
            });
        }
    };

    // Both pipes are drained concurrently: reading one to completion while the
    // other fills its buffer deadlocks, and a failing encode can be verbose.
    let stderr = std::thread::scope(|scope| {
        let stderr = child.stderr.take().map(|stderr| {
            scope.spawn(move || {
                let mut text = String::new();
                let _ = BufReader::new(stderr).read_to_string(&mut text);
                text
            })
        });

        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // `out_time_us=` is microseconds of output written so far.
                if let Some(us) = line.strip_prefix("out_time_us=")
                    && let Ok(us) = us.trim().parse::<i64>()
                    && duration > 0.0
                {
                    let done = (us as f64 / 1_000_000.0 / duration).clamp(0.0, 1.0);
                    on_progress(done as f32);
                }
            }
        }

        stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    });

    let status = match child.wait() {
        Ok(status) => status,
        Err(e) => {
            return Err(FfmpegError {
                kind: crate::error::FfmpegErrorKind::Unclassified,
                stderr: e.to_string(),
            });
        }
    };

    if status.success() {
        return Ok(());
    }

    Err(FfmpegError {
        kind: classify_ffmpeg_failure(&stderr),
        stderr,
    })
}

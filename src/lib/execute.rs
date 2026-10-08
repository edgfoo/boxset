//! Runs the plan's tasks and writes the outputs. Execution decides only
//! ordering and concurrency; each task's executor turns intent into a
//! command or call.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::cancel::Cancel;
use crate::command;
use crate::config::{Codec, TranscriptionModel};
use crate::environment::resolve_tool_path;
use crate::error::{BoxsetError, FfmpegError, Tool, TranscribeError, classify_ffmpeg_failure};
use crate::plan::Plan;
use crate::report::{Phase, Reporter, TaskOutcome, TaskReport};
use crate::task::{Task, TaskId, TaskKind, TaskOutput, TaskWork};
use crate::temp::TempFile;
use crate::transcribe;

#[derive(Debug, Default)]
pub struct ExecutionOutcome {
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub produced: Vec<TaskId>,
    /// The ffmpeg commands each task spawned, in the order they ran
    pub commands: HashMap<TaskId, Vec<String>>,
}

enum Event {
    Started(TaskId),
    Stage(TaskId, &'static str, u32, u32),
    Progress(TaskId, &'static str, f32, f32),
    Spawned(TaskId, String),
    Finished(TaskId, Result<(), BoxsetError>, Duration, Option<u64>),
}

struct Run<'a> {
    tx: &'a mpsc::Sender<Event>,
    cancel: &'a Cancel,
}

impl Run<'_> {
    fn send(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

pub fn execute(
    plan: &Plan,
    reporter: &mut dyn Reporter,
    jobs: usize,
    cancel: &Cancel,
) -> ExecutionOutcome {
    if plan.tasks.is_empty() {
        return ExecutionOutcome::default();
    }

    reporter.phase(Phase::Encoding);

    let queue = Queue::new(&plan.tasks);
    let (tx, rx) = mpsc::channel::<Event>();
    let workers = jobs.max(1).min(plan.tasks.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let queue = &queue;
            let cancel = &cancel;
            let tx = tx.clone();
            scope.spawn(move || {
                let run = Run { tx: &tx, cancel };

                // Tasks that never started are not reported at all.
                while let Some(mut claim) = queue.claim(cancel) {
                    let task = claim.task;
                    run.send(Event::Started(task.id));
                    let started = Instant::now();
                    let result = match run_task(task, &claim.inputs, &run) {
                        Ok(output) => {
                            claim.output = output;
                            Ok(())
                        }
                        Err(e) => Err(e),
                    };
                    let elapsed = started.elapsed();
                    let bytes = match result.is_ok() {
                        true => task
                            .output_path()
                            .and_then(|path| std::fs::metadata(path).ok())
                            .map(|m| m.len()),
                        false => None,
                    };
                    drop(claim);
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
                Event::Spawned(id, command) => {
                    outcome.commands.entry(id).or_default().push(command)
                }
                Event::Finished(id, result, elapsed, bytes) => {
                    let outcome_kind = match result {
                        Ok(()) => {
                            outcome.succeeded += 1;
                            if id.kind != TaskKind::Loudness {
                                outcome.produced.push(id);
                            }
                            TaskOutcome::Succeeded
                        }
                        Err(e) if e.is_cancelled() => {
                            outcome.cancelled += 1;
                            TaskOutcome::Cancelled
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

/// Hands out tasks in plan order, skipping any that depend on an unfinished
/// task of the same target.
struct Queue<'a> {
    state: Mutex<QueueState<'a>>,
    changed: Condvar,
}

struct QueueState<'a> {
    pending: Vec<&'a Task>,
    /// Each target's tasks that are pending or running
    unfinished: HashMap<usize, HashSet<TaskKind>>,
    /// Each target's finished tasks that produced an output
    outputs: HashMap<usize, Vec<(TaskKind, TaskOutput)>>,
}

impl QueueState<'_> {
    fn ready(&self, id: TaskId) -> bool {
        !self.unfinished[&id.target]
            .iter()
            .any(|other| id.kind.depends_on(other))
    }
}

impl<'a> Queue<'a> {
    fn new(tasks: &'a [Task]) -> Self {
        let mut unfinished: HashMap<usize, HashSet<TaskKind>> = HashMap::new();
        for task in tasks {
            unfinished
                .entry(task.id.target)
                .or_default()
                .insert(task.id.kind);
        }

        Self {
            state: Mutex::new(QueueState {
                pending: tasks.iter().collect(),
                unfinished,
                outputs: HashMap::new(),
            }),
            changed: Condvar::new(),
        }
    }

    /// Waits only while every pending task depends on one that is running.
    fn claim(&self, cancel: &Cancel) -> Option<Claim<'_, 'a>> {
        let mut state = self.state.lock().unwrap();
        loop {
            if cancel.is_cancelled() || state.pending.is_empty() {
                return None;
            }

            if let Some(index) = state.pending.iter().position(|task| state.ready(task.id)) {
                let task = state.pending.remove(index);
                let inputs = state
                    .outputs
                    .get(&task.id.target)
                    .into_iter()
                    .flatten()
                    .filter(|(kind, _)| task.id.kind.depends_on(kind))
                    .map(|(_, output)| output.clone())
                    .collect();

                return Some(Claim {
                    queue: self,
                    task,
                    inputs,
                    output: None,
                });
            }

            state = self.changed.wait(state).unwrap();
        }
    }

    fn finish(&self, id: TaskId, output: Option<TaskOutput>) {
        let mut state = self.state.lock().unwrap();

        if let Some(unfinished) = state.unfinished.get_mut(&id.target) {
            unfinished.remove(&id.kind);
        }

        if let Some(output) = output {
            state
                .outputs
                .entry(id.target)
                .or_default()
                .push((id.kind, output));
        }
        self.changed.notify_all();
    }
}

/// Finishes its task when dropped, so tasks waiting on it are released even
/// if it panics.
struct Claim<'q, 'a> {
    queue: &'q Queue<'a>,
    task: &'a Task,
    /// The outputs of the tasks this one depends on
    inputs: Vec<TaskOutput>,
    output: Option<TaskOutput>,
}

impl Drop for Claim<'_, '_> {
    fn drop(&mut self) {
        self.queue.finish(self.task.id, self.output.take());
    }
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

fn run_task(
    task: &Task,
    inputs: &[TaskOutput],
    run: &Run,
) -> Result<Option<TaskOutput>, BoxsetError> {
    match &task.work {
        TaskWork::Subtitles { model, trim, .. } => {
            run_subtitles_task(task, *model, *trim, run).map(|()| None)
        }
        TaskWork::Loudness { trim } => {
            run_loudness_task(task, *trim, run).map(|measured| Some(TaskOutput::Loudness(measured)))
        }
        _ => {
            let measured = inputs
                .iter()
                .map(|TaskOutput::Loudness(measured)| measured)
                .next();
            run_ffmpeg_task(task, measured, run).map(|()| None)
        }
    }
}

fn run_loudness_task(
    task: &Task,
    trim: Option<crate::settings::TimeRange>,
    run: &Run,
) -> Result<command::LoudnessMeasurement, BoxsetError> {
    let stages = command::loudness_stages();
    run.send(Event::Stage(task.id, stages[0], 1, stages.len() as u32));

    let args = command::loudness_measure_args(&task.probe.src, trim);
    let mut spawned = None;
    let measured = run_ffmpeg(&args, |_| {}, 0.0, &mut spawned, run.cancel);
    if let Some(spawned) = spawned {
        run.send(Event::Spawned(task.id, spawned));
    }

    if run.is_cancelled() {
        return Err(BoxsetError::Cancelled { task: task.id });
    }

    let fail = |source| BoxsetError::EncodeFailed {
        task: task.id,
        stage: None,
        source,
    };

    let stderr = measured.map_err(fail)?;
    command::parse_loudness_measurement(&stderr).ok_or_else(|| {
        fail(FfmpegError {
            kind: crate::error::FfmpegErrorKind::Unclassified,
            stderr,
        })
    })
}

/// Extract 16kHz mono PCM, transcribe it, write WebVTT. Two stages, since the
/// extraction is a visible wait of its own on a long source.
fn run_subtitles_task(
    task: &Task,
    model: TranscriptionModel,
    trim: Option<crate::settings::TimeRange>,
    run: &Run,
) -> Result<(), BoxsetError> {
    let fail = |source| match source {
        TranscribeError::Cancelled => BoxsetError::Cancelled { task: task.id },
        source => BoxsetError::TranscribeFailed {
            task: task.id,
            source,
        },
    };

    if !task.probe.has_audio {
        return Err(fail(TranscribeError::NoAudioTrack));
    }

    let output_path = task.output_path().expect("subtitles write a file");

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| BoxsetError::WriteFailed {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let tmp = TempFile::new(temp_path(output_path));
    let audio = TempFile::new(tmp.path().with_extension("pcm"));
    let stages = command::subtitle_stages();

    run.send(Event::Stage(task.id, stages[0], 1, stages.len() as u32));
    let args = command::audio_extract_args(&task.probe.src, audio.path(), trim);
    let mut spawned = None;
    let extracted = run_ffmpeg(&args, |_| {}, 0.0, &mut spawned, run.cancel);
    if let Some(spawned) = spawned {
        run.send(Event::Spawned(task.id, spawned));
    }
    if let Err(source) = extracted {
        if run.is_cancelled() {
            return Err(BoxsetError::Cancelled { task: task.id });
        }
        return Err(fail(TranscribeError::AudioExtract(source)));
    }

    run.send(Event::Stage(task.id, stages[1], 2, stages.len() as u32));
    let cues = transcribe_extracted(task, audio.path(), model, run).map_err(fail)?;
    drop(audio);

    let vtt = transcribe::to_webvtt(&cues);
    std::fs::write(tmp.path(), vtt).map_err(|source| BoxsetError::WriteFailed {
        path: tmp.path().to_path_buf(),
        source,
    })?;

    std::fs::rename(tmp.path(), output_path).map_err(|source| BoxsetError::WriteFailed {
        path: output_path.to_path_buf(),
        source,
    })?;
    tmp.keep();

    Ok(())
}

fn transcribe_extracted(
    task: &Task,
    audio: &Path,
    model: TranscriptionModel,
    run: &Run,
) -> Result<Vec<transcribe::Cue>, TranscribeError> {
    let pcm = std::fs::read(audio).map_err(|e| {
        TranscribeError::AudioExtract(FfmpegError {
            kind: crate::error::FfmpegErrorKind::Unclassified,
            stderr: e.to_string(),
        })
    })?;

    let cues = transcribe::transcribe(
        model,
        &crate::environment::model_path(model),
        &transcribe::pcm_s16le_to_f32(&pcm),
        run.cancel,
    )?;

    // Neither engine reports progress, so the stage only ever goes 0 to 1.
    let stage = command::subtitle_stages()[1];
    run.send(Event::Progress(task.id, stage, 1.0, 1.0));
    Ok(cues)
}

fn run_ffmpeg_task(
    task: &Task,
    measured: Option<&command::LoudnessMeasurement>,
    run: &Run,
) -> Result<(), BoxsetError> {
    let output_path = task
        .output_path()
        .expect("rendition and poster tasks write a file");

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| BoxsetError::WriteFailed {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let tmp = TempFile::new(temp_path(output_path));
    let src = &task.probe.src;

    let (stage_args, stage_names, passlog, codec) = match &task.work {
        TaskWork::Rendition {
            codec,
            width,
            options,
            trim,
            crop,
            fps,
            audio,
            ..
        } => {
            let built = command::rendition_args(
                src,
                tmp.path(),
                *codec,
                *width,
                options,
                *trim,
                *crop,
                *fps,
                audio.as_ref(),
                measured,
                &task.probe,
            );
            (
                built.stages,
                command::stages(*codec),
                built.passlog,
                Some(*codec),
            )
        }
        TaskWork::Poster {
            width, at, crop, ..
        } => (
            vec![command::poster_args(
                src,
                tmp.path(),
                *width,
                *at,
                *crop,
                &task.probe,
            )],
            command::stages(Codec::H264),
            None,
            None,
        ),
        TaskWork::Subtitles { .. } | TaskWork::Loudness { .. } => {
            unreachable!("handled by run_task")
        }
    };

    let total = stage_args.len() as u32;
    let duration = stage_duration(task);

    // libvpx appends a suffix to the name it is given, so both spellings go.
    let passlogs: Vec<TempFile> = passlog
        .iter()
        .flat_map(|log| [PathBuf::from(log), PathBuf::from(format!("{log}-0.log"))])
        .map(TempFile::new)
        .collect();

    for (index, args) in stage_args.iter().enumerate() {
        let name = stage_names.get(index).copied().unwrap_or("encode");
        run.send(Event::Stage(task.id, name, index as u32 + 1, total));

        let mut spawned = None;
        let result = run_ffmpeg(
            args,
            |stage_done| {
                let overall = match codec {
                    Some(codec) => command::overall_progress(codec, index as u32, stage_done),
                    None => stage_done,
                };
                run.send(Event::Progress(task.id, name, overall, stage_done));
            },
            duration,
            &mut spawned,
            run.cancel,
        );
        if let Some(spawned) = spawned {
            run.send(Event::Spawned(task.id, spawned));
        }

        if run.is_cancelled() {
            return Err(BoxsetError::Cancelled { task: task.id });
        }

        if let Err(source) = result {
            return Err(BoxsetError::EncodeFailed {
                task: task.id,
                stage: (total > 1).then_some(name),
                source,
            });
        }
    }

    std::fs::rename(tmp.path(), output_path).map_err(|source| BoxsetError::WriteFailed {
        path: output_path.to_path_buf(),
        source,
    })?;
    tmp.keep();
    drop(passlogs);

    Ok(())
}

fn stage_duration(task: &Task) -> f64 {
    let full = task.probe.duration_secs;
    let TaskWork::Rendition { trim: Some(t), .. } = &task.work else {
        return full;
    };
    t.end_secs.unwrap_or(full) - t.start_secs
}

/// Arguments containing spaces are quoted, so a filter chain or an `extra_args`
/// value reads as one argument rather than several.
fn render_command(ffmpeg: &Path, args: &[String], trailing: &[&str]) -> String {
    std::iter::once(ffmpeg.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .chain(trailing.iter().map(|s| s.to_string()))
        .map(|part| {
            if part.contains(' ') {
                format!("\"{part}\"")
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const PROGRESS_ARGS: [&str; 5] = [
    "-progress",
    "pipe:1",
    "-nostats",
    // Report progress every 200ms vs the default 500ms
    "-stats_period",
    "0.2",
];

/// SIGKILL, because ffmpeg traps SIGINT and SIGTERM. On those it spends a few
/// hundred milliseconds finishing the output file it was told to abandon.
#[cfg(unix)]
fn kill_now(pid: u32) {
    // SAFETY: a kill to a live or already-exited pid is defined; the worst
    // case is ESRCH, which is ignored.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_now(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

/// Polls until cancelled or until `done` is set, killing the child if it is still running.
fn watch_for_cancellation(cancel: &Cancel, pid: u32, done: &AtomicBool) {
    while !done.load(Ordering::SeqCst) {
        if cancel.is_cancelled() {
            kill_now(pid);
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn run_ffmpeg(
    args: &[String],
    mut on_progress: impl FnMut(f32),
    duration: f64,
    spawned: &mut Option<String>,
    cancel: &Cancel,
) -> Result<String, FfmpegError> {
    let Some(ffmpeg) = resolve_tool_path(Tool::Ffmpeg) else {
        return Err(FfmpegError {
            kind: crate::error::FfmpegErrorKind::Unclassified,
            stderr: "ffmpeg not found".to_string(),
        });
    };

    *spawned = Some(render_command(&ffmpeg, args, &PROGRESS_ARGS));

    let mut child = match Command::new(ffmpeg)
        .args(args)
        .args(PROGRESS_ARGS)
        // ffmpeg reads stdin for its interactive keys and puts the terminal
        // into raw mode to do it, which breaks any prompt of ours that follows.
        .stdin(Stdio::null())
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

    let pid = child.id();
    let finished = AtomicBool::new(false);

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

        let finished = &finished;
        scope.spawn(move || watch_for_cancellation(cancel, pid, finished));

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

        let text = stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();

        // Both pipes are closed, so ffmpeg has exited or is about to. Let the
        // watcher stop instead of killing a process that is already leaving.
        finished.store(true, Ordering::SeqCst);
        text
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
        return Ok(stderr);
    }

    Err(FfmpegError {
        kind: classify_ffmpeg_failure(&stderr),
        stderr,
    })
}

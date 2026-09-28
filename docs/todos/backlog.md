# Backlog

## Lockfile-driven skipping

The lockfile is written but nothing reads it, so every task in a plan runs. It
already records what a later build needs — source hash, output hash, args hash,
boxset and ffmpeg versions — so the work is deciding staleness and acting on it.

An output is current when its entry matches _and_ the file is on disk: assets are
often gitignored, so a manifest entry alone never means the file is there.

## Add post-run hints for improving output

Show a hint block at the end of the build section that highlights things like...

- large video files or low compression ratios: how to compress further (--quality low, etc)
- for videos with no speech or mostly-silent audio, suggest --no-audio
- prompt people to review subtitles

## Add boxset config command

Generates a boxset.toml command with the schema directive filled.

The build system will need to bundle the schema so it's present on the consumer's machine.

`boxset config video1.mp4` generates a config with a preset target for the given video.

When `boxset video1.mp4 video2.mp4` is run, we'll prompt to run `boxset config video1.mp4
video2.mp4`, which will generate a config with preset targets for those videos.

## Encoder settings that `extra_args` can't reach

`-tile-columns` (vp9) and `flags=lanczos` on the scale filter both want to be
real fields — `extra_args` can't reach them without replacing the whole filter
chain. `-g` was in this list and is now implemented as `keyframe_interval`.

## `name` is user-facing and unvalidated

A target's `name` replaces the source stem in output filenames, so it ends up in
URLs. Nothing checks it's URL-safe. This came up on a real project where a source
stem containing a space was reaching the web; `name` fixed that case but can
reintroduce it.

## Cleanup after ctrl-c

Ctrl-c leaves passlogs behind. We need to trap exits and cleanup, or some
kind of "finally" clause?

A transcription in flight also runs to completion. `Session::set_cancel_token`
takes a `CancelToken` that aborts between decode steps and returns the partial
transcript.

## Golden subtitle fixtures

transcribe-cpp is young (0.2.x, one vendor). Keep a few golden VTTs in `tests/`
so an upstream regression shows up as a failing test rather than a worse
transcript nobody notices.

## Clear dead whisper.cpp model files

Cached `ggml-*.bin` weights from the whisper-rs era are never read now. Anyone
who ran an older boxset has up to 3.9GB of them in the cache directory.

## Subtitle options we don't expose

`WhisperRunOptions` exposes `initial_prompt` for proper nouns, temperature and
the threshold knobs. `SessionOptions` has `n_threads`, which could come from
`available_parallelism`. `Transcript.tokens` carries a per-token confidence,
which would ground a "review these subtitles" post-run hint.

## Get progress working for transcription

## Add .md documentation

We should cover...

- getting started guide
- transcriptions (models, managing them, etc)
- config file

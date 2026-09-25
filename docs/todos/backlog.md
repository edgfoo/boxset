# Backlog

## Lockfile-driven skipping

The lockfile is written but nothing reads it, so every task in a plan runs. It
already records what a later build needs — source hash, output hash, args hash,
boxset and ffmpeg versions — so the work is deciding staleness and acting on it.

An output is current when its entry matches _and_ the file is on disk: assets are
often gitignored, so a manifest entry alone never means the file is there.

## Prettier errors

At the moment they just look like this:

```
work/interactive-london-buses (main{3}) % boxset videos/foo
error: target 1: can't read videos/foo
  There's no file at that path.
Error: stopped: nothing was encoded
```

I'm not even sure what ffmpeg looks like. Let's make them pretty like atomkit's.

## Add post-run hints for improving output

Show a hint block at the end of the build section that highlights things like...

- large video files and how to compress them further (--quality low, etc)
- for videos with no speech or mostly-silent audio, suggest --no-audio
- ...

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

## Untested

Whisper transcription and (once it exists) the TUI.

# Backlog

Known gaps, roughly in the order they're worth doing.

## Lockfile-driven skipping

The lockfile is written but nothing reads it, so every task in a plan runs. It
already records what a later build needs — source hash, output hash, args hash,
boxset and ffmpeg versions — so the work is deciding staleness and acting on it.

An output is current when its entry matches _and_ the file is on disk: assets are
often gitignored, so a manifest entry alone never means the file is there.

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

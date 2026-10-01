# Boxset

Prepares a video for presentation on the web.

Create multiple widths and transcodings, capture poster images, generate subtitles, and more – all with a single command.

<img width="720" alt="Boxset demo video" src="https://github.com/user-attachments/assets/6a036ccf-84a8-4467-a357-415841319b2d" />

## What it does

Run boxset with the video you want to prepare...

```bash
boxset interview.mov
```

And you'll get:
* MP4 (H264) and WebM (VP9) versions, at a series of widths equal to and below the source video
* fine-tuned compression for video and audio tracks
* a poster image for each output width
* subtitles generated on your machine (using NVidia's Parakeet model by default)

```
export/
  interview-640.mp4     interview-640.webm     interview-640-poster.jpg
  interview-1280.mp4    interview-1280.webm    interview-1280-poster.jpg
  interview.vtt
```

## Installing

Boxset is available in Homebrew.

```bash
brew install edgfoo/boxset/boxset
```

[ffmpeg]([url](https://ffmpeg.org/)) – the tool that powers Boxset's video and audio encoding – is installed as a dependency by Homebrew. Transcription models are installed by Boxset on demand.

Boxset is currently available for macOS (Intel and Apple Silicon).

## How to use it

Boxset offers a wealth of command line options that configure what comes out of Boxset.

```bash
boxset interview.mp4 --quality high --codecs h264,av1 --widths 720,1440
```

Simple editing tasks like cropping and trimming can also be accomplished.

```bash
boxset interview.mp4 --quality high --crop 9:16 --crop-anchor top --trim 0:05-1:30
```

Run `boxset --help` for the full flag list.

## Project files

For repeated work or multiple videos, use a `boxset.toml` to describe the work to be done, before running `boxset build`.

```toml
out_dir = "src/assets/video"

[defaults]
quality = "high"

[[target]]
src = "raw/interview.mov"
crop = "16:9"

[[target]]
src = "raw/interview.mov"
name = "interview-mobile"
crop = "9:16"
crop_anchor = "top"
```

A `target` is a configuration for a single source video. The `defaults` block applies options to all targets.

Find more information in [docs/boxset-toml.md](docs/boxset-toml.md).

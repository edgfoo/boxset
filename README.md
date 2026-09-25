# boxset

Prepares a video for presentation on the web: transcoding to multiple formats and sizes, generating poster images, and producing subtitles.

> [!WARNING]
> **boxset is a work in progress.** Flags, config keys, and output names can all still change.

## How to use

Run boxset with the video you want to prepare.

```bash
boxset interview.mp4
```

This gives you multiple compressed transcodings at different sizes, a poster frame for each size, and subtitles.

```
export/
  interview-640.mp4     interview-640.webm     interview-640-poster.jpg
  interview-1280.mp4    interview-1280.webm    interview-1280-poster.jpg
  interview.vtt
```

The sizes are chosen from the source: the largest matches the source video, and one or two smaller versions for loading on smaller devices.

Subtitles are transcribed on your own machine with Whisper.

Boxset offers a wealth of command line options that control the output set, compression, and other default behaviour.

```bash
boxset interview.mp4 --quality high --codecs h264,av1 --widths 720,1440
```

Simple editing tasks like cropping and trimming can also be accomplished.

```bash
boxset interview.mp4 --quality high --crop 9:16 --crop-anchor top --trim 0:05-1:30
```

Run `boxset --help` for the full flag list, and `boxset --version` to see which ffmpeg it resolved.

### Projects

For repeated work or multiple videos, use a `boxset.toml` to describe the work to be done, before running `boxset build` to act on it.

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

Any options left out of both are worked out from the source with each build.

Paths resolve against the config's directory, so `boxset build` means the same thing from anywhere.

## What you get

|                  |                                                                                                       |
| ---------------- | ----------------------------------------------------------------------------------------------------- |
| 🎞️ **Video**     | `h264` and `vp9` by default; `h265` and `av1` available. Each at a few sizes, chosen from the source. |
| 🖼️ **Posters**   | A JPEG per size, from a frame chosen automatically or one you name.                                   |
| 💬 **Subtitles** | A WebVTT file, transcribed on your machine with Whisper.                                              |
| 📐 **Cropping**  | Aspect-ratio crops with an anchor, so a landscape source can become a vertical one.                   |
| ✂️ **Trimming**  | A single start/end range.                                                                             |
| 🔒 `boxset.lock` | Records the source hash, output hash, ffmpeg version, and the exact commands run.                     |

## Installing

Use Homebrew:

```bash
brew install edgfoo/boxset/boxset
```

That pulls from the [edgfoo/homebrew-boxset](https://github.com/edgfoo/homebrew-boxset) tap. After that, `brew upgrade boxset` picks up new releases.

boxset ships as a prebuilt binary for Apple silicon and Intel Macs, so installing takes seconds. Homebrew pulls in ffmpeg alongside it.

boxset needs ffmpeg 7.0 or newer, built with libx264, libx265, libvpx, libsvtav1 and libopus. Installing boxset via Homebrew should ensure this requirement is met.

## Licence

boxset is [MIT](./LICENSE-MIT) or [Apache-2.0](./LICENSE-APACHE).

It runs ffmpeg as a subprocess but doesn't redistribute it. Installing via Homebrew pulls ffmpeg in as a dependency.

Subtitles are transcribed by [whisper.cpp](https://github.com/ggml-org/whisper.cpp), which is MIT licensed and compiled into the binary.

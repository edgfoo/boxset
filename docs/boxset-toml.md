# Configuring boxset

A `boxset.toml` file is a [TOML file](https://toml.io/en/) that describes video processing tasks for multiple videos.

The **`boxset build`** command looks for a `boxset.toml` file and does the work.

Run **`boxset config`** to generate a starter config file.

A `boxset.toml` config is optional. `boxset interview.mp4` prepares a single video with no
config at all, but this command only works for one video at a time.

## Basic structure

Here's an example of a basic config file.

```toml
#:schema https://raw.githubusercontent.com/edgfoo/boxset/v0.3.0/schema.json

out_dir = "src/assets/videos"

[defaults]
quality = "low"

[[target]]
src = "raw/Interview_Final.mov"
name = "interview"

[[target]]
src = "raw/Cutaway.mov"
name = "cutaway"
```

This config file tells boxset to process two different videos.

Each `[[target]]` block contains a single `src` pointing at a video file. Everything else in
the block tells boxset what to do with the video.

The `[defaults]` block is used to apply config to all targets.

`out_dir` is a top-level option that tells boxset to put all outputs in the `src/assets/videos` folder.

## Top-level options

All options accepted at the "top level" of the file – those that don't have a heading like `[defaults]` or `[[target]]`.

### out_dir

> **Default:** `./export`

Where outputs are written.

Paths are relative to the directory that `boxset.toml` lives in. See [the paths section](#paths).

### jobs

> **Default:** `2`

How many outputs are encoded at once.

Encoding individual videos is already a multi-threaded process,
so higher values here give small returns.

## Target options

### src

> **Required**

The path of the video to work on.

Paths are relative to the directory that `boxset.toml` lives in. See [the paths section](#paths).

### name

> **Default:** the source's filename

Gives a name to all of a target's outputs.

For example, given a target config like this...

```
[[target]]
src = "Interview_Final.mp4"
name = "interview"
```

All outputs created by Boxset will be called "interview" plus some suffix, like so.

```
interview-1280.mp4    interview-1280.webm    interview-1280-poster.jpg
interview.vtt
```

Without a `name` property, these outputs would instead look like this.

```
Interview_Final-1280.mp4    Interview_Final-1280.webm    Interview_Final-1280-poster.jpg
Interview_Final.vtt
```

A codec suffix — like `interview-1280-h265.mp4` — is added when more than one mp4-family codec is
selected. The default `h264` and `vp9` pair needs none, since vp9 is `.webm`.

### quality

> **Default:** `"balanced"`<br/>
> **Values:** `"low"`, `"balanced"`, `"high"`, `"max"`; `{ audio = <quality>, video = <quality> }`

Controls how compressed the video and audio become.

```toml
quality = "low"
```

Audio and video quality can also be set individually, like so. In this form, either key defaults
to `"balanced"` if omitted.

```toml
quality = { video = "high", audio = "low" }
```

Video quality maps to a codec-specific [CRF](https://slhck.info/video/2017/02/24/crf-guide.html). Audio quality maps to a specific [bitrate](https://www.adobe.com/uk/creativecloud/video/discover/audio-bitrate.html).

| Tier       | `h264` | `h265` | `vp9` | `av1` | Audio  |
| ---------- | ------ | ------ | ----- | ----- | ------ |
| `low`      | 30     | 33     | 47    | 48    | `64k`  |
| `balanced` | 26     | 29     | 42    | 42    | `96k`  |
| `high`     | 23     | 26     | 34    | 34    | `128k` |
| `max`      | 20     | 23     | 29    | 28    | `160k` |

These CRF and bitrate values can be set manually via the [`crf`](#crf) and [`audio.bitrate`](#audio) options.

> [!TIP]
> Choose `low` to maximise loading times in the browser.
>
> `low` quality audio and video tends to be 20-40% smaller than `balanced`.
> `high` is around 40% larger than `balanced`. `max` is around double the size of `balanced`.

> [!TIP]
> Audio is a large share of small outputs: about a quarter of a 480px MP4, and half of a 480px WebM.
> Use `quality = { audio = "low" }` for speech, reserve `high` or `max` for complex audio or music.

### codecs

> **Default:** `["h264", "vp9"]` <br/>
> **Values:** `"h264"`, `"h265"`, `"vp9"`, `"av1"`

Decides the different video formats created from the source.

```toml
codecs = ["h264", "av1"]
```

For each output width, one video is created per codec.

`h264`, `h264` and `av1` created `.mp4` files. `vp9` creates a `.webm` file.

A codec suffix is added to the output filename when more than one mp4-family codec is chosen – for example, `interview-1280-h264.mp4` and `interview-1280-h265.mp4`.

The default `codecs` value of `["h264", "vp9"]` doesn't need codec suffixes, as `h264` creates an
`.mp4` and `.vp9` creates `.webm`.

> [!TIP]
> `h264` – the classic MP4 codec – plays everywhere, but typically compresses poorly.<br/>
> `vp9` has better compression than `h264`, and plays almost everywhere, but browser support is patchy on iOS.<br/>
> `h265` compresses well and fairly quickly. Browser support is strong on iOS and macOS, patchy otherwise.<br/>
> `av1` has the best compression, but only works on modern machines and browsers. </br>
>
> Read [MDN's documentation on video codecs](https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Video_codecs#codec_details) for more information.

### widths

> **Default:** a series of widths based on the source's width

Widths of the video outputs in pixels:

```toml
widths = [640, 1280]
```

If left unset, boxset picks from rungs at 480, 640, 960, 1280 and 1920. The result is a set of
videos at different widths designed to be [selectively loaded in the browser according to the device's width](https://scottjehl.com/posts/using-responsive-video/).

This table gives a few examples of the set of widths derived from a source's width, if `widths`
is not set.

| Source width | Output widths      |
| ------------ | ------------------ |
| 1920         | `[480, 960, 1920]` |
| 1280         | `[640, 1280]`      |
| 400          | `[400]`            |

Boxset never upscales. Output widths are always equal to or smaller than the source width.

### crop

> **Default:** none

Crops the video to an aspect ratio.

```toml
crop = "9:16"
```

By default, cropping is anchored to the center – a 9:16 (portrait) crop of a landscape
video will remove equal areas of the left and right sides of the video.

To choose a different anchor point, pass an object like so.

```toml
crop = { ratio = "9:16", anchor = "top" }
```

Anchors are `centre`, `top`, `bottom`, `left` and `right`.

### trim

> **Default:** none

Trim the beginning and/or end of the source video.

```toml
trim = { start = "0:05", end = "1:30" }
```

Either `start` or `end` can be omitted. Times can be written in the following forms.

| Time form  | What it means            |
| ---------- | ------------------------ |
| `4`        | 4 seconds                |
| `5.5`      | 5.5 seconds              |
| `2:30`     | 2 minutes and 20 seconds |
| `00:10:09` | 10 minutes and 9 seconds |

### fps

> **Default:** the source's fps

Output frame rate.

```toml
fps = "25"
```

`fps` accepts whole numbers (`25`), decimals (`29.976`) or fractions (`24000/1001`).

### audio

> **Default:** `{ normalize = true }` for videos with audio

Audio settings, or `false` to drop the track.

```toml
audio = false
audio = { normalize = false, bitrate = "96k" }
```

`normalize` evens out loudness across the track. This is on by default.

`bitrate` is the audio bitrate ([as ffmpeg spells it](https://ffmpeg.org/ffmpeg-codecs.html#Codec-Options)).
If unset, the [audio quality tier](#quality) picks it.

The audio track is always re-encoded, it's never copied. `aac` is used for MP4-family videos,
`libopus` for `vp9`.

A source with no audio track produces no audio. No `audio` option is needed in this case.

> [!TIP]
> If the video doesn't need sound, remove the audio to keep file sizes low.

### poster

> **Default:** `{ at = "0" }`

Poster image file settings, or `false` to skip creating a poster image.

```toml
poster = false
poster = { at = "0:04" }
```

By default, a poster is created of the first frame of the video (after any [trimming](#trim)).

`at` is the timestamp of the frame to capture. This time _does not account for trimming_: make sure
the time you pass is after any `start` trim time, or before an `end` trim time.

### subtitles

> **Default:** `{ model = "parakeet-0.6b" }`

Transcription settings, or `false` to skip transcription.

```toml
subtitles = { model = "whisper-large" }
```

Artificial intelligence models are used to perform transcription, but this runs entirely on your machine.

> [!CAUTION]
> Always review transcriptions – the `.vtt` outputs – before using them.
> AI transcription often makes mistakes.

`model` determines which model is used. The model you pick is downloaded before transcription can run.
On macOS, models are downloaded to `~/Library/Caches/boxset/models`.

Available `model` options are: `parakeet-110m`, `parakeet-0.6b`, `whisper-tiny`, `whisper-base`,
`whisper-small`, `whisper-medium` and `whisper-large`.

> [!TIP]
> Whipser models are generally better for difficult audio: accents, crowds, wind, etc.
>
> Parakeet models run much faster, and produce subtitles with better punctuation and better pacing
> between cues.

## Encoder overrides

[`quality`](#quality) picks encoder settings for you. Where specific or custom encoder behaviour is
needed, objects can be passed to keys named after each codec, like so.

```toml
[[target]]
src = "raw/interview.mov"
codecs = ["h264", "av1"]
av1 = { crf = 30, preset = 6 }
```

Read ffmpeg's documentation for detailed information and guidance for each codec:

- [H.264 Video Encoding Guide](https://trac.ffmpeg.org/wiki/Encode/H.264)
- [VP9 Video Encoding Guide](https://trac.ffmpeg.org/wiki/Encode/VP9)
- [AV1 Video Encoding Guide](https://trac.ffmpeg.org/wiki/Encode/AV1)
- [H.265 Video Encoding Guide](https://trac.ffmpeg.org/wiki/Encode/H.265)

### crf

Every codec takes `crf`, the main compression control. Higher values mean lower quality and more
compression.

CRF ([constant rate factor](https://slhck.info/video/2017/02/24/crf-guide.html)) is not comparable between codecs. Each scale is its own, so the same number means different things:

| Codec  | Range | `quality = "balanced"` sets |
| ------ | ----- | --------------------------- |
| `h264` | 0–51  | 26                          |
| `h265` | 0–51  | 29                          |
| `vp9`  | 0–63  | 42                          |
| `av1`  | 0–63  | 42                          |

Boxset's tiers are tuned so that one `quality` value gives roughly equal measured quality across
all four codecs.

### Encoder effort

Effort controls how hard the encoder works to compress. More effort means a smaller file at the
same quality, but a slower encode.

Boxset defaults to slow, high-effort settings.

| Codec          | Key        | Values                                                                                         | Default    |
| -------------- | ---------- | ---------------------------------------------------------------------------------------------- | ---------- |
| `h264`, `h265` | `preset`   | `ultrafast`, `superfast`, `veryfast`, `faster`, `fast`, `medium`, `slow`, `slower`, `veryslow` | `veryslow` |
| `vp9`          | `cpu_used` | 0–8                                                                                            | `0`        |
| `av1`          | `preset`   | 0–13                                                                                           | `4`        |

> [!NOTE]
> For `vp9` and `av1`, **higher numbers are faster** — `cpu_used = 0` and av1's
> `preset = 0` are the slowest settings, not the fastest.

`av1` defaults to 4. Below 4, `av1` begins _increasing_ file size by maximising video quality for
the given CRF.

### profile

`h264` and `h265` take a `profile`, which limits the encoder to a feature set that older
devices can decode. Leave it unset unless you know you need it; the encoder picks a sensible one.

`h264` accepts `baseline`, `main` and `high`. `h265` accepts `main` and `main10`.

### extra_args

Every codec takes `extra_args`, a list of arguments passed to ffmpeg untouched.

```toml
h264 = { extra_args = ["-x264-params", "aq-mode=3"] }
```

These arguments are added to the end of ffmpeg commands, so they will override any arguments set by
Boxset.

Nothing here is validated. Invalid arguments or values will break ffmpeg.

## Schema

`boxset config` writes a `#:schema` line at the top of the file:

```toml
#:schema https://raw.githubusercontent.com/edgfoo/boxset/v0.4.0/schema.json
```

This schema file can be used by your IDE to provide features like hover documentation and
auto-completion.

For VS Code, you'll need to install the [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml) extension to get this working.

### Schema file versions

The schema URL refers to a schema.json file that's generated with each release of Boxset. The `boxset config` command will set this according to the version of boxset you have installed.

If you update boxset, an existing `boxset.toml` file will then refer to an older version of the schema, and your IDE may show you out-of-date information.

## Paths

Paths in a config resolve against the config file's directory, including `out_dir` and `--out-dir`.
`boxset build` means the same thing from anywhere in the project.

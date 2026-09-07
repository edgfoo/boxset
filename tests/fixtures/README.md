# Fixtures

Small real-world H.264/AAC sources, sourced from the [Chromium media test
data](https://github.com/chromium/chromium/tree/main/media/test/data)
(BSD-3-Clause), used by integration tests that need a real ffmpeg/ffprobe
subprocess rather than a hand-built `Probe`.

| File | Contents | Used for |
|---|---|---|
| `bear.mp4` | 320x180, H.264 + AAC, ~1s | happy path: renditions, poster, audio |
| `bear_silent.mp4` | 320x180, H.264, no audio track | no-audio-track warning and default |
| `bear-1280x720.mp4` | 1280x720, H.264 + AAC, ~2.8s | ladder derivation across multiple rungs |

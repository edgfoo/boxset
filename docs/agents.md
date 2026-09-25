# boxset

A CLI/TUI tool that takes a video and does everything needed to make it ready for
the web: transcoding to multiple formats and resolutions, generating poster
images, and producing subtitles.

## Who it's for

The Guardian's visuals/interactives team — developer-journalists who build custom
web pages for animated and interactive news articles, the kind that can't be made
through the main article CMS. These articles frequently contain video.

Videos usually arrive as plain MP4s, and it falls to the developer to prepare
them. Today that means Handbrake, ffmpeg, Adobe Media Encoder, Whisper, Premiere,
and a good deal of accumulated personal knowledge about flags and settings.

## The problem

Preparing a video properly is complex and slow, which produces two failure modes:

1. **People skip it.** The steps are involved enough that some team members don't
   do them, and videos ship uncompressed, or without subtitles.
2. **People do it differently.** Those who do the work use different tools and
   settings, so the workflow and the final product vary between projects and
   between colleagues.

The second is the more corrosive one: it means there's no shared, improvable
standard, and no way for the team's knowledge about what settings actually work
to compound.

boxset exists to make the correct path the easy path. A colleague who knows
nothing about video encoding should run one command and get output that is
properly compressed, correctly formatted, and subtitled. Someone who knows
exactly what they want should be able to specify all of it.

## Product principles

**Sensible defaults, full control.** The default path should be right for the
common case without any configuration, and every default should be overridable.
Steer people toward good choices; don't prevent expert ones.

**Intent over parameters.** Most users should say what they want ("high quality",
"crop to 9:16") rather than the values that achieve it. Named settings are how
the team's knowledge gets shared; the numbers behind them live in the tool.

**Reproducibility is a feature.** Asset generation should be declarative and
repeatable, not a sequence of commands someone ran once and can't reconstruct.

**Accessibility is not optional.** Subtitles are part of the default path. The
tool's purpose is defeated if the accessible output is the harder one to produce.

**Easy to install, easy to run.** The audience includes people who will give up
if the first run fails. Getting to a working state should not require installing
a toolchain or debugging dependencies.

**Honest about cost.** Encoding is slow. The tool should be clear about what it's
doing and how long it will take, and should never redo work it has already done.

**Problems are warned about, not blocked on.** When boxset detects something
likely wrong, it says so clearly and proceeds. Files that will be overwritten are
named before work starts, so the user can narrow the run before anything runs.

## Scope

**In scope:** transcoding, compression, resolution ladders, format conversion,
poster/thumbnail generation, subtitle generation, cropping and aspect-ratio
changes, trimming, audio normalisation, and emitting whatever a web project needs
to consume the result.

**Out of scope:** editing (cuts beyond a single trim, effects, colour, titles,
watermarks), asset hosting and CDN upload, image processing, and anything
specific to a single article's design. boxset prepares assets; it doesn't build
pages.

## Architecture

One pipeline, in `src/lib/`:

```
Sources ──┐                                 selection
config  ──┴─→  validate  ──→  resolve  ──→  plan  ──→  environment  ──→  execute  ──→  outputs
                   │                                        │
              Vec<Problem>                            Vec<Requirement>
```

The library is the whole tool minus a way to talk to it, and can be built and
tested with no interface at all. `src/cli/` is a client of it. The TUI, when it
arrives, will be another — see [todos/tui.md](todos/tui.md).

### Principles that hold this together

- **Gather before deciding.** Probing fills `Sources` before validation runs,
  which is what lets validation report every problem at once rather than stopping
  at the first file it fails to open.
- **Validation is the only stage that can report a problem with the config.**
  Past that gate resolution and planning are infallible — a `Result` from either
  would mean a check that could have been reported alongside the others was
  instead fatal and alone.
- **Resolution produces total settings.** Nothing downstream re-derives a value
  or handles a choice that was already settled. `Option` survives only where it
  means a decided absence.
- **Execution may depend on planning; never the reverse.**
- **Nothing in the library knows a client exists.** Execution never prints; it
  reports through `Reporter`, which is a sink, never a source. Reporters receive
  values, never strings, so each client words the same event as it likes.
- **Only probing, planning, environment and execution touch the filesystem.**
- **Clients meet the library at three values:** a list of `TargetConfig`, a
  `Selection`, and a filled `Sources`. There is no fourth door. This is what
  makes CLI/TUI equivalence checkable rather than aspirational — neither client
  encodes anything itself, and `src/cli/flags.rs` is the list of what a client
  must be able to express.
- **A config entry is a target, not a video.** Several targets may share a
  source; a landscape video commonly needs a wide crop for desktop and a tall one
  for mobile, swapped at a CSS breakpoint.

### Invariants

- A single-shot run must be complete without config or lockfile. Someone can take
  the assets and delete everything else.
- `boxset.toml` is written by hand and (in future) by `studio`. Flags never
  persist.
- The config records only what the user specified; omitted fields re-derive from
  the source at build time, so a replaced source gets a correct ladder without
  anyone editing the file.
- `[defaults]` holds target fields shared by the whole project. Targets are
  stored unmerged and merged on the way into the pipeline, so the file can be
  written back without defaults exploding out into every target. A target's
  value always wins; settings tables merge field by field, and `false` is a
  decision that never merges with a table.
- Reproducibility lives in `boxset.toml`. The lockfile is machine-local,
  uncommitted, and describes one machine's outputs.
- Every path inside a config resolves against the config's directory, including
  `--out-dir`, so a config describes the same build from any working directory.
- Content hashes, never mtime — mtime differs across machines and CI checkouts.
- A failed task never leaves a truncated file where a valid one was: tasks write
  to a temp path and rename on success. One failed task doesn't stop the others.

## Dependencies worth knowing about

**ffmpeg and ffprobe** are installed by Homebrew, not shipped by us — the
formula declares `depends_on "ffmpeg"`. They are found on `PATH`, after a
`bin/` directory beside boxset's own executable that nothing currently
populates. Because the version is not ours to fix, `check_environment`
fails on an ffmpeg that is too old or lacks an encoder the plan needs, scoped to
the plan so a missing `libsvtav1` is silent until something asks for av1.
`MIN_FFMPEG_VERSION` is the only floor — Homebrew has no minimum-version syntax,
and would not see an ffmpeg earlier on `PATH` in any case. The lockfile's
recorded version means "this machine already encoded this", not that another
machine would produce the same bytes.

ffmpeg reports almost everything as exit code 1 with an
explanation in stderr, so failures are classified by pattern-matching stderr —
every pattern has a real captured fixture in `tests/fixtures/ffmpeg-stderr/`,
never handwritten, since a handwritten sample tests the pattern against itself. A
wrong classification is worse than none.

**whisper-rs** (bindings to whisper.cpp) is compiled in — there is no external
Whisper install and no Python anywhere in the path. Building it needs a current
cmake. Built with the `metal` feature on macOS, plain CPU elsewhere.

**Whisper model weights** are fetched once into the OS cache directory and shared
across projects, rather than embedded, to keep the download small and allow
choosing a quality tier.

**Encoding changes need a quality measurement, not a file size.** The vp9 tiers
looked wrong once — larger files than h264 at the same tier — but measuring with
VMAF showed they were buying 6.5 points of quality for those bytes. Comparing
sizes at a fixed CRF says nothing without a quality number beside it.

## Releasing

`cargo release <level>` — patch, minor, major, rc, beta, alpha, release — bumps,
commits, tags and pushes from a maintainer's Mac. It runs dry by default; add
`--execute` to do it. Configured under `[package.metadata.release]` in
`Cargo.toml`; `publish = false` because boxset ships via Homebrew, not crates.io.

Pushing the tag is the trigger. `.github/workflows/release.yml` builds both macOS
arches, renders `.github/homebrew/boxset.rb.template` with the two tarball
checksums, audits it, drafts the GitHub release, pushes the formula to the
`edgfoo/homebrew-boxset` tap, and only then undrafts. Nothing public exists until
that last step, and a failure deletes the draft. A version containing `-` is
treated as a prerelease: marked as such on the release, and the tap is left
alone, so `brew upgrade` never picks one up.

CI never writes to `main` — the only thing that reaches it is the maintainer's
own push.

## Working conventions

- Plain, idiomatic Rust. No cleverness for its own sake.
- `cargo clippy` clean at all times.
- Unit test real decision points (ordering, precedence, branches), not literal
  defaults or trivial `unwrap_or`s
- Pure stages — `validate`, `resolve`, `plan`, quality expansion, ladder
  derivation — are tested with hand-built values and no subprocess. Integration
  tests in `tests/` drive the real binary against small real-world fixtures.
- Whisper and the TUI are untested.

The user is an experienced developer-journalist with well-developed opinions
about video encoding, refined over many projects. Encoding recipes they provide
have usually been tested in production — treat them as the starting point, and
say so if something looks wrong rather than silently changing it.

The user has Rust and ffmpeg installed. **Do not run installation commands** —
share the command and let the user run it themselves.

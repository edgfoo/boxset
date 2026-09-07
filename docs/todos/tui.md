# TUI

`boxset studio`. Three screens — picker, editor, target list — each with one job,
over the target field set (see `TargetConfig` in `src/config.rs`). Open:
keybindings, how editing works, and what happens during a build.

The layouts below sketch the intent. They are not a specification, and the
keybindings in them are placeholders.

**Picker.** Videos in the working directory, multi-select. It is step one of
adding a target, and a first run enters it directly.

```
  boxset · pick a video to prepare

  [x] interview.mp4      1920×1080 · 2:14
  [ ] feature.mp4        3840×2160 · 0:48
  [ ] cutaway.mp4        1080×1920 · 0:12

  space toggle · enter continue · / search
```

The listing is the working directory only, one level, no recursion. It draws
immediately and fills in dimensions and duration as probes land, via `Sources`.
This is what `Sources`' incremental `request`/`absorb` split exists for: results
must be readable while others are still in flight.

`/` opens a fuzzy finder over paths in the tree below the working directory, for
sources kept in a subdirectory. It matches on path text alone, so the walk reads
directory entries and nothing else. Results stream in as the walk proceeds, so
the finder is usable immediately. It skips hidden directories, `out_dir`, and a
small list of common dependency and build directories, and stops after a large
fixed number of entries.

Search rows show path text alone, and a file is probed once selected. A fuzzy
finder re-filters on every keystroke, so probing the current matches would mean
starting work the next keystroke abandons.

Several files create several targets, all defaulted, and land in the target list.
One file goes via the editor.

Selecting a file that already has a target creates a second target for it and
prompts for a `name`. This is where multi-target configuration happens.

**Editor.** One target, one screen.

```
  feature.mp4 · wide

  name        wide               specified
  quality     high               specified
  crop        16:9 · centre      specified
  widths      1000, 1400         specified
  subtitles   on · en            default

  esc back · b build
```

Marks show provenance: `specified`, `project`, `default`, `derived`. `project`
is a value from `[defaults]`. Derived values display as real values so the
consequences are visible. Editing a derived field makes it specified, and it is
then written to `boxset.toml`.

Editing `[defaults]` itself is undesigned — a fourth screen, or a mode on the
target list, over the same field set minus `src` and `name`.

**Target list — the home screen once a config exists.** One row per target, flat.
This is the selector: ticked rows are the targets the build covers.

```
  boxset · 4 targets

  [x] interview             interview.mp4 · 6 outputs
  [x] feature · wide        feature.mp4 · 4 outputs
  [x] feature · tall        feature.mp4 · 4 outputs
  [ ] cutaway               cutaway.mp4 · 5 outputs

  + add target              2 videos here aren't used

  space toggle · enter configure · a add · b build
```

Rows come from `boxset.toml`, ticked by default. `a` re-enters the picker.

Unticked means "not today" — targets and assets are left alone. There is no
delete affordance.

Building lists the files the ticked rows will overwrite, and confirms before
encoding starts.

**The TUI never encodes anything itself.** `b` writes `boxset.toml` and collects
the ticked rows into the same `TargetConfig` list and `Selection` a CLI
invocation produces, then runs that, which is what keeps TUI/CLI equivalence
exact. Rows are held as written, and defaults are applied only at that
hand-off — holding them merged would write every default into every target.

A per-target summary — output counts, codecs, ladder shape — is read from the
`Plan` before execution starts, not produced separately by the library.

## During a build

A live view implementing `Reporter`. Undesigned. It needs an indeterminate state
for tasks that report no progress (subtitle transcription never calls
`task_progress`), must show a failed task while the rest of the run continues,
and must read as a summary once the run ends.

`task_progress` may fire often, so the view must coalesce rather than render per
call. `Phase` variants and `task_started` already exist in `Reporter` and are
unused by the CLI — they were kept for this.

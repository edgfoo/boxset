//! A `Problem` is a reason a build won't work, collected rather than thrown.

use std::path::PathBuf;

use crate::sources::ProbeErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub severity: Severity,
    pub kind: ProblemKind,
    /// Index into the config list the problem is attributable to.
    pub target: Option<usize>,
    pub field: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub enum ProblemKind {
    SrcMissing,
    SrcUnprobeable {
        path: PathBuf,
        reason: ProbeErrorKind,
    },
    OutputCollision {
        other: usize,
        path: PathBuf,
    },
    CodecOverrideForExcludedCodec,
    OutDirNotWritable {
        path: PathBuf,
    },
    AudioSettingOnSilentSource,
    UnknownField {
        name: String,
        suggestion: Option<String>,
    },
    TargetFieldAtTopLevel {
        name: String,
    },
    /// A field in `[defaults]` that only makes sense per target.
    FieldNotAllowedInDefaults {
        name: &'static str,
    },
    /// A width larger than the source, which would upscale.
    WidthsExceedSource {
        widths: Vec<u32>,
        available: u32,
    },
    /// A value of the right type but the wrong shape: a crop ratio like
    /// `16x9`, or a timestamp that doesn't parse.
    MalformedValue {
        value: String,
        expected: &'static str,
    },
}

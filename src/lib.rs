//! boxset: prepare video for the web.

pub mod command;
pub mod config;
pub mod environment;
pub mod error;
pub mod execute;
pub mod lock;
pub mod outputs;
pub mod plan;
pub mod problem;
pub mod report;
pub mod resolve;
pub mod settings;
pub mod sources;
pub mod task;
pub mod transcribe;
pub mod validate;

pub use config::TargetConfig;
pub use environment::{Requirement, check_environment, ensure};
pub use error::BoxsetError;
pub use execute::execute;
pub use lock::{LockEntry, Lockfile};
pub use plan::{Plan, Selection, plan};
pub use problem::{Problem, ProblemKind, Severity};
pub use report::Reporter;
pub use resolve::resolve;
pub use settings::Settings;
pub use sources::{Probe, SourceLookup, SourceState, Sources};
pub use task::{Task, TaskId, TaskWork};
pub use validate::validate;

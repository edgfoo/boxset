//! boxset: prepare video for the web.

#[path = "lib/command.rs"]
pub mod command;
#[path = "lib/config.rs"]
pub mod config;
#[path = "lib/environment.rs"]
pub mod environment;
#[path = "lib/error.rs"]
pub mod error;
#[path = "lib/execute.rs"]
pub mod execute;
#[path = "lib/lock.rs"]
pub mod lock;
#[path = "lib/outputs.rs"]
pub mod outputs;
#[path = "lib/plan.rs"]
pub mod plan;
#[path = "lib/problem.rs"]
pub mod problem;
#[path = "lib/report.rs"]
pub mod report;
#[path = "lib/resolve.rs"]
pub mod resolve;
#[path = "lib/settings.rs"]
pub mod settings;
#[path = "lib/sources.rs"]
pub mod sources;
#[path = "lib/task.rs"]
pub mod task;
#[path = "lib/transcribe.rs"]
pub mod transcribe;
#[path = "lib/validate.rs"]
pub mod validate;

pub use config::TargetConfig;
pub use environment::{Requirement, check_environment, ensure_available, ensure_met};
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

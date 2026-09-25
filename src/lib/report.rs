//! `Reporter`: execution's only way to speak. It's a sink, never a source —
//! it returns nothing, so execution can't come to depend on what a client did.

use std::time::Duration;

use crate::environment::Requirement;
use crate::error::BoxsetError;
use crate::problem::Problem;
use crate::task::TaskId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Probing,
    Hashing,
    FetchingModels,
    Encoding,
    WritingLockfile,
}

#[derive(Debug)]
pub enum TaskOutcome {
    Succeeded,
    Failed(BoxsetError),
}

#[derive(Debug)]
pub struct TaskReport {
    pub outcome: TaskOutcome,
    pub elapsed: Duration,
    /// `None` when the task failed.
    pub bytes: Option<u64>,
}

#[allow(unused_variables)]
pub trait Reporter {
    fn phase(&mut self, phase: Phase) {}
    fn task_started(&mut self, task: TaskId) {}
    fn task_stage(&mut self, task: TaskId, stage: &'static str, index: u32, total: u32) {}
    fn task_progress(&mut self, task: TaskId, stage: &'static str, overall: f32, stage_done: f32) {}
    fn task_finished(&mut self, task: TaskId, report: TaskReport) {}
    fn requirement_started(&mut self, req: &Requirement) {}
    fn requirement_progress(&mut self, req: &Requirement, done: f32) {}
    fn requirement_finished(&mut self, req: &Requirement, outcome: &Result<(), BoxsetError>) {}
    fn problem(&mut self, problem: &Problem) {}
}

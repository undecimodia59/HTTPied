//! Bounded experiment plans. Organization stays in Replay; only explicit runs execute traffic.
use crate::{model::WireExecution, state::Variable};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
pub mod access;
mod runner;
pub mod sequence;
pub use runner::run;
pub const MAX_STEPS: usize = 32;
pub const MAX_RESULTS: usize = 4096;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Race,
    Access,
    Sequence,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Race => "Race condition",
            Self::Access => "Access-control",
            Self::Sequence => "Sequence",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub name: String,
    pub destination: String,
    pub request: String,
    pub expected: String,
    pub extractors: Vec<sequence::Extractor>,
}
impl Default for Step {
    fn default() -> Self {
        Self {
            name: "Request".into(),
            destination: String::new(),
            request: "GET / HTTP/1.1\r\n\r\n".into(),
            expected: "2,3".into(),
            extractors: vec![],
        }
    }
}
impl From<crate::state::SavedRequest> for Step {
    fn from(value: crate::state::SavedRequest) -> Self {
        Self {
            name: value.name,
            destination: value.destination,
            request: value.request,
            ..Default::default()
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub kind: Kind,
    pub steps: Vec<Step>,
    pub burst_size: usize,
    pub bursts: usize,
    pub connections: usize,
    pub synchronized: bool,
    pub delay_ms: u64,
    pub timeout_secs: u64,
    pub profiles: Vec<access::Identity>,
    pub anonymous: bool,
    pub remove_headers: String,
    pub denial_marker: String,
    pub carry_cookies: bool,
    pub stop_on_failure: bool,
    pub cycles: usize,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            kind: Kind::Race,
            steps: vec![],
            burst_size: 10,
            bursts: 1,
            connections: 10,
            synchronized: true,
            delay_ms: 0,
            timeout_secs: 30,
            profiles: vec![access::Identity::default()],
            anonymous: true,
            remove_headers: "Authorization, Cookie, Proxy-Authorization, X-Api-Key, X-Auth-Token"
                .into(),
            denial_marker: String::new(),
            carry_cookies: true,
            stop_on_failure: true,
            cycles: 1,
        }
    }
}
impl Workspace {
    pub fn total(&self) -> usize {
        match self.kind {
            Kind::Race => self.burst_size.saturating_mul(self.bursts),
            Kind::Access => self
                .steps
                .len()
                .saturating_mul(1 + self.profiles.len() + usize::from(self.anonymous)),
            Kind::Sequence => self.steps.len().saturating_mul(self.cycles),
        }
    }
    pub fn validate_storage(&self) -> Result<(), String> {
        if self.steps.len() > MAX_STEPS
            || self.profiles.len() > 8
            || self.remove_headers.len() > 2048
            || self.denial_marker.len() > 1024
            || self
                .profiles
                .iter()
                .any(|p| p.name.len() > 64 || p.headers.len() > 16 * 1024)
            || self.steps.iter().any(|step| {
                step.name.len() > 128
                    || step.destination.len() > 2048
                    || step.request.len() > 128 * 1024
                    || step.expected.len() > 32
                    || step.extractors.len() > 16
                    || step
                        .extractors
                        .iter()
                        .any(|e| e.name.len() > 64 || e.selector.len() > 2048)
            })
        {
            return Err("Attack workspace exceeds its storage limits".into());
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        self.validate_storage()?;
        if self.steps.is_empty() || self.steps.len() > MAX_STEPS {
            return Err("Add 1–32 requests to the experiment".into());
        }
        if (self.kind == Kind::Race
            && (!(1..=64).contains(&self.burst_size)
                || !(1..=64).contains(&self.connections)
                || !(1..=4096).contains(&self.bursts)))
            || (self.kind == Kind::Sequence && !(1..=4096).contains(&self.cycles))
            || !(1..=300).contains(&self.timeout_secs)
            || self.delay_ms > 60_000
            || self.total() > MAX_RESULTS
        {
            return Err("Experiment limits: 64 connections, 4,096 requests, 1–300 s timeout and ≤60 s delay".into());
        }
        if self.kind == Kind::Race && self.synchronized && self.connections < self.burst_size {
            return Err("Synchronized bursts need at least one connection per request; increase Parallel connections or reduce Requests per burst".into());
        }
        if self.profiles.len() > 8
            || self.remove_headers.len() > 2048
            || self.denial_marker.len() > 1024
        {
            return Err(
                "At most 8 identities and bounded credential/detector fields are supported".into(),
            );
        }
        if self.kind == Kind::Access && self.profiles.is_empty() && !self.anonymous {
            return Err("Add an identity or include the anonymous check".into());
        }
        if self.kind == Kind::Access
            && self
                .profiles
                .iter()
                .any(|profile| profile.headers.trim().is_empty())
        {
            return Err("Supply credential headers for every alternate identity, or remove the identity to run only the anonymous check".into());
        }
        if self.kind == Kind::Race && self.burst_size < self.steps.len() {
            return Err("Requests per burst must include every input request at least once".into());
        }
        if self.kind == Kind::Access {
            for profile in &self.profiles {
                profile.validate()?;
            }
        }
        for step in &self.steps {
            if step.name.len() > 128
                || step.destination.len() > 2048
                || step.request.len() > 128 * 1024
                || step.expected.len() > 32
                || step.extractors.len() > 16
            {
                return Err("Request, name or extraction configuration exceeds its limit".into());
            }
            if self.kind != Kind::Sequence {
                continue;
            }
            let mut names = std::collections::HashSet::new();
            for extractor in &step.extractors {
                extractor.validate()?;
                if !names.insert(&extractor.name) {
                    return Err("Extractor variable names must be unique within a step".into());
                }
            }
            if !step.expected.is_empty()
                && !step.expected.split(',').all(|p| {
                    let p = p.trim();
                    !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit())
                })
            {
                return Err(
                    "Expected status uses comma-separated prefixes, for example 2,3 or 200,302"
                        .into(),
                );
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedRun {
    #[serde(default)]
    pub status: String,
    pub id: u64,
    pub name: String,
    pub workspace: Workspace,
    #[serde(default)]
    pub variables: Vec<Variable>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Record {
    pub run_id: u64,
    pub index: usize,
    pub step: usize,
    pub label: String,
    pub identity: String,
    pub burst: usize,
    pub position: usize,
    pub status: u16,
    pub bytes: u64,
    pub elapsed_ms: f64,
    pub first_byte_ms: f64,
    pub release_us: Option<u64>,
    pub error: String,
    pub verdict: String,
    pub baseline: Option<usize>,
    pub capture_id: Option<u64>,
    pub body_hash: String,
}
pub struct Job {
    pub run: SavedRun,
    pub variables: Vec<Variable>,
    pub scope: Vec<crate::state::ScopeRule>,
    pub enforce_scope: bool,
    pub global_rps: u32,
    pub host_rps: u32,
    pub cancel: watch::Receiver<bool>,
}
pub enum Update {
    Result {
        record: Box<Record>,
        execution: Option<Box<WireExecution>>,
    },
    Done {
        run_id: u64,
        message: String,
    },
}
pub type Sender = mpsc::Sender<Job>;
pub type Receiver = mpsc::Receiver<Update>;

#[derive(Clone, Debug)]
pub struct RunSummary {
    pub id: u64,
    pub name: String,
    pub kind: Kind,
}

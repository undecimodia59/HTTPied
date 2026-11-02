//! Bounded scheduler with lazy generation, dispatch pacing and cancellation of active sockets.
use crate::{
    model::WireExecution,
    payload::Payloads,
    proxy::{self, Config},
    state::ScopeRule,
};
use std::{
    io,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};
use tracing::Instrument;
pub enum Requests {
    Raw(std::vec::IntoIter<Vec<u8>>),
    Payload {
        destination: String,
        template: String,
        repair: bool,
        values: Payloads,
    },
    Multi {
        destination: String,
        template: String,
        repair: bool,
        plan: crate::attack::Plan,
    },
}
impl From<Vec<Vec<u8>>> for Requests {
    fn from(requests: Vec<Vec<u8>>) -> Self {
        Self::Raw(requests.into_iter())
    }
}
impl Requests {
    pub fn len(&self) -> usize {
        match self {
            Self::Raw(v) => v.len(),
            Self::Payload { values, .. } => values.len(),
            Self::Multi { plan, .. } => plan.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn next_attributed(&mut self) -> Option<io::Result<(Vec<u8>, Vec<String>)>> {
        match self {
            Self::Raw(v) => v.next().map(|v| Ok((v, vec![]))),
            Self::Payload {
                destination,
                template,
                repair,
                values,
            } => values.next().map(|value| {
                crate::replay::build(
                    destination,
                    &template.replace("{{payload}}", &value),
                    *repair,
                )
                .map(|wire| (wire, vec![value]))
            }),
            Self::Multi {
                destination,
                template,
                repair,
                plan,
            } => plan.next().map(|values| {
                // Single-pass replacement prevents a payload from becoming another marker.
                let mut request = String::new();
                let mut rest = template.as_str();
                while let Some(start) = rest.find("{{payload") {
                    request.push_str(&rest[..start]);
                    let marker = &rest[start..];
                    let Some(end) = marker.find("}}") else {
                        return Err(io::Error::other("Unclosed payload marker"));
                    };
                    let key = &marker[2..end];
                    let slot = if key == "payload" {
                        0
                    } else {
                        key.strip_prefix("payload")
                            .and_then(|s| s.parse::<usize>().ok())
                            .and_then(|n| n.checked_sub(1))
                            .ok_or_else(|| io::Error::other("Invalid payload marker"))?
                    };
                    request.push_str(
                        values
                            .get(slot)
                            .ok_or_else(|| io::Error::other("Payload marker has no source"))?,
                    );
                    rest = &marker[end + 2..];
                }
                request.push_str(rest);
                crate::replay::build(destination, &request, *repair).map(|wire| (wire, values))
            }),
        }
    }
}
impl Iterator for Requests {
    type Item = io::Result<Vec<u8>>;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_attributed().map(|v| v.map(|(wire, _)| wire))
    }
}
#[derive(Clone)]
pub struct Options {
    pub timeout_secs: u64,
    pub concurrency: usize,
    pub retries: usize,
    pub backoff: Duration,
    pub global_rps: u32,
    pub host_rps: u32,
    pub enforce_scope: bool,
    pub scope: Vec<ScopeRule>,
    pub stop_on_match: bool,
    pub matcher: String,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            timeout_secs: 0,
            concurrency: 1,
            retries: 0,
            backoff: Duration::from_secs(1),
            global_rps: 0,
            host_rps: 0,
            enforce_scope: false,
            scope: vec![],
            stop_on_match: false,
            matcher: String::new(),
        }
    }
}
pub struct Job {
    pub id: u64,
    pub requests: Requests,
    pub delay: Duration,
    pub cancel: watch::Receiver<bool>,
    pub pause: Option<watch::Receiver<bool>>,
    pub options: Options,
}
pub struct ResultEvent {
    pub id: u64,
    pub index: usize,
    pub elapsed: Duration,
    pub result: Result<WireExecution, String>,
    pub done: bool,
    pub total: usize,
    pub payloads: Vec<String>,
    pub terminal: bool,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct Record {
    #[serde(default)]
    pub capture_id: Option<u64>,
    pub run_id: u64,
    pub index: usize,
    pub total: usize,
    pub payloads: Vec<String>,
    pub status: u16,
    pub bytes: u64,
    pub elapsed_ms: f64,
    pub error: String,
    pub matched: bool,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ResultFilter {
    pub query: String,
    pub status: String,
    pub errors_only: bool,
    pub matched_only: bool,
    pub sort_by: usize,
    pub descending: bool,
    pub latest_only: bool,
}
impl ResultFilter {
    pub fn matches(&self, record: &Record) -> bool {
        (!self.errors_only || !record.error.is_empty())
            && (!self.matched_only || record.matched)
            && (self.status.is_empty() || record.status.to_string().starts_with(&self.status))
            && (self.query.is_empty()
                || format!(
                    "{} {} {}",
                    record.payloads.join(" "),
                    record.status,
                    record.error
                )
                .to_lowercase()
                .contains(&self.query.to_lowercase()))
    }
}
async fn paused_change(pause: &mut Option<watch::Receiver<bool>>) {
    match pause {
        Some(p) => {
            let _ = p.changed().await;
        }
        None => std::future::pending::<()>().await,
    }
}
fn is_paused(pause: &Option<watch::Receiver<bool>>) -> bool {
    pause.as_ref().is_some_and(|p| *p.borrow())
}
async fn execute_retry(
    bytes: Vec<u8>,
    mut config: Config,
    options: Options,
    gate: std::sync::Arc<tokio::sync::Mutex<tokio::time::Instant>>,
    pace: Duration,
) -> Result<WireExecution, String> {
    if options.timeout_secs > 0 {
        config.total_timeout = Duration::from_secs(options.timeout_secs);
    }
    let method = bytes.split(|b| *b == b' ').next().unwrap_or_default();
    let retryable = [b"GET".as_slice(), b"HEAD", b"OPTIONS"].contains(&method);
    let target = bytes
        .split(|b| *b == b' ')
        .nth(1)
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("");
    if options.enforce_scope && !crate::state::in_scope(&options.scope, target) {
        return Err(format!("Outside target scope: {target}"));
    }
    let mut attempt = 0;
    loop {
        let at = {
            let mut next = gate.lock().await;
            let at = (*next).max(tokio::time::Instant::now());
            *next = at + pace;
            at
        };
        tokio::time::sleep_until(at).await;
        tracing::debug!(%target,attempt,"job HTTP attempt started");
        let result = proxy::execute(bytes.clone(), &config)
            .await
            .map_err(|e| e.to_string());
        match &result {
            Ok(execution) => tracing::debug!(
                attempt,
                status = execution.response_status,
                "job HTTP attempt completed"
            ),
            Err(error) => tracing::warn!(attempt,%error,"job HTTP attempt failed"),
        }
        let retry = retryable
            && attempt < options.retries
            && match &result {
                Err(_) => true,
                Ok(e) => matches!(e.response_status, 429 | 503),
            };
        if !retry {
            return result;
        }
        let retry_after = if let Ok(e) = &result {
            let head = String::from_utf8_lossy(&e.response);
            head.lines()
                .take_while(|l| !l.is_empty())
                .find_map(|l| {
                    l.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
                        .and_then(|(_, v)| v.trim().parse::<u64>().ok())
                })
                .map(Duration::from_secs)
                .unwrap_or_default()
        } else {
            Duration::ZERO
        };
        let wait = options
            .backoff
            .saturating_mul(1u32 << attempt)
            .max(retry_after)
            .min(Duration::from_secs(60));
        attempt += 1;
        tokio::time::sleep(wait).await;
    }
}
type TaskResult = (usize, Duration, Result<WireExecution, String>, Vec<String>);
type InFlight = std::collections::BTreeMap<usize, (Vec<String>, Instant)>;
fn result_event(id: u64, total: usize, value: TaskResult, options: &Options) -> ResultEvent {
    let (index, elapsed, mut result, payloads) = value;
    let matched = !options.matcher.is_empty()
        && result.as_ref().is_ok_and(|e| {
            e.response
                .windows(options.matcher.len())
                .any(|w| w == options.matcher.as_bytes())
        });
    if let Ok(e) = &mut result {
        e.run = Some(Box::new(Record {
            capture_id: None,
            run_id: id,
            index,
            total,
            payloads: payloads.clone(),
            status: e.response_status,
            bytes: e.response_total_bytes,
            elapsed_ms: elapsed.as_secs_f64() * 1000.0,
            error: String::new(),
            matched,
        }));
    }
    ResultEvent {
        id,
        index,
        total,
        elapsed,
        result,
        payloads,
        done: false,
        terminal: false,
    }
}
async fn abort_pending(
    tasks: &mut tokio::task::JoinSet<TaskResult>,
    inflight: &mut InFlight,
    id: u64,
    total: usize,
    options: &Options,
    reason: &str,
) -> Vec<ResultEvent> {
    tasks.abort_all();
    let mut events = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Ok(value) = result {
            inflight.remove(&value.0);
            events.push(result_event(id, total, value, options));
        }
    }
    for (index, (payloads, start)) in std::mem::take(inflight) {
        events.push(result_event(
            id,
            total,
            (index, start.elapsed(), Err(reason.into()), payloads),
            options,
        ));
    }
    events
}
async fn deliver(
    results: &mpsc::Sender<ResultEvent>,
    stop: &mut watch::Receiver<bool>,
    event: ResultEvent,
) -> bool {
    tokio::select! {biased;_=stop.changed()=>false,result=results.send(event)=>result.is_ok()}
}
pub async fn run(
    mut commands: mpsc::Receiver<Job>,
    results: mpsc::Sender<ResultEvent>,
    config: Config,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let mut job = tokio::select! {biased;_=stop.changed()=>return,job=commands.recv()=>match job{Some(job)=>job,None=>return}};
        let count = job.requests.len();
        let id = job.id;
        if count == 0
            || count > crate::payload::MAX_PAYLOADS
            || !(1..=16).contains(&job.options.concurrency)
            || job.options.retries > 3
            || job.options.timeout_secs > 86400
        {
            if !deliver(
                &results,
                &mut stop,
                ResultEvent {
                    id,
                    index: 0,
                    total: count,
                    elapsed: Duration::ZERO,
                    result: Err("Invalid job count/concurrency/retries".into()),
                    done: true,
                    payloads: vec![],
                    terminal: true,
                },
            )
            .await
            {
                return;
            }
            continue;
        }
        let mut tasks = tokio::task::JoinSet::<TaskResult>::new();
        let mut inflight = InFlight::new();
        let mut dispatched: usize = 0;
        let mut completed = 0;
        let mut next = tokio::time::Instant::now();
        let strict_rate = match (job.options.global_rps, job.options.host_rps) {
            (0, 0) => 0,
            (0, b) => b,
            (a, 0) => a,
            (a, b) => a.min(b),
        };
        // UI runs have one explicit host. Every attempt, including retries, shares this gate.
        let pace = job.delay.max(if strict_rate > 0 {
            Duration::from_secs_f64(1.0 / strict_rate as f64)
        } else {
            Duration::ZERO
        });
        let gate = std::sync::Arc::new(tokio::sync::Mutex::new(next));
        loop {
            if *job.cancel.borrow() || job.cancel.has_changed().is_err() {
                let mut events = abort_pending(
                    &mut tasks,
                    &mut inflight,
                    id,
                    count,
                    &job.options,
                    "Cancelled",
                )
                .await;
                if events.is_empty() {
                    events.push(ResultEvent {
                        id,
                        index: dispatched.saturating_sub(1),
                        total: count,
                        elapsed: Duration::ZERO,
                        result: Err("Cancelled".into()),
                        done: false,
                        payloads: vec![],
                        terminal: true,
                    });
                }
                let last = events.len() - 1;
                events[last].done = true;
                for event in events {
                    if !deliver(&results, &mut stop, event).await {
                        return;
                    }
                }
                break;
            }
            if dispatched < count
                && !is_paused(&job.pause)
                && tasks.len() < job.options.concurrency
                && tokio::time::Instant::now() >= next
            {
                let request = job
                    .requests
                    .next_attributed()
                    .expect("count matches generator");
                let payloads = request
                    .as_ref()
                    .map(|(_, payloads)| payloads.clone())
                    .unwrap_or_default();
                let index = dispatched;
                inflight.insert(index, (payloads, Instant::now()));
                let cfg = config.clone();
                let options = job.options.clone();
                let gate = gate.clone();
                let span = tracing::info_span!("job_request", run_id = job.id, index);
                tasks.spawn(
                    async move {
                        let started = Instant::now();
                        let (result, payloads) = match request {
                            Ok((bytes, payloads)) => (
                                execute_retry(bytes, cfg, options, gate, pace).await,
                                payloads,
                            ),
                            Err(e) => (Err(e.to_string()), vec![]),
                        };
                        (index, started.elapsed(), result, payloads)
                    }
                    .instrument(span),
                );
                dispatched += 1;
                next = tokio::time::Instant::now() + pace;
                continue;
            }
            tokio::select! {
                biased;
                _=stop.changed()=>{tasks.abort_all();while tasks.join_next().await.is_some(){}return;},
                _=job.cancel.changed()=>{},
                _=paused_change(&mut job.pause)=>{},
                result=tasks.join_next(),if !tasks.is_empty()=>{
                    let value=match result.expect("task exists"){Ok(value)=>value,Err(error)=>{
                        let index=*inflight.keys().next().expect("inflight task");
                        (index,Duration::ZERO,Err(format!("Execution task: {error}")),inflight[&index].0.clone())
                    }};
                    inflight.remove(&value.0);completed+=1;
                    let mut event=result_event(id,count,value,&job.options);
                    let matched=job.options.stop_on_match&&event.result.as_ref().is_ok_and(|e|e.run.as_ref().is_some_and(|r|r.matched));
                    if matched{
                        for pending in abort_pending(&mut tasks,&mut inflight,id,count,&job.options,"Stopped after match").await{
                            if !deliver(&results,&mut stop,pending).await{return;}
                        }
                    }
                    event.done=completed==count||matched;
                    let done=event.done;
                    let permit=tokio::select!{
                        biased;_=stop.changed()=>return,
                        _=job.cancel.changed()=>{
                            // Abort sockets even when result delivery is backpressured.
                            tasks.abort_all();
                            tokio::select!{_=stop.changed()=>return,permit=results.reserve()=>permit}
                        },
                        permit=results.reserve()=>permit,
                    };
                    match permit{Ok(permit)=>permit.send(event),Err(_)=>return}
                    if done{break;}
                },
                _=tokio::time::sleep_until(next),if dispatched<count&&!is_paused(&job.pause)&&tasks.len()<job.options.concurrency=>{},
            }
        }
    }
}

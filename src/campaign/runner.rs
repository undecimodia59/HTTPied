use super::*;
use crate::{model::WireExecution, proxy};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tokio::{task::JoinSet, time::sleep};

async fn deliver(
    tx: &mpsc::Sender<Update>,
    record: Record,
    execution: Option<WireExecution>,
) -> Result<(), String> {
    tx.send(Update::Result {
        record: Box::new(record),
        execution: execution.map(Box::new),
    })
    .await
    .map_err(|_| "Attack results receiver closed".into())
}
fn record(run: &SavedRun, index: usize, step: usize) -> Record {
    Record {
        run_id: run.id,
        index,
        step,
        label: run.workspace.steps[step].name.clone(),
        ..Default::default()
    }
}
fn observed(record: &mut Record, execution: &WireExecution) {
    record.status = execution.response_status;
    record.bytes = execution.response_total_bytes;
    record.elapsed_ms = execution.elapsed_ms;
    record.first_byte_ms = execution.first_byte_ms;
    if !execution.response_truncated
        && let Ok(message) = sequence::final_message(&execution.response)
        && let Ok(body) = crate::decoding::decoded_bytes(message, 256 * 1024)
    {
        record.body_hash = format!("{:x}", Sha256::digest(&body));
    }
}
fn check_scope(job: &Job, wire: &[u8]) -> Result<(), String> {
    let target = wire
        .split(|b| *b == b' ')
        .nth(1)
        .and_then(|v| std::str::from_utf8(v).ok())
        .ok_or("Invalid request target")?;
    if job.enforce_scope && !crate::state::in_scope(&job.scope, target) {
        return Err(format!("Outside target scope: {target}"));
    }
    Ok(())
}
fn pace(job: &Job) -> Duration {
    let rate = match (job.global_rps, job.host_rps) {
        (0, b) => b,
        (a, 0) => a,
        (a, b) => a.min(b),
    };
    if rate == 0 {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(1.0 / rate as f64)
    }
}
async fn execute(
    job: &Job,
    cfg: &proxy::Config,
    context: &sequence::Context,
    step: &Step,
    carry: bool,
) -> Result<(WireExecution, url::Url), String> {
    let (wire, target) = context.resolve(step, carry)?;
    check_scope(job, &wire)?;
    sleep(pace(job)).await;
    proxy::execute(wire, cfg)
        .await
        .map(|execution| (execution, target))
        .map_err(|e| e.to_string())
}
async fn race(job: &Job, cfg: &proxy::Config, tx: &mpsc::Sender<Update>) -> Result<(), String> {
    let run = &job.run;
    let w = &run.workspace;
    if w.synchronized && !pace(job).is_zero() {
        return Err("Synchronized bursts conflict with configured rate limits. Use Parallel sending, or explicitly change the project rate limits.".into());
    }
    if w.burst_size < w.steps.len() {
        return Err("Requests per burst must include every input request at least once".into());
    }
    let context = sequence::Context::new(job.variables.clone());
    let mut wires = Vec::new();
    let mut origins = Vec::new();
    for step in &w.steps {
        let (wire, url) = context.resolve(step, false)?;
        check_scope(job, &wire)?;
        wires.push(wire);
        origins.push(url.origin().ascii_serialization());
    }
    if w.synchronized && origins.iter().any(|value| value != &origins[0]) {
        return Err("A synchronized burst must use the same scheme, host and port".into());
    }
    for burst in 0..w.bursts {
        if burst > 0 {
            sleep(Duration::from_millis(w.delay_ms)).await;
        }
        if w.synchronized {
            let mut tasks = JoinSet::new();
            for offset in 0..w.burst_size {
                let wire = wires[offset % wires.len()].clone();
                let cfg = cfg.clone();
                tasks.spawn(async move {
                    (
                        offset,
                        proxy::stage_race(wire, &cfg)
                            .await
                            .map_err(|e| e.to_string()),
                    )
                });
            }
            let mut prepared = Vec::new();
            let mut failed = None;
            while let Some(value) = tasks.join_next().await {
                let (index, result) = value.map_err(|e| format!("Race preparation failed: {e}"))?;
                match result {
                    Ok(staged) => prepared.push((index, staged)),
                    Err(error) => failed = Some(error),
                }
            }
            if let Some(error) = failed {
                drop(prepared);
                for offset in 0..w.burst_size {
                    let mut row = record(run, burst * w.burst_size + offset, offset % wires.len());
                    row.burst = burst;
                    row.error = format!("Burst was not released: {error}");
                    deliver(tx, row, None).await?;
                }
                continue;
            }
            let origin = Instant::now();
            let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(w.burst_size));
            let mut released = JoinSet::new();
            for (offset, staged) in prepared {
                let cfg = cfg.clone();
                let barrier = barrier.clone();
                released.spawn(async move {
                    barrier.wait().await;
                    (
                        offset,
                        staged.release(cfg, origin).await.map_err(|e| e.to_string()),
                    )
                });
            }
            let mut position = 0;
            while let Some(value) = released.join_next().await {
                position += 1;
                let (offset, result) = value.map_err(|e| e.to_string())?;
                let mut row = record(run, burst * w.burst_size + offset, offset % wires.len());
                row.burst = burst;
                row.position = position;
                match result {
                    Ok((execution, release)) => {
                        row.release_us = Some(release);
                        observed(&mut row, &execution);
                        deliver(tx, row, Some(execution)).await?;
                    }
                    Err(error) => {
                        row.error = error;
                        deliver(tx, row, None).await?;
                    }
                }
            }
        } else {
            let mut tasks = JoinSet::new();
            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(w.connections));
            let gate = std::sync::Arc::new(tokio::sync::Mutex::new(tokio::time::Instant::now()));
            for offset in 0..w.burst_size {
                let wire = wires[offset % wires.len()].clone();
                let cfg = cfg.clone();
                let semaphore = semaphore.clone();
                let gate = gate.clone();
                let pace = pace(job);
                tasks.spawn(async move {
                    let _permit = semaphore.acquire_owned().await.map_err(|e| e.to_string())?;
                    let at = {
                        let mut next = gate.lock().await;
                        let at = (*next).max(tokio::time::Instant::now());
                        *next = at + pace;
                        at
                    };
                    tokio::time::sleep_until(at).await;
                    Ok::<_, String>((
                        offset,
                        proxy::execute(wire, &cfg).await.map_err(|e| e.to_string()),
                    ))
                });
            }
            let mut position = 0;
            while let Some(value) = tasks.join_next().await {
                position += 1;
                let (offset, result) = value.map_err(|e| e.to_string())??;
                let mut row = record(run, burst * w.burst_size + offset, offset % wires.len());
                row.burst = burst;
                row.position = position;
                match result {
                    Ok(execution) => {
                        observed(&mut row, &execution);
                        deliver(tx, row, Some(execution)).await?;
                    }
                    Err(error) => {
                        row.error = error;
                        deliver(tx, row, None).await?;
                    }
                }
            }
        }
    }
    Ok(())
}
async fn access(job: &Job, cfg: &proxy::Config, tx: &mpsc::Sender<Update>) -> Result<(), String> {
    let run = &job.run;
    let w = &run.workspace;
    let context = sequence::Context::new(job.variables.clone());
    let mut remove = w
        .remove_headers
        .split(',')
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>();
    for name in ["Authorization", "Cookie", "Proxy-Authorization"] {
        if !remove.iter().any(|key| key.eq_ignore_ascii_case(name)) {
            remove.push(name.into());
        }
    }
    for profile in &w.profiles {
        for (key, _) in super::access::parse_headers(&profile.headers)? {
            if !remove.iter().any(|v| v.eq_ignore_ascii_case(&key)) {
                remove.push(key);
            }
        }
    }
    // Validate every input and identity before replaying the first baseline.
    for step in &w.steps {
        let (wire, _) = context.resolve(step, false)?;
        check_scope(job, &wire)?;
    }
    let mut profiles = Vec::new();
    for profile in &w.profiles {
        let resolved = crate::state::resolve(&profile.headers, &job.variables)?;
        profiles.push((
            profile.name.clone(),
            super::access::parse_headers(&resolved)?,
        ));
    }
    if w.anonymous {
        profiles.push(("Anonymous".into(), vec![]));
    }
    let mut index = 0;
    for (step_index, step) in w.steps.iter().enumerate() {
        let mut row = record(run, index, step_index);
        row.identity = "Original".into();
        let baseline_index = index;
        index += 1;
        let baseline = match execute(job, cfg, &context, step, false).await {
            Ok((execution, _)) => {
                observed(&mut row, &execution);
                row.verdict = "Fresh baseline".into();
                let retained = execution.clone();
                deliver(tx, row, Some(execution)).await?;
                Some(retained)
            }
            Err(error) => {
                row.error = error;
                deliver(tx, row, None).await?;
                None
            }
        };
        for (identity, headers) in &profiles {
            let mut row = record(run, index, step_index);
            index += 1;
            row.identity = identity.clone();
            row.baseline = Some(baseline_index);
            if baseline.is_none() {
                row.error = "Skipped: baseline request failed".into();
                deliver(tx, row, None).await?;
                continue;
            }
            let (original, _) = context.resolve(step, false)?;
            let original = std::str::from_utf8(&original).map_err(|_| "Request is not UTF-8")?;
            // Mutate the resolved wire request once; do not recursively expand credentials.
            let variant = super::access::headers(original, &remove, headers)?.into_bytes();
            check_scope(job, &variant)?;
            sleep(pace(job)).await;
            match proxy::execute(variant, cfg)
                .await
                .map_err(|error| error.to_string())
            {
                Ok(execution) => {
                    observed(&mut row, &execution);
                    row.verdict = super::access::assess(
                        baseline.as_ref().unwrap(),
                        &execution,
                        &w.denial_marker,
                    );
                    deliver(tx, row, Some(execution)).await?;
                }
                Err(error) => {
                    row.error = error;
                    deliver(tx, row, None).await?;
                }
            }
        }
    }
    Ok(())
}
async fn sequence(job: &Job, cfg: &proxy::Config, tx: &mpsc::Sender<Update>) -> Result<(), String> {
    let run = &job.run;
    let w = &run.workspace;
    for cycle in 0..w.cycles {
        let mut context = sequence::Context::new(job.variables.clone());
        for (step_index, step) in w.steps.iter().enumerate() {
            if step_index > 0 || cycle > 0 {
                sleep(Duration::from_millis(w.delay_ms)).await;
            }
            let mut row = record(run, cycle * w.steps.len() + step_index, step_index);
            row.burst = cycle;
            let mut failed = false;
            match execute(job, cfg, &context, step, w.carry_cookies).await {
                Ok((execution, target)) => {
                    observed(&mut row, &execution);
                    if !step.expected.is_empty()
                        && !step.expected.split(',').any(|prefix| {
                            execution
                                .response_status
                                .to_string()
                                .starts_with(prefix.trim())
                        })
                    {
                        row.error = format!(
                            "Expected HTTP {}; received {}",
                            step.expected, execution.response_status
                        );
                        failed = true;
                    }
                    if !failed {
                        match context.receive(step, &target, &execution.response, w.carry_cookies) {
                            Ok(names) => {
                                row.verdict = if names.is_empty() {
                                    "Step completed".into()
                                } else {
                                    format!("Extracted: {}", names.join(", "))
                                }
                            }
                            Err(error) => {
                                row.error = error;
                                failed = true;
                            }
                        }
                    }
                    deliver(tx, row, Some(execution)).await?;
                }
                Err(error) => {
                    row.error = error;
                    failed = true;
                    deliver(tx, row, None).await?;
                }
            }
            if failed && w.stop_on_failure {
                return Err(format!(
                    "Sequence stopped at step {}: {}",
                    step_index + 1,
                    step.name
                ));
            }
        }
    }
    Ok(())
}
pub async fn run(
    mut jobs: mpsc::Receiver<Job>,
    updates: mpsc::Sender<Update>,
    config: proxy::Config,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let job = tokio::select! {biased;_=stop.changed()=>return,value=jobs.recv()=>match value{Some(job)=>job,None=>return}};
        let id = job.run.id;
        let outcome = match job.run.workspace.validate() {
            Err(error) => Err(error),
            Ok(()) => {
                let mut cfg = config.clone();
                cfg.interception = None;
                cfg.policy = None;
                cfg.max_capture = cfg.max_capture.min(256 * 1024);
                cfg.total_timeout = Duration::from_secs(job.run.workspace.timeout_secs);
                let mut cancel = job.cancel.clone();
                if *cancel.borrow() {
                    Err("Cancelled".into())
                } else {
                    tokio::select! {biased;_=stop.changed()=>return,_=cancel.changed()=>Err("Cancelled; requests already released may have reached the target".into()),result=async{match job.run.workspace.kind{Kind::Race=>race(&job,&cfg,&updates).await,Kind::Access=>access(&job,&cfg,&updates).await,Kind::Sequence=>sequence(&job,&cfg,&updates).await}}=>result}
                }
            }
        };
        let message = outcome.err().unwrap_or_else(|| "Completed".into());
        tokio::select! {
            biased;
            _=stop.changed()=>return,
            sent=updates.send(Update::Done{run_id:id,message})=>if sent.is_err(){return;},
        }
    }
}

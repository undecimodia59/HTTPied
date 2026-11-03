use super::*;
use crate::{
    browser,
    control::{Decision, Interception, Pending},
    jobs::{Job, ResultEvent},
    replay,
};
use std::{net::SocketAddr, time::Duration};

#[path = "replay_groups.rs"]
mod replay_groups;

static NEXT_EDITOR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
#[derive(Clone, Copy, PartialEq)]
enum PayloadMode {
    Text,
    Numbers,
    File,
    Charset,
    Random,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tab {
    Intercept,
    History,
    Replay,
    Intruder,
    Attack,
    Settings,
    Compare,
    Coder,
}
struct Held {
    pending: Pending,
    text: String,
    editable: bool,
    original: String,
}
pub(super) struct Tool {
    pub destination: String,
    pub request: String,
    name: String,
    baseline: String,
    baseline_destination: String,
    group: String,
    timeout_secs: u64,
    response_bytes: Vec<u8>,
    response_mode: u8,
    response_find: String,
    response_search_next: usize,
    response_note: Option<String>,
    last_target: String,
    response: String,
    status: String,
    editor_id: u64,
    repair: bool,
}
impl Tool {
    fn curl(&self, variables: &[crate::state::Variable], redact: bool) -> Result<String, String> {
        let mut variables = variables.to_vec();
        if redact {
            for variable in &mut variables {
                if variable.secret {
                    variable.value = "[REDACTED]".into();
                }
            }
        }
        let destination = crate::state::resolve(&self.destination, &variables)?;
        let request = crate::state::resolve(&self.request, &variables)?;
        crate::interchange::curl(&destination, &request, redact)
    }
    fn clear_response(&mut self) {
        self.response_bytes.clear();
        self.response.clear();
        self.response_note = None;
        self.last_target.clear();
        self.response_search_next = 0;
    }
    fn response_view(&mut self) -> String {
        self.response_note = None;
        if self.response_bytes.is_empty() {
            return String::new();
        }
        match self.response_mode {
            1 => crate::exchange::pretty(&self.automatic_response()),
            2 => hex_preview(&self.response_bytes),
            3 => crate::exchange::decoded(&self.response_bytes)
                .unwrap_or_else(|e| format!("Decoded view unavailable: {e}")),
            4 => self.automatic_response(),
            _ => preview(&self.response_bytes, PREVIEW),
        }
    }
    fn automatic_response(&mut self) -> String {
        let view = crate::exchange::auto_preview(&self.response_bytes);
        self.response_note = view.note;
        view.text
    }
    fn new() -> Self {
        Self {
            name: "Request".into(),
            baseline: "GET / HTTP/1.1\r\n\r\n".into(),
            baseline_destination: String::new(),
            group: String::new(),
            timeout_secs: 0,
            response_bytes: Vec::new(),
            response_mode: 4,
            response_find: String::new(),
            response_search_next: 0,
            response_note: None,
            last_target: String::new(),
            destination: String::new(),
            request: "GET / HTTP/1.1\r\n\r\n".into(),
            response: String::new(),
            status: "Ready".into(),
            editor_id: NEXT_EDITOR.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            repair: true,
        }
    }
}
pub(super) struct Workflow {
    pub tab: Tab,
    pub trust_ready: bool,
    pub listener_ready: bool,
    pub variables: Vec<crate::state::Variable>,
    pub scope: Vec<crate::state::ScopeRule>,
    pub settings: crate::state::Settings,
    pub control: Interception,
    pending_rx: mpsc::Receiver<Pending>,
    held: VecDeque<Held>,
    replays: Vec<Tool>,
    replay_groups: Vec<String>,
    replay_search: String,
    new_group: String,
    replay_selection: std::collections::BTreeSet<u64>,
    selected_replay: usize,
    intruder: Tool,
    payloads: String,
    attack_mode: crate::attack::Mode,
    encoding: crate::attack::Encoding,
    positions: usize,
    extra_payloads: Vec<String>,
    defaults: Vec<String>,
    prefix: String,
    suffix: String,
    pause: Option<watch::Sender<bool>>,
    pub completed: VecDeque<crate::jobs::Record>,
    matcher: String,
    stop_on_match: bool,
    payload_mode: PayloadMode,
    number_start: String,
    number_end: String,
    number_step: String,
    alphabet: String,
    generated_width: usize,
    generated_count: usize,
    generated_seed: u64,
    file_worker: crate::payload::FileWorker,
    file_path: String,
    file_values: Option<std::sync::Arc<Vec<String>>>,
    file_busy: bool,
    jobs: mpsc::Sender<Job>,
    results: mpsc::Receiver<ResultEvent>,
    active: Option<(u64, Tab, watch::Sender<bool>)>,
    pub(super) external_active: bool,
    next_job: u64,
    browser_tx: Option<std::sync::mpsc::SyncSender<browser::Action>>,
    browser_rx: Option<std::sync::mpsc::Receiver<String>>,
    browser_status: String,
    pub listen: SocketAddr,
    pub(super) message: String,
}
impl Workflow {
    pub fn new(
        control: Interception,
        pending_rx: mpsc::Receiver<Pending>,
        jobs: mpsc::Sender<Job>,
        results: mpsc::Receiver<ResultEvent>,
        listen: SocketAddr,
    ) -> Self {
        Self {
            tab: Tab::History,
            trust_ready: false,
            listener_ready: false,
            variables: Vec::new(),
            scope: Vec::new(),
            settings: Default::default(),
            control,
            pending_rx,
            held: VecDeque::new(),
            replays: vec![Tool::new()],
            replay_groups: vec![],
            replay_search: String::new(),
            new_group: String::new(),
            replay_selection: Default::default(),
            selected_replay: 0,
            intruder: Tool::new(),
            payloads: String::new(),
            attack_mode: Default::default(),
            encoding: Default::default(),
            positions: 1,
            extra_payloads: vec![String::new(); 3],
            defaults: vec![String::new(); 4],
            prefix: String::new(),
            suffix: String::new(),
            pause: None,
            completed: VecDeque::new(),
            matcher: String::new(),
            stop_on_match: false,
            payload_mode: PayloadMode::Text,
            number_start: "0".into(),
            number_end: "100".into(),
            number_step: "1".into(),
            alphabet: "0123456789abcdef".into(),
            generated_width: 4,
            generated_count: 100,
            generated_seed: 1,
            file_worker: crate::payload::FileWorker::start(),
            file_path: String::new(),
            file_values: None,
            file_busy: false,
            jobs,
            results,
            active: None,
            external_active: false,
            next_job: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            browser_tx: None,
            browser_rx: None,
            browser_status: "Not launched".into(),
            listen,
            message: String::new(),
        }
    }
    pub(super) fn restore(&mut self, state: &crate::state::ProjectState) {
        self.settings = state.settings.clone();
        self.variables = state.variables.clone();
        self.scope = state.scope.clone();
        self.replay_groups = state.replay_groups.clone();
        if !state.replays.is_empty() {
            self.replays = state
                .replays
                .iter()
                .take(crate::state::MAX_REPLAYS)
                .map(|saved| {
                    let mut tool = Tool::new();
                    tool.destination = saved.destination.clone();
                    tool.request = saved.request.clone();
                    tool.baseline = saved.request.clone();
                    tool.name = saved.name.clone();
                    tool.group = saved.group.clone();
                    tool.timeout_secs = saved.timeout_secs;
                    tool.baseline_destination = saved.destination.clone();
                    tool
                })
                .collect();
            self.selected_replay = state.selected_replay.min(self.replays.len() - 1);
        }
    }
    pub(super) fn mark_saved(&mut self, saved: &[crate::state::SavedRequest]) {
        for tool in &mut self.replays {
            if saved
                .iter()
                .any(|s| s.request == tool.request && s.destination == tool.destination)
            {
                tool.baseline = tool.request.clone();
                tool.baseline_destination = tool.destination.clone();
            }
        }
    }
    pub(super) fn is_running(&self) -> bool {
        self.active.is_some()
    }
    pub(super) fn stop(&self) {
        if let Some((_, _, cancel)) = &self.active {
            cancel.send_replace(true);
        }
    }
    pub(super) fn saved_requests(&self) -> Vec<crate::state::SavedRequest> {
        self.replays
            .iter()
            .map(|tool| crate::state::SavedRequest {
                name: tool.name.clone(),
                group: tool.group.clone(),
                timeout_secs: tool.timeout_secs,
                destination: tool.destination.clone(),
                request: tool.request.clone(),
            })
            .collect()
    }
    pub fn offline(listen: SocketAddr) -> Self {
        let (control, pending) = Interception::channel(16, Duration::from_secs(60));
        let (jobs, _) = mpsc::channel(2);
        let (_, results) = mpsc::channel(2);
        Self::new(control, pending, jobs, results, listen)
    }
    pub fn set_browser(
        &mut self,
        tx: std::sync::mpsc::SyncSender<browser::Action>,
        rx: std::sync::mpsc::Receiver<String>,
    ) {
        self.browser_tx = Some(tx);
        self.browser_rx = Some(rx);
    }
    fn poll(&mut self) -> Vec<Event> {
        self.held.retain(|held| !held.pending.decision.is_closed());
        while self.held.len() < 16 {
            let Ok(pending) = self.pending_rx.try_recv() else {
                break;
            };
            if pending.decision.is_closed() {
                continue;
            }
            let editable = std::str::from_utf8(&pending.request).is_ok();
            let text = preview(&pending.request, 128 * 1024);
            self.held.push_back(Held {
                original: text.clone(),
                text,
                pending,
                editable,
            });
        }
        if let Some(rx) = &self.browser_rx {
            while let Ok(message) = rx.try_recv() {
                if message.to_lowercase().contains("failed") || message.contains("already running")
                {
                    self.message = message.clone();
                }
                self.browser_status = message;
            }
        }
        while let Ok(loaded) = self.file_worker.results.try_recv() {
            self.file_busy = false;
            match loaded.result {
                Ok(Some(values)) => {
                    self.file_path = loaded
                        .path
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    self.file_values = Some(values);
                    self.message.clear();
                }
                Ok(None) => {}
                Err(error) => {
                    self.file_values = None;
                    self.message = format!("Wordlist: {error}");
                }
            }
        }
        let mut events = Vec::new();
        for _ in 0..4 {
            let Ok(result) = self.results.try_recv() else {
                break;
            };
            let Some((id, tab, _)) = &self.active else {
                continue;
            };
            if *id != result.id {
                continue;
            }
            let tab = *tab;
            if tab == Tab::Intruder && !result.terminal {
                let (status, bytes, error, matched) = match &result.result {
                    Ok(e) => (
                        e.response_status,
                        e.response_total_bytes,
                        String::new(),
                        !self.matcher.is_empty()
                            && e.response
                                .windows(self.matcher.len())
                                .any(|w| w == self.matcher.as_bytes()),
                    ),
                    Err(error) => (0, 0, error.clone(), false),
                };
                self.completed.push_back(crate::jobs::Record {
                    capture_id: None,
                    run_id: result.id,
                    index: result.index,
                    total: result.total,
                    payloads: result.payloads.clone(),
                    status,
                    bytes,
                    elapsed_ms: result.elapsed.as_secs_f64() * 1000.0,
                    error,
                    matched,
                });
            }
            let tool = if tab == Tab::Replay {
                &mut self.replays[self.selected_replay]
            } else {
                &mut self.intruder
            };
            let (response, summary) = match result.result {
                Ok(execution) => {
                    tool.response_bytes = execution.response
                        [..execution.response.len().min(2 * 1024 * 1024)]
                        .to_vec();
                    tool.last_target = execution.parsed.target.clone();
                    let response = tool.response_view();
                    let summary = format!(
                        "{}/{}  HTTP {}  {:.1} ms  {}{}",
                        result.index + 1,
                        result.total,
                        execution.response_status,
                        result.elapsed.as_secs_f64() * 1000.0,
                        crate::state::size(
                            execution.response_total_bytes,
                            self.settings.binary_units
                        ),
                        if execution.response_truncated || execution.response.len() > PREVIEW {
                            " (preview truncated)"
                        } else {
                            ""
                        }
                    );
                    events.push(Event::Exchange(execution));
                    (response, summary)
                }
                Err(error) => {
                    tool.clear_response();
                    events.push(Event::Error(error.clone()));
                    (
                        String::new(),
                        format!("{}/{}  Failed · see Logs", result.index + 1, result.total),
                    )
                }
            };
            tool.response = response;
            tool.status = summary;
            if result.done {
                self.active = None;
            }
        }
        events
    }
    fn launch_browser(&mut self) {
        if !self.listener_ready {
            self.message = "The proxy listener is not ready. Check the listener status.".into();
            return;
        }
        if !self.trust_ready {
            self.message =
                "HTTPS certificate setup is not ready. See Settings / Certificates.".into();
            return;
        }

        let Some(tx) = &self.browser_tx else {
            self.browser_status = "Browser worker unavailable".into();
            return;
        };
        match tx.try_send(browser::Action::Launch {
            executable: self.settings.browser.clone(),
            proxy: self.listen,
            url: self.settings.browser_url.clone(),
        }) {
            Ok(()) => self.browser_status = "Launching…".into(),
            Err(error) => self.browser_status = error.to_string(),
        }
    }
    pub(super) fn load(&mut self, bytes: &[u8], tab: Tab) {
        if bytes.len() > 128 * 1024 {
            self.message =
                "Request exceeds the 128 KiB editor limit; it was not truncated or loaded.".into();
            return;
        }
        match replay::origin_draft(bytes) {
            Ok((destination, request)) => {
                self.load_draft(destination, request, tab);
            }
            Err(error) => self.message = error.to_string(),
        }
    }
    pub(super) fn load_draft(&mut self, destination: String, request: String, tab: Tab) {
        if request.len() > 128 * 1024 || destination.len() > 2048 {
            self.message =
                "Request exceeds the editor limit; it was not truncated or loaded.".into();
            return;
        }
        if self
            .active
            .as_ref()
            .is_some_and(|(_, active, _)| *active == tab)
        {
            self.message =
                "Wait for the active run or cancel it before replacing its request.".into();
            return;
        }
        if tab == Tab::Replay {
            if self.replays.len() >= crate::state::MAX_REPLAYS {
                self.message =
                    "Replay has 64 requests. Close one before sending another request.".into();
                return;
            }
            let mut tool = Tool::new();
            tool.group = self.replays[self.selected_replay].group.clone();
            self.replays.push(tool);
            self.selected_replay = self.replays.len() - 1;
            self.replay_selection.clear();
        }
        let tool = if tab == Tab::Replay {
            &mut self.replays[self.selected_replay]
        } else {
            &mut self.intruder
        };
        tool.destination = destination;
        tool.request = request;
        tool.clear_response();
        tool.status = "Request loaded".into();
        self.tab = tab;
        self.message.clear();
    }
    pub(super) fn load_execution(&mut self, execution: &crate::model::WireExecution, tab: Tab) {
        match replay::executed_request(execution) {
            Ok(bytes) => self.load(&bytes, tab),
            Err(error) => self.message = error.to_string(),
        }
    }
    pub(super) fn load_result(&mut self, bytes: std::io::Result<Vec<u8>>, tab: Tab) {
        match bytes {
            Ok(bytes) => self.load(&bytes, tab),
            Err(error) => self.message = error.to_string(),
        }
    }
    fn make_plan(&self) -> Result<crate::attack::Plan, String> {
        let primary = match self.payload_mode {
            PayloadMode::Text => crate::payload::Payloads::lines(std::sync::Arc::new(
                self.payloads.lines().map(str::to_owned).collect(),
            )),
            PayloadMode::Numbers => crate::payload::Payloads::numbers(
                &self.number_start,
                &self.number_end,
                &self.number_step,
            ),
            PayloadMode::Charset => crate::payload::Payloads::generated(
                &self.alphabet,
                self.generated_width,
                None,
                None,
            ),
            PayloadMode::Random => crate::payload::Payloads::generated(
                &self.alphabet,
                self.generated_width,
                Some(self.generated_count),
                Some(self.generated_seed),
            ),
            PayloadMode::File => self
                .file_values
                .clone()
                .ok_or_else(|| std::io::Error::other("Load a wordlist first"))
                .and_then(crate::payload::Payloads::lines),
        }
        .map_err(|e| e.to_string())?;
        let mut sources = vec![primary.clone()];
        for text in self.extra_payloads.iter().take(self.positions - 1) {
            sources.push(if text.is_empty() {
                primary.clone()
            } else {
                crate::payload::Payloads::lines(std::sync::Arc::new(
                    text.lines().map(str::to_owned).collect(),
                ))
                .map_err(|e| e.to_string())?
            });
        }
        let mut plan = crate::attack::Plan::new(sources, self.attack_mode)?;
        plan.encoding = self.encoding;
        plan.prefix = self.prefix.clone();
        plan.suffix = self.suffix.clone();
        plan.defaults = self.defaults[..self.positions].to_vec();
        Ok(plan)
    }
    pub(super) fn start_job(&mut self, tab: Tab) {
        if self.external_active {
            self.message =
                "An Attack experiment is running. Stop it before starting another execution."
                    .into();
            return;
        }

        if tab == Tab::Intruder && self.payload_mode == PayloadMode::File && self.file_busy {
            self.message = "Wait for the wordlist to finish loading.".into();
            return;
        }
        if self.active.is_some() {
            self.message = "One execution job can run at a time. Cancel it or wait.".into();
            return;
        }
        let plan = if tab == Tab::Intruder {
            match self.make_plan() {
                Ok(plan) => Some(plan),
                Err(error) => {
                    self.message = error;
                    return;
                }
            }
        } else {
            None
        };
        let tool = if tab == Tab::Replay {
            &mut self.replays[self.selected_replay]
        } else {
            &mut self.intruder
        };
        let mut variables = self.variables.clone();
        if tab == Tab::Intruder {
            for slot in 0..self.positions {
                let key = if slot == 0 {
                    "payload".into()
                } else {
                    format!("payload{}", slot + 1)
                };
                variables.push(crate::state::Variable {
                    value: format!("{{{{{key}}}}}"),
                    key,
                    secret: false,
                    enabled: true,
                });
            }
        }
        let resolved =
            crate::state::resolve(&tool.destination, &variables).and_then(|destination| {
                crate::state::resolve(&tool.request, &variables)
                    .map(|request| (destination, request))
            });
        let (destination, request) = match resolved {
            Ok(value) => value,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        let requests = if tab == Tab::Intruder {
            for slot in 0..self.positions {
                let marker = if slot == 0 {
                    "{{payload}}".into()
                } else {
                    format!("{{{{payload{}}}}}", slot + 1)
                };
                if !request.contains(&marker) {
                    self.message = format!("Insert {marker} in the request");
                    return;
                }
            }
            crate::jobs::Requests::Multi {
                destination: destination.clone(),
                template: request.clone(),
                repair: tool.repair,
                plan: plan.unwrap(),
            }
        } else {
            match replay::build(&destination, &request, tool.repair) {
                Ok(request) => vec![request].into(),
                Err(error) => {
                    self.message = error.to_string();
                    return;
                }
            }
        };
        let (cancel, rx) = watch::channel(false);
        let (pause, pause_rx) = watch::channel(false);
        self.pause = Some(pause);
        let id = self.next_job;
        self.next_job += 1;
        match self.jobs.try_send(Job {
            id,
            requests,
            delay: Duration::from_millis(self.settings.delay_ms),
            cancel: rx,
            pause: Some(pause_rx),
            options: crate::jobs::Options {
                timeout_secs: tool.timeout_secs,
                concurrency: self.settings.concurrency,
                retries: self.settings.retries,
                backoff: Duration::from_millis(self.settings.backoff_ms),
                enforce_scope: self.settings.enforce_scope,
                scope: self.scope.clone(),
                global_rps: self.settings.global_rps,
                host_rps: self.settings.host_rps,
                matcher: self.matcher.clone(),
                stop_on_match: self.stop_on_match,
            },
        }) {
            Ok(()) => {
                self.active = Some((id, tab, cancel));
                tool.clear_response();
                tool.status = "Running…".into();
                self.message.clear();
            }
            Err(error) => self.message = error.to_string(),
        }
    }
    fn intercept(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let enabled = self.control.enabled();
            let label = if enabled {
                "Intercept: ON"
            } else {
                "Intercept: OFF"
            };
            if ui
                .add(egui::Button::new(label).fill(if enabled {
                    egui::Color32::from_rgb(49, 65, 87)
                } else {
                    SURFACE
                }))
                .clicked()
            {
                self.control.set_enabled(!enabled);
            }
            ui.label(format!("{} held", self.held.len()));
        });
        muted(
            ui,
            "Requests matching your interception rules pause here. Response buffering is limited to 128 KiB.",
        );
        let mut action = None;
        if let Some(held) = self.held.front_mut() {
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!(
                    "{} #{} • {}",
                    if held.pending.response {
                        "Response"
                    } else {
                        "Request"
                    },
                    held.pending.id,
                    held.pending.authority
                ));
                muted(
                    ui,
                    format!(
                        "{} s remaining",
                        held.pending
                            .deadline
                            .saturating_duration_since(std::time::Instant::now())
                            .as_secs()
                    ),
                );
                if primary(ui, true, "Forward").clicked() {
                    if held.text == held.original {
                        action = Some(Decision::Forward);
                    } else {
                        match if held.pending.response {
                            crate::response::repair(held.text.as_bytes(), false)
                        } else {
                            replay::build(&held.pending.destination, &held.text, true)
                        } {
                            Ok(bytes) => action = Some(Decision::Edit(bytes)),
                            Err(error) => self.message = error.to_string(),
                        }
                    }
                }
                if held.text != held.original && ui.button("Discard edits & forward").clicked() {
                    action = Some(Decision::Forward);
                }
                if ui.button("Drop").clicked() {
                    action = Some(Decision::Drop);
                }
            });
            if !held.editable {
                ui.colored_label(
                    WARN,
                    "Binary request: editing disabled; forward original or drop.",
                );
            }
            card().show(ui, |ui| {
                egui::ScrollArea::both()
                    .max_height((ui.available_height() - 100.0).max(180.0))
                    .show(ui, |ui| {
                        let id = egui::Id::new(("held", held.pending.id));
                        syntax::register_request(
                            ui,
                            id,
                            (!held.pending.response).then(|| {
                                if !held.editable || held.pending.request.len() > 128 * 1024 {
                                    Err("This request cannot be loaded losslessly into the text editor".into())
                                } else {
                                    Ok(syntax::RequestSource {
                                        destination: held.pending.destination.clone(),
                                        request: held.text.clone(),
                                    })
                                }
                            }),
                        );
                        syntax::editor(
                            ui,
                            egui::Id::new(("held", held.pending.id)),
                            &mut held.text,
                            held.editable,
                            24,
                        );
                    });
            });
        } else {
            empty_state(
                ui,
                if self.control.enabled() {
                    "Waiting for a matching request"
                } else {
                    "Traffic is flowing without interception"
                },
                if self.control.enabled() {
                    "Use the browser. Matching requests pause here for editing before they are forwarded."
                } else {
                    "Captured traffic is available in History. Turn interception on when you need to pause a request."
                },
            );
        }
        if let Some(decision) = action
            && let Some(held) = self.held.pop_front()
        {
            let _ = held.pending.decision.send(decision);
            self.message.clear();
        }
    }
    fn tool(&mut self, ui: &mut egui::Ui, tab: Tab) {
        let busy = self.active.is_some();
        let active = self
            .active
            .as_ref()
            .is_some_and(|(_, owner, _)| *owner == tab);
        let mut send = false;
        let mut cancel = false;
        if active
            && tab == Tab::Intruder
            && let Some(pause) = &self.pause
        {
            let paused = *pause.borrow();
            if ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
                pause.send_replace(!paused);
            }
        }
        let planned = if tab == Tab::Intruder {
            self.make_plan().ok().map(|plan| plan.len())
        } else {
            None
        };
        if tab == Tab::Replay {
            // Side panels must shrink this Ui directly. A surrounding scope would
            // allocate their full height and push the editors below the viewport.
            self.replay_sessions(ui);
        }
        let tool = if tab == Tab::Replay {
            &mut self.replays[self.selected_replay]
        } else {
            &mut self.intruder
        };
        ui.horizontal_wrapped(|ui| {
            ui.label("Destination");
            ui.add_enabled(
                !active,
                egui::TextEdit::singleline(&mut tool.destination)
                    .hint_text("https://target.example")
                    .desired_width((ui.available_width() - 200.0).clamp(240.0, 720.0))
                    .char_limit(2048),
            );
            send = primary(
                ui,
                !busy
                    && !(tab == Tab::Intruder
                        && self.payload_mode == PayloadMode::File
                        && self.file_busy),
                if tab == Tab::Replay {
                    "Send  Ctrl+Enter"
                } else {
                    "Start attack"
                },
            )
            .clicked();
            if active {
                cancel = ui.button("Stop").clicked();
            }
        });
        if tab == Tab::Intruder {
            egui::CollapsingHeader::new("Payload configuration")
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("payload-config-scroll")
                        .max_height(210.0)
                        .show(ui, |ui| {
                            ui.add_enabled_ui(!active, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Payload source");
                                    egui::ComboBox::from_id_salt("payload-source")
                                        .selected_text(match self.payload_mode {
                                            PayloadMode::Text => "Text",
                                            PayloadMode::Numbers => "Numbers",
                                            PayloadMode::File => "Wordlist",
                                            PayloadMode::Charset => "Character set",
                                            PayloadMode::Random => "Seeded random",
                                        })
                                        .show_ui(ui, |ui| {
                                            for (mode, label) in [
                                                (PayloadMode::Text, "Text"),
                                                (PayloadMode::Numbers, "Numbers"),
                                                (PayloadMode::File, "Wordlist"),
                                                (PayloadMode::Charset, "Character set"),
                                                (PayloadMode::Random, "Seeded random"),
                                            ] {
                                                ui.selectable_value(
                                                    &mut self.payload_mode,
                                                    mode,
                                                    label,
                                                );
                                            }
                                        });
                                    ui.menu_button("Attack options", |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.selectable_value(
                                                &mut self.attack_mode,
                                                crate::attack::Mode::Sniper,
                                                "Sniper",
                                            );
                                            ui.selectable_value(
                                                &mut self.attack_mode,
                                                crate::attack::Mode::Pitchfork,
                                                "Pitchfork",
                                            );
                                            ui.selectable_value(
                                                &mut self.attack_mode,
                                                crate::attack::Mode::ClusterBomb,
                                                "Cluster bomb",
                                            );
                                            ui.label("Positions");
                                            ui.add(
                                                egui::DragValue::new(&mut self.positions)
                                                    .range(1..=4),
                                            );
                                            ui.selectable_value(
                                                &mut self.encoding,
                                                crate::attack::Encoding::None,
                                                "Raw",
                                            );
                                            ui.selectable_value(
                                                &mut self.encoding,
                                                crate::attack::Encoding::Url,
                                                "URL",
                                            );
                                            ui.selectable_value(
                                                &mut self.encoding,
                                                crate::attack::Encoding::Base64,
                                                "Base64",
                                            );
                                        });
                                        ui.collapsing("Additional sets and transforms", |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label("Prefix");
                                                ui.text_edit_singleline(&mut self.prefix);
                                                ui.label("Suffix");
                                                ui.text_edit_singleline(&mut self.suffix);
                                            });
                                            for slot in 1..self.positions {
                                                ui.label(format!(
                                    "{{{{payload{}}}}} · one per line; empty uses primary set",
                                    slot + 1
                                ));
                                                ui.add(
                                                    egui::TextEdit::multiline(
                                                        &mut self.extra_payloads[slot - 1],
                                                    )
                                                    .char_limit(16384)
                                                    .desired_rows(2),
                                                );
                                            }
                                            ui.label("Sniper defaults for inactive positions");
                                            for slot in 0..self.positions {
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("Position {}", slot + 1));
                                                    ui.add(
                                                        egui::TextEdit::singleline(
                                                            &mut self.defaults[slot],
                                                        )
                                                        .char_limit(4096),
                                                    );
                                                });
                                            }
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Response contains");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.matcher)
                                                    .char_limit(256),
                                            );
                                            ui.checkbox(&mut self.stop_on_match, "Stop on match");
                                        });
                                    });
                                    ui.label("{{payload}}").on_hover_text(
                                        "Use {{payload}}, {{payload2}}, etc. in the request",
                                    );
                                });
                                match self.payload_mode {
                                    PayloadMode::Charset | PayloadMode::Random => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label("Alphabet");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.alphabet)
                                                    .char_limit(1024)
                                                    .desired_width(220.0),
                                            );
                                            ui.label("Length");
                                            ui.add(
                                                egui::DragValue::new(&mut self.generated_width)
                                                    .range(1..=256),
                                            );
                                            if self.payload_mode == PayloadMode::Random {
                                                ui.label("Count");
                                                ui.add(
                                                    egui::DragValue::new(&mut self.generated_count)
                                                        .range(1..=1000000),
                                                );
                                                ui.label("Seed");
                                                ui.add(egui::DragValue::new(
                                                    &mut self.generated_seed,
                                                ));
                                            }
                                        });
                                    }
                                    PayloadMode::Text => {
                                        ui.add(
                                            egui::TextEdit::multiline(&mut self.payloads)
                                                .hint_text("One payload per line")
                                                .char_limit(16 * 1024)
                                                .desired_rows(3),
                                        );
                                    }
                                    PayloadMode::Numbers => {
                                        ui.horizontal(|ui| {
                                            ui.label("From");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.number_start)
                                                    .desired_width(130.0)
                                                    .char_limit(20),
                                            );
                                            ui.label("To");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.number_end)
                                                    .desired_width(130.0)
                                                    .char_limit(20),
                                            );
                                            ui.label("Step");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.number_step)
                                                    .desired_width(60.0)
                                                    .char_limit(20),
                                            );
                                        });
                                        match crate::payload::Payloads::numbers(
                                            &self.number_start,
                                            &self.number_end,
                                            &self.number_step,
                                        ) {
                                            Ok(mut source) => {
                                                let count = source.len();
                                                ui.label(format!(
                                    "{count} payloads; first: {} (leading zeros preserved)",
                                    source.next().unwrap_or_default()
                                ));
                                            }
                                            Err(error) => {
                                                ui.colored_label(ERROR, error.to_string());
                                            }
                                        }
                                    }
                                    PayloadMode::File => {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add_enabled(
                                                    !self.file_busy,
                                                    egui::TextEdit::singleline(&mut self.file_path)
                                                        .hint_text("Wordlist path")
                                                        .desired_width(450.0)
                                                        .char_limit(4096),
                                                )
                                                .changed()
                                            {
                                                self.file_values = None;
                                            }
                                            if ui
                                                .add_enabled(
                                                    !self.file_busy,
                                                    egui::Button::new("Browse…"),
                                                )
                                                .clicked()
                                            {
                                                match self
                                                    .file_worker
                                                    .commands
                                                    .try_send(crate::payload::FileAction::Browse)
                                                {
                                                    Ok(()) => self.file_busy = true,
                                                    Err(error) => self.message = error.to_string(),
                                                }
                                            }
                                            if ui
                                                .add_enabled(
                                                    !self.file_busy,
                                                    egui::Button::new("Load"),
                                                )
                                                .clicked()
                                            {
                                                match self.file_worker.commands.try_send(
                                                    crate::payload::FileAction::Load(
                                                        self.file_path.clone().into(),
                                                    ),
                                                ) {
                                                    Ok(()) => self.file_busy = true,
                                                    Err(error) => self.message = error.to_string(),
                                                }
                                            }
                                        });
                                        if self.file_busy {
                                            ui.label("Loading wordlist…");
                                        } else if let Some(values) = &self.file_values {
                                            ui.label(format!("{} payloads loaded", values.len()));
                                        }
                                    }
                                }
                            });
                        });
                });
        }
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("Request actions", |ui| {
                if tab == Tab::Replay {
                    for (label, redacted) in
                        [("Copy as cURL", false), ("Copy as cURL (redacted)", true)]
                    {
                        if ui.button(label).clicked() {
                            match tool.curl(&self.variables, redacted) {
                                Ok(command) => ui.ctx().copy_text(command),
                                Err(error) => self.message = error,
                            }
                            ui.close();
                        }
                    }
                }
                ui.separator();
                ui.checkbox(&mut tool.repair, "Update Content-Length when sending");
                ui.label("Group");
                ui.add(
                    egui::TextEdit::singleline(&mut tool.group)
                        .char_limit(64)
                        .desired_width(180.0),
                );
                ui.label("Timeout (0 uses Settings)");
                ui.add(
                    egui::DragValue::new(&mut tool.timeout_secs)
                        .range(0..=86400)
                        .suffix(" s"),
                );
                if tab == Tab::Replay {
                    ui.collapsing("Resolved preview", |ui| {
                        let mut masked = self.variables.clone();
                        for var in &mut masked {
                            if var.secret {
                                var.value = "[REDACTED]".into();
                            }
                        }
                        match crate::state::resolve(&tool.destination, &masked).and_then(
                            |destination| {
                                crate::state::resolve(&tool.request, &masked)
                                    .map(|request| format!("{destination}\n{request}"))
                            },
                        ) {
                            Ok(text) => {
                                ui.monospace(text);
                            }
                            Err(error) => {
                                ui.colored_label(ERROR, error);
                            }
                        }
                    });
                }
            });
            ui.weak(&tool.status);
            if let Some(count) = planned {
                ui.weak(format!("{count} planned requests"));
            }
        });
        let width = ui.available_width();
        let height = (ui.available_height() - 75.0).max(90.0);
        resize_split_with_window(ui, egui::Id::new(("request-split", tool.editor_id)), width);
        let left = egui::Panel::left(egui::Id::new(("request-split", tool.editor_id)))
            .frame(egui::Frame::NONE)
            .resizable(true)
            .default_size(width * self.settings.request_ratio)
            .size_range(210.0..=(width - 210.0).max(210.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(27.0);
                    ui.strong("Request");
                    syntax::wrap_control(ui, egui::Id::new(("request", tool.editor_id)));
                });
                egui::ScrollArea::both()
                    .id_salt(("tool-request", tool.editor_id))
                    .max_height(height)
                    .min_scrolled_height(height)
                    .show(ui, |ui| {
                        syntax::register_request(
                            ui,
                            egui::Id::new(("request", tool.editor_id)),
                            Some(Ok(syntax::RequestSource {
                                destination: tool.destination.clone(),
                                request: tool.request.clone(),
                            })),
                        );
                        syntax::editor(
                            ui,
                            egui::Id::new(("request", tool.editor_id)),
                            &mut tool.request,
                            !active,
                            25,
                        );
                    });
            });
        self.settings.request_ratio = (left.response.rect.width() / width).clamp(0.15, 0.85);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let mut response_jump = None;
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Response");
                    syntax::wrap_control(ui, egui::Id::new(("response", tool.editor_id)));
                    let previous = tool.response_mode;
                    egui::ComboBox::from_id_salt(("response-mode", tool.editor_id))
                        .selected_text(match tool.response_mode {
                            0 => "Raw",
                            1 => "Pretty",
                            2 => "Hex",
                            3 => "Decoded",
                            _ => "Auto",
                        })
                        .show_ui(ui, |ui| {
                            for (mode, label) in [
                                (4, "Auto"),
                                (0, "Raw"),
                                (1, "Pretty"),
                                (2, "Hex"),
                                (3, "Decoded"),
                            ] {
                                ui.selectable_value(&mut tool.response_mode, mode, label);
                            }
                        });
                    ui.menu_button("Response actions", |ui| {
                        if ui.button("Copy response view").clicked() {
                            ui.ctx().copy_text(tool.response.clone());
                            ui.close();
                        }
                        for (label, id) in [
                            ("Set Compare baseline", "compare-baseline"),
                            ("Compare with baseline", "compare-candidate"),
                        ] {
                            if ui.button(label).clicked() {
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(egui::Id::new(id), tool.response.clone())
                                });
                                ui.close();
                            }
                        }
                    });
                    if previous != tool.response_mode {
                        tool.response = tool.response_view();
                    }
                });
                if let Some(note) = &tool.response_note {
                    ui.weak(note);
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Find");
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut tool.response_find)
                            .desired_width(120.0)
                            .char_limit(256),
                    );
                    if search.changed() {
                        tool.response_search_next = 0;
                    }
                    let next = ui
                        .add_enabled(
                            !tool.response_find.is_empty(),
                            egui::Button::new("Next match"),
                        )
                        .clicked()
                        || (search.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                    if next {
                        response_jump = syntax::find_next(
                            ui,
                            egui::Id::new(("response", tool.editor_id)),
                            &tool.response,
                            &tool.response_find,
                            &mut tool.response_search_next,
                        );
                    }
                    if !tool.response_find.is_empty() {
                        ui.label(format!(
                            "{} matches",
                            tool.response.matches(&tool.response_find).count()
                        ));
                    }
                });
                let mut area = egui::ScrollArea::both().id_salt(("tool-response", tool.editor_id));
                if let Some(line) = response_jump {
                    area =
                        area.vertical_scroll_offset(line as f32 * (self.settings.font_size + 3.0));
                }
                area.max_height(height)
                    .min_scrolled_height(height)
                    .show(ui, |ui| {
                        syntax::editor(
                            ui,
                            egui::Id::new(("response", tool.editor_id)),
                            &mut tool.response,
                            false,
                            25,
                        );
                    });
            });
        if send {
            self.start_job(tab);
        }
        if cancel && let Some((_, _, cancel)) = &self.active {
            cancel.send_replace(true);
        }
    }
    pub(super) fn browser(&mut self, ui: &mut egui::Ui) {
        ui.heading("Browser");
        ui.horizontal(|ui| {
            ui.label("Executable");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.browser)
                    .hint_text("Auto-detect Chromium / Chrome, or enter its full executable path")
                    .char_limit(2048)
                    .desired_width((ui.available_width() - 110.0).max(240.0)),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Start URL");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.browser_url)
                    .char_limit(2048)
                    .desired_width((ui.available_width() - 110.0).max(240.0)),
            );
        });
        ui.weak("Use Open browser in the application header to start the isolated browser.");
        ui.label(&self.browser_status);
        ui.label(format!("Proxy: http://{}", self.listen));
        ui.label(if self.trust_ready {
            "HTTPS certificate setup is ready."
        } else {
            "HTTPS certificate setup is pending or failed; see Certificates."
        });
        ui.collapsing("Browser details", |ui| {
            ui.label("Uses an isolated temporary profile; your normal profile is not changed.");
            ui.label("Certificate verification and the browser sandbox remain enabled.");
            ui.label("Install Chromium/Chrome separately, or specify its executable above.");
        });
    }
}
impl Spike {
    pub(super) fn workflow_content(&mut self, ui: &mut egui::Ui) {
        self.workflow.external_active = self.attack.running();
        if !self.features.importing && self.features.pending.len() < 16 {
            for event in self.workflow.poll() {
                self.receive(event);
            }
            while let Some(mut record) = self.workflow.completed.pop_front() {
                self.features.results_dirty = true;
                record.capture_id =
                    self.history
                        .entries
                        .iter()
                        .rev()
                        .find_map(|entry| match &entry.event {
                            Event::Exchange(e)
                                if e.run.as_ref().is_some_and(|r| {
                                    r.run_id == record.run_id && r.index == record.index
                                }) =>
                            {
                                Some(entry.id)
                            }
                            _ => None,
                        });
                self.project_command(crate::project::Command::Result(record));
            }
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("HTTPied").size(17.0).strong());
            muted(ui, &self.status);
            if let Some((_,tab,_)) = &self.workflow.active {
                let tab=*tab;
                if ui.link("Execution running →").clicked(){self.workflow.tab=tab;}
            }
            if !self.workflow.held.is_empty() { ui.colored_label(WARN,format!("{} held",self.workflow.held.len())); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let browser_running=self.workflow.browser_status.starts_with("Browser started")||self.workflow.browser_status.starts_with("Browser already running");
                if browser_running {
                    if ui.button("Stop browser").clicked()&&let Some(tx)=&self.workflow.browser_tx{let _=tx.try_send(browser::Action::Stop);}
                } else if ui
                    .add_enabled(
                        self.workflow.trust_ready && self.workflow.listener_ready,
                        egui::Button::new("Open browser").fill(egui::Color32::from_rgb(46,99,172)),
                    )
                    .on_disabled_hover_text("Waiting for the proxy listener and HTTPS certificate setup. See Settings / Certificates.")
                    .clicked()
                {
                    self.workflow.launch_browser();
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (
                    Tab::Intercept,
                    if self.workflow.held.is_empty() {
                        "Intercept".into()
                    } else {
                        format!("Intercept ({})", self.workflow.held.len())
                    },
                ),
                (
                    Tab::History,
                    format!("History  {}", self.history.entries.len()),
                ),
                (Tab::Replay, "Replay".into()),
                (Tab::Intruder, "Intruder".into()),
                (Tab::Attack, "Attack".into()),
                (Tab::Compare, "Compare".into()),
                (Tab::Coder, "Coder".into()),
                (Tab::Settings, "Settings".into()),
            ] {
                if ui
                    .add_sized(
                        [90.0, 30.0],
                        egui::Button::new(label).selected(self.workflow.tab == tab),
                    )
                    .clicked()
                {
                    self.workflow.tab = tab;
                }
            }
        });
        ui.separator();
        match self.workflow.tab {
            Tab::Intercept => self.workflow.intercept(ui),
            Tab::History => self.workspace(ui),
            Tab::Attack => self.attack_page(ui),
            Tab::Replay => {
                self.workflow.tool(ui, Tab::Replay);
                self.features.state.settings.request_ratio = self.workflow.settings.request_ratio;
            }
            Tab::Intruder => {
                let height = ui.available_height();
                egui::Panel::bottom("intruder-result-panel")
                    .resizable(true)
                    .default_size(240.0)
                    .min_size(if self.features.results_total > 0 {
                        200.0
                    } else {
                        120.0
                    })
                    .max_size((height * 0.55).max(150.0))
                    .show(ui, |ui| self.results_view(ui));
                if self.features.result_inspection.is_some() {
                    self.result_inspector(ui);
                } else {
                    self.workflow.tool(ui, Tab::Intruder);
                }
                self.features.state.settings.request_ratio = self.workflow.settings.request_ratio;
            }
            _ => self.feature_tab(ui),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_transfers_preserve_destinations_variables_and_payload_templates() {
        let mut workflow = Workflow::offline("127.0.0.1:8080".parse().unwrap());
        let destination = "https://{{host}}:8443";
        let request = "POST /{{payload}} HTTP/1.1\r\nX-Test: one\r\nX-Test: two\r\n\r\n{{token}}\n";
        workflow.load_draft(destination.into(), request.into(), Tab::Intruder);
        assert_eq!(workflow.intruder.request, request);
        workflow.load_draft(
            workflow.intruder.destination.clone(),
            workflow.intruder.request.clone(),
            Tab::Replay,
        );
        let selected = &workflow.replays[workflow.selected_replay];
        assert_eq!(selected.destination, destination);
        assert_eq!(selected.request, request);
        assert_eq!(workflow.intruder.request, request);
    }
    #[test]
    fn failed_replay_cannot_redisplay_a_previous_response_when_switching_views() {
        let mut workflow = Workflow::offline("127.0.0.1:8080".parse().unwrap());
        let (tx, rx) = mpsc::channel(1);
        workflow.results = rx;
        let (cancel, _) = watch::channel(false);
        workflow.active = Some((1, Tab::Replay, cancel));
        workflow.replays[0].response_bytes = b"HTTP/1.1 200 OK\r\n\r\nOLD RESULT".to_vec();
        workflow.replays[0].last_target = "http://old/".into();
        tx.try_send(ResultEvent {
            id: 1,
            index: 0,
            elapsed: Duration::ZERO,
            result: Err("Connection refused".into()),
            done: true,
            total: 1,
            payloads: vec![],
            terminal: false,
        })
        .unwrap();
        let events = workflow.poll();
        assert!(matches!(&events[0], Event::Error(error) if error == "Connection refused"));
        assert!(workflow.active.is_none());
        for mode in 0..=4 {
            workflow.replays[0].response_mode = mode;
            assert!(workflow.replays[0].response_view().is_empty());
        }
        assert!(workflow.replays[0].response_bytes.is_empty());
        assert!(workflow.replays[0].last_target.is_empty());
    }
    #[test]
    fn redacted_curl_masks_secret_variables_in_custom_headers() {
        let mut tool = Tool::new();
        tool.destination = "https://local".into();
        tool.request =
            "GET /?token=query-secret HTTP/1.1\r\nX-Custom: {{credential}}\r\n\r\n".into();
        let variables = [crate::state::Variable {
            key: "credential".into(),
            value: "header-secret".into(),
            secret: true,
            enabled: true,
        }];
        let redacted = tool.curl(&variables, true).unwrap();
        assert!(!redacted.contains("query-secret"));
        assert!(!redacted.contains("header-secret"));
        assert!(redacted.contains("X-Custom: [REDACTED]"));
        let complete = tool.curl(&variables, false).unwrap();
        assert!(complete.contains("query-secret") && complete.contains("header-secret"));
    }
    #[test]
    fn replay_starts_with_readable_gzip_while_raw_retains_encoded_bytes() {
        use std::io::Write;
        let mut tool = Tool::new();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"window.fixture = 'readable';").unwrap();
        tool.response_bytes = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n".to_vec();
        tool.response_bytes.extend(encoder.finish().unwrap());
        assert!(
            tool.response_view()
                .contains("window.fixture = 'readable';")
        );
        tool.response_mode = 0;
        assert_eq!(tool.response_view(), preview(&tool.response_bytes, PREVIEW));
        tool.response_mode = 1;
        assert!(
            tool.response_view()
                .contains("window.fixture = 'readable';")
        );
    }
    #[test]
    fn an_in_flight_wordlist_cannot_dispatch_a_stale_source() {
        let mut workflow = Workflow::offline("127.0.0.1:8080".parse().unwrap());
        workflow.payload_mode = PayloadMode::File;
        workflow.file_busy = true;
        workflow.file_values = Some(std::sync::Arc::new(vec!["old".into()]));
        workflow.start_job(Tab::Intruder);
        assert!(workflow.active.is_none());
        assert!(workflow.message.contains("finish loading"));
    }
    #[test]
    fn history_transfer_creates_bounded_independent_replay_sessions() {
        let mut workflow = Workflow::offline("127.0.0.1:8080".parse().unwrap());
        for index in 0..63 {
            workflow.load(
                format!("GET http://local/{index} HTTP/1.1\r\nHost: local\r\n\r\n").as_bytes(),
                Tab::Replay,
            );
        }
        assert_eq!(workflow.replays.len(), crate::state::MAX_REPLAYS);
        assert!(workflow.replays[1].request.starts_with("GET /0"));
        assert!(workflow.replays[63].request.starts_with("GET /62"));
        workflow.load(
            b"GET http://local/overflow HTTP/1.1\r\nHost: local\r\n\r\n",
            Tab::Replay,
        );
        assert_eq!(workflow.replays.len(), crate::state::MAX_REPLAYS);
        assert!(workflow.message.contains("64 requests"));
    }
}

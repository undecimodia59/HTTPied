use super::*;
use crate::{
    project::{Command, Reply, Service},
    state::{Preset, ProjectState, ScopeRule, Variable},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum SettingsSection {
    General,
    Display,
    Execution,
    Scope,
    Variables,
    Interception,
    Shortcuts,
    Browser,
    Project,
    Certificates,
}
pub(super) struct Features {
    pub settings_section: SettingsSection,
    pub(super) result_inspection: Option<crate::jobs::Record>,
    pub(super) result_loading: bool,
    pub(super) result_error: String,
    pub trust_pending: bool,
    pub trust_status: Option<Result<crate::trust::Status, String>>,
    history_dialog: Option<bool>,
    results_fingerprint: String,
    pub results_dirty: bool,
    results_updated: Instant,
    results_errors: bool,
    results_busy: bool,
    results_export: bool,
    results_matched: bool,
    results_status: String,
    results_latest: bool,
    pub state: ProjectState,
    pub open: bool,
    pub policy: Option<watch::Sender<std::sync::Arc<crate::policy::Policy>>>,
    service: Service,
    pub pending: VecDeque<Command>,
    pub startup: Option<
        tokio::sync::oneshot::Sender<(
            crate::state::Settings,
            std::sync::Arc<crate::authority::Authority>,
        )>,
    >,
    pub(super) path: String,
    name: String,
    temporary: bool,
    busy: bool,
    pub importing: bool,
    pub(super) message: String,
    pub total: u64,
    offset: usize,
    matches: u64,
    preset_name: String,
    saved: String,
    changed: Option<Instant>,
    last_snapshot: Instant,
    filter_snapshot: String,
    filter_changed: Option<Instant>,
    transfer_path: String,
    transfer_format: String,
    import_text: String,
    purge: bool,
    pub selection: std::collections::BTreeSet<u64>,
    pub(super) pending_transfer: Option<(u64, Tab)>,
    pub export_subset: u8,
    redact: bool,
    pub(super) compare_left: String,
    pub(super) compare_right: String,
    compare_diff: String,
    search: String,
    search_next: usize,
    pub jump: Option<usize>,
    pub pretty: bool,
    pub decoded: bool,
    pub auto_decode: bool,
    pub parameters: bool,
    pub(super) ignore_headers: String,
    pub(super) compare_hex: bool,
    results: Vec<crate::jobs::Record>,
    pub(super) results_total: u64,
    results_offset: usize,
    results_run: u64,
    results_query: String,
    results_sort: usize,
    results_reverse: bool,
}
impl Features {
    pub fn new() -> Self {
        Self {
            settings_section: SettingsSection::General,
            result_inspection: None,
            result_loading: false,
            result_error: String::new(),
            trust_pending: false,
            trust_status: None,
            history_dialog: None,
            results_fingerprint: String::new(),
            results_dirty: true,
            results_updated: Instant::now(),
            results_errors: false,
            results_busy: false,
            results_export: false,
            results_matched: false,
            results_status: String::new(),
            results_latest: true,
            state: ProjectState::default(),
            open: false,
            policy: None,
            service: Service::start(),
            pending: VecDeque::new(),
            startup: None,
            path: String::new(),
            name: "Untitled".into(),
            temporary: false,
            busy: false,
            importing: false,
            message: String::new(),
            total: 0,
            offset: 0,
            matches: 0,
            preset_name: String::new(),
            saved: String::new(),
            changed: None,
            last_snapshot: Instant::now(),
            filter_snapshot: String::new(),
            filter_changed: None,
            transfer_path: String::new(),
            transfer_format: "jsonl".into(),
            import_text: String::new(),
            purge: false,
            selection: Default::default(),
            pending_transfer: None,
            export_subset: 0,
            redact: true,
            compare_left: String::new(),
            compare_right: String::new(),
            compare_diff: String::new(),
            search: String::new(),
            search_next: 0,
            jump: None,
            pretty: false,
            decoded: false,
            auto_decode: true,
            parameters: false,
            ignore_headers: "Date, X-Request-Id".into(),
            compare_hex: false,
            results: vec![],
            results_total: 0,
            results_offset: 0,
            results_run: 0,
            results_query: String::new(),
            results_sort: 0,
            results_reverse: false,
        }
    }
}
impl Spike {
    pub(super) fn transfer_history(&mut self, id: u64, tab: Tab) {
        let Some(entry) = self.history.entries.iter().find(|entry| entry.id == id) else {
            return;
        };
        if entry.loaded {
            if let Event::Exchange(execution) = &entry.event {
                if tab == Tab::Attack {
                    let transfer = crate::replay::executed_request(execution)
                        .and_then(|wire| crate::replay::origin_draft(&wire))
                        .map_err(|e| e.to_string())
                        .and_then(|(destination, request)| {
                            self.attack.load(vec![crate::state::SavedRequest {
                                name: "Request".into(),
                                group: String::new(),
                                timeout_secs: 0,
                                destination,
                                request,
                            }])
                        });
                    match transfer {
                        Ok(()) => self.workflow.tab = Tab::Attack,
                        Err(error) => self.workflow.message = error,
                    }
                } else {
                    self.workflow
                        .load_result(crate::replay::executed_request(execution), tab);
                }
                if tab == Tab::Intruder && self.workflow.message.is_empty() {
                    self.features.result_inspection = None;
                }
            }
        } else {
            self.features.pending_transfer = Some((id, tab));
            self.project_command(Command::Detail(id));
            self.toast = Some(("Loading saved request…".into(), Instant::now()));
        }
    }
    fn finish_history_transfer(&mut self, id: u64, event: &Event) {
        if let Some((pending, tab)) = self.features.pending_transfer
            && pending == id
        {
            self.features.pending_transfer = None;
            if let Event::Exchange(execution) = event {
                if tab == Tab::Attack {
                    let transfer = crate::replay::executed_request(execution)
                        .and_then(|wire| crate::replay::origin_draft(&wire))
                        .map_err(|e| e.to_string())
                        .and_then(|(destination, request)| {
                            self.attack.load(vec![crate::state::SavedRequest {
                                name: "Request".into(),
                                group: String::new(),
                                timeout_secs: 0,
                                destination,
                                request,
                            }])
                        });
                    match transfer {
                        Ok(()) => self.workflow.tab = Tab::Attack,
                        Err(error) => self.workflow.message = error,
                    }
                } else {
                    self.workflow
                        .load_result(crate::replay::executed_request(execution), tab);
                }
                if tab == Tab::Intruder && self.workflow.message.is_empty() {
                    self.features.result_inspection = None;
                }
                self.toast = None;
            }
        }
    }
    pub(super) fn project_command(&mut self, command: Command) {
        // Calls are limited per frame; capture intake stops while a backlog exists.
        if self.features.pending.len() < 64 {
            self.features.pending.push_back(command);
        } else {
            self.features.message = "Project queue is full; retry the operation.".into();
        }
    }
    pub(super) fn poll_project(&mut self) {
        while let Some(command) = self.features.pending.pop_front() {
            match self.features.service.commands.try_send(command) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(command)) => {
                    self.features.pending.push_front(command);
                    break;
                }
                Err(_) => {
                    self.features.message = "Project worker stopped".into();
                    break;
                }
            }
        }
        for _ in 0..16 {
            let Ok(reply) = self.features.service.replies.try_recv() else {
                break;
            };
            match reply {
                Reply::CampaignComparison { original, modified } => {
                    if let (Event::Exchange(original), Event::Exchange(modified)) =
                        (*original, *modified)
                    {
                        self.features.compare_left =
                            crate::exchange::auto_preview(&original.response).text;
                        self.features.compare_right =
                            crate::exchange::auto_preview(&modified.response).text;
                        self.compare_view.editing = false;
                        self.workflow.tab = Tab::Compare;
                    }
                }
                Reply::CampaignList(runs) => self.attack.runs = runs,
                Reply::CampaignLoad {
                    run,
                    rows,
                    total,
                    offset,
                } => self.attack.loaded(*run, rows, total, offset),
                Reply::Opened {
                    path,
                    temporary,
                    ca,
                    state,
                    max_id,
                    total,
                    rows,
                    logs,
                } => {
                    self.features.path = path.display().to_string();
                    self.features.temporary = temporary;
                    self.features.state = *state;
                    self.filter = self.features.state.filter.text.clone();
                    self.sort_column = match self.features.state.filter.sort_by.as_str() {
                        "Method" => SortColumn::Method,
                        "Status" => SortColumn::Status,
                        "Destination" => SortColumn::Destination,
                        "Host" => SortColumn::Host,
                        "Endpoint" => SortColumn::Endpoint,
                        "Size" => SortColumn::Size,
                        "Duration" => SortColumn::Duration,
                        "TTFB" => SortColumn::Ttfb,
                        "Held" => SortColumn::Held,
                        "MIME" => SortColumn::Mime,
                        "Protocol" => SortColumn::Protocol,
                        "Stream ID" => SortColumn::Stream,
                        _ => SortColumn::Id,
                    };
                    self.sort_descending = self.features.state.filter.descending;
                    self.features.total = total;
                    self.features.matches = total;
                    self.features.open = true;
                    let pending_logs = self.logs.restore(logs);
                    if !pending_logs.is_empty() {
                        self.project_command(Command::WriteLogs(pending_logs));
                    }
                    self.features.busy = false;
                    self.history.next_id = max_id + 1;
                    self.history.byte_limit = self.features.state.settings.retain_mib * 1024 * 1024;
                    self.history.row_limit = self.features.state.settings.retain_rows;
                    self.replace_page(rows);
                    self.workflow.restore(&self.features.state);
                    self.coder.restore(&self.features.state.coder_workspace);
                    self.attack.workspace = self.features.state.attack_workspace.clone();
                    self.project_command(Command::CampaignList);
                    self.install_trust();
                    if let Some(policy) = &self.features.policy {
                        policy.send_replace(std::sync::Arc::new(crate::policy::Policy::from(
                            &self.features.state,
                        )));
                    }
                    if let Some(startup) = self.features.startup.take() {
                        let _ = startup.send((self.features.state.settings.clone(), ca));
                    }
                    self.features.saved =
                        serde_json::to_string(&self.features.state).unwrap_or_default();
                }
                Reply::Page {
                    offset,
                    total,
                    rows,
                } => {
                    self.features.offset = offset;
                    self.features.matches = total;
                    self.features.busy = false;
                    self.replace_page(rows);
                }
                Reply::Detail { id, event } => {
                    let event = *event;
                    self.finish_history_transfer(id, &event);
                    let inspected_result =
                        self.features
                            .result_inspection
                            .as_ref()
                            .is_some_and(|record| {
                                record.capture_id == Some(id)
                                    || matches!(&event, Event::Exchange(execution)
                            if execution.run.as_ref().is_some_and(|run|
                                run.run_id == record.run_id && run.index == record.index))
                            });
                    if inspected_result {
                        self.selected = Some(id);
                        if let Some(record) = &mut self.features.result_inspection {
                            record.capture_id = Some(id);
                        }
                        self.features.result_loading = false;
                    }
                    if self.selected != Some(id) {
                        continue;
                    }
                    for entry in &mut self.history.entries {
                        if entry.id == id || !entry.loaded {
                            continue;
                        }
                        if let Event::Exchange(e) = &mut entry.event {
                            e.original.0.clear();
                            e.original.0.shrink_to_fit();
                            e.outgoing.clear();
                            e.outgoing.shrink_to_fit();
                            e.response.clear();
                            e.response.shrink_to_fit();
                            e.response_outgoing.clear();
                            e.response_outgoing.shrink_to_fit();
                        }
                        if let Event::WebSocket { payload, .. } = &mut entry.event {
                            payload.clear();
                            payload.shrink_to_fit();
                        }
                        entry.loaded = false;
                        let size = event_size(&entry.event);
                        self.history.bytes = self.history.bytes.saturating_sub(entry.size) + size;
                        entry.size = size;
                    }
                    let size = event_size(&event);
                    if size <= self.history.byte_limit {
                        if !self.history.entries.iter().any(|e| e.id == id) {
                            while self.history.bytes + size > self.history.byte_limit
                                || self.history.entries.len() >= self.history.row_limit
                            {
                                if let Some(entry) = self.history.entries.pop_front() {
                                    self.history.bytes =
                                        self.history.bytes.saturating_sub(entry.size);
                                } else {
                                    break;
                                }
                            }
                            self.history.entries.push_back(Entry {
                                id,
                                event: event.clone(),
                                size: 0,
                                loaded: false,
                            });
                        }
                        // Hydrate only selected captures.
                        if let Some(entry) = self.history.entries.iter_mut().find(|e| e.id == id) {
                            self.history.bytes =
                                self.history.bytes.saturating_sub(entry.size) + size;
                            entry.event = event;
                            entry.size = size;
                            entry.loaded = true;
                        }
                        if self.selected == Some(id) {
                            self.select(id);
                        }
                    } else {
                        self.features.message = "Capture exceeds the memory limit".into();
                    }
                }
                Reply::Stored { total } => self.features.total = total,
                Reply::Imported {
                    count,
                    max_id,
                    total,
                } => {
                    self.features.importing = false;
                    self.history.next_id = max_id + 1;
                    self.features.total = total;
                    self.add_log(
                        crate::activity::Level::Info,
                        &format!("Imported {count} captures"),
                    );
                    self.request_page(0);
                }
                Reply::Trust(result) => {
                    tracing::info!(outcome=?result,"project CA trust setup completed");
                    self.features.trust_pending = false;
                    self.workflow.trust_ready = result.is_ok();
                    if let Err(error) = &result {
                        self.features.message = error.clone();
                    } else {
                        self.features.message.clear();
                    }
                    self.features.trust_status = Some(result);
                }
                Reply::Saved => {}
                Reply::LogWrite(Ok(())) => {}
                Reply::LogWrite(Err(error)) => {
                    self.logs.push(crate::activity::Level::Warning, &error);
                }
                Reply::StateSaved(requests) => self.workflow.mark_saved(&requests),
                Reply::Results {
                    rows,
                    total,
                    run_id,
                } => {
                    self.features.results = rows;
                    self.features.results_run = run_id;
                    self.features.results_total = total;
                    self.features.results_busy = false;
                }
                Reply::Message(message) => {
                    self.add_log(crate::activity::Level::Info, &message);
                    self.features.busy = false;
                }
                Reply::Error(error) => {
                    if self.features.result_loading {
                        self.features.result_loading = false;
                        self.features.result_error = error.clone();
                        self.selected = None;
                    }
                    self.features.pending_transfer = None;
                    self.features.results_busy = false;
                    self.features.importing = false;
                    self.features.message = error;
                    self.features.busy = false;
                }
            }
        }
        if self.features.open {
            if self.features.state.settings.validate().is_ok() {
                self.history.byte_limit = self.features.state.settings.retain_mib * 1024 * 1024;
                self.history.row_limit = self.features.state.settings.retain_rows;
                while self.history.bytes > self.history.byte_limit
                    || self.history.entries.len() > self.history.row_limit.max(128)
                {
                    if let Some(entry) = self.history.entries.pop_front() {
                        self.history.bytes = self.history.bytes.saturating_sub(entry.size);
                        self.history.evicted += 1;
                        if self.selected == Some(entry.id) {
                            self.selected = None;
                            self.text.clear();
                            self.draft.clear();
                        }
                    } else {
                        break;
                    }
                }
            }
            self.workflow.variables = self.features.state.variables.clone();
            self.workflow.scope = self.features.state.scope.clone();
            self.workflow.settings = self.features.state.settings.clone();
            if self.features.last_snapshot.elapsed() < Duration::from_millis(300) {
                return;
            }
            self.features.last_snapshot = Instant::now();
            // Replay recovery is debounced, including edits made outside Settings.
            self.features.state.replays = self.workflow.saved_requests();
            self.workflow.save_group_state(&mut self.features.state);
            self.features.state.attack_workspace = self.attack.workspace.clone();
            self.features.state.coder_workspace = self.coder.saved_workspace();
            let snapshot = serde_json::to_string(&self.features.state).unwrap_or_default();
            if snapshot != self.features.saved {
                if let Some(policy) = &self.features.policy {
                    policy.send_replace(std::sync::Arc::new(crate::policy::Policy::from(
                        &self.features.state,
                    )));
                }
                self.features.saved = snapshot;
                self.features.changed = Some(Instant::now());
            }
            if self
                .features
                .changed
                .is_some_and(|t| t.elapsed() > Duration::from_millis(700))
                && self.features.pending.len() < 8
            {
                self.features.changed = None;
                if self.features.state.settings.validate().is_ok() {
                    self.project_command(Command::Save(Box::new(self.features.state.clone())));
                }
            }
        }
    }
    fn replace_page(&mut self, mut rows: Vec<(u64, Event)>) {
        let selected = self.selected;
        let mut retained = selected
            .and_then(|id| self.history.entries.iter().position(|e| e.id == id))
            .and_then(|index| self.history.entries.remove(index));
        self.history.entries.clear();
        self.history.bytes = 0;
        self.selected = None;
        rows.sort_by_key(|(id, _)| *id);
        for (id, event) in rows {
            let entry = if retained.as_ref().is_some_and(|e| e.id == id) {
                retained.take().unwrap()
            } else {
                Entry {
                    id,
                    size: event_size(&event),
                    event,
                    loaded: false,
                }
            };
            self.history.bytes += entry.size;
            self.history.entries.push_back(entry);
        }
        if let Some(id) = selected
            && self.history.entries.iter().any(|e| e.id == id)
        {
            self.selected = Some(id);
            self.refresh();
        } else {
            self.text.clear();
            self.draft.clear();
        }
    }
    fn request_page(&mut self, offset: usize) {
        if !self.features.busy {
            self.features.busy = true;
            self.project_command(Command::Page {
                offset,
                filter: self.features.state.filter.clone(),
                scope: self.features.state.scope.clone(),
            });
        }
    }
    pub(super) fn start_screen(&mut self, ui: &mut egui::Ui) {
        ui.add_space(70.0);
        ui.vertical_centered(|ui| {
            ui.heading("HTTPied");
            ui.label("Open a project or start a new session.");
            ui.add_space(24.0);
        });
        egui::Grid::new("project-start")
            .min_col_width(150.0)
            .show(ui, |ui| {
                ui.label("Project name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.features.name)
                        .char_limit(128)
                        .min_size(egui::vec2(450.0, 20.0))
                        .desired_width(450.0),
                );
                ui.end_row();
                ui.label("Project directory");
                ui.add(
                    egui::TextEdit::singleline(&mut self.features.path)
                        .char_limit(4096)
                        .hint_text("Existing project folder, or a new folder to create")
                        .min_size(egui::vec2(450.0, 20.0))
                        .desired_width(450.0),
                );
                ui.end_row();
            });
        ui.add_space(16.0);
        ui.add_enabled_ui(!self.features.busy, |ui| {
            ui.horizontal(|ui| {
                for (label, create, temporary) in [
                    ("Open project", false, false),
                    ("Create project", true, false),
                    ("Temporary session", true, true),
                ] {
                    if ui.button(label).clicked() {
                        self.features.busy = true;
                        self.features.message.clear();
                        self.project_command(Command::Open {
                            path: PathBuf::from(&self.features.path),
                            create,
                            temporary,
                            name: self.features.name.clone(),
                        });
                    }
                }
            });
        });
        ui.label("Saved projects keep traffic, variables and Replay drafts. Temporary sessions are deleted on exit.");
        if self.features.busy {
            ui.spinner();
        }
        ui.colored_label(ERROR, &self.features.message);
    }
    pub(super) fn history_filters(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Search method, URL, status or MIME…")
                    .desired_width((ui.available_width() - 470.0).max(210.0))
                    .char_limit(512),
            );
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::F)) {
                search.request_focus();
            }
            if search.has_focus()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.filter.clear();
                search.surrender_focus();
            }
            ui.checkbox(&mut self.features.state.filter.in_scope, "In scope");
            let advanced = &self.features.state.filter;
            let count = [
                &advanced.method,
                &advanced.status,
                &advanced.mime,
                &advanced.extensions,
                &advanced.excluded_extensions,
            ]
            .iter()
            .filter(|value| !value.is_empty())
            .count()
                + usize::from(advanced.errors_only)
                + usize::from(advanced.min_ms > 0.0)
                + usize::from(advanced.min_bytes > 0)
                + usize::from(!advanced.show_ws);
            if ui
                .button(if count == 0 {
                    "Filters".into()
                } else {
                    format!("Filters ({count})")
                })
                .clicked()
            {
                self.features.history_dialog = Some(false);
            }
            ui.menu_button("View", |ui| {
                if ui.button("Choose columns…").clicked() {
                    self.features.history_dialog = Some(true);
                    ui.close();
                }
                if ui.button("Reload saved traffic").clicked() {
                    self.request_page(0);
                    ui.close();
                }
                if ui.button("Clear live view…").clicked() {
                    self.confirm_clear = !self.history.entries.is_empty();
                    ui.close();
                }
            });
            if self.features.offset > 0 || self.features.matches > 128 {
                if ui
                    .add_enabled(
                        self.features.offset > 0 && !self.features.busy,
                        egui::Button::new("Previous"),
                    )
                    .clicked()
                {
                    self.request_page(self.features.offset.saturating_sub(128));
                }
                if ui
                    .add_enabled(
                        self.features.offset as u64 + 128 < self.features.matches
                            && !self.features.busy,
                        egui::Button::new("Next"),
                    )
                    .clicked()
                {
                    self.request_page(self.features.offset + 128);
                }
            }
            if self.features.busy {
                ui.spinner();
            }
            if self.features.matches > 0 && (self.features.offset as u64) < self.features.matches {
                ui.weak(format!(
                    "{}–{} / {}",
                    self.features.offset + 1,
                    (self.features.offset as u64 + 128).min(self.features.matches),
                    self.features.matches
                ));
            } else {
                ui.weak(format!("{} matching", self.features.matches));
            }
        });
        self.history_options(ui.ctx());
        self.features.state.filter.text = self.filter.clone();
        let snapshot = if self.features.state.filter.in_scope {
            serde_json::to_string(&(&self.features.state.filter, &self.features.state.scope))
        } else {
            serde_json::to_string(&self.features.state.filter)
        }
        .unwrap_or_default();
        if snapshot != self.features.filter_snapshot {
            self.features.filter_snapshot = snapshot;
            self.features.filter_changed = Some(Instant::now());
        }
        if self
            .features
            .filter_changed
            .is_some_and(|t| t.elapsed() > Duration::from_millis(350))
            && !self.features.busy
        {
            self.features.filter_changed = None;
            self.request_page(0);
        }
    }
    fn history_options(&mut self, ctx: &egui::Context) {
        let Some(columns) = self.features.history_dialog else {
            return;
        };
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.features.history_dialog = None;
            return;
        }
        let mut open = true;
        egui::Window::new(if columns {
            "History columns"
        } else {
            "History filters"
        })
        .id(egui::Id::new("history-options"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(490.0)
        .default_pos(egui::pos2(
            (ctx.content_rect().center().x - 245.0).max(8.0),
            120.0,
        ))
        .show(ctx, |ui| {
            if columns {
                ui.label("Choose visible columns and their order.");
                ui.weak("Drag a header edge to resize. Double-click its edge to reset.");
                if ui.button("Reset all widths").clicked() {
                    self.features.state.column_widths.clear();
                }
                let mut movement = None;
                egui::Grid::new("column-order")
                    .min_col_width(70.0)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        let enabled = self.features.state.columns.clone();
                        for (index, name) in enabled.iter().enumerate() {
                            let mut visible = true;
                            if ui.checkbox(&mut visible, name).changed() {
                                self.features.state.columns.retain(|c| c != name);
                            }
                            if ui.add_enabled(index > 0, egui::Button::new("Up")).clicked() {
                                movement = Some((index, -1));
                            }
                            if ui
                                .add_enabled(index + 1 < enabled.len(), egui::Button::new("Down"))
                                .clicked()
                            {
                                movement = Some((index, 1));
                            }
                            ui.end_row();
                        }
                        for name in [
                            "#",
                            "Method",
                            "Status",
                            "Protocol",
                            "Stream ID",
                            "Destination",
                            "Host",
                            "Endpoint",
                            "Duration",
                            "TTFB",
                            "Held",
                            "Size",
                            "MIME",
                        ] {
                            if !enabled.iter().any(|c| c == name) {
                                let mut visible = false;
                                if ui.checkbox(&mut visible, name).changed() {
                                    self.features.state.columns.push(name.into());
                                }
                                ui.end_row();
                            }
                        }
                    });
                if let Some((index, direction)) = movement {
                    let next = index.saturating_add_signed(direction);
                    if next < self.features.state.columns.len() {
                        self.features.state.columns.swap(index, next);
                    }
                }
                if self.features.state.columns.is_empty() {
                    self.features.state.columns.push("Destination".into());
                }
            } else {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.features.state.filter.show_tunnels, "Show CONNECT");
                    ui.checkbox(&mut self.features.state.filter.show_ws, "Show WebSockets");
                    ui.checkbox(
                        &mut self.features.state.filter.errors_only,
                        "HTTP 4xx / 5xx",
                    );
                });
                let mut chosen = None;
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("filter-presets")
                        .selected_text("Load preset")
                        .show_ui(ui, |ui| {
                            for preset in &self.features.state.presets {
                                if ui.selectable_label(false, &preset.name).clicked() {
                                    chosen = Some(preset.filter.clone());
                                    ui.close();
                                }
                            }
                        });
                    if ui.button("Reset filters").clicked() {
                        chosen = Some(Default::default());
                    }
                });
                if let Some(filter) = chosen {
                    self.filter = filter.text.clone();
                    self.features.state.filter = filter;
                }
                let filter = &mut self.features.state.filter;
                egui::Grid::new("history-filters")
                    .spacing([18.0, 10.0])
                    .show(ui, |ui| {
                        for (label, value, hint) in [
                            ("Include extensions", &mut filter.extensions, "css, js, png"),
                            (
                                "Exclude extensions",
                                &mut filter.excluded_extensions,
                                "woff, ico, map",
                            ),
                            ("MIME contains", &mut filter.mime, "application/json"),
                            ("Method", &mut filter.method, "GET (one method)"),
                            ("Status prefix", &mut filter.status, "200, 4 or 5"),
                        ] {
                            ui.label(label);
                            ui.add(
                                egui::TextEdit::singleline(value)
                                    .min_size(egui::vec2(280.0, 24.0))
                                    .hint_text(hint)
                                    .char_limit(512),
                            );
                            ui.end_row();
                        }
                        ui.label("Minimum duration");
                        ui.add(
                            egui::DragValue::new(&mut filter.min_ms)
                                .range(0.0..=86400000.0)
                                .suffix(" ms"),
                        );
                        ui.end_row();
                        ui.label("Minimum size");
                        ui.add(egui::DragValue::new(&mut filter.min_bytes).suffix(" B"));
                        ui.end_row();
                    });
                ui.weak("Extensions are comma-separated. Use (none) for extensionless paths.");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.features.preset_name)
                            .hint_text("Preset name")
                            .desired_width(280.0)
                            .char_limit(64),
                    );
                    if ui.button("Save preset").clicked()
                        && !self.features.preset_name.trim().is_empty()
                    {
                        let name = self.features.preset_name.trim().to_owned();
                        self.features.state.presets.retain(|p| p.name != name);
                        self.features.state.presets.push(Preset {
                            name,
                            filter: self.features.state.filter.clone(),
                        });
                    }
                });
            }
        });
        if !open {
            self.features.history_dialog = None;
        }
    }
    pub(super) fn install_trust(&mut self) {
        self.features.trust_pending = true;
        self.project_command(Command::InstallTrust {
            test_home: std::env::var_os("HTTPIED_TEST_TRUST_HOME").map(PathBuf::from),
        });
    }
    pub(super) fn feature_tab(&mut self, ui: &mut egui::Ui) {
        match self.workflow.tab {
            Tab::Settings => self.settings_page(ui),
            Tab::Compare => self.compare(ui),
            Tab::Coder => self.coder_page(ui),
            _ => {}
        }
    }
    pub(super) fn scope(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scope");
        ui.weak("Domains include subdomains by default. Exclusions take precedence.");
        let mut remove = None;
        egui::Grid::new("scope-rules")
            .striped(true)
            .spacing([14.0, 10.0])
            .show(ui, |ui| {
                for label in ["Exclude", "Domain / IP", "Subdomains", "", ""] {
                    ui.strong(label);
                }
                ui.end_row();
                for (index, rule) in self.features.state.scope.iter_mut().enumerate() {
                    ui.checkbox(&mut rule.exclude, "");
                    ui.add(
                        egui::TextEdit::singleline(&mut rule.host)
                            .min_size(egui::vec2(280.0, 24.0))
                            .hint_text("example.com or *.example.com")
                            .char_limit(253),
                    );
                    ui.checkbox(&mut rule.subdomains, "");
                    ui.menu_button("Options", |ui| {
                        egui::Grid::new(("scope-options", index))
                            .spacing([14.0, 10.0])
                            .show(ui, |ui| {
                                for (label, value, hint) in [
                                    ("Scheme", &mut rule.scheme, "https (empty = both)"),
                                    ("Port", &mut rule.port, "443 (empty = any)"),
                                    ("Path prefix", &mut rule.path, "/api/ (empty = all)"),
                                ] {
                                    ui.label(label);
                                    ui.add(
                                        egui::TextEdit::singleline(value)
                                            .min_size(egui::vec2(230.0, 24.0))
                                            .hint_text(hint)
                                            .char_limit(2048),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
                    if ui.button("Remove").clicked() {
                        remove = Some(index);
                    }
                    ui.end_row();
                }
            });
        if let Some(index) = remove {
            self.features.state.scope.remove(index);
        }
        if ui.button("Add domain").clicked() && self.features.state.scope.len() < 256 {
            self.features.state.scope.push(ScopeRule::default());
        }
        ui.weak("Use *.example.com for subdomains only. Use domain names without a URL scheme.");
        ui.separator();
        ui.checkbox(
            &mut self.features.state.settings.enforce_scope,
            "Restrict Replay and Intruder to scope",
        );
        ui.weak("Use In scope only on History to filter displayed traffic.");
    }
    pub(super) fn variables(&mut self, ui: &mut egui::Ui) {
        ui.heading("Variables");
        ui.label("Use {{key}} in Replay destination, headers or body. Values expand once before sending; missing keys stop the send.");
        let mut remove = None;
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("variables-table")
                .striped(true)
                .show(ui, |ui| {
                    for label in ["Key", "Value", ""] {
                        ui.strong(label);
                    }
                    ui.end_row();
                    for (index, var) in self.features.state.variables.iter_mut().enumerate() {
                        ui.add(
                            egui::TextEdit::singleline(&mut var.key)
                                .min_size(egui::vec2(180.0, 20.0))
                                .desired_width(180.0)
                                .char_limit(128),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut var.value)
                                .password(var.secret)
                                .min_size(egui::vec2(260.0, 20.0))
                                .desired_width(320.0)
                                .char_limit(65536),
                        );
                        ui.menu_button(if var.secret { "Secret" } else { "Options" }, |ui| {
                            ui.checkbox(&mut var.secret, "Mask as secret");
                            ui.checkbox(&mut var.enabled, "Use this variable");
                            if ui.button("Remove variable").clicked() {
                                remove = Some(index);
                                ui.close();
                            }
                        });
                        ui.end_row();
                    }
                });
        });
        if let Some(index) = remove {
            self.features.state.variables.remove(index);
        }
        if ui.button("Add variable").clicked() && self.features.state.variables.len() < 256 {
            self.features.state.variables.push(Variable {
                key: String::new(),
                value: String::new(),
                secret: false,
                enabled: true,
            });
        }
        ui.label("Secret values are masked here. Project files store variables locally without encryption.");
    }
    pub(super) fn project_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(&self.features.state.name);
        ui.label(&self.features.path);
        if self.features.temporary {
            ui.colored_label(WARN, "Temporary project — removed on exit");
        }
        ui.label(format!("{} saved captures", self.features.total));
        ui.horizontal(|ui| {
            ui.label("Import / export path");
            ui.add(
                egui::TextEdit::singleline(&mut self.features.transfer_path)
                    .min_size(egui::vec2(500.0, 20.0))
                    .desired_width(500.0)
                    .char_limit(4096),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Import format");
            egui::ComboBox::from_id_salt("project-import-format")
                .selected_text(&self.features.transfer_format)
                .show_ui(ui, |ui| {
                    for format in ["jsonl", "har", "raw", "curl"] {
                        ui.selectable_value(
                            &mut self.features.transfer_format,
                            format.to_owned(),
                            format,
                        );
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Export");
            ui.selectable_value(&mut self.features.export_subset, 0, "All captures");
            ui.selectable_value(&mut self.features.export_subset, 1, "Current filter");
            ui.selectable_value(
                &mut self.features.export_subset,
                2,
                format!("Selected ({})", self.features.selection.len()),
            );
        });
        ui.checkbox(
            &mut self.features.redact,
            "Redact authentication, cookies, URL queries and bodies when exporting",
        );
        ui.horizontal(|ui| {
            ui.menu_button("Export captures", |ui| {
                for format in ["jsonl", "har"] {
                    if ui
                        .button(format!("Export {}", format.to_uppercase()))
                        .clicked()
                    {
                        self.project_command(Command::Export {
                            path: PathBuf::from(&self.features.transfer_path),
                            format: format.to_owned(),
                            redact: self.features.redact,
                            ids: if self.features.export_subset == 2 {
                                Some(self.features.selection.iter().copied().collect())
                            } else {
                                None
                            },
                            filter: if self.features.export_subset == 1 {
                                Some(self.features.state.filter.clone())
                            } else {
                                None
                            },
                            scope: self.features.state.scope.clone(),
                        });
                        ui.close();
                    }
                }
            });
            if ui
                .add_enabled(!self.features.importing, egui::Button::new("Import file"))
                .clicked()
            {
                self.features.importing = true;
                self.project_command(Command::Import {
                    path: PathBuf::from(&self.features.transfer_path),
                    format: self.features.transfer_format.clone(),
                });
            }
            ui.weak("Project changes save automatically");
        });
        ui.label("Exports: JSONL / HAR. Imports: JSONL / HAR / raw HTTP / cURL. Original HTTP/2 frames and TLS records cannot be represented in HAR.");
        ui.collapsing("Paste raw request or cURL", |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.features.import_text)
                    .code_editor()
                    .char_limit(131072)
                    .desired_rows(8),
            );
            if ui.button("Load into Replay").clicked() {
                let text = &self.features.import_text;
                let result = if text.trim_start().starts_with("curl ") {
                    crate::interchange::import_curl(text.trim_start())
                } else {
                    crate::interchange::raw(text.as_bytes())
                };
                match result {
                    Ok(Event::Exchange(e)) => self.workflow.load_execution(&e, Tab::Replay),
                    Err(error) => self.features.message = error,
                    _ => {}
                }
            }
        });
        ui.label("Export creates a new file. Existing files are never overwritten.");
        ui.separator();
        if ui
            .add_enabled(
                !self.workflow.is_running() && !self.attack.running(),
                egui::Button::new("Purge project captures and results…"),
            )
            .on_disabled_hover_text(
                "Stop the current execution before purging its saved captures and results.",
            )
            .clicked()
        {
            self.features.purge = true;
        }
        if self.features.purge {
            ui.horizontal(|ui| {
                ui.colored_label(
                    WARN,
                    "Delete all saved traffic and results from this project?",
                );
                if ui.button("Delete captures and results").clicked() {
                    self.project_command(Command::Purge);
                    self.features.purge = false;
                    self.features.total = 0;
                    self.clear();
                    self.features.results.clear();
                    self.attack.records.clear();
                    self.attack.runs.clear();
                    self.attack.total = 0;
                    self.attack.loaded_run = None;
                }
                if ui.button("Keep captures").clicked() {
                    self.features.purge = false;
                }
            });
        }

        ui.colored_label(WARN, &self.features.message);
    }
    fn refresh_comparison(&mut self) {
        self.features.compare_diff = crate::exchange::diff(
            &crate::exchange::comparison_view(
                &self.features.compare_left,
                &self.features.ignore_headers,
                self.features.compare_hex,
            ),
            &crate::exchange::comparison_view(
                &self.features.compare_right,
                &self.features.ignore_headers,
                self.features.compare_hex,
            ),
        );
    }
    fn compare(&mut self, ui: &mut egui::Ui) {
        self.compare_page(ui);
    }
    fn result_filter(&self) -> crate::jobs::ResultFilter {
        crate::jobs::ResultFilter {
            query: self.features.results_query.clone(),
            status: self.features.results_status.clone(),
            errors_only: self.features.results_errors,
            matched_only: self.features.results_matched,
            sort_by: self.features.results_sort,
            descending: self.features.results_reverse,
            latest_only: self.features.results_latest,
        }
    }
    fn query_results(&mut self) {
        if self.features.results_busy {
            return;
        }
        self.features.results_busy = true;
        self.features.results_dirty = false;
        self.features.results_updated = Instant::now();
        self.project_command(Command::Results {
            run_id: 0,
            offset: self.features.results_offset,
            filter: self.result_filter(),
        });
    }
    pub(super) fn results_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Results");
            ui.add(
                egui::TextEdit::singleline(&mut self.features.results_query)
                    .hint_text("Search payload or error")
                    .desired_width(240.0)
                    .char_limit(256),
            );
            ui.menu_button("Filter", |ui| {
                ui.checkbox(&mut self.features.results_latest, "Latest run only");
                ui.checkbox(&mut self.features.results_errors, "Errors only");
                ui.checkbox(&mut self.features.results_matched, "Matches only");
                ui.horizontal(|ui| {
                    ui.label("Status prefix");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.features.results_status)
                            .min_size(egui::vec2(160.0, 24.0))
                            .hint_text("200, 4 or 5")
                            .char_limit(3),
                    );
                });
            });
            if ui
                .add_enabled(
                    self.features.results_total > 0,
                    egui::Button::new("Export…"),
                )
                .clicked()
            {
                self.features.results_export = true;
            }
            if self.features.results_busy {
                ui.spinner();
            }
            ui.weak(format!("{} results", self.features.results_total));
            if self.features.results_total > 128 || self.features.results_offset > 0 {
                if ui
                    .add_enabled(
                        self.features.results_offset > 0,
                        egui::Button::new("Previous"),
                    )
                    .clicked()
                {
                    self.features.results_offset = self.features.results_offset.saturating_sub(128);
                    self.features.results_dirty = true;
                }
                if ui
                    .add_enabled(
                        (self.features.results_offset as u64 + 128) < self.features.results_total,
                        egui::Button::new("Next"),
                    )
                    .clicked()
                {
                    self.features.results_offset += 128;
                    self.features.results_dirty = true;
                }
            }
        });
        let selected = self
            .features
            .result_inspection
            .as_ref()
            .map(|record| (record.run_id, record.index));
        let action = super::results::table(
            ui,
            &self.features.results,
            selected,
            self.features.results_latest,
            self.features.state.settings.binary_units,
            self.features.results_sort,
            self.features.results_reverse,
        );
        if let Some(index) = action.sort {
            if self.features.results_sort == index {
                self.features.results_reverse = !self.features.results_reverse;
            } else {
                self.features.results_sort = index;
                self.features.results_reverse = false;
            }
        }
        if let Some(index) = action.selected {
            self.inspect_result(self.features.results[index].clone());
        }
        let fingerprint = serde_json::to_string(&self.result_filter()).unwrap_or_default();
        if fingerprint != self.features.results_fingerprint {
            self.features.results_fingerprint = fingerprint;
            self.features.results_offset = 0;
            self.features.results_dirty = true;
        }
        if self.features.results_dirty
            && !self.features.results_busy
            && self.features.results_updated.elapsed() > Duration::from_millis(200)
        {
            self.query_results();
        }
        if self.features.results_export {
            let mut open = true;
            egui::Window::new("Export Intruder results")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.features.transfer_path)
                            .hint_text("Full CSV file path")
                            .min_size(egui::vec2(420.0, 24.0))
                            .char_limit(4096),
                    );
                    if ui.button("Export CSV").clicked() {
                        self.project_command(Command::ExportResults {
                            path: PathBuf::from(&self.features.transfer_path),
                            run_id: if self.features.results_latest {
                                self.features.results_run
                            } else {
                                0
                            },
                        });
                        self.features.results_export = false;
                    }
                });
            if !open {
                self.features.results_export = false;
            }
        }
    }
    pub(super) fn keyboard_actions(&mut self, ui: &mut egui::Ui) {
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Num7)) {
            self.workflow.tab = Tab::Attack;
        }

        if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Num5)) {
            self.workflow.tab = Tab::Coder;
        }
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Num6)) {
            self.workflow.tab = Tab::Compare;
        }
        if self.workflow.tab == Tab::History {
            for (key, tab) in [(egui::Key::R, Tab::Replay), (egui::Key::I, Tab::Intruder)] {
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, key))
                    && let Some(id) = self.selected
                {
                    self.transfer_history(id, tab);
                }
            }
        }
        let mut action = None;
        for (name, binding) in &self.features.state.settings.shortcuts {
            if let Some(key) = shortcut(binding)
                && ui.input_mut(|input| input.consume_shortcut(&key))
            {
                action = Some(name.clone());
                break;
            }
        }
        if let Some(action) = action {
            match action.as_str() {
                "History" => self.workflow.tab = Tab::History,
                "Replay" => self.workflow.tab = Tab::Replay,
                "Intruder" => self.workflow.tab = Tab::Intruder,
                "Intercept" => self.workflow.tab = Tab::Intercept,
                "Settings" => self.workflow.tab = Tab::Settings,
                "Send" if self.workflow.tab == Tab::Replay => self.workflow.start_job(Tab::Replay),
                "Stop" => {
                    self.workflow.stop();
                    self.attack.stop();
                }
                _ => {}
            }
        }
    }
    pub(super) fn inspector_tools(&mut self, ui: &mut egui::Ui) {
        let previous = if self.hex {
            2
        } else if self.features.decoded {
            3
        } else if self.features.parameters {
            4
        } else if self.features.pretty {
            1
        } else if self.features.auto_decode {
            5
        } else {
            0
        };
        let mut mode = previous;
        ui.horizontal_wrapped(|ui| {
            ui.label("Find");
            ui.add(
                egui::TextEdit::singleline(&mut self.features.search)
                    .desired_width(180.0)
                    .char_limit(256),
            );
            if ui.button("Next match").clicked() && !self.features.search.is_empty() {
                let (id, text) = if self.pane == Pane::Draft {
                    (egui::Id::new("history-draft"), &self.draft)
                } else {
                    (
                        egui::Id::new(("history-message", self.pane as u8)),
                        &self.text,
                    )
                };
                self.features.jump = syntax::find_next(
                    ui,
                    id,
                    text,
                    &self.features.search,
                    &mut self.features.search_next,
                );
            }
            if !self.features.search.is_empty() {
                let text = if self.pane == Pane::Draft {
                    &self.draft
                } else {
                    &self.text
                };
                ui.weak(format!(
                    "{} matches",
                    text.matches(&self.features.search).count()
                ));
            }
            egui::ComboBox::from_id_salt("inspection-view")
                .selected_text(
                    [
                        "Raw",
                        "Pretty JSON / XML",
                        "Hex",
                        "Decoded",
                        "Parameters / cookies",
                        "Auto",
                    ][mode],
                )
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (5, "Auto"),
                        (0, "Raw"),
                        (1, "Pretty JSON / XML"),
                        (2, "Hex"),
                        (3, "Decoded"),
                        (4, "Parameters / cookies"),
                    ] {
                        ui.selectable_value(&mut mode, value, label);
                    }
                });
            ui.menu_button("Compare", |ui| {
                if ui.button("Set left side").clicked() {
                    self.features.compare_left = if self.pane == Pane::Draft {
                        self.draft.clone()
                    } else {
                        self.text.clone()
                    };
                    ui.close();
                }
                if ui.button("Set right side and compare").clicked() {
                    self.features.compare_right = if self.pane == Pane::Draft {
                        self.draft.clone()
                    } else {
                        self.text.clone()
                    };
                    self.refresh_comparison();
                    self.workflow.tab = Tab::Compare;
                    ui.close();
                }
            });
        });
        if mode != previous {
            self.hex = mode == 2;
            self.features.pretty = mode == 1;
            self.features.decoded = mode == 3;
            self.features.parameters = mode == 4;
            self.features.auto_decode = mode == 5;
            self.refresh();
        }
    }
}

impl Drop for Spike {
    fn drop(&mut self) {
        self.attack.stop();
        if self.features.open {
            if let Some(id) = self.attack.active_run() {
                self.features.pending.push_back(Command::CampaignFinish {
                    id,
                    status: "Interrupted by application exit".into(),
                });
            }

            self.features.state.replays = self.workflow.saved_requests();
            self.workflow.save_group_state(&mut self.features.state);
            self.features.state.attack_workspace = self.attack.workspace.clone();
            self.features.state.coder_workspace = self.coder.saved_workspace();
            if self.features.state.settings.validate().is_ok() {
                self.features
                    .pending
                    .push_back(Command::Save(Box::new(self.features.state.clone())));
            }
        }
        self.features
            .service
            .finish(self.features.pending.drain(..));
    }
}

pub(super) fn shortcut(text: &str) -> Option<egui::KeyboardShortcut> {
    if text.trim().is_empty() {
        return None;
    }
    let mut modifiers = egui::Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_lowercase().as_str() {
            "ctrl" => modifiers.ctrl = true,
            "shift" => modifiers.shift = true,
            "alt" => modifiers.alt = true,
            _ => {
                if key.is_some() {
                    return None;
                }
                key = egui::Key::from_name(part);
                key?;
            }
        }
    }
    Some(egui::KeyboardShortcut::new(modifiers, key?))
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    #[test]
    fn saved_history_actions_load_the_clicked_request_without_selecting_it_first() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        let event =
            crate::interchange::raw(b"GET http://local/saved HTTP/1.1\r\nHost: local\r\n\r\n")
                .unwrap();
        app.history.push(event.clone());
        app.history.entries[0].loaded = false;
        app.transfer_history(1, Tab::Replay);
        assert!(matches!(
            app.features.pending.back(),
            Some(Command::Detail(1))
        ));
        app.finish_history_transfer(2, &event);
        assert!(app.features.pending_transfer.is_some());
        app.finish_history_transfer(1, &event);
        assert!(app.features.pending_transfer.is_none());
        assert!(app.workflow.tab == Tab::Replay);
        let saved = app.workflow.saved_requests();
        assert_eq!(saved.last().unwrap().destination, "http://local");
        assert!(
            saved
                .last()
                .unwrap()
                .request
                .starts_with("GET /saved HTTP/1.1")
        );
        assert!(app.selected.is_none());
    }
    #[test]
    fn editor_transfer_returns_intruder_to_the_loaded_attack_template() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        app.features.result_inspection = Some(crate::jobs::Record::default());
        let context = egui::Context::default();
        context.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("request-transfer"),
                (
                    true,
                    Ok::<_, String>(syntax::RequestSource {
                        destination: "https://local:8443".into(),
                        request: "GET /{{payload}} HTTP/1.1\r\nHost: local\r\n\r\n".into(),
                    }),
                ),
            )
        });
        app.receive_coder_selection(&context);
        assert!(app.features.result_inspection.is_none());
        assert!(app.workflow.tab == Tab::Intruder);
    }
    fn frame(app: &mut Spike) {
        let context = egui::Context::default();
        let mut output = context.run_ui(Default::default(), |ui| app.history_filters(ui));
        output.textures_delta.clear();
    }
    #[test]
    fn scope_edits_invalidate_saved_history_only_when_scope_filtering_is_enabled() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        frame(&mut app);
        let off = app.features.filter_snapshot.clone();
        app.features.state.scope.push(ScopeRule {
            host: "web-security-academy.net".into(),
            ..Default::default()
        });
        frame(&mut app);
        assert_eq!(app.features.filter_snapshot, off);
        app.features.state.filter.in_scope = true;
        frame(&mut app);
        let enabled = app.features.filter_snapshot.clone();
        assert_ne!(enabled, off);
        app.features.state.scope[0].host = "example.test".into();
        frame(&mut app);
        assert_ne!(app.features.filter_snapshot, enabled);
        app.features.filter_changed = Some(Instant::now() - Duration::from_secs(1));
        frame(&mut app);
        assert!(app.features.pending.iter().any(|command|matches!(command,Command::Page{filter,scope,..} if filter.in_scope&&scope[0].host=="example.test")));
    }
}

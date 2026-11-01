use super::*;
use crate::{
    project::{Command, Reply, Service},
    state::{Preset, ProjectState, ScopeRule, Variable},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) struct Features {
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
    path: String,
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
    pub export_subset: u8,
    redact: bool,
    compare_left: String,
    compare_right: String,
    compare_diff: String,
    search: String,
    search_next: usize,
    pub jump: Option<usize>,
    pub pretty: bool,
    pub decoded: bool,
    pub parameters: bool,
    ignore_headers: String,
    compare_hex: bool,
    results: Vec<crate::jobs::Record>,
    results_total: u64,
    results_offset: usize,
    results_run: u64,
    results_query: String,
    results_sort: usize,
    results_reverse: bool,
}
impl Features {
    pub fn new() -> Self {
        Self {
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
                Reply::Opened {
                    path,
                    temporary,
                    ca,
                    state,
                    max_id,
                    total,
                    rows,
                } => {
                    self.features.path = path.display().to_string();
                    self.features.temporary = temporary;
                    self.features.state = *state;
                    self.filter = self.features.state.filter.text.clone();
                    self.sort_column = match self.features.state.filter.sort_by.as_str() {
                        "Method" => SortColumn::Method,
                        "Status" => SortColumn::Status,
                        "Destination" => SortColumn::Destination,
                        "Size" => SortColumn::Size,
                        "Duration" => SortColumn::Duration,
                        "TTFB" => SortColumn::Ttfb,
                        "Held" => SortColumn::Held,
                        "MIME" => SortColumn::Mime,
                        "Protocol" => SortColumn::Protocol,
                        _ => SortColumn::Id,
                    };
                    self.sort_descending = self.features.state.filter.descending;
                    self.features.total = total;
                    self.features.matches = total;
                    self.features.open = true;
                    self.features.busy = false;
                    self.history.next_id = max_id + 1;
                    self.history.byte_limit = self.features.state.settings.retain_mib * 1024 * 1024;
                    self.history.row_limit = self.features.state.settings.retain_rows;
                    self.replace_page(rows);
                    self.workflow.restore(&self.features.state);
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
                                label: event_label(&event),
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
                    self.features.message = format!("Imported {count} captures");
                    self.request_page(0);
                }
                Reply::Saved => {}
                Reply::StateSaved(requests) => self.workflow.mark_saved(&requests),
                Reply::Results { rows, total } => {
                    self.features.results = rows;
                    self.features.results_total = total;
                }
                Reply::Message(message) => {
                    self.features.message = message;
                    self.features.busy = false;
                }
                Reply::Error(error) => {
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
                    label: event_label(&event),
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
                        .hint_text("Full path to a project folder")
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
                    ("Temporary project", true, true),
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
        ui.label("Temporary projects are deleted when the app closes. Saved projects keep captures, scope, variables and Replay requests.");
        if self.features.busy {
            ui.spinner();
        }
        ui.colored_label(ERROR, &self.features.message);
    }
    pub(super) fn history_filters(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let mut chosen = None;
            egui::ComboBox::from_id_salt("filter-presets")
                .selected_text("Filter presets")
                .show_ui(ui, |ui| {
                    for preset in &self.features.state.presets {
                        if ui.button(&preset.name).clicked() {
                            chosen = Some(preset.filter.clone());
                            ui.close();
                        }
                    }
                });
            if let Some(filter) = chosen {
                self.filter = filter.text.clone();
                self.sort_column = match filter.sort_by.as_str() {
                    "Method" => SortColumn::Method,
                    "Status" => SortColumn::Status,
                    "Destination" => SortColumn::Destination,
                    "Size" => SortColumn::Size,
                    "Duration" => SortColumn::Duration,
                    "TTFB" => SortColumn::Ttfb,
                    "Held" => SortColumn::Held,
                    "MIME" => SortColumn::Mime,
                    "Protocol" => SortColumn::Protocol,
                    _ => SortColumn::Id,
                };
                self.sort_descending = filter.descending;
                self.features.state.filter = filter;
            }
            ui.checkbox(&mut self.features.state.filter.show_tunnels, "CONNECT");
            ui.checkbox(&mut self.features.state.filter.in_scope, "In scope only");
            if ui.button("Refresh saved history").clicked() {
                self.request_page(0);
            }
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
                    (self.features.offset as u64 + 128) < self.features.matches
                        && !self.features.busy,
                    egui::Button::new("Next"),
                )
                .clicked()
            {
                self.request_page(self.features.offset + 128);
            }
            ui.label(format!("{} saved", self.features.total));
        });
        ui.menu_button("Filters / columns", |ui| {
            let f = &mut self.features.state.filter;
            egui::Grid::new("history-filters").show(ui, |ui| {
                for (label, value) in [
                    ("Include extensions", &mut f.extensions),
                    ("Exclude extensions", &mut f.excluded_extensions),
                    ("MIME contains", &mut f.mime),
                    ("Method", &mut f.method),
                    ("Status", &mut f.status),
                ] {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(value).char_limit(512));
                    ui.end_row();
                }
                ui.label("Minimum duration (ms)");
                ui.add(egui::DragValue::new(&mut f.min_ms).range(0.0..=86400000.0));
                ui.end_row();
                ui.label("Minimum size (bytes)");
                ui.add(egui::DragValue::new(&mut f.min_bytes));
                ui.end_row();
            });
            ui.label("Extensions: css, js, png; use (none) for paths without an extension.");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.features.preset_name)
                        .hint_text("Preset name")
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
            ui.horizontal_wrapped(|ui| {
                ui.label("Columns");
                for name in [
                    "#",
                    "Method",
                    "Status",
                    "Destination",
                    "Duration",
                    "TTFB",
                    "Held",
                    "Size",
                    "MIME",
                    "Protocol",
                ] {
                    let mut enabled = self.features.state.columns.iter().any(|c| c == name);
                    if ui.checkbox(&mut enabled, name).changed() {
                        if enabled {
                            self.features.state.columns.push(name.into());
                        } else {
                            self.features.state.columns.retain(|c| c != name);
                        }
                    }
                }
            });
            if self.features.state.columns.is_empty() {
                self.features.state.columns.push("Destination".into());
            }
            let mut movement = None;
            ui.horizontal_wrapped(|ui| {
                for (index, name) in self.features.state.columns.iter().enumerate() {
                    ui.label(name);
                    if ui
                        .small_button("←")
                        .on_hover_text("Move column left")
                        .clicked()
                    {
                        movement = Some((index, -1));
                    }
                    if ui
                        .small_button("→")
                        .on_hover_text("Move column right")
                        .clicked()
                    {
                        movement = Some((index, 1));
                    }
                    ui.separator();
                }
            });
            if let Some((index, direction)) = movement {
                let next = index
                    .saturating_add_signed(direction)
                    .min(self.features.state.columns.len() - 1);
                self.features.state.columns.swap(index, next);
            }
        });
        self.features.state.filter.text = self.filter.clone();
        let snapshot = serde_json::to_string(&self.features.state.filter).unwrap_or_default();
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
    pub(super) fn feature_tab(&mut self, ui: &mut egui::Ui) {
        match self.workflow.tab {
            Tab::Target => self.target(ui),
            Tab::Settings => self.settings(ui),
            Tab::Variables => self.variables(ui),
            Tab::Compare => self.compare(ui),
            Tab::Project => self.project_view(ui),
            Tab::Results => self.results_view(ui),
            _ => {}
        }
    }
    fn target(&mut self, ui: &mut egui::Ui) {
        ui.heading("Target scope");
        ui.label("Include domains you want to see. Exclusions take precedence. Empty scope matches no domains.");
        let mut remove = None;
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("target-rules")
                .striped(true)
                .show(ui, |ui| {
                    for label in [
                        "Exclude",
                        "Domain / IP",
                        "Subdomains",
                        "Scheme",
                        "Port",
                        "Path prefix",
                        "",
                    ] {
                        ui.strong(label);
                    }
                    ui.end_row();
                    for (index, rule) in self.features.state.scope.iter_mut().enumerate() {
                        ui.checkbox(&mut rule.exclude, "");
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.host)
                                .min_size(egui::vec2(190.0, 20.0))
                                .desired_width(190.0)
                                .char_limit(253),
                        );
                        ui.checkbox(&mut rule.subdomains, "");
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.scheme)
                                .min_size(egui::vec2(60.0, 20.0))
                                .desired_width(60.0)
                                .char_limit(5),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.port)
                                .min_size(egui::vec2(55.0, 20.0))
                                .desired_width(55.0)
                                .char_limit(5),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.path)
                                .min_size(egui::vec2(160.0, 20.0))
                                .desired_width(160.0)
                                .char_limit(2048),
                        );
                        if ui.button("Remove").clicked() {
                            remove = Some(index);
                        }
                        ui.end_row();
                    }
                });
        });
        if let Some(index) = remove {
            self.features.state.scope.remove(index);
        }
        if ui.button("Add domain").clicked() && self.features.state.scope.len() < 256 {
            self.features.state.scope.push(ScopeRule::default());
        }
        ui.checkbox(
            &mut self.features.state.filter.in_scope,
            "Show only in-scope history",
        );
        ui.checkbox(
            &mut self.features.state.settings.enforce_scope,
            "Restrict Replay and Intruder to scope",
        );
        if ui.button("Apply to saved history").clicked() {
            self.request_page(0);
        }
    }
    fn variables(&mut self, ui: &mut egui::Ui) {
        ui.heading("Variables");
        ui.label("Use {{key}} in Replay destination, headers or body. Values expand once before sending; missing keys stop the send.");
        let mut remove = None;
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("variables-table")
                .striped(true)
                .show(ui, |ui| {
                    for label in ["Enabled", "Key", "Value", "Secret", ""] {
                        ui.strong(label);
                    }
                    ui.end_row();
                    for (index, var) in self.features.state.variables.iter_mut().enumerate() {
                        ui.checkbox(&mut var.enabled, "");
                        ui.add(
                            egui::TextEdit::singleline(&mut var.key)
                                .min_size(egui::vec2(180.0, 20.0))
                                .desired_width(180.0)
                                .char_limit(128),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut var.value)
                                .password(var.secret)
                                .min_size(egui::vec2(420.0, 20.0))
                                .desired_width(420.0)
                                .char_limit(65536),
                        );
                        ui.checkbox(&mut var.secret, "");
                        if ui.button("Remove").clicked() {
                            remove = Some(index);
                        }
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
    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.label("Listener, timeouts, capture and HTTPS changes apply when the project is reopened. Display settings and rules apply live; execution settings apply to new runs.");
        egui::ScrollArea::vertical().show(ui, |ui| {
            let s = &mut self.features.state.settings;
            egui::Grid::new("settings-grid").show(ui, |ui| {
                ui.label("Listen address");
                ui.add(egui::TextEdit::singleline(&mut s.listen).min_size(egui::vec2(350.0,20.0)));
                ui.end_row();
                for (label, value) in [
                    ("HTTP timeout (seconds)", &mut s.http_timeout),
                    ("Idle timeout (seconds)", &mut s.idle_timeout),
                    ("Connect timeout (seconds)", &mut s.connect_timeout),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(value).range(1..=86400));
                    ui.end_row();
                }
                for (label, value, low, high) in [
                    ("Capture limit (MiB)", &mut s.capture_mib, 1, 16),
                    ("Memory limit (MiB)", &mut s.retain_mib, 16, 1024),
                    ("Live history rows", &mut s.retain_rows, 16, 4096),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(value).range(low..=high));
                    ui.end_row();
                }
                ui.label("Project disk quota (MiB)");
                ui.add(egui::DragValue::new(&mut s.disk_quota_mib).range(64..=1048576));
                ui.end_row();
                ui.label("Browser executable");
                ui.add(egui::TextEdit::singleline(&mut s.browser).min_size(egui::vec2(350.0,20.0)));
                ui.end_row();
                ui.label("Browser start URL");
                ui.add(egui::TextEdit::singleline(&mut s.browser_url).min_size(egui::vec2(350.0,20.0)));
                ui.end_row();
                ui.label("Editor font size");
                ui.add(egui::DragValue::new(&mut s.font_size).range(9.0..=28.0));
                ui.end_row();
                ui.label("Binary size units (KiB, MiB)");
                ui.checkbox(&mut s.binary_units, "");
                ui.end_row();
                ui.label("Only execute in scope");
                ui.checkbox(&mut s.enforce_scope, "");
                ui.end_row();
                ui.label("Concurrency");ui.add(egui::DragValue::new(&mut s.concurrency).range(1..=16));ui.end_row();
                ui.label("Retries for GET / HEAD / OPTIONS");ui.add(egui::DragValue::new(&mut s.retries).range(0..=3));ui.end_row();
                ui.label("429/503 backoff (ms)");ui.add(egui::DragValue::new(&mut s.backoff_ms).range(100..=60000));ui.end_row();
                ui.label("Global requests / second (0 = delay only)");ui.add(egui::DragValue::new(&mut s.global_rps).range(0..=1000));ui.end_row();
                ui.label("Per-host requests / second");ui.add(egui::DragValue::new(&mut s.host_rps).range(0..=1000));ui.end_row();
                ui.label("Intruder delay (ms)");
                ui.add(egui::DragValue::new(&mut s.delay_ms).range(100..=60000));
                ui.end_row();
            });
            ui.checkbox(&mut s.tls_intercept,"Capture HTTPS (requires CA trust; reopen project to apply)");
            ui.label(format!("Public CA certificate: {}/ca.pem",self.features.path));
            ui.label("Import that certificate into your test browser or OS trust settings. The app never installs it automatically.");
            if let Err(error) = s.validate() {
                ui.colored_label(ERROR, error);
            }

            ui.separator();
            ui.collapsing("Keyboard shortcuts",|ui|{
                egui::Grid::new("shortcuts").show(ui,|ui|{
                    for (action,binding) in &mut self.features.state.settings.shortcuts{
                        ui.label(action);ui.add(egui::TextEdit::singleline(binding).char_limit(40));
                        if !binding.is_empty()&&shortcut(binding).is_none(){ui.colored_label(ERROR,"Use Ctrl / Alt / Shift + key");}else{ui.label("");}ui.end_row();
                    }
                });
                if ui.button("Reset shortcuts").clicked(){self.features.state.settings.shortcuts=crate::state::Settings::default().shortcuts;}
                ui.label("Ctrl+Z / Ctrl+Y: editor undo / redo. Ctrl+F: History search.");
            });
            ui.heading("Interception rules");
            ui.checkbox(&mut self.features.state.settings.intercept_responses,"Intercept matching responses (128 KiB buffering limit)");
            let state=&mut self.features.state;
            egui::Grid::new("intercept-conditions").show(ui,|ui|{
                for (label,value) in [("Host",&mut state.intercept_host),("Path prefix",&mut state.intercept_path),("Method",&mut state.intercept_method),("Status prefix",&mut state.intercept_status),("MIME contains",&mut state.intercept_mime)]{ui.label(label);ui.add(egui::TextEdit::singleline(value).char_limit(256));ui.end_row();}
            });
            ui.label("Empty conditions match all. Rewrites run in order before manual edits. Request replacements must preserve valid framing.");
            let mut action=None;
            egui::Grid::new("rewrite-rules").striped(true).show(ui,|ui|{
                for label in ["On","Response","Host","Find","Replace","","",""]{ui.strong(label);}ui.end_row();
                for (index,rule) in state.rules.iter_mut().enumerate(){
                    ui.checkbox(&mut rule.enabled,"");ui.checkbox(&mut rule.response,"");
                    for value in [&mut rule.host,&mut rule.find,&mut rule.replace]{ui.add(egui::TextEdit::singleline(value).min_size(egui::vec2(150.0,20.0)).desired_width(150.0).char_limit(4096));}
                    if ui.small_button("Up").clicked(){action=Some((index,-1));}
                    if ui.small_button("Down").clicked(){action=Some((index,1));}
                    if ui.small_button("Remove").clicked(){action=Some((index,0));}ui.end_row();
                }
            });
            if let Some((index,direction))=action{if direction==0{state.rules.remove(index);}else{let next=index.saturating_add_signed(direction).min(state.rules.len()-1);state.rules.swap(index,next);}}
            if ui.button("Add rewrite").clicked()&&state.rules.len()<64{state.rules.push(crate::state::RewriteRule{enabled:true,response:false,host:String::new(),find:String::new(),replace:String::new()});}
        });
    }
    fn project_view(&mut self, ui: &mut egui::Ui) {
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
            for format in ["jsonl", "har", "raw", "curl"] {
                ui.selectable_value(
                    &mut self.features.transfer_format,
                    format.to_owned(),
                    format,
                );
            }
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
            "Redact authentication, cookies and bodies when exporting",
        );
        ui.horizontal(|ui| {
            if ui.button("Export captures").clicked() {
                self.project_command(Command::Export {
                    path: PathBuf::from(&self.features.transfer_path),
                    format: self.features.transfer_format.clone(),
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
            }
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
            if ui.button("Save project").clicked() {
                self.project_command(Command::Save(Box::new(self.features.state.clone())));
            }
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
        if ui.button("Purge project captures and results…").clicked() {
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
                }
                if ui.button("Keep captures").clicked() {
                    self.features.purge = false;
                }
            });
        }

        ui.colored_label(WARN, &self.features.message);
    }
    fn compare(&mut self, ui: &mut egui::Ui) {
        ui.heading("Compare");
        ui.horizontal(|ui| {
            ui.label("Ignore headers");
            ui.add(egui::TextEdit::singleline(&mut self.features.ignore_headers).char_limit(512));
            ui.checkbox(&mut self.features.compare_hex, "Hex bodies");
        });
        ui.label(format!(
            "Left: {} · Right: {}",
            crate::state::size(
                self.features.compare_left.len() as u64,
                self.features.state.settings.binary_units
            ),
            crate::state::size(
                self.features.compare_right.len() as u64,
                self.features.state.settings.binary_units
            )
        ));
        ui.horizontal(|ui| {
            if ui.button("Selection → left").clicked() {
                self.features.compare_left = self.text.clone();
            }
            if ui.button("Selection → right").clicked() {
                self.features.compare_right = self.text.clone();
            }
            if ui.button("Compare lines").clicked() {
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
        });
        ui.columns(2, |cols| {
            for (col, text) in cols.iter_mut().zip([
                &mut self.features.compare_left,
                &mut self.features.compare_right,
            ]) {
                egui::ScrollArea::both().max_height(220.0).show(col, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(text)
                            .code_editor()
                            .char_limit(262144)
                            .desired_rows(10),
                    );
                });
            }
        });
        egui::ScrollArea::both().show(ui, |ui| {
            ui.monospace(&self.features.compare_diff);
        });
    }
    fn query_results(&mut self) {
        self.project_command(Command::Results {
            run_id: self.features.results_run,
            offset: self.features.results_offset,
            query: self.features.results_query.clone(),
        });
    }
    fn results_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Intruder results");
        ui.horizontal(|ui| {
            ui.label("Run (0 = all)");
            ui.add(egui::DragValue::new(&mut self.features.results_run));
            ui.add(
                egui::TextEdit::singleline(&mut self.features.results_query)
                    .hint_text("Filter payload, status, errors or matches")
                    .char_limit(256),
            );
            if ui.button("Refresh").clicked() {
                self.features.results_offset = 0;
                self.query_results();
            }
            if ui
                .add_enabled(
                    self.features.results_offset > 0,
                    egui::Button::new("Previous"),
                )
                .clicked()
            {
                self.features.results_offset = self.features.results_offset.saturating_sub(128);
                self.query_results();
            }
            if ui
                .add_enabled(
                    (self.features.results_offset as u64 + 128) < self.features.results_total,
                    egui::Button::new("Next"),
                )
                .clicked()
            {
                self.features.results_offset += 128;
                self.query_results();
            }
        });
        ui.label(format!(
            "{} results · click a row to inspect its saved exchange",
            self.features.results_total
        ));
        let mut selected = None;
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("intruder-results")
                .striped(true)
                .show(ui, |ui| {
                    for (index, label) in [
                        "Run", "#", "Payloads", "Status", "Size", "Duration", "Match", "Error",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if ui.button(*label).clicked() {
                            if self.features.results_sort == index {
                                self.features.results_reverse = !self.features.results_reverse;
                            } else {
                                self.features.results_sort = index;
                            }
                        }
                    }
                    ui.end_row();
                    let mut records = self.features.results.iter().collect::<Vec<_>>();
                    records.sort_by(|a, b| {
                        let order = match self.features.results_sort {
                            0 => a.run_id.cmp(&b.run_id),
                            1 => a.index.cmp(&b.index),
                            2 => a.payloads.cmp(&b.payloads),
                            3 => a.status.cmp(&b.status),
                            4 => a.bytes.cmp(&b.bytes),
                            5 => a.elapsed_ms.total_cmp(&b.elapsed_ms),
                            6 => a.matched.cmp(&b.matched),
                            _ => a.error.cmp(&b.error),
                        };
                        if self.features.results_reverse {
                            order.reverse()
                        } else {
                            order
                        }
                    });
                    for record in records {
                        ui.label(record.run_id.to_string());
                        if ui
                            .selectable_label(false, (record.index + 1).to_string())
                            .clicked()
                        {
                            selected = record.capture_id;
                        }
                        ui.label(format!("{:?}", record.payloads));
                        ui.label(record.status.to_string());
                        ui.label(crate::state::size(
                            record.bytes,
                            self.features.state.settings.binary_units,
                        ));
                        ui.label(format!("{:.1} ms", record.elapsed_ms));
                        ui.label(if record.matched { "yes" } else { "" });
                        ui.label(&record.error);
                        ui.end_row();
                    }
                });
        });
        if let Some(id) = selected {
            self.selected = Some(id);
            self.project_command(Command::Detail(id));
            self.workflow.tab = Tab::History;
        }
        ui.horizontal(|ui| {
            ui.label("Export CSV path");
            ui.add(egui::TextEdit::singleline(&mut self.features.transfer_path).char_limit(4096));
            if ui.button("Export results").clicked() {
                self.project_command(Command::ExportResults {
                    path: PathBuf::from(&self.features.transfer_path),
                    run_id: self.features.results_run,
                });
            }
        });
    }
    pub(super) fn keyboard_actions(&mut self, ui: &mut egui::Ui) {
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
                "Stop" => self.workflow.stop(),
                _ => {}
            }
        }
    }
    pub(super) fn inspector_tools(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Find");
            ui.add(egui::TextEdit::singleline(&mut self.features.search).char_limit(256));
            if ui.button("Next match").clicked() && !self.features.search.is_empty() {
                let id = egui::Id::new(("history-message", self.pane as u8));
                self.features.jump = syntax::find_next(
                    ui,
                    id,
                    &self.text,
                    &self.features.search,
                    &mut self.features.search_next,
                );
            }
            if !self.features.search.is_empty() {
                ui.label(format!(
                    "{} matches in preview",
                    self.text.matches(&self.features.search).count()
                ));
            }
            if ui.checkbox(&mut self.features.decoded, "Decoded").changed() {
                self.features.parameters = false;
                self.hex = false;
                self.refresh();
            }
            if ui
                .checkbox(&mut self.features.parameters, "Parameters / cookies")
                .changed()
            {
                self.features.decoded = false;
                self.refresh();
            }
            if ui
                .checkbox(&mut self.features.pretty, "Pretty JSON / XML")
                .changed()
            {
                self.refresh();
            }
            if ui.button("Compare left").clicked() {
                self.features.compare_left = self.text.clone();
            }
            if ui.button("Compare right").clicked() {
                self.features.compare_right = self.text.clone();
                self.workflow.tab = Tab::Compare;
            }
        });
    }
}

impl Drop for Spike {
    fn drop(&mut self) {
        if self.features.open {
            self.features.state.replays = self.workflow.saved_requests();
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

fn shortcut(text: &str) -> Option<egui::KeyboardShortcut> {
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

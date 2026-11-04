use super::*;
use crate::{
    campaign::{self, Kind, Record, Step, Workspace},
    project::Command,
};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub(super) struct Attack {
    pub workspace: Workspace,
    selected_step: usize,
    pub sender: Option<campaign::Sender>,
    pub receiver: Option<campaign::Receiver>,
    active: Option<(u64, watch::Sender<bool>)>,
    pub records: Vec<Record>,
    pub runs: Vec<campaign::RunSummary>,
    pub loaded_run: Option<u64>,
    pub pending_load: Option<(u64, bool)>,
    pub total: u64,
    pub offset: usize,
    filter: String,
    pub message: String,
    inspected: Option<Record>,
    next_run: u64,
    editor_ids: Vec<u64>,
    next_editor: u64,
    reset_editors: bool,
}
impl Attack {
    pub fn running(&self) -> bool {
        self.active.is_some()
    }
    pub(super) fn active_run(&self) -> Option<u64> {
        self.active.as_ref().map(|(id, _)| *id)
    }
    fn new_editor(&mut self) -> u64 {
        self.next_editor += 1;
        self.next_editor
    }
    fn prepare_editors(&mut self, ctx: &egui::Context) {
        if self.reset_editors {
            for id in self.editor_ids.drain(..) {
                syntax::forget_editor(ctx, egui::Id::new(("attack-step", id)));
            }
            self.reset_editors = false;
        }
        while self.editor_ids.len() > self.workspace.steps.len() {
            if let Some(id) = self.editor_ids.pop() {
                syntax::forget_editor(ctx, egui::Id::new(("attack-step", id)));
            }
        }
        while self.editor_ids.len() < self.workspace.steps.len() {
            let id = self.new_editor();
            self.editor_ids.push(id);
        }
    }
    pub fn stop(&self) {
        if let Some((_, cancel)) = &self.active {
            cancel.send_replace(true);
        }
    }
    pub fn load(&mut self, requests: Vec<crate::state::SavedRequest>) -> Result<(), String> {
        if self.running() {
            return Err("Stop the current Attack run before replacing its inputs".into());
        }
        if requests.is_empty() || requests.len() > campaign::MAX_STEPS {
            return Err("Send between 1 and 32 requests to Attack".into());
        }
        if requests
            .iter()
            .any(|v| v.destination.len() > 2048 || v.request.len() > 128 * 1024)
        {
            return Err("Attack inputs must fit the 128 KiB request editor".into());
        }
        self.pending_load = None;
        self.loaded_run = None;
        self.records.clear();
        self.total = 0;
        self.offset = 0;
        self.filter.clear();
        self.reset_editors = true;
        self.workspace.steps = requests.into_iter().map(Into::into).collect();
        self.selected_step = 0;
        self.inspected = None;
        self.workspace.burst_size = self.workspace.burst_size.max(self.workspace.steps.len());
        self.workspace.connections = self.workspace.connections.max(self.workspace.burst_size);
        self.message = "Inputs loaded. Choose a mode and review its options before running.".into();
        Ok(())
    }
    pub fn loaded(
        &mut self,
        run: campaign::SavedRun,
        rows: Vec<Record>,
        total: u64,
        offset: usize,
    ) {
        let Some((id, restore)) = self.pending_load else {
            return;
        };
        if id != run.id || self.running() {
            return;
        }
        self.pending_load = None;
        self.loaded_run = Some(id);
        self.records = rows;
        self.total = total;
        self.offset = offset;
        self.inspected = None;
        if restore {
            self.reset_editors = true;
            self.workspace = run.workspace;
            self.selected_step = 0;
            self.message = format!(
                "Saved run: {}. Reruns use current project variables.",
                if run.status.is_empty() {
                    "status unavailable"
                } else {
                    &run.status
                }
            );
        }
    }
}
impl Spike {
    pub(super) fn poll_attack(&mut self) {
        if self.features.importing || self.features.pending.len() >= 16 {
            return;
        }
        for _ in 0..4 {
            let update = self
                .attack
                .receiver
                .as_mut()
                .and_then(|rx| rx.try_recv().ok());
            let Some(update) = update else {
                break;
            };
            match update {
                campaign::Update::Result { record, execution } => {
                    let mut record = *record;
                    if self.attack.active.as_ref().map(|(id, _)| *id) != Some(record.run_id) {
                        continue;
                    }
                    if let Some(execution) = execution {
                        record.capture_id = Some(self.history.next_id);
                        self.receive(Event::Exchange(*execution));
                    }
                    self.project_command(Command::CampaignRecord(record.clone()));
                    self.attack.total += 1;
                    if self.attack.records.len() >= 128 {
                        self.attack.records.remove(0);
                    }
                    self.attack.records.push(record);
                }
                campaign::Update::Done { run_id, message } => {
                    if self.attack.active.as_ref().map(|(id, _)| *id) == Some(run_id) {
                        self.attack.active = None;
                        self.attack.message = message.clone();
                        self.project_command(Command::CampaignFinish {
                            id: run_id,
                            status: message,
                        });
                        self.attack.pending_load = Some((run_id, false));
                        self.project_command(Command::CampaignLoad {
                            id: run_id,
                            offset: 0,
                        });
                        self.project_command(Command::CampaignList);
                    }
                }
            }
        }
    }
    fn start_attack(&mut self) {
        if self.attack.running() || self.workflow.is_running() {
            self.attack.message =
                "An execution is already running. Stop it before starting another.".into();
            return;
        }
        if !self.workflow.listener_ready {
            self.attack.message = "Wait for the project listener to be ready.".into();
            return;
        }
        if let Err(error) = self.attack.workspace.validate() {
            self.attack.message = error;
            return;
        }
        let Some(sender) = &self.attack.sender else {
            self.attack.message = "Attack worker unavailable".into();
            return;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros()
            .min(u64::MAX as u128) as u64;
        let id = now.max(self.attack.next_run.saturating_add(1));
        self.attack.next_run = id;
        let (cancel, cancel_rx) = watch::channel(false);
        let run = campaign::SavedRun {
            status: "Running".into(),
            id,
            name: {
                let seconds = id / 1_000_000 % 86400;
                format!(
                    "{} · {:02}:{:02}:{:02} UTC",
                    self.attack.workspace.kind.label(),
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60
                )
            },
            workspace: self.attack.workspace.clone(),
            variables: self.features.state.variables.clone(),
        };
        let job = campaign::Job {
            run: run.clone(),
            variables: run.variables.clone(),
            scope: self.features.state.scope.clone(),
            enforce_scope: self.features.state.settings.enforce_scope,
            global_rps: self.features.state.settings.global_rps,
            host_rps: self.features.state.settings.host_rps,
            cancel: cancel_rx,
        };
        match sender.try_send(job) {
            Ok(()) => {
                self.attack.active = Some((id, cancel));
                self.attack.loaded_run = Some(id);
                self.attack.records.clear();
                self.attack.inspected = None;
                self.attack.total = 0;
                self.attack.offset = 0;
                self.attack.filter.clear();
                self.attack.message = "Running…".into();
                self.project_command(Command::CampaignSave(run));
            }
            Err(error) => self.attack.message = format!("Attack could not start: {error}"),
        }
    }
    pub(super) fn attack_page(&mut self, ui: &mut egui::Ui) {
        let mut start = false;
        let mut load = None;
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.heading("Attack");
            ui.add_enabled_ui(!self.attack.running(), |ui| {
                egui::ComboBox::from_id_salt("attack-mode")
                    .selected_text(self.attack.workspace.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in [Kind::Race, Kind::Access, Kind::Sequence] {
                            ui.selectable_value(
                                &mut self.attack.workspace.kind,
                                kind,
                                kind.label(),
                            );
                        }
                    });
            });
            if self.attack.running() {
                ui.spinner();
                if ui.button("Stop run").clicked() {
                    self.attack.stop();
                }
            } else {
                start = primary(
                    ui,
                    !self.attack.workspace.steps.is_empty() && self.workflow.listener_ready,
                    "Run experiment",
                )
                .clicked();
            }
            ui.add_enabled_ui(!self.attack.running(), |ui| {
                egui::ComboBox::from_id_salt("attack-saved-runs")
                    .selected_text("Saved runs")
                    .show_ui(ui, |ui| {
                        if ui.button("Refresh saved runs").clicked() {
                            refresh = true;
                            ui.close();
                        }
                        ui.separator();
                        if self.attack.runs.is_empty() {
                            ui.weak("No saved runs yet");
                        }
                        for run in &self.attack.runs {
                            if ui
                                .selectable_label(
                                    self.attack.loaded_run == Some(run.id),
                                    run.name.clone(),
                                )
                                .on_hover_text(&run.name)
                                .clicked()
                            {
                                load = Some(run.id);
                            }
                        }
                    });
            });
        });
        if start {
            self.start_attack();
        }
        if refresh {
            self.project_command(Command::CampaignList);
        }
        if let Some(id) = load {
            self.attack.pending_load = Some((id, true));
            self.project_command(Command::CampaignLoad { id, offset: 0 });
        }
        if !self.attack.message.is_empty() {
            ui.label(&self.attack.message);
        }
        let available = ui.available_height();
        egui::Panel::bottom("attack-results-panel")
            .resizable(true)
            .default_size(220.0)
            .min_size(200.0)
            .max_size((available * 0.55).max(140.0))
            .show(ui, |ui| self.attack_results(ui));
        if let Some(record) = self.attack.inspected.clone() {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Back to experiment").clicked() {
                    self.attack.inspected = None;
                }
                ui.strong(format!("Result #{} · {}", record.index + 1, record.label));
                ui.weak(format!("{} · {}", record.identity, record.verdict));
            });
            if !record.error.is_empty() {
                ui.colored_label(ERROR, &record.error);
            }
            if let Some(baseline) = record.baseline
                && ui.button("Compare response with original").clicked()
            {
                self.project_command(Command::CampaignCompare {
                    id: record.run_id,
                    baseline,
                    variant: record.index,
                });
            }

            if let Some(id) = record.capture_id {
                if self.selected != Some(id) {
                    self.selected = Some(id);
                    self.project_command(Command::Detail(id));
                }
                self.inspector(ui);
            } else {
                ui.weak("No HTTP response was captured for this attempt.");
            }
            return;
        }
        let running = self.attack.running();
        egui::Panel::left("attack-configuration").default_size(275.0).size_range(230.0..=380.0).resizable(true).show(ui,|ui|{
            egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui,|ui|{
                ui.add_enabled_ui(!running,|ui|{
                    ui.strong("Requests");
                    if self.attack.workspace.steps.is_empty(){ui.weak("Right-click a request and choose Send to Attack, or send a Replay group.");}
                    for (index,step) in self.attack.workspace.steps.iter().enumerate(){
                        let label=format!("{}. {}",index+1,step.name);
                        if ui.selectable_label(self.attack.selected_step==index,label).clicked(){self.attack.selected_step=index;}
                    }
                    ui.horizontal_wrapped(|ui|{
                        if ui.add_enabled(self.attack.workspace.steps.len()<campaign::MAX_STEPS,egui::Button::new("+ Request")).clicked(){self.attack.workspace.steps.push(Step::default());self.attack.selected_step=self.attack.workspace.steps.len()-1;}
                        if ui.button("Clear inputs").clicked(){self.attack.workspace.steps.clear();self.attack.selected_step=0;}
                    });
                    ui.separator();
                    let w=&mut self.attack.workspace;
                    match w.kind{
                        Kind::Race=>{
                            ui.strong("Race options");
                            field_number(ui,"Requests per burst",&mut w.burst_size,1..=64);
                            field_number(ui,"Bursts",&mut w.bursts,1..=4096);
                            field_number(ui,"Parallel connections",&mut w.connections,1..=64);
                            ui.checkbox(&mut w.synchronized,"Synchronize final byte (HTTP/1.1)");
                            ui.weak("Each burst cycles through the input requests. All connections must be ready before a synchronized burst is released.");
                            if w.synchronized&&w.connections<w.burst_size{ui.colored_label(WARN,"Increase Parallel connections to match the burst size.");}
                        }
                        Kind::Access=>{
                            ui.strong("Alternate identities");
                            ui.weak("Credential headers replace the original credentials. Use project variables such as {{user_b_token}}.");
                            let mut remove=None;
                            for (index,profile) in w.profiles.iter_mut().enumerate(){ui.push_id(index,|ui|{
                                ui.horizontal(|ui|{
                                    ui.add(egui::TextEdit::singleline(&mut profile.name).desired_width((ui.available_width()-35.0).max(90.0)).char_limit(64));
                                    if ui.small_button("×").on_hover_text("Remove identity").clicked(){remove=Some(index);}
                                });
                                ui.add(egui::TextEdit::multiline(&mut profile.headers).code_editor().desired_width(f32::INFINITY).desired_rows(3).char_limit(16384).hint_text("Authorization: Bearer {{user_b_token}}\nCookie: session=..."));
                            });}
                            if let Some(index)=remove{w.profiles.remove(index);}
                            if ui.add_enabled(w.profiles.len()<8,egui::Button::new("+ Identity")).clicked(){w.profiles.push(campaign::access::Identity{name:format!("Identity {}",w.profiles.len()+1),headers:String::new()});}
                            ui.checkbox(&mut w.anonymous,"Include anonymous request");
                            ui.label("Credential headers to remove (in addition to Authorization and Cookie)");
                            ui.add(egui::TextEdit::multiline(&mut w.remove_headers).desired_width(f32::INFINITY).desired_rows(2).char_limit(2048));
                            ui.label("Denied-response marker (optional)");
                            ui.add(egui::TextEdit::singleline(&mut w.denial_marker).desired_width(f32::INFINITY).char_limit(1024));
                            ui.weak("Identical successful responses are potential bypasses. Inspect the baseline and variant to verify authorization behavior.");
                        }
                        Kind::Sequence=>{
                            ui.strong("Sequence options");
                            field_number(ui,"Cycles",&mut w.cycles,1..=4096);
                            ui.checkbox(&mut w.carry_cookies,"Carry cookies on the same origin");
                            ui.checkbox(&mut w.stop_on_failure,"Stop on a failed step");
                            ui.weak("Steps run in order. Extracted values are available as {{name}} in later requests. Each cycle starts with a fresh cookie jar and variables.");
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui|{ui.label("Delay between bursts / steps");ui.add(egui::DragValue::new(&mut w.delay_ms).range(0..=60000).suffix(" ms"));});
                    ui.horizontal(|ui|{ui.label("Timeout");ui.add(egui::DragValue::new(&mut w.timeout_secs).range(1..=300).suffix(" s"));});
                    ui.weak(format!("{} total requests · no automatic retries",w.total()));
                    if let Err(error)=w.validate(){ui.colored_label(WARN,error);}
                });
            });
        });
        self.attack.prepare_editors(ui.ctx());
        let index = self
            .attack
            .selected_step
            .min(self.attack.workspace.steps.len().saturating_sub(1));
        if let Some(step) = self.attack.workspace.steps.get_mut(index) {
            let mut movement = 0;
            let mut remove = false;
            let mut duplicate = false;
            ui.add_enabled_ui(!running, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Name");
                    ui.add(
                        egui::TextEdit::singleline(&mut step.name)
                            .desired_width(160.0)
                            .char_limit(128),
                    );
                    if ui
                        .button("Move up")
                        .on_hover_text("Move step earlier")
                        .clicked()
                    {
                        movement = -1;
                    }
                    if ui
                        .button("Move down")
                        .on_hover_text("Move step later")
                        .clicked()
                    {
                        movement = 1;
                    }
                    duplicate = ui.button("Duplicate").clicked();
                    remove = ui.button("Remove").clicked();
                });
                ui.horizontal(|ui| {
                    ui.label("Destination");
                    ui.add(
                        egui::TextEdit::singleline(&mut step.destination)
                            .desired_width(f32::INFINITY)
                            .hint_text("https://target.example")
                            .char_limit(2048),
                    );
                });
                if self.attack.workspace.kind == Kind::Sequence {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Expected status prefixes");
                        ui.add(
                            egui::TextEdit::singleline(&mut step.expected)
                                .desired_width(140.0)
                                .char_limit(32)
                                .hint_text("2,3; empty accepts any"),
                        );
                    });
                    egui::CollapsingHeader::new(format!(
                        "Extract response values ({})",
                        step.extractors.len()
                    ))
                    .id_salt(("step-extractors", index))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("step-extraction-scroll", index))
                            .max_height(140.0)
                            .show(ui, |ui| {
                                let mut removed = None;
                                for (index, extraction) in step.extractors.iter_mut().enumerate() {
                                    ui.push_id(index, |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.add(
                                                egui::TextEdit::singleline(&mut extraction.name)
                                                    .desired_width(100.0)
                                                    .char_limit(64),
                                            );
                                            egui::ComboBox::from_id_salt("source")
                                                .selected_text(extraction.source.label())
                                                .show_ui(ui, |ui| {
                                                    for source in [
                                                        campaign::sequence::Source::Json,
                                                        campaign::sequence::Source::Header,
                                                        campaign::sequence::Source::Cookie,
                                                        campaign::sequence::Source::Regex,
                                                    ] {
                                                        ui.selectable_value(
                                                            &mut extraction.source,
                                                            source,
                                                            source.label(),
                                                        );
                                                    }
                                                });
                                            ui.checkbox(&mut extraction.required, "Required");
                                            if ui.small_button("×").clicked() {
                                                removed = Some(index);
                                            }
                                        });
                                        ui.add(
                                            egui::TextEdit::singleline(&mut extraction.selector)
                                                .desired_width(f32::INFINITY)
                                                .char_limit(2048)
                                                .hint_text(
                                                    "/token, header name, cookie name, or regex",
                                                ),
                                        );
                                    });
                                }
                                if let Some(index) = removed {
                                    step.extractors.remove(index);
                                }
                                if ui
                                    .add_enabled(
                                        step.extractors.len() < 16,
                                        egui::Button::new("+ Extractor"),
                                    )
                                    .clicked()
                                {
                                    step.extractors.push(Default::default());
                                }
                            });
                    });
                }
            });
            let id = egui::Id::new(("attack-step", self.attack.editor_ids[index]));
            syntax::wrap_control(ui, id);
            let target =
                crate::state::resolve(&step.destination, &self.features.state.variables).ok();
            syntax::register_scope(ui, id, target.as_deref());
            syntax::register_request(
                ui,
                id,
                Some(Ok(syntax::RequestSource {
                    destination: step.destination.clone(),
                    request: step.request.clone(),
                })),
            );
            let height = ui.available_height().max(90.0);
            egui::ScrollArea::both()
                .id_salt(("attack-request", index))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    syntax::editor_limited(ui, id, &mut step.request, !running, 18, 128 * 1024);
                });
            if remove {
                let id = self.attack.editor_ids.remove(index);
                syntax::forget_editor(ui.ctx(), egui::Id::new(("attack-step", id)));
                self.attack.workspace.steps.remove(index);
                self.attack.selected_step = index.saturating_sub(1);
            } else if duplicate && self.attack.workspace.steps.len() < campaign::MAX_STEPS {
                let step = self.attack.workspace.steps[index].clone();
                self.attack.workspace.steps.insert(index + 1, step);
                let id = self.attack.new_editor();
                self.attack.editor_ids.insert(index + 1, id);
                self.attack.selected_step = index + 1;
            } else if movement != 0 {
                let next = index.saturating_add_signed(movement);
                if next < self.attack.workspace.steps.len() {
                    self.attack.workspace.steps.swap(index, next);
                    self.attack.editor_ids.swap(index, next);
                    self.attack.selected_step = next;
                }
            }
        } else {
            empty_state(
                ui,
                "Build an experiment",
                "Send a request or Replay group to Attack, then choose Race condition, Access-control, or Sequence.",
            );
        }
    }
    fn attack_results(&mut self, ui: &mut egui::Ui) {
        let mut page = None;
        let running = self.attack.running();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Results");
            ui.add(
                egui::TextEdit::singleline(&mut self.attack.filter)
                    .hint_text("Search this page")
                    .desired_width(180.0)
                    .char_limit(128),
            );
            ui.weak(format!("{} captured attempts", self.attack.total));
            if !running && self.attack.total > 128 {
                if ui
                    .add_enabled(self.attack.offset > 0, egui::Button::new("Previous"))
                    .clicked()
                {
                    page = Some(self.attack.offset.saturating_sub(128));
                }
                if ui
                    .add_enabled(
                        (self.attack.offset + 128) < self.attack.total as usize,
                        egui::Button::new("Next"),
                    )
                    .clicked()
                {
                    page = Some(self.attack.offset + 128);
                }
            }
        });
        if self.attack.workspace.kind == Kind::Race {
            let mut bursts = std::collections::BTreeMap::<usize, (u64, u64)>::new();
            for record in &self.attack.records {
                if let Some(release) = record.release_us {
                    let span = bursts.entry(record.burst).or_insert((release, release));
                    span.0 = span.0.min(release);
                    span.1 = span.1.max(release);
                }
            }
            if let Some(spread) = bursts.values().map(|(first, last)| last - first).max() {
                ui.weak(format!(
                    "Largest client release spread: {spread} µs · server processing may differ"
                ));
            }
        }
        if let Some(offset) = page
            && let Some(id) = self.attack.loaded_run
        {
            self.attack.pending_load = Some((id, false));
            self.project_command(Command::CampaignLoad { id, offset });
        }
        let query = self.attack.filter.to_lowercase();
        let records: Vec<_> = self
            .attack
            .records
            .iter()
            .filter(|record| {
                query.is_empty()
                    || format!(
                        "{} {} {} {}",
                        record.label, record.identity, record.verdict, record.error
                    )
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        let width = ui.available_width() - ui.spacing().scroll.allocated_width();
        let flexible = (width - 360.0).max(240.0);
        let columns = [
            ("#", 50.0),
            ("Request / identity", flexible * 0.45),
            ("Status", 65.0),
            ("Size", 75.0),
            ("Duration", 95.0),
            ("Assessment / timing", flexible * 0.55 + 75.0),
        ];
        let (header, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
        let mut x = header.left();
        for (label, span) in &columns {
            ui.painter().text(
                egui::pos2(x + 5.0, header.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(12.0),
                MUTED,
            );
            x += span;
        }
        ui.separator();
        let height = (ui.available_height() - ui.spacing().item_spacing.y).max(45.0);
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .id_salt("attack-result-rows")
            .max_height(height)
            .auto_shrink([false, false])
            .show_rows(ui, 28.0, records.len(), |ui, range| {
                for index in range {
                    let record = records[index];
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
                    let selected = self.attack.inspected.as_ref().is_some_and(|selected| {
                        selected.run_id == record.run_id && selected.index == record.index
                    });
                    if selected || response.hovered() || index % 2 == 0 {
                        ui.painter().rect_filled(
                            rect,
                            0,
                            if selected {
                                ui.visuals().selection.bg_fill
                            } else {
                                SURFACE
                            },
                        );
                    }
                    let assessment = if !record.error.is_empty() {
                        record.error.clone()
                    } else if !record.verdict.is_empty() {
                        record.verdict.clone()
                    } else {
                        format!(
                            "Burst {} · response #{}{}",
                            record.burst + 1,
                            record.position,
                            record
                                .release_us
                                .map(|value| format!(" · release +{value} µs"))
                                .unwrap_or_default()
                        )
                    };
                    let values = [
                        (record.index + 1).to_string(),
                        format!(
                            "{}{}",
                            record.label,
                            if record.identity.is_empty() {
                                String::new()
                            } else {
                                format!(" / {}", record.identity)
                            }
                        ),
                        if record.status == 0 {
                            "—".into()
                        } else {
                            record.status.to_string()
                        },
                        crate::state::size(record.bytes, self.features.state.settings.binary_units),
                        format!("{:.1} ms", record.elapsed_ms),
                        assessment,
                    ];
                    let mut x = rect.left();
                    for ((_, span), value) in columns.iter().zip(&values) {
                        let cell = egui::Rect::from_min_size(
                            egui::pos2(x, rect.top()),
                            egui::vec2(*span, rect.height()),
                        );
                        ui.painter()
                            .with_clip_rect(cell.intersect(ui.clip_rect()))
                            .text(
                                egui::pos2(x + 5.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                value,
                                egui::FontId::monospace(12.0),
                                if record.error.is_empty() {
                                    ui.visuals().text_color()
                                } else {
                                    ERROR
                                },
                            );
                        x += span;
                    }
                    if response.clicked() {
                        clicked = Some(record.clone());
                    }
                    response.on_hover_text(format!(
                        "{}\n{}\nClick to inspect exact request and response.",
                        values[1], values[5]
                    ));
                }
            });
        if let Some(record) = clicked {
            self.attack.inspected = Some(record.clone());
            if let Some(id) = record.capture_id {
                self.selected = Some(id);
                self.project_command(Command::Detail(id));
            }
        }
    }
}
fn field_number(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut usize,
    range: std::ops::RangeInclusive<usize>,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(range));
    });
}

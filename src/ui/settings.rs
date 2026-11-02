use super::features::SettingsSection;
use super::*;
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    let caption = ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .min_size(egui::vec2(330.0, 24.0))
            .hint_text(hint)
            .char_limit(4096),
    )
    .labelled_by(caption.id);
    ui.end_row();
}
impl Spike {
    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        egui::Panel::left("settings-sections")
            .exact_size(155.0)
            .resizable(false)
            .show(ui, |ui| {
                for (section, label) in [
                    (SettingsSection::General, "Network"),
                    (SettingsSection::Display, "Display & storage"),
                    (SettingsSection::Execution, "Execution"),
                    (SettingsSection::Scope, "Scope"),
                    (SettingsSection::Variables, "Variables"),
                    (SettingsSection::Interception, "Interception"),
                    (SettingsSection::Shortcuts, "Shortcuts"),
                    (SettingsSection::Browser, "Browser"),
                    (SettingsSection::Certificates, "Certificates"),
                    (SettingsSection::Project, "Project"),
                ] {
                    ui.selectable_value(&mut self.features.settings_section, section, label);
                }
            });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("settings-content", self.features.settings_section as u8))
                .show(ui, |ui| match self.features.settings_section {
                    SettingsSection::General => self.network_settings(ui),
                    SettingsSection::Display => self.display_settings(ui),
                    SettingsSection::Execution => self.execution_settings(ui),
                    SettingsSection::Scope => self.scope(ui),
                    SettingsSection::Variables => self.variables(ui),
                    SettingsSection::Interception => self.interception_settings(ui),
                    SettingsSection::Shortcuts => self.shortcut_settings(ui),
                    SettingsSection::Project => self.project_view(ui),
                    SettingsSection::Certificates => self.certificate_settings(ui),
                    SettingsSection::Browser => {
                        self.workflow.browser(ui);
                        self.features.state.settings.browser =
                            self.workflow.settings.browser.clone();
                        self.features.state.settings.browser_url =
                            self.workflow.settings.browser_url.clone();
                    }
                });
        });
    }
    fn network_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Network");
        ui.weak(
            "Changes apply when this project is reopened. HTTPS interception is always enabled.",
        );
        let settings = &mut self.features.state.settings;
        egui::Grid::new("network-settings")
            .spacing([22.0, 12.0])
            .show(ui, |ui| {
                field(ui, "Listen address", &mut settings.listen, "127.0.0.1:8080");
                for (label, value) in [
                    ("HTTP timeout", &mut settings.http_timeout),
                    ("Idle timeout", &mut settings.idle_timeout),
                    ("Connect timeout", &mut settings.connect_timeout),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(value).range(1..=86400).suffix(" s"));
                    ui.end_row();
                }
                ui.label("Response capture limit");
                ui.add(
                    egui::DragValue::new(&mut settings.capture_mib)
                        .range(1..=16)
                        .suffix(" MiB"),
                );
                ui.end_row();
            });
        if let Err(error) = settings.validate() {
            ui.colored_label(ERROR, error);
        }
    }
    fn display_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Display & storage");
        let settings = &mut self.features.state.settings;
        egui::Grid::new("display-settings")
            .spacing([22.0, 12.0])
            .show(ui, |ui| {
                ui.label("Editor font");
                ui.add(
                    egui::DragValue::new(&mut settings.font_size)
                        .range(9.0..=28.0)
                        .suffix(" px"),
                );
                ui.end_row();
                ui.label("Size units");
                ui.checkbox(&mut settings.binary_units, "Binary (KiB, MiB)");
                ui.end_row();
                ui.label("Live history rows");
                ui.add(egui::DragValue::new(&mut settings.retain_rows).range(16..=4096));
                ui.end_row();
                ui.label("Capture memory limit");
                ui.add(
                    egui::DragValue::new(&mut settings.retain_mib)
                        .range(16..=1024)
                        .suffix(" MiB"),
                );
                ui.end_row();
                ui.label("Retained content quota");
                ui.add(
                    egui::DragValue::new(&mut settings.disk_quota_mib)
                        .range(64..=1048576)
                        .suffix(" MiB"),
                );
                ui.end_row();
            });
    }
    fn execution_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Execution");
        ui.weak("Applies to new Replay and Intruder runs.");
        let settings = &mut self.features.state.settings;
        egui::Grid::new("execution-settings")
            .spacing([22.0, 12.0])
            .show(ui, |ui| {
                ui.label("Concurrent requests");
                ui.add(egui::DragValue::new(&mut settings.concurrency).range(1..=16));
                ui.end_row();
                ui.label("Delay between attempts");
                ui.add(
                    egui::DragValue::new(&mut settings.delay_ms)
                        .range(100..=60000)
                        .suffix(" ms"),
                );
                ui.end_row();
                ui.label("Global requests / second");
                ui.add(egui::DragValue::new(&mut settings.global_rps).range(0..=1000));
                ui.end_row();
                ui.label("Per-host requests / second");
                ui.add(egui::DragValue::new(&mut settings.host_rps).range(0..=1000));
                ui.end_row();
                ui.label("Idempotent-method retries");
                ui.add(egui::DragValue::new(&mut settings.retries).range(0..=3));
                ui.end_row();
                ui.label("429 / 503 backoff");
                ui.add(
                    egui::DragValue::new(&mut settings.backoff_ms)
                        .range(100..=60000)
                        .suffix(" ms"),
                );
                ui.end_row();
            });
        ui.weak("A rate of 0 uses the delay. Retries apply to GET, HEAD and OPTIONS.");
    }
    fn interception_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Interception");
        ui.checkbox(
            &mut self.features.state.settings.intercept_responses,
            "Hold matching responses",
        );
        ui.weak("Empty conditions match all. Responses over 128 KiB stream without being held.");
        let state = &mut self.features.state;
        egui::Grid::new("intercept-conditions")
            .spacing([22.0, 12.0])
            .show(ui, |ui| {
                field(
                    ui,
                    "Host",
                    &mut state.intercept_host,
                    "example.com or *.example.com",
                );
                field(ui, "Path prefix", &mut state.intercept_path, "/api/");
                field(ui, "Method", &mut state.intercept_method, "GET or POST");
                field(
                    ui,
                    "Status prefix",
                    &mut state.intercept_status,
                    "200, 4 or 5 (responses)",
                );
                field(
                    ui,
                    "MIME contains",
                    &mut state.intercept_mime,
                    "application/json (responses)",
                );
            });
        ui.separator();
        ui.strong("Rewrites");
        ui.weak("Literal replacements run in order before manual editing. Request framing must remain valid.");
        let mut action = None;
        egui::ScrollArea::horizontal()
            .id_salt("rewrite-table")
            .show(ui, |ui| {
                egui::Grid::new("rewrite-rules")
                    .striped(true)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        for label in ["Enabled", "Response", "Host", "Find", "Replace", ""] {
                            ui.strong(label);
                        }
                        ui.end_row();
                        for (index, rule) in state.rules.iter_mut().enumerate() {
                            ui.checkbox(&mut rule.enabled, "");
                            ui.checkbox(&mut rule.response, "");
                            for (value, hint, width) in [
                                (&mut rule.host, "Any host", 160.0),
                                (&mut rule.find, "Literal text", 180.0),
                                (&mut rule.replace, "Replacement", 180.0),
                            ] {
                                ui.add(
                                    egui::TextEdit::singleline(value)
                                        .min_size(egui::vec2(width, 24.0))
                                        .hint_text(hint)
                                        .char_limit(4096),
                                );
                            }
                            ui.horizontal(|ui| {
                                if ui.small_button("Up").clicked() {
                                    action = Some((index, -1));
                                }
                                if ui.small_button("Down").clicked() {
                                    action = Some((index, 1));
                                }
                                if ui.small_button("Remove").clicked() {
                                    action = Some((index, 0));
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
        if let Some((index, direction)) = action {
            if direction == 0 {
                state.rules.remove(index);
            } else {
                let next = index
                    .saturating_add_signed(direction)
                    .min(state.rules.len() - 1);
                state.rules.swap(index, next);
            }
        }
        if ui.button("Add rewrite").clicked() && state.rules.len() < 64 {
            state.rules.push(crate::state::RewriteRule {
                enabled: true,
                response: false,
                host: String::new(),
                find: String::new(),
                replace: String::new(),
            });
        }
    }
    fn shortcut_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Shortcuts");
        egui::Grid::new("shortcut-bindings")
            .spacing([22.0, 12.0])
            .show(ui, |ui| {
                for (action, binding) in &mut self.features.state.settings.shortcuts {
                    field(ui, action, binding, "Ctrl+1, Ctrl+Enter, F9");
                }
            });
        if ui.button("Restore defaults").clicked() {
            self.features.state.settings.shortcuts = crate::state::Settings::default().shortcuts;
        }
        ui.weak("Ctrl+Z / Ctrl+Y: undo / redo. Ctrl+F: History search.");
    }
    fn certificate_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Certificates");
        if self.features.trust_pending {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Installing the project CA for this user…");
            });
        } else {
            match &self.features.trust_status {
                Some(Ok(status)) => {
                    ui.label("HTTPS interception is ready.");
                    ui.label(format!("Trust store: {}", status.store));
                    ui.monospace(format!("SHA-256: {}", status.fingerprint));
                }
                Some(Err(error)) => {
                    ui.colored_label(ERROR, error);
                }
                None => {
                    ui.label("Certificate setup has not completed.");
                }
            }
        }
        ui.weak("The public CA is installed for the current user. Upstream certificates are still verified.");
        let path = std::path::Path::new(&self.features.path).join("ca.pem");
        ui.horizontal(|ui| {
            if ui.button("Copy public CA path").clicked() {
                ui.ctx().copy_text(path.display().to_string());
            }
            if ui
                .add_enabled(
                    !self.features.trust_pending,
                    egui::Button::new("Run setup again"),
                )
                .clicked()
            {
                self.install_trust();
            }
        });
    }
}

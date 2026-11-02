use super::*;
use crate::activity::{Level, Record};

pub(super) struct LogView {
    open: bool,
    query: String,
    levels: [bool; 3],
    last_features: String,
    last_workflow: String,
}
impl Default for LogView {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            levels: [true; 3],
            last_features: String::new(),
            last_workflow: String::new(),
        }
    }
}
fn line(record: &Record) -> String {
    let seconds = record.when_ms / 1000 % 86400;
    let time = if record.when_ms == 0 {
        "Saved".to_owned()
    } else {
        format!(
            "{:02}:{:02}:{:02} UTC",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    };
    format!(
        "{time}  {}  {}{}",
        record.level.label(),
        record.message,
        if record.repeats > 1 {
            format!(" (×{})", record.repeats)
        } else {
            String::new()
        }
    )
}
impl Spike {
    pub(super) fn add_log(&mut self, level: Level, message: &str) {
        let record = self.logs.push(level, message);
        if self.features.open {
            self.project_command(crate::project::Command::WriteLogs(vec![record]));
        }
    }
    pub(super) fn capture_notices(&mut self) {
        let feature = self.features.message.clone();
        if feature != self.log_view.last_features && !feature.is_empty() {
            self.add_log(Level::Warning, &feature);
        }
        self.log_view.last_features = feature;
        let workflow = self.workflow.message.clone();
        if workflow != self.log_view.last_workflow && !workflow.is_empty() {
            self.add_log(Level::Error, &workflow);
        }
        self.log_view.last_workflow = workflow;
    }
    pub(super) fn log_panels(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("activity-footer")
            .exact_size(28.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let label = if self.logs.unread == 0 {
                        "Logs".to_owned()
                    } else {
                        format!("Logs ({})", self.logs.unread)
                    };
                    if ui.selectable_label(self.log_view.open, label).clicked() {
                        self.log_view.open = !self.log_view.open;
                    }
                    let errors = self
                        .logs
                        .records
                        .iter()
                        .filter(|r| r.level == Level::Error)
                        .count();
                    let warnings = self
                        .logs
                        .records
                        .iter()
                        .filter(|r| r.level == Level::Warning)
                        .count();
                    if errors > 0 {
                        ui.weak(format!(
                            "{errors} error{}",
                            if errors == 1 { "" } else { "s" }
                        ));
                    }
                    if warnings > 0 {
                        ui.weak(format!(
                            "{warnings} warning{}",
                            if warnings == 1 { "" } else { "s" }
                        ));
                    }
                    if self
                        .toast
                        .as_ref()
                        .is_some_and(|(_, when)| when.elapsed() > std::time::Duration::from_secs(3))
                    {
                        self.toast = None;
                    }
                    if let Some((message, _)) = &self.toast {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(message);
                        });
                    }
                });
            });
        if !self.log_view.open {
            return;
        }
        self.logs.unread = 0;
        egui::Panel::bottom("activity-drawer")
            .default_size(220.0)
            .min_size(120.0)
            .max_size(400.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Logs");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.log_view.query)
                            .hint_text("Search logs")
                            .desired_width(180.0)
                            .char_limit(256),
                    );
                    for (index, label) in ["Info", "Warnings", "Errors"].iter().enumerate() {
                        ui.checkbox(&mut self.log_view.levels[index], *label);
                    }
                    if ui.small_button("Clear").clicked() {
                        self.logs.records.clear();
                        self.project_command(crate::project::Command::ClearLogs);
                    }
                    if ui.small_button("Close").clicked() {
                        self.log_view.open = false;
                    }
                });
                let query = self.log_view.query.to_lowercase();
                let records: Vec<_> = self
                    .logs
                    .records
                    .iter()
                    .filter(|record| {
                        let index = match record.level {
                            Level::Info => 0,
                            Level::Warning => 1,
                            Level::Error => 2,
                        };
                        self.log_view.levels[index]
                            && (query.is_empty() || record.message.to_lowercase().contains(&query))
                    })
                    .collect();
                ui.horizontal(|ui| {
                    ui.weak(format!(
                        "{} entries · last {} retained",
                        records.len(),
                        crate::activity::LIMIT
                    ));
                    if ui.small_button("Copy visible").clicked() {
                        ui.ctx().copy_text(
                            records
                                .iter()
                                .map(|r| line(r))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                });
                egui::ScrollArea::both()
                    .id_salt("activity-rows")
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show_rows(ui, 22.0, records.len(), |ui, range| {
                        for record in &records[range] {
                            let color = match record.level {
                                Level::Error => ERROR,
                                Level::Warning => WARN,
                                Level::Info => MUTED,
                            };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(line(record).replace(['\r', '\n'], " "))
                                        .color(color)
                                        .monospace(),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            )
                            .on_hover_text(&record.message);
                        }
                    });
            });
    }
}

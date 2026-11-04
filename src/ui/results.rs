use super::*;
use crate::jobs::Record;

#[derive(Default)]
pub(super) struct Action {
    pub selected: Option<usize>,
    pub sort: Option<usize>,
}
fn columns(width: f32, latest: bool) -> Vec<(usize, &'static str, f32, f32)> {
    let fixed = if latest { 440.0 } else { 520.0 };
    let flexible = (width - fixed).max(300.0);
    let mut x = 0.0;
    [
        (0, "Run", 80.0),
        (1, "#", 60.0),
        (2, "Payloads", flexible * 0.55),
        (3, "Status", 80.0),
        (4, "Size", 90.0),
        (5, "Duration", 120.0),
        (6, "Match", 90.0),
        (7, "Error", flexible * 0.45),
    ]
    .into_iter()
    .filter(|(index, _, _)| !latest || *index != 0)
    .map(|(index, label, span)| {
        let column = (index, label, x, span);
        x += span;
        column
    })
    .collect()
}
pub(super) fn table(
    ui: &mut egui::Ui,
    records: &[Record],
    selected: Option<(u64, usize)>,
    latest: bool,
    binary: bool,
    sort: usize,
    reverse: bool,
) -> Action {
    let mut action = Action::default();
    let available_height = ui.available_height();
    // Reserve the vertical scrollbar so a long run does not introduce a second, horizontal scrollbar.
    let columns = columns(
        ui.available_width() - ui.spacing().scroll.allocated_width(),
        latest,
    );
    let width = columns.last().map_or(0.0, |(_, _, x, span)| x + span);
    egui::ScrollArea::horizontal()
        .id_salt("results-horizontal")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_min_width(width);
            let top = ui.cursor().min.y;
            let (header, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
            for (index, label, x, span) in &columns {
                let cell = egui::Rect::from_min_size(
                    header.min + egui::vec2(*x, 0.0), egui::vec2(*span, header.height()),
                );
                let response = ui.interact(cell, ui.id().with(("result-sort", index)), egui::Sense::click());
                if response.clicked() {
                    action.sort = Some(*index);
                }
                let title = if *index == sort {
                    format!("{label} {}", if reverse { "↓" } else { "↑" })
                } else {
                    label.to_string()
                };
                ui.painter().with_clip_rect(cell.intersect(ui.clip_rect())).text(
                    cell.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER,
                    title, egui::FontId::proportional(13.0), MUTED,
                );
            }
            ui.separator();
            if records.is_empty() {
                ui.weak("No matching results.");
                return;
            }
            // Include the header, spacing and horizontal scrollbar in the panel budget.
            // Otherwise a resizable bottom panel grows a little more on every frame.
            let height = (available_height - (ui.cursor().min.y - top)
                - ui.spacing().scroll.allocated_width() - ui.spacing().item_spacing.y).max(40.0);
            egui::ScrollArea::vertical()
                .id_salt("intruder-results-scroll")
                .max_height(height)
                .auto_shrink([false, false])
                .show_rows(ui, 30.0, records.len(), |ui, range| {
                    for index in range {
                        let record = &records[index];
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::click());
                        let is_selected = selected == Some((record.run_id, record.index));
                        if is_selected || response.hovered() || index % 2 == 0 {
                            let fill = if is_selected {
                                ui.visuals().selection.bg_fill
                            } else if response.hovered() {
                                egui::Color32::from_rgb(35, 45, 58)
                            } else {
                                SURFACE
                            };
                            ui.painter().rect_filled(rect, 0, fill);
                        }
                        let values = [
                            record.run_id.to_string(), (record.index + 1).to_string(),
                            record.payloads.join(" | "),
                            if record.status == 0 { "—".into() } else { record.status.to_string() },
                            crate::state::size(record.bytes, binary), format!("{:.1} ms", record.elapsed_ms),
                            if record.matched { "yes".into() } else { String::new() }, record.error.clone(),
                        ];
                        for (column, _, x, span) in &columns {
                            let cell = egui::Rect::from_min_size(
                                rect.min + egui::vec2(*x, 0.0), egui::vec2(*span, rect.height()),
                            );
                            let color = if *column == 7 && !record.error.is_empty() {
                                ERROR
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.painter().with_clip_rect(cell.intersect(ui.clip_rect())).text(
                                cell.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER,
                                &values[*column], egui::FontId::monospace(13.0), color,
                            );
                        }
                        if response.clicked() {
                            action.selected = Some(index);
                        }
                        response.widget_info(|| egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel, true, is_selected,
                            format!("Result {} payload {} status {} {}", record.index + 1, values[2], values[3], record.error),
                        ));
                        response.on_hover_text(format!(
                            "Payload: {}\n{}\nClick anywhere on this row to inspect its request and response.",
                            values[2], record.error,
                        ));
                    }
                });
        });
    action
}
impl Spike {
    pub(super) fn inspect_result(&mut self, record: Record) {
        self.features.result_error.clear();
        self.features.result_loading = record.capture_id.is_some() || record.status != 0;
        self.selected = record.capture_id;
        if let Some(id) = record.capture_id {
            self.project_command(crate::project::Command::Detail(id));
        } else if record.status != 0 {
            self.project_command(crate::project::Command::FindResult {
                run_id: record.run_id,
                index: record.index,
            });
        }
        self.features.result_inspection = Some(record);
    }
    pub(super) fn result_inspector(&mut self, ui: &mut egui::Ui) {
        // History and the result inspector share the bounded capture cache. Restore this
        // result after visiting History rather than displaying its newly selected capture.
        if let Some(record) = &self.features.result_inspection
            && self.selected != record.capture_id
            && self.features.result_error.is_empty()
        {
            self.inspect_result(record.clone());
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("← Attack template").clicked() {
                self.features.result_inspection = None;
            }
            if let Some(record) = &self.features.result_inspection {
                ui.strong(format!("Result #{}", record.index + 1));
                ui.weak(format!("Payload: {}", record.payloads.join(" | ")));
            }
        });
        let Some(record) = &self.features.result_inspection else {
            return;
        };
        if !record.error.is_empty() {
            ui.colored_label(ERROR, &record.error);
        }
        if !self.features.result_error.is_empty() {
            ui.colored_label(ERROR, &self.features.result_error);
        }
        if self.features.result_loading {
            ui.spinner();
            ui.label("Loading captured request and response…");
        } else if self.selected.is_some() {
            self.inspector(ui);
        } else {
            ui.weak("No HTTP response was captured for this attempt.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_result_tables_fill_the_available_width() {
        for latest in [true, false] {
            for width in [940.0, 1910.0] {
                let layout = columns(width, latest);
                let (_, _, x, span) = layout.last().unwrap();
                assert!((x + span - width).abs() < 1.0);
            }
        }
    }
    #[test]
    fn results_panel_does_not_grow_after_layout_settles() {
        for count in [0, 1, 128] {
            let ctx = egui::Context::default();
            let records = vec![Record::default(); count];
            let mut heights = Vec::new();
            for _ in 0..12 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1240.0, 860.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            let panel = egui::Panel::bottom("result-sizing")
                                .resizable(true)
                                .default_size(240.0)
                                .min_size(120.0)
                                .max_size(400.0)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.strong("Results");
                                        ui.button("Filter").clicked();
                                    });
                                    table(ui, &records, None, true, false, 1, false);
                                });
                            heights.push(panel.response.rect.height());
                        });
                    },
                );
                output.textures_delta.clear();
            }
            assert!(
                *heights.last().unwrap() <= 245.0,
                "results panel grew with {count} rows: {heights:?}"
            );
        }
    }
    #[test]
    fn payload_cell_click_selects_the_whole_result() {
        let ctx = egui::Context::default();
        let records = [Record {
            run_id: 7,
            index: 2,
            payloads: vec!["needle".into()],
            ..Default::default()
        }];
        let mut point = egui::pos2(180.0, 80.0);
        let mut clicked = false;
        for step in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 300.0),
                )),
                events: if step > 0 {
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed: step == 1,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                } else {
                    vec![]
                },
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                point = ui.cursor().min + egui::vec2(180.0, 48.0);
                clicked |= table(ui, &records, None, true, false, 1, false).selected == Some(0);
            });
            output.textures_delta.clear();
        }
        assert!(clicked);
    }
}

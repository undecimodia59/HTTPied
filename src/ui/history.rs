use super::*;

pub(super) struct Inspector {
    pub split: bool,
    ratio: f32,
    original: bool,
    sent: bool,
    mode: u8,
    key: String,
    request: String,
    response: String,
    request_find: String,
    response_find: String,
    request_next: usize,
    response_next: usize,
    note: Option<String>,
}
impl Default for Inspector {
    fn default() -> Self {
        Self {
            split: true,
            ratio: 0.5,
            original: false,
            sent: false,
            mode: 4,
            key: String::new(),
            request: String::new(),
            response: String::new(),
            request_find: String::new(),
            response_find: String::new(),
            request_next: 0,
            response_next: 0,
            note: None,
        }
    }
}
fn find(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &str,
    query: &mut String,
    next: &mut usize,
) -> Option<usize> {
    let mut jump = None;
    ui.horizontal(|ui| {
        let search = ui.add(
            egui::TextEdit::singleline(query)
                .hint_text("Find in message")
                .desired_width((ui.available_width() - 110.0).clamp(100.0, 220.0))
                .char_limit(256),
        );
        if search.changed() {
            *next = 0;
        }
        if ui
            .add_enabled(!query.is_empty(), egui::Button::new("Next"))
            .clicked()
            || (search.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
        {
            jump = syntax::find_next(ui, id, text, query, next);
        }
        if !query.is_empty() {
            ui.weak(format!("{} matches", text.matches(query.as_str()).count()));
        }
    });
    jump
}
impl Spike {
    pub(super) fn inspector(&mut self, ui: &mut egui::Ui) {
        if !self.inspect.split {
            if ui.button("← Request / response").clicked() {
                self.inspect.split = true;
            }
            self.single_inspector(ui);
            return;
        }
        let Some(entry) = self
            .history
            .entries
            .iter()
            .find(|entry| Some(entry.id) == self.selected)
        else {
            empty_state(
                ui,
                "Select a request to inspect it",
                "The executed request and response appear together. Right-click traffic to send it to another tool.",
            );
            return;
        };
        if !entry.loaded {
            ui.spinner();
            ui.weak("Loading saved capture…");
            return;
        }
        let Event::Exchange(execution) = &entry.event else {
            self.single_inspector(ui);
            return;
        };
        let id = entry.id;
        let key = format!(
            "{id}:{}:{}:{}:{}:{}:{}",
            execution.outgoing.len(),
            execution.response.len(),
            execution.response_outgoing.len(),
            self.inspect.original,
            self.inspect.sent,
            self.inspect.mode
        );
        if key != self.inspect.key {
            self.inspect.key = key;
            self.inspect.request = preview(
                if self.inspect.original {
                    &execution.original.0
                } else {
                    &execution.outgoing
                },
                PREVIEW,
            );
            if self.inspect.original && execution.original.0.is_empty() {
                self.inspect.request="Original HTTP/2 frames were not retained. Choose Executed request for the normalized message.".into();
            }
            let bytes = if self.inspect.sent && !execution.response_outgoing.is_empty() {
                &execution.response_outgoing
            } else {
                &execution.response
            };
            self.inspect.note = None;
            self.inspect.response = if bytes.is_empty() {
                self.inspect.note =
                    Some("No response captured. Send to Replay to execute this request.".into());
                String::new()
            } else {
                match self.inspect.mode {
                    0 => preview(bytes, PREVIEW),
                    1 => crate::exchange::pretty(&crate::exchange::auto_preview(bytes).text),
                    2 => hex_preview(bytes),
                    3 => crate::exchange::decoded(bytes)
                        .unwrap_or_else(|error| format!("Decoded view unavailable: {error}")),
                    _ => {
                        let view = crate::exchange::auto_preview(bytes);
                        self.inspect.note = view.note;
                        view.text
                    }
                }
            };
            self.pane = Pane::Response;
            self.text = self.inspect.response.clone();
        }
        let url = execution.parsed.target.clone();
        let metadata = format!(
            "{} · {} · {:.1} ms · {}{}",
            execution.parsed.method,
            if execution.response_status == 0 {
                "Not sent".into()
            } else {
                format!("HTTP {}", execution.response_status)
            },
            execution.elapsed_ms,
            crate::state::size(
                execution.response_total_bytes,
                self.features.state.settings.binary_units
            ),
            if execution.response_truncated {
                " · capture truncated"
            } else {
                ""
            }
        );
        let changed_response = !execution.response_outgoing.is_empty()
            && execution.response_outgoing != execution.response;
        let mut transfer = None;
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("#{id}"));
            ui.weak(metadata);
            if primary(ui, true, "Send to Replay").clicked() {
                transfer = Some(Tab::Replay);
            }
            ui.menu_button("More", |ui| {
                ui.weak("Message previews are bounded. Export captures for full saved bytes.");
                if ui.button("Send to Intruder").clicked() {
                    transfer = Some(Tab::Intruder);
                    ui.close();
                }
                if ui.button("Copy URL").clicked() {
                    ui.ctx().copy_text(url.clone());
                    ui.close();
                }
                for (label, action) in [
                    ("Set response as Compare baseline", "compare-baseline"),
                    ("Compare response with baseline", "compare-candidate"),
                ] {
                    if ui.button(label).clicked() {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new(action), self.inspect.response.clone())
                        });
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Capture variants & draft").clicked() {
                    self.inspect.split = false;
                    ui.close();
                }
            });
        });
        if let Some(tab) = transfer {
            self.transfer_history(id, tab);
        }
        let width = ui.available_width();
        resize_split_with_window(ui, egui::Id::new("history-request-pane"), width);
        let request_panel = egui::Panel::left("history-request-pane")
            .frame(egui::Frame::NONE)
            .resizable(true)
            .default_size(width * self.inspect.ratio)
            .size_range(200.0..=(width - 240.0).max(200.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("history-request-kind")
                        .selected_text(if self.inspect.original {
                            "Original request"
                        } else {
                            "Executed request"
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.inspect.original,
                                false,
                                "Executed request",
                            );
                            ui.selectable_value(
                                &mut self.inspect.original,
                                true,
                                "Original request",
                            );
                        });
                });
                let editor = egui::Id::new(("history-request", id));
                let jump = find(
                    ui,
                    editor,
                    &self.inspect.request,
                    &mut self.inspect.request_find,
                    &mut self.inspect.request_next,
                );
                let mut scroll = egui::ScrollArea::both()
                    .id_salt(("history-request-scroll", id))
                    .auto_shrink([false, false]);
                if let Some(line) = jump {
                    scroll = scroll.vertical_scroll_offset(
                        line as f32 * ui.text_style_height(&egui::TextStyle::Monospace),
                    );
                }
                scroll.show(ui, |ui| {
                    syntax::editor(ui, editor, &mut self.inspect.request, false, 24);
                });
            });
        self.inspect.ratio = (request_panel.response.rect.width() / width).clamp(0.25, 0.75);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Response");
                    egui::ComboBox::from_id_salt("history-response-mode")
                        .selected_text(match self.inspect.mode {
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
                                ui.selectable_value(&mut self.inspect.mode, mode, label);
                            }
                        });
                    if changed_response {
                        ui.checkbox(&mut self.inspect.sent, "Sent response");
                    }
                });
                let editor = egui::Id::new(("history-response", id));
                let jump = find(
                    ui,
                    editor,
                    &self.inspect.response,
                    &mut self.inspect.response_find,
                    &mut self.inspect.response_next,
                );
                if let Some(note) = &self.inspect.note {
                    ui.weak(note);
                }
                let mut scroll = egui::ScrollArea::both()
                    .id_salt(("history-response-scroll", id))
                    .auto_shrink([false, false]);
                if let Some(line) = jump {
                    scroll = scroll.vertical_scroll_offset(
                        line as f32 * ui.text_style_height(&egui::TextStyle::Monospace),
                    );
                }
                scroll.show(ui, |ui| {
                    syntax::editor(ui, editor, &mut self.inspect.response, false, 24);
                });
            });
    }
}

use super::*;
use crate::coder::{self, Codec};

#[derive(Default)]
pub(super) struct Coder {
    input: String,
    input_hex: bool,
    output: Vec<u8>,
    output_view: String,
    output_hex: bool,
    error: String,
    completed: String,
    codec: Option<Codec>,
    reset_editor: bool,
}
impl Coder {
    pub(super) fn load(&mut self, text: String) {
        self.input = text;
        self.input_hex = false;
        self.clear_output();
        self.reset_editor = true;
    }
    fn clear_output(&mut self) {
        self.output.clear();
        self.output_view.clear();
        self.error.clear();
        self.completed.clear();
    }
    fn refresh_output(&mut self) {
        self.output_view = if self.output_hex {
            coder::hex(&self.output[..self.output.len().min(PREVIEW / 2)])
        } else {
            // Text is shown only after validating UTF-8. Binary bytes never become replacements.
            preview(&self.output, PREVIEW)
        };
    }
    fn convert(&mut self, encode: bool) {
        self.clear_output();
        let input = if self.input_hex {
            coder::unhex(&self.input)
        } else {
            Ok(self.input.as_bytes().to_vec())
        };
        match input
            .and_then(|input| coder::transform(&input, self.codec.unwrap_or(Codec::Base64), encode))
        {
            Ok(output) => {
                self.output_hex = std::str::from_utf8(&output).is_err()
                    || output
                        .iter()
                        .any(|b| b.is_ascii_control() && !b"\r\n\t".contains(b));
                self.output = output;
                self.completed = format!(
                    "{} · {}",
                    self.codec.unwrap_or(Codec::Base64).label(),
                    if encode { "Encoded" } else { "Decoded" }
                );
                self.refresh_output();
            }
            Err(error) => self.error = error,
        }
    }
    fn use_output(&mut self) {
        if let Ok(text) = std::str::from_utf8(&self.output)
            && !self.output_hex
        {
            self.input = text.to_owned();
            self.input_hex = false;
        } else {
            self.input = coder::hex(&self.output);
            self.input_hex = true;
        }
        self.clear_output();
        self.reset_editor = true;
    }
    fn page(&mut self, ui: &mut egui::Ui) {
        if self.reset_editor {
            syntax::forget_editor(ui.ctx(), egui::Id::new("coder-input"));
            self.reset_editor = false;
        }
        ui.horizontal_wrapped(|ui| {
            ui.heading("Coder");
            ui.weak("Select text in an editor, right-click, then Send selection to Coder.");
        });
        ui.horizontal_wrapped(|ui| {
            let mut codec = self.codec.unwrap_or(Codec::Base64);
            egui::ComboBox::from_id_salt("coder-codec")
                .selected_text(codec.label())
                .show_ui(ui, |ui| {
                    for value in Codec::ALL {
                        ui.selectable_value(&mut codec, value, value.label());
                    }
                });
            if Some(codec) != self.codec {
                self.codec = Some(codec);
                self.clear_output();
            }
            if primary(ui, true, "Decode").clicked() {
                self.convert(false);
            }
            if ui.button("Encode").clicked() {
                self.convert(true);
            }
            if ui.button("Clear").clicked() {
                self.load(String::new());
            }
            ui.weak("128 KiB input · session only");
        });
        if !self.error.is_empty() {
            ui.colored_label(ERROR, &self.error);
        }
        let height = (ui.available_height() - 70.0).max(100.0);
        ui.columns(2, |columns| {
            columns[0].horizontal_wrapped(|ui| {
                ui.strong("Input");
                let before = self.input_hex;
                ui.checkbox(&mut self.input_hex, "Interpret as hex bytes");
                if before != self.input_hex {
                    self.clear_output();
                }
                ui.weak(format!("{} characters", self.input.chars().count()));
            });
            columns[0].weak("Text is UTF-8. Binary results stay as exact hex bytes.");
            egui::ScrollArea::both()
                .id_salt("coder-input-scroll")
                .max_height(height)
                .min_scrolled_height(height)
                .show(&mut columns[0], |ui| {
                    if syntax::editor_limited(
                        ui,
                        egui::Id::new("coder-input"),
                        &mut self.input,
                        true,
                        24,
                        4 * coder::INPUT_LIMIT,
                    )
                    .changed()
                    {
                        self.clear_output();
                    }
                });
            columns[1].horizontal_wrapped(|ui| {
                ui.strong("Output");
                let old = self.output_hex;
                ui.selectable_value(&mut self.output_hex, true, "Hex");
                ui.add_enabled_ui(std::str::from_utf8(&self.output).is_ok(), |ui| {
                    ui.selectable_value(&mut self.output_hex, false, "Text");
                });
                if old != self.output_hex {
                    self.refresh_output();
                }
                if ui
                    .add_enabled(
                        !self.completed.is_empty(),
                        egui::Button::new(if self.output_hex {
                            "Copy hex"
                        } else {
                            "Copy output"
                        }),
                    )
                    .clicked()
                {
                    ui.ctx().copy_text(if self.output_hex {
                        coder::hex(&self.output)
                    } else {
                        String::from_utf8(self.output.clone()).unwrap_or_default()
                    });
                }
                if ui
                    .add_enabled(
                        !self.completed.is_empty() && self.output.len() <= coder::INPUT_LIMIT,
                        egui::Button::new("Use as input"),
                    )
                    .clicked()
                {
                    self.use_output();
                }
            });
            columns[1].weak(format!(
                "{}{} bytes",
                if self.completed.is_empty() {
                    String::new()
                } else {
                    format!("{} · ", self.completed)
                },
                self.output.len()
            ));
            if self.output.len()
                > if self.output_hex {
                    PREVIEW / 2
                } else {
                    PREVIEW
                }
            {
                columns[1].weak("Preview limited to 64 KiB; Copy includes the full result.");
            }
            egui::ScrollArea::both()
                .id_salt("coder-output-scroll")
                .max_height(height)
                .min_scrolled_height(height)
                .show(&mut columns[1], |ui| {
                    syntax::editor(
                        ui,
                        egui::Id::new("coder-output"),
                        &mut self.output_view,
                        false,
                        24,
                    );
                });
        });
    }
}
impl Spike {
    pub(super) fn coder_page(&mut self, ui: &mut egui::Ui) {
        self.coder.page(ui);
    }
    pub(super) fn receive_coder_selection(&mut self, ctx: &egui::Context) {
        if let Some(text) = syntax::take_coder_selection(ctx) {
            self.coder.load(text);
            self.workflow.tab = Tab::Coder;
            ctx.request_repaint();
        }
        if let Some(text) =
            ctx.data_mut(|data| data.remove_temp::<String>(egui::Id::new("compare-baseline")))
        {
            self.features.compare_left = text;
            self.compare_view.editing = true;
            self.toast = Some(("Compare baseline set".into(), std::time::Instant::now()));
        }
        if let Some(text) =
            ctx.data_mut(|data| data.remove_temp::<String>(egui::Id::new("compare-candidate")))
        {
            self.features.compare_right = text;
            self.compare_view.editing = false;
            self.workflow.tab = Tab::Compare;
            ctx.request_repaint();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_chaining_is_lossless_and_failed_conversions_clear_old_output() {
        let mut coder = Coder::default();
        coder.load("/wA=".into());
        coder.convert(false);
        assert_eq!(coder.output, [255, 0]);
        assert!(coder.output_hex);
        coder.use_output();
        assert_eq!(coder.input, "ff00");
        assert!(coder.input_hex);
        coder.convert(true);
        assert_eq!(coder.output, b"/wA=");
        coder.load("not base64!".into());
        coder.convert(false);
        assert!(!coder.error.is_empty() && coder.output.is_empty() && coder.output_view.is_empty());
    }
}

use super::*;
use crate::coder::{self, Codec};

#[derive(Default)]
pub(super) struct Coder {
    jwt_mode: bool,
    jwt: super::jwt::Workbench,
    input: String,
    input_hex: bool,
    output: Vec<u8>,
    output_view: String,
    output_hex: bool,
    error: String,
    completed: String,
    codec: Option<Codec>,
    operation: coder::Operation,
    reset_editor: bool,
}
impl Coder {
    pub(super) fn load(&mut self, text: String) {
        if crate::jwt::parse(&text).is_ok() {
            self.jwt_mode = true;
            self.jwt.load(text);
            return;
        }
        self.jwt_mode = false;
        self.input = text;
        self.input_hex = false;
        self.clear_output();
        self.reset_editor = true;
    }
    pub(super) fn saved_workspace(&self) -> coder::Workspace {
        coder::Workspace {
            input: self.input.clone(),
            input_hex: self.input_hex,
            codec: self.codec.unwrap_or_default(),
            operation: self.operation,
            output_hex: self.output_hex,
            converted: !self.completed.is_empty() || !self.error.is_empty(),
        }
    }
    pub(super) fn restore(&mut self, saved: &coder::Workspace) {
        self.jwt_mode = false;
        self.input = saved.input.clone();
        self.input_hex = saved.input_hex;
        self.codec = Some(saved.codec);
        self.operation = saved.operation;
        self.clear_output();
        self.output_hex = saved.output_hex;
        if saved.converted {
            self.convert(saved.operation == coder::Operation::Encode);
            self.output_hex = saved.output_hex || std::str::from_utf8(&self.output).is_err();
            self.refresh_output();
        }
        self.reset_editor = true;
    }
    fn clear_input(&mut self) {
        self.input.clear();
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
        self.operation = if encode {
            coder::Operation::Encode
        } else {
            coder::Operation::Decode
        };
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
            ui.selectable_value(&mut self.jwt_mode, false, "Encoders");
            ui.selectable_value(&mut self.jwt_mode, true, "JWT workbench");
        });
        if self.jwt_mode {
            self.jwt.page(ui);
            return;
        }
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
            if (if self.operation == coder::Operation::Decode {
                primary(ui, true, "Decode")
            } else {
                ui.button("Decode")
            })
            .clicked()
            {
                self.convert(false);
            }
            if (if self.operation == coder::Operation::Encode {
                primary(ui, true, "Encode")
            } else {
                ui.button("Encode")
            })
            .clicked()
            {
                self.convert(true);
            }
            if ui.button("Clear").clicked() {
                self.clear_input();
            }
            ui.weak("128 KiB input · saved with project");
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
        if let Some((host, subdomains)) =
            ctx.data_mut(|data| data.remove_temp::<(String, bool)>(egui::Id::new("scope-add-host")))
        {
            match self.features.state.include_host(&host, subdomains) {
                Ok((host, changed)) => {
                    self.toast = Some((
                        format!(
                            "{} {}{}",
                            if changed {
                                "Added to scope:"
                            } else {
                                "Already in scope:"
                            },
                            host,
                            if subdomains { " + subdomains" } else { "" }
                        ),
                        std::time::Instant::now(),
                    ))
                }
                Err(error) => self.features.message = error,
            }
        }

        let transfer = ctx.data_mut(|data| {
            let key = egui::Id::new("request-transfer");
            let value = data.get_temp::<(bool, Result<syntax::RequestSource, String>)>(key);
            data.remove::<(bool, Result<syntax::RequestSource, String>)>(key);
            value
        });
        if let Some((intruder, source)) = transfer {
            match source {
                Ok(source) => {
                    self.workflow.load_draft(
                        source.destination,
                        source.request,
                        if intruder { Tab::Intruder } else { Tab::Replay },
                    );
                    if intruder && self.workflow.message.is_empty() {
                        self.features.result_inspection = None;
                    }
                }
                Err(error) => self.workflow.message = error,
            }
            ctx.request_repaint();
        }
        let attack_request = ctx.data_mut(|data| {
            let key = egui::Id::new("attack-request-transfer");
            let value = data.get_temp::<Result<syntax::RequestSource, String>>(key);
            data.remove::<Result<syntax::RequestSource, String>>(key);
            value
        });
        if let Some(source) = attack_request {
            match source {
                Ok(source) => {
                    let request = crate::state::SavedRequest {
                        name: "Request".into(),
                        group: String::new(),
                        timeout_secs: 0,
                        destination: source.destination,
                        request: source.request,
                    };
                    match self.attack.load(vec![request]) {
                        Ok(()) => self.workflow.tab = Tab::Attack,
                        Err(error) => self.workflow.message = error,
                    }
                }
                Err(error) => self.workflow.message = error,
            }
        }
        if let Some(requests) = ctx.data_mut(|data| {
            data.remove_temp::<Vec<crate::state::SavedRequest>>(egui::Id::new(
                "attack-group-transfer",
            ))
        }) {
            match self.attack.load(requests) {
                Ok(()) => self.workflow.tab = Tab::Attack,
                Err(error) => self.workflow.message = error,
            }
        }
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
    fn restored_workspace_keeps_input_codec_direction_and_binary_output() {
        let mut coder = Coder {
            input: "68 69".into(),
            input_hex: true,
            codec: Some(Codec::Base64Url),
            ..Default::default()
        };
        coder.convert(true);
        coder.output_hex = true;
        coder.refresh_output();
        let saved: crate::state::ProjectState = serde_json::from_str(
            &serde_json::to_string(&crate::state::ProjectState {
                coder_workspace: coder.saved_workspace(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let mut restored = Coder::default();
        restored.restore(&saved.coder_workspace);
        assert_eq!(restored.input, "68 69");
        assert!(restored.input_hex && restored.output_hex);
        assert_eq!(restored.codec, Some(Codec::Base64Url));
        assert_eq!(restored.operation, coder::Operation::Encode);
        assert_eq!(restored.output, b"aGk");
        restored.clear_input();
        assert!(restored.input.is_empty() && restored.output.is_empty());
        assert!(restored.input_hex);
        assert_eq!(restored.operation, coder::Operation::Encode);
        coder.input = "ff0061".into();
        coder.input_hex = false;
        coder.codec = Some(Codec::Hex);
        coder.convert(false);
        restored.restore(&coder.saved_workspace());
        assert_eq!(restored.output, vec![255, 0, 97]);
        assert!(restored.output_hex);
        assert_eq!(restored.operation, coder::Operation::Decode);
        coder.input = "edited but not converted".into();
        coder.clear_output();
        restored.restore(&coder.saved_workspace());
        assert!(restored.output.is_empty());
        assert_eq!(restored.input, "edited but not converted");
    }
    #[test]
    fn old_projects_open_with_default_coder_preferences() {
        let state: crate::state::ProjectState = serde_json::from_str("{}").unwrap();
        let mut coder = Coder::default();
        coder.restore(&state.coder_workspace);
        assert!(coder.input.is_empty() && coder.output.is_empty());
        assert_eq!(coder.codec, Some(Codec::Base64));
        assert_eq!(coder.operation, coder::Operation::Decode);
    }
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

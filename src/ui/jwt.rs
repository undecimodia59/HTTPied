use super::*;
use crate::jwt::{self, Algorithm};
#[derive(Default)]
pub(super) struct Workbench {
    token: String,
    header: String,
    claims: String,
    key: String,
    key_hex: bool,
    algorithm: Algorithm,
    output: String,
    status: String,
    error: String,
    decoded: bool,
    reset_editors: bool,
}
impl Workbench {
    pub(super) fn load(&mut self, text: String) {
        self.token = text;
        self.decode();
    }
    fn decode(&mut self) {
        self.reset_editors = true;
        self.error.clear();
        self.status.clear();
        self.output.clear();
        self.decoded = false;
        match jwt::parse(&self.token) {
            Ok(parsed) => {
                self.header = serde_json::to_string_pretty(&parsed.header).unwrap_or_default();
                self.claims = serde_json::to_string_pretty(&parsed.claims).unwrap_or_default();
                self.decoded = true;
                self.status = format!(
                    "Decoded · {} signature bytes · signature not verified",
                    parsed.signature.len()
                );
            }
            Err(error) => {
                self.header.clear();
                self.claims.clear();
                self.error = error;
            }
        }
    }
    fn key_bytes(&self) -> Result<Vec<u8>, String> {
        if self.key_hex {
            crate::coder::unhex(&self.key)
        } else {
            Ok(self.key.as_bytes().to_vec())
        }
    }
    pub(super) fn page(&mut self, ui: &mut egui::Ui) {
        if self.reset_editors {
            for index in 0..2 {
                syntax::forget_editor(ui.ctx(), egui::Id::new(("jwt-json", index)));
            }
            self.reset_editors = false;
        }

        ui.weak("Local JWT inspection and HMAC signing. Keys and tokens stay in this session. Decoding does not verify a signature.");
        ui.label("Input token");
        if ui
            .add(
                egui::TextEdit::multiline(&mut self.token)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(3)
                    .char_limit(jwt::LIMIT)
                    .hint_text("eyJ...eyJ...signature"),
            )
            .changed()
        {
            self.status = "Input changed; Decode to refresh the claims.".into();
            self.error.clear();
            self.output.clear();
            self.decoded = false;
        }
        ui.horizontal_wrapped(|ui|{
            if primary(ui,!self.token.trim().is_empty(),"Decode").clicked(){self.decode();}
            let before=self.algorithm;
            egui::ComboBox::from_id_salt("jwt-algorithm").selected_text(self.algorithm.label()).show_ui(ui,|ui|{for alg in Algorithm::ALL{ui.selectable_value(&mut self.algorithm,alg,alg.label());}});
            if before!=self.algorithm{self.status.clear();self.error.clear();self.output.clear();}
            ui.label("Key");
            let key_changed=ui.add(egui::TextEdit::singleline(&mut self.key).password(true).desired_width(210.0).char_limit(16384).hint_text("HMAC secret")).changed();
            let hex_changed=ui.checkbox(&mut self.key_hex,"Hex key").changed();
            if key_changed||hex_changed{self.status.clear();self.error.clear();self.output.clear();}
            if ui.button("Verify input signature").clicked(){
                self.error.clear();self.status.clear();
                match self.key_bytes().and_then(|key|jwt::verify(&self.token,&key,self.algorithm)){
                    Ok(())=>self.status="Input signature matches this key. Check expiry, issuer and audience separately.".into(),
                    Err(error)=>self.error=error,
                }
            }
        });
        if let Ok(parsed) = jwt::parse(&self.token) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            ui.horizontal_wrapped(|ui| {
                ui.weak(format!(
                    "Declared alg: {}",
                    parsed
                        .header
                        .get("alg")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("missing")
                ));
                for (claim, label) in [
                    ("exp", "Expiry"),
                    ("nbf", "Not before"),
                    ("iat", "Issued at"),
                ] {
                    if let Some(value) = parsed.claims.get(claim) {
                        if let Some(timestamp) = value.as_i64() {
                            let detail = if timestamp <= now {
                                format!("{} s ago", now.saturating_sub(timestamp))
                            } else {
                                format!("in {} s", timestamp.saturating_sub(now))
                            };
                            let color = if (claim == "exp" && timestamp <= now)
                                || (claim == "nbf" && timestamp > now)
                            {
                                WARN
                            } else {
                                MUTED
                            };
                            ui.colored_label(color, format!("{label}: {timestamp} ({detail})"));
                        } else {
                            ui.colored_label(
                                WARN,
                                format!("{claim} is not an integer NumericDate"),
                            );
                        }
                    }
                }
            });
        }
        if !self.error.is_empty() {
            ui.colored_label(ERROR, &self.error);
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
        let height = (ui.available_height() * 0.48).max(90.0);
        let mut changed = false;
        ui.columns(2, |columns| {
            for (index, (column, text)) in columns
                .iter_mut()
                .zip([&mut self.header, &mut self.claims])
                .enumerate()
            {
                column.horizontal_wrapped(|ui| {
                    ui.strong(if index == 0 {
                        "Protected header"
                    } else {
                        "Claims"
                    });
                    syntax::wrap_control(ui, egui::Id::new(("jwt-json", index)));
                });
                egui::ScrollArea::both()
                    .id_salt(("jwt-json-scroll", index))
                    .max_height(height)
                    .min_scrolled_height(height)
                    .show(column, |ui| {
                        changed |= syntax::editor_limited(
                            ui,
                            egui::Id::new(("jwt-json", index)),
                            text,
                            self.decoded,
                            9,
                            jwt::LIMIT,
                        )
                        .changed();
                    });
            }
        });
        if changed {
            self.output.clear();
            self.error.clear();
            self.status =
                "JSON edited. Build a new token below; the input token is unchanged.".into();
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.decoded, egui::Button::new("Sign with key"))
                .clicked()
            {
                self.error.clear();
                match self
                    .key_bytes()
                    .and_then(|key| jwt::sign(&self.header, &self.claims, &key, self.algorithm))
                {
                    Ok(output) => {
                        self.output = output;
                        self.status = format!("Built new {} token", self.algorithm.label());
                    }
                    Err(error) => {
                        self.output.clear();
                        self.error = error;
                    }
                }
            }
            if ui
                .add_enabled(
                    self.decoded,
                    egui::Button::new("Create unsigned (alg=none)"),
                )
                .clicked()
            {
                self.error.clear();
                match jwt::unsigned(&self.header, &self.claims) {
                    Ok(output) => {
                        self.output = output;
                        self.status = "Built unsigned test token; it has no signature.".into();
                    }
                    Err(error) => {
                        self.output.clear();
                        self.error = error;
                    }
                }
            }
            if ui
                .add_enabled(!self.output.is_empty(), egui::Button::new("Copy token"))
                .clicked()
            {
                ui.ctx().copy_text(self.output.clone());
            }
            if ui
                .add_enabled(!self.output.is_empty(), egui::Button::new("Use as input"))
                .clicked()
            {
                self.token = self.output.clone();
                self.decode();
            }
        });
        ui.strong("Built token");
        egui::ScrollArea::both()
            .id_salt("jwt-output-scroll")
            .max_height(ui.available_height().max(45.0))
            .show(ui, |ui| {
                syntax::editor(ui, egui::Id::new("jwt-output"), &mut self.output, false, 3);
            });
    }
}

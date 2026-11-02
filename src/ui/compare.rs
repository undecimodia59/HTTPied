use super::*;

#[derive(Default)]
pub(super) struct CompareView {
    pub editing: bool,
    fingerprint: String,
    left: String,
    right: String,
    left_colors: Vec<egui::Color32>,
    right_colors: Vec<egui::Color32>,
    changes: Vec<usize>,
    current: usize,
    jump: bool,
    error: String,
}
fn aligned(diff: &str) -> Vec<(Option<String>, Option<String>)> {
    let lines = diff.lines().collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        if let Some(text) = lines[at].strip_prefix("  ") {
            rows.push((Some(text.into()), Some(text.into())));
            at += 1;
        } else {
            let mut left = Vec::new();
            let mut right = Vec::new();
            while at < lines.len() && !lines[at].starts_with("  ") {
                if let Some(text) = lines[at].strip_prefix("- ") {
                    left.push(text.to_owned());
                }
                if let Some(text) = lines[at].strip_prefix("+ ") {
                    right.push(text.to_owned());
                }
                at += 1;
            }
            for index in 0..left.len().max(right.len()) {
                rows.push((left.get(index).cloned(), right.get(index).cloned()));
            }
        }
    }
    rows
}
impl CompareView {
    fn rebuild(&mut self, left: &str, right: &str, ignored: &str, hex: bool) {
        let fingerprint = format!("{left:?}{right:?}{ignored:?}{hex}");
        if fingerprint == self.fingerprint {
            return;
        }
        self.fingerprint = fingerprint;
        self.left.clear();
        self.right.clear();
        self.left_colors.clear();
        self.right_colors.clear();
        self.changes.clear();
        self.error.clear();
        self.current = 0;
        let diff = crate::exchange::diff(
            &crate::exchange::comparison_view(left, ignored, hex),
            &crate::exchange::comparison_view(right, ignored, hex),
        );
        if diff.starts_with('[') || diff.contains("\n[Diff truncated]") {
            self.error = "Comparison exceeds the bounded view. Narrow the inputs to 2,048 lines and 256 KiB per side.".into();
            return;
        }
        let mut left_no = 0;
        let mut right_no = 0;
        for (index, (left, right)) in aligned(&diff).into_iter().enumerate() {
            let changed = left != right;
            if changed {
                self.changes.push(index);
            }
            let left_color = if changed && left.is_some() {
                egui::Color32::from_rgba_unmultiplied(179, 61, 76, 65)
            } else {
                egui::Color32::TRANSPARENT
            };
            let right_color = if changed && right.is_some() {
                egui::Color32::from_rgba_unmultiplied(50, 143, 104, 65)
            } else {
                egui::Color32::TRANSPARENT
            };
            if let Some(line) = left {
                left_no += 1;
                self.left.push_str(&format!("{left_no:>4}  {line}\n"));
            } else {
                self.left.push_str("      \n");
            }
            if let Some(line) = right {
                right_no += 1;
                self.right.push_str(&format!("{right_no:>4}  {line}\n"));
            } else {
                self.right.push_str("      \n");
            }
            self.left_colors.push(left_color);
            self.right_colors.push(right_color);
        }
        self.jump = !self.changes.is_empty();
    }
}
impl Spike {
    pub(super) fn compare_page(&mut self, ui: &mut egui::Ui) {
        if self.features.compare_left.is_empty() && self.features.compare_right.is_empty() {
            self.compare_view.editing = true;
        }
        ui.horizontal_wrapped(|ui| {
            ui.heading("Compare");
            if self.compare_view.editing {
                if primary(
                    ui,
                    !self.features.compare_left.is_empty()
                        || !self.features.compare_right.is_empty(),
                    "Compare inputs",
                )
                .clicked()
                {
                    self.compare_view.editing = false;
                }
            } else if ui.button("Edit inputs").clicked() {
                self.compare_view.editing = true;
            }
            if ui.button("Swap sides").clicked() {
                std::mem::swap(
                    &mut self.features.compare_left,
                    &mut self.features.compare_right,
                );
            }
            ui.menu_button("Options", |ui| {
                ui.label("Ignore response headers (comma separated)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.features.ignore_headers)
                        .desired_width(240.0)
                        .char_limit(512),
                );
                ui.checkbox(&mut self.features.compare_hex, "Compare body as hex");
            });
            if !self.compare_view.editing {
                let count = self.compare_view.changes.len();
                if ui
                    .add_enabled(count > 0, egui::Button::new("Previous change"))
                    .clicked()
                {
                    self.compare_view.current = (self.compare_view.current + count - 1) % count;
                    self.compare_view.jump = true;
                }
                if ui
                    .add_enabled(count > 0, egui::Button::new("Next change"))
                    .clicked()
                {
                    self.compare_view.current = (self.compare_view.current + 1) % count;
                    self.compare_view.jump = true;
                }
            }
        });
        if self.compare_view.editing {
            ui.weak("Paste two messages below, or use a message editor's context menu to set a baseline and compare another message.");
        } else {
            self.compare_view.rebuild(
                &self.features.compare_left,
                &self.features.compare_right,
                &self.features.ignore_headers,
                self.features.compare_hex,
            );
            if self.compare_view.error.is_empty() {
                ui.horizontal(|ui| {
                    if self.compare_view.changes.is_empty() {
                        ui.colored_label(
                            egui::Color32::from_rgb(133, 211, 168),
                            "Messages are identical after applying comparison options",
                        );
                    } else {
                        ui.strong(format!("{} changed lines", self.compare_view.changes.len()));
                        ui.colored_label(ERROR, "− Baseline");
                        ui.colored_label(egui::Color32::from_rgb(133, 211, 168), "+ Candidate");
                    }
                    ui.weak("Views scroll together");
                });
            } else {
                ui.colored_label(WARN, &self.compare_view.error);
            }
        }
        ui.separator();
        ui.columns(2, |cols| {
            for (ui, (name, bytes)) in cols.iter_mut().zip([
                ("Baseline", self.features.compare_left.len()),
                ("Candidate", self.features.compare_right.len()),
            ]) {
                ui.horizontal(|ui| {
                    ui.strong(name);
                    ui.weak(crate::state::size(
                        bytes as u64,
                        self.features.state.settings.binary_units,
                    ));
                });
            }
        });
        let height = ui.available_height().max(100.0);
        let mut scroll = egui::ScrollArea::both()
            .id_salt(("compare-shared", self.compare_view.editing))
            .auto_shrink([false, false]);
        if self.compare_view.jump {
            if let Some(row) = self.compare_view.changes.get(self.compare_view.current) {
                scroll = scroll.vertical_scroll_offset(
                    *row as f32 * ui.text_style_height(&egui::TextStyle::Monospace),
                );
            }
            self.compare_view.jump = false;
        }
        scroll.max_height(height).show(ui, |ui| {
            ui.columns(2, |cols| {
                if self.compare_view.editing {
                    for (index, (col, text)) in cols
                        .iter_mut()
                        .zip([
                            &mut self.features.compare_left,
                            &mut self.features.compare_right,
                        ])
                        .enumerate()
                    {
                        syntax::editor_limited(
                            col,
                            egui::Id::new(("compare-input", index)),
                            text,
                            true,
                            30,
                            262144,
                        );
                    }
                } else {
                    syntax::diff_editor(
                        &mut cols[0],
                        egui::Id::new("compare-baseline-diff"),
                        &mut self.compare_view.left,
                        &self.compare_view.left_colors,
                    );
                    syntax::diff_editor(
                        &mut cols[1],
                        egui::Id::new("compare-candidate-diff"),
                        &mut self.compare_view.right,
                        &self.compare_view.right_colors,
                    );
                }
            });
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn insertions_deletions_and_unicode_stay_aligned() {
        let rows = aligned(&crate::exchange::diff(
            "same\nold\nКиїв\nend",
            "same\nnew\nextra\nКиїв\nend",
        ));
        assert_eq!(rows[1], (Some("old".into()), Some("new".into())));
        assert_eq!(rows[2], (None, Some("extra".into())));
        assert_eq!(rows[3], (Some("Київ".into()), Some("Київ".into())));
        let mut view = CompareView::default();
        view.rebuild("same\nold", "same\nnew", "", false);
        assert_eq!(view.changes.len(), 1);
        assert_eq!(view.left.lines().count(), view.right.lines().count());
        view.rebuild("same", "same", "", false);
        assert!(view.changes.is_empty());
        view.rebuild("", "added", "", false);
        assert_eq!(view.changes.len(), 1);
        assert!(view.jump);
        view.rebuild(&"a\n".repeat(2050), "b", "", false);
        assert!(!view.error.is_empty());
    }
}

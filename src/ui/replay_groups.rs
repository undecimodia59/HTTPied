use super::*;

impl Workflow {
    pub(in crate::ui) fn save_group_state(&self, state: &mut crate::state::ProjectState) {
        state.replay_groups = self.replay_groups.clone();
        state.selected_replay = self.selected_replay;
    }
    fn groups(&self) -> Vec<String> {
        let mut groups = self.replay_groups.clone();
        for tool in &self.replays {
            if !tool.group.is_empty() && !groups.contains(&tool.group) {
                groups.push(tool.group.clone());
            }
        }
        groups
    }
    fn move_selection(&mut self, group: &str) {
        let selected = self.replays[self.selected_replay].editor_id;
        for tool in &mut self.replays {
            if self.replay_selection.contains(&tool.editor_id)
                || (self.replay_selection.is_empty() && tool.editor_id == selected)
            {
                tool.group = group.into();
            }
        }
    }
    fn create_group(&mut self) {
        let name = self.new_group.trim().to_owned();
        if !name.is_empty()
            && name.len() <= 64
            && !self.groups().contains(&name)
            && self.replay_groups.len() < 64
        {
            self.replay_groups.push(name.clone());
            self.move_selection(&name);
            self.new_group.clear();
        }
    }
    pub(super) fn replay_sessions(&mut self, ui: &mut egui::Ui) {
        let groups = self.groups();
        let active = self.active.is_some();
        egui::Panel::left("replay-collections")
            .resizable(true)
            .default_size(205.0)
            .size_range(160.0..=340.0)
            .show(ui, |ui| {
                if active { ui.disable(); }
                ui.strong("Replay requests");
                ui.add(
                    egui::TextEdit::singleline(&mut self.replay_search)
                        .hint_text("Find request or group")
                        .desired_width(f32::INFINITY)
                        .char_limit(128),
                );
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_group)
                            .hint_text("New group")
                            .desired_width((ui.available_width() - 40.0).max(70.0))
                            .char_limit(64),
                    );
                    if ui
                        .button("+")
                        .on_hover_text("Create group and move selected requests into it")
                        .clicked()
                    {
                        self.create_group();
                    }
                });
                let search = self.replay_search.to_lowercase();
                let mut moved = None;
                let mut renamed: Option<(String, String)> = None;
                let mut removed = None;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for group in std::iter::once(String::new()).chain(groups.iter().cloned()) {
                            let members: Vec<_> = self
                                .replays
                                .iter()
                                .enumerate()
                                .filter(|(_, tool)| tool.group == group)
                                .filter(|(_, tool)| {
                                    search.is_empty()
                                        || format!(
                                            "{} {} {} {}",
                                            tool.name,
                                            tool.group,
                                            tool.destination,
                                            tool.request.lines().next().unwrap_or("")
                                        )
                                        .to_lowercase()
                                        .contains(&search)
                                })
                                .map(|(i, tool)| {
                                    (
                                        i,
                                        tool.editor_id,
                                        tool.name.clone(),
                                        tool.request != tool.baseline
                                            || tool.destination != tool.baseline_destination,
                                    )
                                })
                                .collect();
                            if !search.is_empty() && members.is_empty() {
                                continue;
                            }
                            let name = if group.is_empty() {
                                "Ungrouped"
                            } else {
                                &group
                            };
                            let header =
                                egui::CollapsingHeader::new(format!("{name}  {}", members.len()))
                                    .id_salt(("replay-group", &group))
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        for (index, id, name, changed) in members {
                                            let title = format!(
                                                "{}{}",
                                                if name == "Request" {
                                                    format!("Request {}", index + 1)
                                                } else {
                                                    name
                                                },
                                                if changed { " •" } else { "" }
                                            );
                                            let selected = index == self.selected_replay
                                                || self.replay_selection.contains(&id);
                                            let response = ui.add(
                                                egui::Button::new(title)
                                                    .selected(selected)
                                                    .sense(egui::Sense::click_and_drag()),
                                            );
                                            response.dnd_set_drag_payload(id);
                                            if response.clicked() {
                                                let modifiers = ui.input(|input| input.modifiers);
                                                if modifiers.shift {
                                                    let from = self.selected_replay.min(index);
                                                    let to = self.selected_replay.max(index);
                                                    for tool in &self.replays[from..=to] {
                                                        self.replay_selection
                                                            .insert(tool.editor_id);
                                                    }
                                                } else if modifiers.ctrl {
                                                    if !self.replay_selection.remove(&id) {
                                                        self.replay_selection.insert(id);
                                                    }
                                                } else {
                                                    self.replay_selection.clear();
                                                }
                                                self.selected_replay = index;
                                            }
                                            response.context_menu(|ui| {
                                                if ui.button("Select request").clicked() {
                                                    self.selected_replay = index;
                                                    ui.close();
                                                }
                                                ui.menu_button("Move to group", |ui| {
                                                    for target in std::iter::once(String::new())
                                                        .chain(groups.iter().cloned())
                                                    {
                                                        if ui
                                                            .button(if target.is_empty() {
                                                                "Ungrouped"
                                                            } else {
                                                                &target
                                                            })
                                                            .clicked()
                                                        {
                                                            moved = Some((id, target));
                                                            ui.close();
                                                        }
                                                    }
                                                });
                                            });
                                        }
                                    });
                            if let Some(id) = header.header_response.dnd_release_payload::<u64>() {
                                moved = Some((*id, group.clone()));
                            }
                            if !group.is_empty() {
                                header.header_response.context_menu(|ui| {
                                    let rename_key = ui.id().with(("rename-group", &group));
                                    let mut name = ui
                                        .ctx()
                                        .data(|data| data.get_temp::<String>(rename_key))
                                        .unwrap_or_else(|| group.clone());
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut name)
                                                .char_limit(64)
                                                .desired_width(130.0),
                                        );
                                        if ui.button("Rename").clicked() {
                                            renamed = Some((group.clone(), name.trim().into()));
                                            ui.close();
                                        }
                                    });
                                    ui.ctx().data_mut(|data| data.insert_temp(rename_key, name));
                                    if ui.button("Remove group; keep requests").clicked() {
                                        removed = Some(group.clone());
                                        ui.close();
                                    }
                                });
                            }
                        }
                    });
                if let Some((id, target)) = moved {
                    if self.replay_selection.contains(&id) {
                        self.move_selection(&target);
                    } else if let Some(tool) =
                        self.replays.iter_mut().find(|tool| tool.editor_id == id)
                    {
                        tool.group = target;
                    }
                }
                if let Some((old, new)) = renamed
                    && !new.is_empty()
                    && new.len() <= 64
                    && !groups.contains(&new)
                {
                    for tool in &mut self.replays {
                        if tool.group == old {
                            tool.group = new.clone();
                        }
                    }
                    self.replay_groups.retain(|group| group != &old);
                    self.replay_groups.push(new);
                }
                if let Some(old) = removed {
                    for tool in &mut self.replays {
                        if tool.group == old {
                            tool.group.clear();
                        }
                    }
                    self.replay_groups.retain(|group| group != &old);
                }
            });
        ui.scope(|ui| {
            if active { ui.disable(); }
            ui.horizontal_wrapped(|ui| {
            ui.strong(&self.replays[self.selected_replay].name);
            if ui
                .add_enabled(
                    self.replays.len() < crate::state::MAX_REPLAYS,
                    egui::Button::new("+ New"),
                )
                .clicked()
            {
                let mut tool = Tool::new();
                tool.group = self.replays[self.selected_replay].group.clone();
                self.replays.push(tool);
                self.selected_replay = self.replays.len() - 1;
                self.replay_selection.clear();
            }
            ui.menu_button("Session", |ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.replays[self.selected_replay].name)
                        .char_limit(64)
                        .desired_width(180.0),
                );
                ui.menu_button("Move selection to group", |ui| {
                    for group in std::iter::once(String::new()).chain(groups.iter().cloned()) {
                        if ui
                            .button(if group.is_empty() {
                                "Ungrouped"
                            } else {
                                &group
                            })
                            .clicked()
                        {
                            self.move_selection(&group);
                            ui.close();
                        }
                    }
                });
                if ui
                    .add_enabled(
                        self.replays.len() < crate::state::MAX_REPLAYS,
                        egui::Button::new("Duplicate request"),
                    )
                    .clicked()
                {
                    let current = &self.replays[self.selected_replay];
                    let mut copy = Tool::new();
                    copy.destination = current.destination.clone();
                    copy.request = current.request.clone();
                    copy.name = format!("{} copy", current.name);
                    copy.group = current.group.clone();
                    copy.timeout_secs = current.timeout_secs;
                    self.replays.push(copy);
                    self.selected_replay = self.replays.len() - 1;
                    ui.close();
                }
                for (label, direction) in [("Move up", -1), ("Move down", 1)] {
                    let next = self.selected_replay.saturating_add_signed(direction);
                    if ui
                        .add_enabled(
                            next != self.selected_replay && next < self.replays.len(),
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        self.replays.swap(self.selected_replay, next);
                        self.selected_replay = next;
                        ui.close();
                    }
                }
                if ui
                    .add_enabled(self.replays.len() > 1, egui::Button::new("Close request"))
                    .clicked()
                {
                    let id = self.replays[self.selected_replay].editor_id;
                    syntax::forget_editor(ui.ctx(), egui::Id::new(("request", id)));
                    syntax::forget_editor(ui.ctx(), egui::Id::new(("response", id)));
                    self.replay_selection.remove(&id);
                    self.replays.remove(self.selected_replay);
                    self.selected_replay = self.selected_replay.min(self.replays.len() - 1);
                    ui.close();
                }
            });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_and_selected_request_survive_reopen_without_losing_drafts() {
        let mut workflow = Workflow::offline("127.0.0.1:8080".parse().unwrap());
        workflow.load(
            b"GET http://local/one HTTP/1.1\r\nHost: local\r\n\r\n",
            Tab::Replay,
        );
        workflow.new_group = "Authentication".into();
        workflow.create_group();
        let mut state = crate::state::ProjectState {
            replays: workflow.saved_requests(),
            ..Default::default()
        };
        workflow.save_group_state(&mut state);
        let state = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        let mut reopened = Workflow::offline("127.0.0.1:8080".parse().unwrap());
        reopened.restore(&state);
        assert_eq!(
            reopened.replays[reopened.selected_replay].group,
            "Authentication"
        );
        assert!(
            reopened.replays[reopened.selected_replay]
                .request
                .contains("/one")
        );
        assert_eq!(reopened.groups(), vec!["Authentication"]);
        reopened.move_selection("");
        assert!(reopened.replays[reopened.selected_replay].group.is_empty());
    }
}

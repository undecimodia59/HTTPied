//! Display-only highlighting. Layout text is always identical to the supplied editor text.
use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};
#[derive(Clone)]
pub(super) struct RequestSource {
    pub destination: String,
    pub request: String,
}
pub(super) fn register_request(
    ui: &egui::Ui,
    id: egui::Id,
    source: Option<Result<RequestSource, String>>,
) {
    ui.ctx().data_mut(|data| {
        data.remove::<Result<RequestSource, String>>(id.with("request-source"));
        if let Some(source) = source {
            data.insert_temp(id.with("request-source"), source);
        }
    });
}
/// Target metadata is independent of the displayed bytes, especially for responses.
pub(super) fn register_scope(ui: &egui::Ui, id: egui::Id, target: Option<&str>) {
    let host = target
        .and_then(|target| url::Url::parse(target).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .and_then(|url| url.host().map(|host| host.to_string()));
    ui.ctx().data_mut(|data| {
        data.remove::<String>(id.with("scope-host"));
        if let Some(host) = host {
            data.insert_temp(id.with("scope-host"), host);
        }
    });
}
pub(super) fn scope_menu(ui: &mut egui::Ui, host: &str) {
    ui.menu_button("Add to scope", |ui| {
        ui.monospace(host);
        let domain = matches!(url::Host::parse(host), Ok(url::Host::Domain(_)));
        scope_option(ui, "Host only", host, false, true);
        scope_option(ui, "Host + subdomains", host, true, domain);
        if let Some(site) = crate::state::site_domain(host)
            && !site.eq_ignore_ascii_case(host.trim_end_matches('.'))
        {
            ui.separator();
            ui.weak("Site domain");
            ui.monospace(&site);
            scope_option(ui, "Site domain only", &site, false, true);
            scope_option(ui, "Site domain + subdomains", &site, true, true);
        }
    });
}
fn scope_option(ui: &mut egui::Ui, label: &str, host: &str, subdomains: bool, enabled: bool) {
    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("scope-add-host"),
                (host.to_owned(), subdomains),
            )
        });
        ui.close();
    }
}
pub(super) fn set_default_wrap(ctx: &egui::Context, value: bool) {
    ctx.data_mut(|data| {
        let key = egui::Id::new("editor-wrap-default");
        let old = data.get_temp::<bool>(key);
        if old != Some(value) {
            let generation = data.get_temp::<u64>(key.with("generation")).unwrap_or(0) + 1;
            data.insert_temp(key.with("generation"), generation);
        }
        data.insert_temp(key, value);
    });
}
fn wrapping(ui: &egui::Ui, id: egui::Id) -> bool {
    ui.ctx().data(|data| {
        let key = egui::Id::new("editor-wrap-default");
        let generation = data.get_temp::<u64>(key.with("generation")).unwrap_or(0);
        data.get_temp::<(u64, bool)>(id.with("wrap"))
            .filter(|(version, _)| *version == generation)
            .map(|(_, value)| value)
            .unwrap_or_else(|| data.get_temp::<bool>(key).unwrap_or(false))
    })
}
pub(super) fn wrap_control(ui: &mut egui::Ui, id: egui::Id) {
    let mut wrap = wrapping(ui, id);
    if ui.checkbox(&mut wrap, "Wrap").changed() {
        ui.ctx().data_mut(|data| {
            let generation = data
                .get_temp::<u64>(egui::Id::new("editor-wrap-default").with("generation"))
                .unwrap_or(0);
            data.insert_temp(id.with("wrap"), (generation, wrap));
        });
    }
}
const TEXT: Color32 = Color32::from_rgb(220, 220, 220);
const KEY: Color32 = Color32::from_rgb(133, 185, 235);
const VALUE: Color32 = Color32::from_rgb(213, 176, 128);
const COMMENT: Color32 = Color32::from_rgb(132, 165, 123);
const PUNCT: Color32 = Color32::from_rgb(170, 170, 170);
#[derive(Default)]
struct UndoHistory {
    past: std::collections::VecDeque<String>,
    future: Vec<String>,
    last: String,
    changed_at: f64,
}
impl UndoHistory {
    fn record(&mut self, text: &str, now: f64, boundary: bool) {
        if self.last == text {
            return;
        }
        if boundary || self.past.is_empty() || now - self.changed_at > 0.4 {
            self.past.push_back(self.last.clone());
        }
        self.future.clear();
        self.last = text.into();
        self.changed_at = now;
        while self.past.len() > 64
            || self.past.iter().map(String::len).sum::<usize>() > 2 * 1024 * 1024
        {
            self.past.pop_front();
        }
    }
    fn undo(&mut self) -> Option<String> {
        let value = self.past.pop_back()?;
        self.future
            .push(std::mem::replace(&mut self.last, value.clone()));
        Some(value)
    }
    fn redo(&mut self) -> Option<String> {
        let value = self.future.pop()?;
        self.past
            .push_back(std::mem::replace(&mut self.last, value.clone()));
        Some(value)
    }
}
fn append(job: &mut LayoutJob, text: &str, color: Color32) {
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: FontId::monospace(13.0),
            color,
            ..Default::default()
        },
    );
}
pub fn layout(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let split = text
        .find("\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| text.find("\n\n").map(|i| i + 2));
    if !text.starts_with("HTTP/")
        && !text.lines().next().is_some_and(|line| {
            line.ends_with("HTTP/1.1")
                || line.ends_with("HTTP/1.0")
                || line.trim_end_matches('\r').ends_with("HTTP/1.1")
        })
    {
        body(&mut job, text);
        return job;
    }
    let (head, content) = split.map_or((text, ""), |i| text.split_at(i));
    for (index, line) in head.split_inclusive('\n').enumerate() {
        if index == 0 {
            if let Some(space) = line.find(' ') {
                append(&mut job, &line[..space], KEY);
                append(&mut job, &line[space..], VALUE);
            } else {
                append(&mut job, line, KEY);
            }
        } else if let Some(colon) = line.find(':') {
            append(&mut job, &line[..colon], KEY);
            append(&mut job, &line[colon..colon + 1], PUNCT);
            append(&mut job, &line[colon + 1..], TEXT);
        } else {
            append(&mut job, line, TEXT);
        }
    }
    body(&mut job, content);
    job
}
fn body(job: &mut LayoutJob, mut text: &str) {
    let mut in_tag = false;
    while !text.is_empty() {
        if text.starts_with("<!--") {
            let end = text.find("-->").map_or(text.len(), |i| i + 3);
            append(job, &text[..end], COMMENT);
            text = &text[end..];
            continue;
        }
        let first = text.chars().next().unwrap();
        if first == '<' {
            in_tag = true;
            append(job, "<", PUNCT);
            text = &text[1..];
            continue;
        }
        if first == '>' {
            in_tag = false;
            append(job, ">", PUNCT);
            text = &text[1..];
            continue;
        }
        if first == '"' || (in_tag && first == '\'') {
            let tail = &text[1..];
            let mut escaped = false;
            let mut end = text.len();
            for (i, ch) in tail.char_indices() {
                if ch == first && !escaped {
                    end = i + 2;
                    break;
                }
                escaped = ch == '\\' && !escaped;
            }
            append(job, &text[..end], VALUE);
            text = &text[end..];
            continue;
        }
        let end = text
            .char_indices()
            .skip(1)
            .find(|(_, ch)| matches!(ch, '<' | '>' | '"') || (in_tag && *ch == '\''))
            .map_or(text.len(), |(i, _)| i);
        append(job, &text[..end], if in_tag { KEY } else { TEXT });
        text = &text[end..];
    }
}
pub fn editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    editable: bool,
    rows: usize,
) -> egui::Response {
    editor_limited(ui, id, text, editable, rows, 128 * 1024)
}
pub(super) fn editor_limited(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    editable: bool,
    rows: usize,
    limit: usize,
) -> egui::Response {
    let selection_before = selected_text(ui.ctx(), id, text);
    let wrap = wrapping(ui, id);
    // egui's built-in undo compares cursor positions as well as text. Keeping text-only
    // revisions preserves redo when the operator clicks or sends between undo and redo.
    let history = editable.then(|| {
        ui.ctx().data_mut(|data| {
            data.get_temp_mut_or_insert_with(id.with("undo-text"), || {
                std::sync::Arc::new(std::sync::Mutex::new(UndoHistory {
                    last: text.clone(),
                    ..Default::default()
                }))
            })
            .clone()
        })
    });
    let now = ui.input(|input| input.time);
    if let Some(history) = &history {
        let mut history = history.lock().unwrap_or_else(|e| e.into_inner());
        history.record(text, now, true);
        if ui.memory(|memory| memory.has_focus(id)) {
            let undo = ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z));
            let redo = ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL, egui::Key::Y)
                    || input
                        .consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z)
            });
            let revision = if undo {
                history.undo()
            } else if redo {
                history.redo()
            } else {
                None
            };
            if let Some(revision) = revision {
                *text = revision;
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(
                            egui::text::CCursor::new(text.chars().count()),
                        )));
                    state.store(ui.ctx(), id);
                }
            }
        }
    }
    let before = text.clone();
    let diff_colors = ui
        .ctx()
        .data(|data| data.get_temp::<Vec<Color32>>(id.with("diff-colors")));
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, width: f32| {
        let mut job = if let Some(colors) = &diff_colors {
            let mut job = LayoutJob::default();
            job.wrap.max_width = f32::INFINITY;
            for (index, line) in buf.as_str().split_inclusive('\n').enumerate() {
                job.append(
                    line,
                    0.0,
                    TextFormat {
                        color: TEXT,
                        background: colors.get(index).copied().unwrap_or(Color32::TRANSPARENT),
                        ..Default::default()
                    },
                );
            }
            job
        } else {
            layout(buf.as_str())
        };
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        for section in &mut job.sections {
            section.format.font_id = font.clone();
        }
        job.wrap.max_width = if wrap { width.max(1.0) } else { f32::INFINITY };
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let response = ui
        .horizontal_top(|ui| {
            if diff_colors.is_none() && !wrap {
                let count = text.lines().count().clamp(1, 5000);
                let numbers = (1..=count)
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.label(egui::RichText::new(numbers).monospace().color(PUNCT));
            }
            if editable {
                ui.add(
                    egui::TextEdit::multiline(text)
                        .id(id)
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter)
                        .char_limit(limit)
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows),
                )
            } else {
                ui.add(
                    egui::TextEdit::multiline(&mut text.as_str())
                        .id(id)
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter)
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows),
                )
            }
        })
        .inner;
    if editable && before != *text {
        let boundary = ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)))
        });
        history
            .as_ref()
            .unwrap()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(text, now, boundary);
    }
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
        // A toolbar click is outside the editor, so TextEdit relinquishes focus
        // while handling that click. Apply Find's selection after that handling.
        if let Some(range) = ui.ctx().data_mut(|data| {
            data.remove_temp::<egui::text::CCursorRange>(id.with("find-selection"))
        }) {
            state.cursor.set_char_range(Some(range));
            ui.memory_mut(|memory| memory.request_focus(id));
            ui.ctx().request_repaint();
        }
        state.clear_undoer();
        state.store(ui.ctx(), id);
    }
    selection_menu(ui, &response, id, text, selection_before);
    response
}
fn selected_text(ctx: &egui::Context, id: egui::Id, text: &str) -> Option<String> {
    let range = egui::TextEdit::load_state(ctx, id)?
        .cursor
        .char_range()?
        .as_sorted_char_range();
    let start = usize::from(range.start);
    let end = usize::from(range.end);
    if start == end {
        return None;
    }
    Some(
        text.chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect(),
    )
}
fn selection_menu(
    ui: &mut egui::Ui,
    response: &egui::Response,
    id: egui::Id,
    text: &str,
    before: Option<String>,
) {
    let snapshot = id.with("context-selection");
    if response.hovered()
        && ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Secondary))
    {
        let selection = before
            .or_else(|| selected_text(ui.ctx(), id, text))
            .unwrap_or_default();
        ui.ctx()
            .data_mut(|data| data.insert_temp(snapshot, selection));
    }
    response.context_menu(|ui| {
        if let Some(source) = ui
            .ctx()
            .data(|data| data.get_temp::<Result<RequestSource, String>>(id.with("request-source")))
        {
            for (label, intruder) in [
                ("Send request to Replay", false),
                ("Send request to Intruder", true),
            ] {
                if ui.button(label).clicked() {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            egui::Id::new("request-transfer"),
                            (intruder, source.clone()),
                        )
                    });
                    ui.close();
                }
            }
            if ui.button("Send request to Attack").clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("attack-request-transfer"), source)
                });
                ui.close();
            }
            ui.separator();
        }
        let selected = ui
            .ctx()
            .data(|data| data.get_temp::<String>(snapshot))
            .unwrap_or_default();
        let available = !selected.is_empty();
        if ui
            .add_enabled(available, egui::Button::new("Send selection to Coder"))
            .clicked()
        {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("coder-selection"), selected.clone())
            });
            ui.close();
        }
        if ui
            .add_enabled(available, egui::Button::new("Copy selection"))
            .clicked()
        {
            ui.ctx().copy_text(selected);
            ui.close();
        }
        if !available {
            ui.weak("Select text with the mouse or Shift + arrows first.");
        }
        ui.separator();
        for (label, action) in [
            ("Set Compare baseline", "compare-baseline"),
            ("Compare with baseline", "compare-candidate"),
        ] {
            if ui.button(label).clicked() {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::new(action), text.to_owned()));
                ui.close();
            }
        }
        ui.separator();
        wrap_control(ui, id);
        if let Some(host) = ui
            .ctx()
            .data(|data| data.get_temp::<String>(id.with("scope-host")))
        {
            ui.separator();
            scope_menu(ui, &host);
        }
    });
}
pub(super) fn take_coder_selection(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|data| data.remove_temp::<String>(egui::Id::new("coder-selection")))
}
pub(super) fn diff_editor(ui: &mut egui::Ui, id: egui::Id, text: &mut String, colors: &[Color32]) {
    ui.ctx()
        .data_mut(|data| data.insert_temp(id.with("diff-colors"), colors.to_vec()));
    editor(ui, id, text, false, 30);
}
pub fn forget_editor(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|data| {
        data.remove::<std::sync::Arc<std::sync::Mutex<UndoHistory>>>(id.with("undo-text"));
        data.remove::<Result<RequestSource, String>>(id.with("request-source"));
        data.remove::<String>(id.with("context-selection"));
        data.remove::<String>(id.with("scope-host"));
    });
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state.clear_undoer();
        state.store(ctx, id);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn site_scope_menu_queues_the_displayed_domain_and_selected_breadth() {
        for (label, subdomains) in [
            ("Site domain only", false),
            ("Site domain + subdomains", true),
        ] {
            let ctx = egui::Context::default();
            let draw = |events: Vec<egui::Event>| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(940.0, 700.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| scope_menu(ui, "api.shop.example.co.uk"),
                );
                let texts = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some((
                            text.galley.text().to_owned(),
                            text.pos + text.galley.size() * 0.5,
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                output.textures_delta.clear();
                texts
            };
            let click = |point: egui::Pos2| {
                draw(vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
                draw(vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }])
            };
            let point = draw(vec![])
                .into_iter()
                .find(|(text, _)| text == "Add to scope")
                .unwrap()
                .1;
            click(point);
            let items = draw(vec![]);
            assert!(items.iter().any(|(text, _)| text == "example.co.uk"));
            let point = items.into_iter().find(|(text, _)| text == label).unwrap().1;
            click(point);
            assert_eq!(
                ctx.data_mut(
                    |data| data.remove_temp::<(String, bool)>(egui::Id::new("scope-add-host"))
                ),
                Some(("example.co.uk".into(), subdomains))
            );
        }
    }
    #[test]
    fn wrapping_reflows_long_lines_without_changing_message_bytes() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("wrap-regression");
        let mut text = format!("X-Token: {}", "Київabc".repeat(60));
        let original = text.clone();
        let mut sizes = Vec::new();
        for wrap in [false, true] {
            set_default_wrap(&ctx, wrap);
            let mut rect = egui::Rect::NOTHING;
            for _ in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(360.0, 400.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        egui::ScrollArea::both().show(ui, |ui| {
                            rect = editor(ui, id, &mut text, true, 1).rect;
                        });
                    },
                );
                output.textures_delta.clear();
            }
            sizes.push(rect.size());
        }
        assert!(sizes[1].y > sizes[0].y);
        assert!(
            sizes[1].x <= 360.0,
            "wrapped editor is still wider than its viewport: {:?}",
            sizes[1]
        );
        assert_eq!(text, original);
    }
    #[test]
    fn right_click_preserves_unicode_selection_for_editable_and_readonly_menus() {
        for editable in [true, false] {
            let ctx = egui::Context::default();
            let id = egui::Id::new("selection-context-regression");
            let mut text = "aКиївz".to_owned();
            let mut rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(Default::default(), |ui| {
                rect = editor(ui, id, &mut text, editable, 3).rect;
            });
            output.textures_delta.clear();
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(1),
                    egui::text::CCursor::new(5),
                )));
            state.store(&ctx, id);
            ctx.memory_mut(|memory| memory.request_focus(id));
            let pos = rect.min + egui::vec2(10.0, 8.0);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Secondary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| {
                    editor(ui, id, &mut text, editable, 3);
                },
            );
            output.textures_delta.clear();
            assert_eq!(
                ctx.data(|data| data.get_temp::<String>(id.with("context-selection")))
                    .as_deref(),
                Some("Київ")
            );
            assert_eq!(text, "aКиївz");
        }
    }
    #[test]
    fn find_next_selects_unicode_matches_and_keeps_them_for_copy() {
        let context = egui::Context::default();
        let id = egui::Id::new("find-regression");
        let mut text = "Header\nα one\nα two".to_owned();
        let mut next = 0;
        let mut button = egui::Rect::NOTHING;
        for step in 0..5 {
            let events = if step == 1 || step == 2 {
                vec![
                    egui::Event::PointerMoved(button.center()),
                    egui::Event::PointerButton {
                        pos: button.center(),
                        button: egui::PointerButton::Primary,
                        pressed: step == 1,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else if step == 4 {
                vec![egui::Event::Copy]
            } else {
                vec![]
            };
            let mut output = context.run_ui(
                egui::RawInput {
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.button("Next match");
                    button = response.rect;
                    if response.clicked() {
                        assert_eq!(find_next(ui, id, &text, "α two", &mut next), Some(2));
                    }
                    editor(ui, id, &mut text, false, 5);
                },
            );
            output.textures_delta.clear();
            if step == 4 {
                assert!(output.platform_output.commands.iter().any(|command| {
                    matches!(command, egui::OutputCommand::CopyText(text) if text == "α two")
                }));
            }
            if step >= 2 {
                assert!(next > 0, "button was not clicked in step {step}");
                assert!(context.memory(|memory| memory.has_focus(id)));
                let state = egui::TextEdit::load_state(&context, id).unwrap();
                let range = state.cursor.char_range().unwrap().as_sorted_char_range();
                assert_eq!(
                    text.chars()
                        .skip(usize::from(range.start))
                        .take(usize::from(range.end) - usize::from(range.start))
                        .collect::<String>(),
                    "α two"
                );
            }
        }
    }
    #[test]
    fn highlighting_preserves_http_html_and_unicode_exactly() {
        let text =
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<!-- hi --><a href=\"/ü\">link</a>";
        let job = layout(text);
        assert_eq!(job.text, text);
        assert!(job.sections.len() > 8);
        assert!(job.sections.iter().any(|s| s.format.color == COMMENT));
        assert!(job.sections.iter().any(|s| s.format.color == VALUE));
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;
    fn frame(context: &egui::Context, text: &mut String, time: f64, events: Vec<egui::Event>) {
        let mut output = context.run_ui(
            egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                let response = editor(ui, egui::Id::new("undo-test"), text, true, 5);
                if time == 0.0 {
                    response.request_focus();
                }
            },
        );
        output.textures_delta.clear();
    }
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        }
    }
    #[test]
    fn ctrl_z_and_ctrl_y_restore_editor_text() {
        let context = egui::Context::default();
        let mut text = "GET / HTTP/1.1\n\n".to_owned();
        let original = text.clone();
        frame(&context, &mut text, 0.0, vec![]);
        frame(
            &context,
            &mut text,
            1.0,
            vec![egui::Event::Text("edited".into())],
        );
        let edited = text.clone();
        assert_ne!(edited, original);
        frame(&context, &mut text, 2.0, vec![key(egui::Key::Z)]);
        assert_eq!(text, original);
        frame(&context, &mut text, 3.0, vec![key(egui::Key::Y)]);
        assert_eq!(text, edited);
    }
}

pub fn find_next(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &str,
    query: &str,
    next: &mut usize,
) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    let start = (*next).min(text.len());
    let start = text.floor_char_boundary(start);
    let at = text[start..]
        .find(query)
        .map(|n| start + n)
        .or_else(|| text.find(query))?;
    *next = at + query.len();
    let first = text[..at].chars().count();
    let end = first + query.chars().count();
    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    let range = egui::text::CCursorRange::two(
        egui::text::CCursor::new(first),
        egui::text::CCursor::new(end),
    );
    state.cursor.set_char_range(Some(range));
    state.store(ui.ctx(), id);
    ui.ctx()
        .data_mut(|data| data.insert_temp(id.with("find-selection"), range));
    ui.memory_mut(|memory| memory.request_focus(id));
    Some(text[..at].bytes().filter(|b| *b == b'\n').count())
}
